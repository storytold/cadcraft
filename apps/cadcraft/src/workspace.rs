//! Native workspace preferences. Drawing data and transient dialogs never enter this file.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cadcraft_ui_egui::UiState;
use serde_json::{Value, json};

const MAX_BYTES: u64 = 64 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct Workspace {
    path: Option<PathBuf>,
    last_attempt: Option<Value>,
    read_failed: bool,
    pending_write_error: Option<String>,
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
    ui.docking.validate()?;
    if !matches!(ui.toolset_tab.as_str(), "Drafting" | "Modeling") {
        return Err("toolsetTab must be Drafting or Modeling".into());
    }
    if ui.collapsed_groups.len() > 64 || ui.collapsed_groups.iter().any(|name| name.len() > 256) {
        return Err("collapsedGroups exceeds the workspace size limit".into());
    }
    if ui.history_lines > 12 {
        return Err("historyLines must be between 0 and 12".into());
    }
    if ui.ribbon_tab.len() > 64 {
        return Err("ribbonTab exceeds the workspace size limit".into());
    }
    Ok(json!({
        "docking": ui.docking,
        "showToolsets": ui.show_toolsets,
        "showPalettes": ui.show_palettes,
        "showToolbar": ui.show_toolbar,
        "showRibbon": ui.show_ribbon,
        "ribbonTab": ui.ribbon_tab,
        "ribbonMinimized": ui.ribbon_minimized,
        "pcLayoutApplied": ui.pc_layout_applied,
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
    let mut fields = workspace.clone();
    let docking = fields.as_object_mut().and_then(|fields| fields.remove("docking"));
    let mut ui: UiState = serde_json::from_value(fields).map_err(|error| format!("{}: invalid workspace: {error}", path.display()))?;
    if let Some(docking) = docking
        && let Ok(docking) = serde_json::from_value::<cadcraft_ui_egui::docking::Workspace>(docking)
        && docking.validate().is_ok()
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
                (Self { path, last_attempt: snapshot(&initial).ok(), read_failed: false, pending_write_error: None }, ui, None)
            }
            Err(error) => (
                Self { path, last_attempt: None, read_failed: true, pending_write_error: None },
                None,
                Some(format!("Workspace could not be restored; its file was left unchanged. {error}")),
            ),
        }
    }

    pub fn save_if_changed(&mut self, ui: &UiState) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        if self.read_failed {
            // Preserve a corrupt, oversized or newer-version file until the user repairs it.
            return Ok(());
        }
        let value = snapshot(ui)?;
        if self.last_attempt.as_ref() == Some(&value) {
            return self.pending_write_error.clone().map_or(Ok(()), Err);
        }
        // Keep a failed state's warning without retrying disk I/O every rendered frame.
        self.last_attempt = Some(value.clone());
        let result = write(path, &value);
        self.pending_write_error = result.as_ref().err().cloned();
        result
    }

    pub fn save_on_exit(&mut self, ui: &UiState) -> Result<(), String> {
        if self.pending_write_error.is_some() {
            self.last_attempt = None;
        }
        self.save_if_changed(ui)
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
        first.save_if_changed(&ui).unwrap();
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
        first.save_if_changed(&transient_only).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_ribbon_survives_native_workspace_file_reload() {
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        let (mut first, _, _) = Workspace::at(Some(path.clone()));
        let ui = UiState { show_ribbon: true, ribbon_tab: "annotate".into(), ribbon_minimized: true, pc_layout_applied: true, ..Default::default() };
        first.save_if_changed(&ui).unwrap();
        let (_, restored, error) = Workspace::at(Some(path.clone()));
        assert!(error.is_none(), "{error:?}");
        let restored = restored.unwrap();
        assert!(restored.show_ribbon && restored.ribbon_minimized && restored.pc_layout_applied);
        assert_eq!(restored.ribbon_tab, "annotate");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn docked_floating_and_hidden_panels_survive_native_workspace_file_reload() {
        use cadcraft_ui_egui::docking::Panel;
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        let (mut store, _, _) = Workspace::at(Some(path.clone()));
        let mut ui = UiState::default();
        let mut app = cadcraft_ui_egui::CadApp::new(cadcraft_engine::Session::default(), Default::default());
        app.run("ui.dock", json!({"operation":"float", "panel":"layers", "rect":[120,90,340,410]})).unwrap();
        ui.docking = app.ui.docking;
        ui.docking.set_visible(Panel::ToolSets, false).unwrap();
        ui.show_toolsets = false;
        store.save_if_changed(&ui).unwrap();
        let (_, reloaded, error) = Workspace::at(Some(path));
        assert!(error.is_none());
        let mut reloaded = reloaded.unwrap();
        assert_eq!(serde_json::to_value(&reloaded.docking).unwrap(), serde_json::to_value(&ui.docking).unwrap());
        reloaded.docking.set_visible(Panel::ToolSets, true).unwrap();
        assert!(reloaded.docking.layout.contains(&Panel::ToolSets));
        assert_eq!(reloaded.docking.layout.floating[0].rect, [120.0, 90.0, 340.0, 410.0]);
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
            store.save_if_changed(&UiState { show_palettes: false, ..Default::default() }).unwrap();
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
            let workspace = json!({"showToolbar":false,"showPalettes":false,"toolsetTab":"Modeling", "collapsedGroups":["Draw"], "historyLines":7,"docking":docking});
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
    fn invalid_update_cannot_replace_last_good_workspace() {
        let directory = temp_dir();
        let path = directory.join("workspace.json");
        let (mut store, _, _) = Workspace::at(Some(path.clone()));
        let mut ui = UiState { show_palettes: false, ..Default::default() };
        store.save_if_changed(&ui).unwrap();
        let good = fs::read(&path).unwrap();
        ui.collapsed_groups = vec!["x".repeat(257)];
        assert!(store.save_if_changed(&ui).is_err());
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
        let first_error = store.save_if_changed(&ui).unwrap_err();
        fs::remove_dir(&path).unwrap();
        assert_eq!(store.save_if_changed(&ui).unwrap_err(), first_error);
        assert!(!path.exists(), "ordinary frames must not retry disk writes");
        store.save_on_exit(&ui).unwrap();
        assert!(!read(&path).unwrap().unwrap().show_palettes);
        fs::remove_dir_all(directory).unwrap();
    }
}
