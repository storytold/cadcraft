//! Native workspace preferences. Drawing data and transient dialogs never enter this file.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cadcraft_ui_egui::UiState;
use serde_json::{Value, json};

const MAX_BYTES: u64 = 64 * 1024;
/// Dragging a floating window changes its rect every frame; writes (each with an fsync) are
/// spaced at least this far apart, and the newest state is written when the interval ends.
const MIN_WRITE_INTERVAL: Duration = Duration::from_millis(500);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct Workspace {
    path: Option<PathBuf>,
    last_attempt: Option<Value>,
    read_failed: bool,
    pending_write_error: Option<String>,
    last_write: Option<Instant>,
}

fn config_dir_from(platform: &str, get: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(root) = get("CADCRAFT_CONFIG_DIR") {
        return (!root.is_empty()).then(|| PathBuf::from(root));
    }
    let nonempty = |key| get(key).filter(|value| !value.is_empty()).map(PathBuf::from);
    match platform {
        "macos" => nonempty("HOME").map(|home| home.join("Library/Application Support/CADCraft")),
        "windows" => nonempty("APPDATA").map(|root| root.join("CADCraft")),
        _ => nonempty("XDG_CONFIG_HOME").map(|root| root.join("cadcraft")).or_else(|| nonempty("HOME").map(|home| home.join(".config/cadcraft"))),
    }
}

fn snapshot(ui: &UiState) -> Result<Value, String> {
    cadcraft_ui_egui::docking::validate(&ui.docking)?;
    if !matches!(ui.toolset_tab.as_str(), "Drafting" | "Modeling") {
        return Err("toolsetTab must be Drafting or Modeling".into());
    }
    if ui.collapsed_groups.len() > 64 || ui.collapsed_groups.iter().any(|name| name.len() > 256) {
        return Err("collapsedGroups exceeds the workspace size limit".into());
    }
    if ui.history_lines > 12 {
        return Err("historyLines must be between 0 and 12".into());
    }
    Ok(json!({
        "docking": ui.docking,
        "dockingVersion": cadcraft_ui_egui::docking::LAYOUT_VERSION,
        "showToolsets": ui.show_toolsets,
        "showPalettes": ui.show_palettes,
        "showToolbar": ui.show_toolbar,
        "showFileTabs": ui.show_file_tabs,
        "showStatusBar": ui.show_status_bar,
        "showCommandLine": ui.show_command_line,
        "showViewcube": ui.show_viewcube,
        "showUcsIcon": ui.show_ucs_icon,
        "showLayerList": ui.show_layer_list,
        "toolsetTab": ui.toolset_tab,
        "collapsedGroups": ui.collapsed_groups,
        "propertiesAll": ui.properties_all,
        "inWindowMenu": ui.in_window_menu,
        "historyLines": ui.history_lines,
    }))
}

fn read(path: &Path) -> Result<Option<UiState>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("{}: workspace file exceeds 64 KiB", path.display()));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| format!("{}: invalid workspace JSON: {error}", path.display()))?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(format!("{}: unsupported workspace version", path.display()));
    }
    let workspace =
        value.get("workspace").filter(|value| value.is_object()).ok_or_else(|| format!("{}: missing workspace object", path.display()))?;
    // Dock schemas can evolve independently of the other saved workspace preferences.
    // Recover only this optional field; runtime UiState deserialization remains strict.
    // A layout from before the current shape (split frames, saved without `dockingVersion`)
    // gives way to the default, its hidden panels with it.
    let mut fields = workspace.clone();
    let (docking, version) = match fields.as_object_mut() {
        Some(fields) => (fields.remove("docking"), fields.remove("dockingVersion")),
        None => (None, None),
    };
    let current = version.and_then(|version| version.as_u64()).is_some_and(|version| version >= u64::from(cadcraft_ui_egui::docking::LAYOUT_VERSION));
    let mut ui: UiState = serde_json::from_value(fields).map_err(|error| format!("{}: invalid workspace: {error}", path.display()))?;
    if let Some(docking) = docking.filter(|_| current)
        && let Ok(docking) = serde_json::from_value::<cadcraft_ui_egui::docking::Workspace>(docking)
        && cadcraft_ui_egui::docking::validate(&docking).is_ok()
    {
        ui.docking = docking;
    }
    // Old or hand-edited files cannot reopen a dialog or replace the active drawing view.
    ui.dialog = None;
    ui.start_tab = false;
    snapshot(&ui).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Some(ui))
}

fn write(path: &Path, workspace: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&json!({"version": 1, "workspace": workspace})).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("workspace exceeds 64 KiB".into());
    }
    let parent = path.parent().ok_or("workspace path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".workspace-{}-{sequence}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|error| format!("{}: {error}", temporary.display()))?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(format!("{}: could not save workspace: {error}", path.display()));
    }
    Ok(())
}

impl Workspace {
    pub fn load() -> (Self, Option<UiState>, Option<String>) {
        Self::at(config_dir_from(std::env::consts::OS, |key| std::env::var_os(key)).map(|root| root.join("workspace.json")))
    }

    fn at(path: Option<PathBuf>) -> (Self, Option<UiState>, Option<String>) {
        let result = path.as_deref().map(read).transpose().map(Option::flatten);
        match result {
            Ok(ui) => {
                let initial = ui.as_ref().cloned().unwrap_or_default();
                (Self { path, last_attempt: snapshot(&initial).ok(), read_failed: false, pending_write_error: None, last_write: None }, ui, None)
            }
            Err(error) => (
                Self { path, last_attempt: None, read_failed: true, pending_write_error: None, last_write: None },
                None,
                Some(format!("Workspace could not be restored; its file was left unchanged. {error}")),
            ),
        }
    }

    /// Writes `ui` if it differs from the last state written. `Ok(Some(wait))` means a change
    /// is held back by [`MIN_WRITE_INTERVAL`]; call again after `wait` to write it.
    pub fn save_if_changed(&mut self, ui: &UiState, now: Instant) -> Result<Option<Duration>, String> {
        let Some(path) = &self.path else { return Ok(None) };
        if self.read_failed {
            // Preserve a corrupt, oversized or newer-version file until the user repairs it.
            return Ok(None);
        }
        let value = snapshot(ui)?;
        if self.last_attempt.as_ref() == Some(&value) {
            return self.pending_write_error.clone().map_or(Ok(None), Err);
        }
        if let Some(wait) = self.last_write.and_then(|last| MIN_WRITE_INTERVAL.checked_sub(now.saturating_duration_since(last)))
            && !wait.is_zero()
        {
            return Ok(Some(wait));
        }
        // Keep a failed state's warning without retrying disk I/O every rendered frame.
        self.last_attempt = Some(value.clone());
        self.last_write = Some(now);
        let result = write(path, &value);
        self.pending_write_error = result.as_ref().err().cloned();
        result.map(|()| None)
    }

    pub fn save_on_exit(&mut self, ui: &UiState) -> Result<(), String> {
        if self.pending_write_error.is_some() {
            self.last_attempt = None;
        }
        self.last_write = None;
        self.save_if_changed(ui, Instant::now()).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("cadcraft-workspace-test-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn profile_override_isolated_or_explicitly_disabled() {
        let get = |key: &str| match key {
            "CADCRAFT_CONFIG_DIR" => Some(OsString::from("/private/test-state")),
            "HOME" | "APPDATA" | "XDG_CONFIG_HOME" => Some(OsString::from("/unrelated-user-profile")),
            _ => None,
        };
        for platform in ["macos", "windows", "linux", "freebsd"] {
            assert_eq!(config_dir_from(platform, get), Some(PathBuf::from("/private/test-state")));
            assert_eq!(config_dir_from(platform, |key| if key == "CADCRAFT_CONFIG_DIR" { Some(OsString::new()) } else { get(key) }), None);
        }
        assert_eq!(
            config_dir_from("linux", |key| (key == "XDG_CONFIG_HOME").then(|| OsString::from("/config"))),
            Some(PathBuf::from("/config/cadcraft"))
        );
        assert_eq!(config_dir_from("macos", |_| None), None);
    }

    #[test]
    fn workspace_survives_new_store_without_transient_or_document_state() {
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        let (mut first, restored, error) = Workspace::at(Some(path.clone()));
        assert!(restored.is_none() && error.is_none());
        let ui = UiState {
            show_palettes: false,
            show_command_line: false,
            toolset_tab: "Modeling".into(),
            collapsed_groups: vec!["Solid".into()],
            history_lines: 5,
            dialog: Some("layers".into()),
            start_tab: true,
            ..Default::default()
        };
        first.save_if_changed(&ui, Instant::now()).unwrap();
        let bytes = fs::read(&path).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value["workspace"].get("dialog").is_none());
        assert!(value["workspace"].get("startTab").is_none());
        let (_, restored, error) = Workspace::at(Some(path.clone()));
        assert!(error.is_none());
        let restored = restored.unwrap();
        assert_eq!(snapshot(&restored).unwrap(), snapshot(&ui).unwrap());
        assert!(restored.dialog.is_none() && !restored.start_tab);
        let mut transient_only = ui.clone();
        transient_only.dialog = None;
        transient_only.start_tab = false;
        first.save_if_changed(&transient_only, Instant::now()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dock_columns_floating_and_hidden_panels_survive_native_workspace_file_reload() {
        use cadcraft_ui_egui::docking::{LAYOUT_VERSION, Panel, permissions, reveal_panel};
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        let (mut store, _, _) = Workspace::at(Some(path.clone()));
        let mut ui = UiState::default();
        let mut app = cadcraft_ui_egui::CadApp::new(cadcraft_engine::Session::default(), Default::default());
        app.run("ui.dock", json!({"operation":"move", "panel":"layers", "target":"properties", "zone":"left"})).unwrap();
        app.run("ui.dock", json!({"operation":"float", "panel":"properties", "rect":[120,90,340,410]})).unwrap();
        ui.docking = app.ui.docking;
        ui.docking.set_visible(Panel::ToolSets, false, None, permissions).unwrap();
        ui.show_toolsets = false;
        store.save_if_changed(&ui, Instant::now()).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["workspace"]["dockingVersion"], json!(LAYOUT_VERSION));
        let (_, reloaded, error) = Workspace::at(Some(path));
        assert!(error.is_none());
        let mut reloaded = reloaded.unwrap();
        assert_eq!(serde_json::to_value(&reloaded.docking).unwrap(), serde_json::to_value(&ui.docking).unwrap());
        assert!(!reloaded.show_toolsets);
        reveal_panel(&mut reloaded.docking, Panel::ToolSets).unwrap();
        let tools = reloaded.docking.layout.location(&Panel::ToolSets).unwrap();
        assert_eq!(serde_json::to_value(&tools.group).unwrap()["column"]["side"], json!("Start"), "{tools:?}");
        assert_eq!(reloaded.docking.layout.floating[0].rect, [120.0, 90.0, 340.0, 410.0]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn split_frame_layout_file_loads_as_the_dock_default_with_its_other_settings() {
        use cadcraft_ui_egui::docking::default_workspace;
        // Written as CADCraft 0.7 writes it: craft-ui's v1 layout with no docking version, a
        // floating window as one `{panels, active, rect}` group, Tool Sets closed.
        let file = r#"{"version":1,"workspace":{
            "docking":{
                "layout":{
                    "root":{"Split":{"axis":"Horizontal","size":{"FixedSecond":300.0},
                        "first":{"Tabs":{"panels":["canvas"],"active":0}},
                        "second":{"Stack":{"entries":[{"panel":"properties","open":false,"height":null}]}}}},
                    "floating":[{"panels":["layers","externalReferences"],"active":1,"rect":[120.0,90.0,340.0,410.0]}]},
                "hidden":[["toolSets",{"floating":null,"anchor":"canvas","placement":{"Split":"Left"},
                    "size":{"FixedFirst":220.0},"stack":null,"sibling":["canvas","properties"]}]]},
            "showToolsets":false,"showPalettes":true,"showToolbar":false,"toolsetTab":"Modeling","historyLines":4}}"#;
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        fs::write(&path, file).unwrap();
        let (_, restored, error) = Workspace::at(Some(path));
        assert!(error.is_none(), "{error:?}");
        let ui = restored.unwrap();
        assert_eq!(ui.toolset_tab, "Modeling");
        assert_eq!(ui.history_lines, 4);
        assert!(!ui.show_toolsets && ui.show_palettes && !ui.show_toolbar);
        assert_eq!(ui.docking.layout, default_workspace().layout);
        assert!(ui.docking.hidden.is_empty(), "{:?}", ui.docking.hidden);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_oversized_and_newer_state_are_reported_and_preserved() {
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        for bytes in [
            b"{invalid".to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
            br#"{"version":2,"workspace":{}}"#.to_vec(),
            br#"{"version":1,"workspace":{"toolsetTab":"unknown"}}"#.to_vec(),
            br#"{"version":1,"workspace":{"historyLines":18446744073709551615}}"#.to_vec(),
        ] {
            fs::write(&path, &bytes).unwrap();
            let (mut store, ui, error) = Workspace::at(Some(path.clone()));
            assert!(ui.is_none());
            assert!(error.as_deref().is_some_and(|error| error.contains("left unchanged")));
            store.save_if_changed(&UiState { show_palettes: false, ..Default::default() }, Instant::now()).unwrap();
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unsupported_docking_preserves_other_workspace_preferences() {
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        for docking in [
            json!({"layout":{"root":{"FuturePanelGroup":{}}},"hidden":[]}),
            json!({"layout":false,"hidden":[]}),
            json!({"layout":{"root":null,"floating":[]},"hidden":[["futurePanel",{}]]}),
        ] {
            let workspace = json!({"showToolbar":false,"showPalettes":false,"toolsetTab":"Modeling", "collapsedGroups":["Draw"], "historyLines":7,"docking":docking,
                "dockingVersion":cadcraft_ui_egui::docking::LAYOUT_VERSION});
            assert!(serde_json::from_value::<UiState>(workspace.clone()).is_err(), "runtime state must stay strict");
            fs::write(&path, serde_json::to_vec(&json!({"version":1,"workspace":workspace})).unwrap()).unwrap();
            let (_, restored, error) = Workspace::at(Some(path.clone()));
            assert!(error.is_none());
            let restored = restored.unwrap();
            assert!(!restored.show_toolbar);
            assert!(!restored.show_palettes);
            assert_eq!(restored.toolset_tab, "Modeling");
            assert_eq!(restored.collapsed_groups, ["Draw"]);
            assert_eq!(restored.history_lines, 7);
            assert_eq!(serde_json::to_value(restored.docking).unwrap(), serde_json::to_value(UiState::default().docking).unwrap());
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rapid_changes_are_written_once_per_interval_with_the_newest_state() {
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        let (mut store, _, _) = Workspace::at(Some(path.clone()));
        let start = Instant::now();
        let mut ui = UiState { history_lines: 1, ..Default::default() };
        assert_eq!(store.save_if_changed(&ui, start).unwrap(), None);
        // A drag: a new state every frame, none of them written until the interval ends.
        for (frame, lines) in (2..=5).enumerate() {
            ui.history_lines = lines;
            let now = start + Duration::from_millis(16 * (frame as u64 + 1));
            let wait = store.save_if_changed(&ui, now).unwrap();
            assert_eq!(wait, Some(MIN_WRITE_INTERVAL - (now - start)));
        }
        assert_eq!(read(&path).unwrap().unwrap().history_lines, 1);
        assert_eq!(store.save_if_changed(&ui, start + MIN_WRITE_INTERVAL).unwrap(), None);
        assert_eq!(read(&path).unwrap().unwrap().history_lines, 5);
        ui.history_lines = 6;
        assert!(store.save_if_changed(&ui, start + MIN_WRITE_INTERVAL).unwrap().is_some());
        store.save_on_exit(&ui).unwrap();
        assert_eq!(read(&path).unwrap().unwrap().history_lines, 6);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_update_cannot_replace_last_good_workspace() {
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        let (mut store, _, _) = Workspace::at(Some(path.clone()));
        let mut ui = UiState { show_palettes: false, ..Default::default() };
        store.save_if_changed(&ui, Instant::now()).unwrap();
        let good = fs::read(&path).unwrap();
        ui.collapsed_groups = vec!["x".repeat(257)];
        assert!(store.save_if_changed(&ui, Instant::now()).is_err());
        assert_eq!(fs::read(&path).unwrap(), good);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_write_keeps_warning_and_retries_once_on_exit() {
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        let (mut store, _, _) = Workspace::at(Some(path.clone()));
        // Simulate a destination becoming unavailable after startup.
        fs::create_dir(&path).unwrap();
        let ui = UiState { show_palettes: false, ..Default::default() };
        let first_error = store.save_if_changed(&ui, Instant::now()).unwrap_err();
        fs::remove_dir(&path).unwrap();
        assert_eq!(store.save_if_changed(&ui, Instant::now()).unwrap_err(), first_error);
        assert!(!path.exists(), "ordinary frames must not retry disk writes");
        store.save_on_exit(&ui).unwrap();
        assert!(!read(&path).unwrap().unwrap().show_palettes);
        fs::remove_dir_all(directory).unwrap();
    }
}
