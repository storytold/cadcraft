//! Named profiles (Options ▸ Profiles, `options.profile`): added, copied, renamed, made current,
//! deleted, reset, exported and imported; kept across restarts; and the Profiles tab driving them
//! with real clicks and typing.

use std::path::PathBuf;

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, menus};
use egui::Color32;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

const UNNAMED: &str = "<<Unnamed Profile>>";

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn profiles_are_added_switched_renamed_deleted_exported_and_imported() {
    let dir = temp_dir("options_profiles_json");
    let mut a = app();
    assert_eq!(a.run("options.profile", json!({})).unwrap(), json!({"current": UNNAMED, "profiles": [UNNAMED]}));
    assert!(a.ui.dialog.is_none(), "JSON calls never open dialogs");

    // Add copies the current settings; Set Current keeps the old profile's and applies the new.
    a.run("options", json!({"PICKBOX": 7, "modelBackground": "#ffffff"})).unwrap();
    a.run("options.profile", json!({"action": "add", "name": "Office"})).unwrap();
    a.run("options", json!({"PICKBOX": 3, "modelBackground": null})).unwrap();
    a.run("options.profile", json!({"action": "set", "name": "office"})).unwrap();
    assert_eq!(a.profiles.current, "Office", "names match in any case");
    assert_eq!(a.session.settings.pickbox, 7.0);
    assert_eq!(a.options.model_background, Some(Color32::WHITE));
    a.run("options.profile", json!({"action": "set", "name": UNNAMED})).unwrap();
    assert_eq!(a.session.settings.pickbox, 3.0);
    assert_eq!(a.options.model_background, None);

    // The interface language belongs to no profile.
    a.run("options", json!({"interfaceLanguage": "it"})).unwrap();
    a.run("options.profile", json!({"action": "set", "name": "Office"})).unwrap();
    assert_eq!(a.run("options", json!({})).unwrap()["interfaceLanguage"], "it");
    a.run("options.profile", json!({"action": "set", "name": UNNAMED})).unwrap();

    // Rename and copy; names are unique, and the current profile can't be deleted.
    a.run("options.profile", json!({"action": "rename", "name": "Office", "newName": "Studio"})).unwrap();
    a.run("options.profile", json!({"action": "copy", "name": "Studio", "newName": "Studio B"})).unwrap();
    for bad in [
        json!({"action": "add", "name": "studio"}),
        json!({"action": "rename", "name": "Studio", "newName": "Studio B"}),
        json!({"action": "delete", "name": UNNAMED}),
        json!({"action": "set", "name": "Nobody"}),
        json!({"action": "add", "name": ""}),
        json!({"action": "add", "name": "a\nb"}),
        json!({"action": "add", "name": "x".repeat(65)}),
        json!({"action": "fly"}),
        json!({"action": 3}),
        json!({"action": "import", "text": "{\"format\": \"other\"}"}),
        json!({"action": "import", "text": "not json"}),
        json!({"action": "import", "text": "x".repeat(300 * 1024)}),
        json!({"action": "import"}),
        json!({"action": "export", "name": "Studio", "path": ""}),
    ] {
        let before = a.run("options.profile", json!({})).unwrap();
        assert!(a.run("options.profile", bad.clone()).is_err(), "{}", bad.to_string().get(..80).unwrap_or_default());
        assert_eq!(a.run("options.profile", json!({})).unwrap(), before);
    }
    a.run("options.profile", json!({"action": "delete", "name": "Studio B"})).unwrap();

    // Export to a file and import it back, as a new name since Studio is taken.
    let file = dir.join("studio.json");
    a.run("options.profile", json!({"action": "export", "name": "Studio", "path": file.to_string_lossy()})).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"format\": \"cadcraft-profile\"") && text.contains("\"PICKBOX\": 7.0"), "{text}");
    let r = a.run("options.profile", json!({"action": "import", "path": file.to_string_lossy()})).unwrap();
    assert_eq!(r["name"], "Studio (2)");
    let r = a.run("options.profile", json!({"action": "import", "text": text, "name": "Site"})).unwrap();
    assert_eq!(r["name"], "Site");
    let exported = a.run("options.profile", json!({"action": "export", "name": "Site"})).unwrap();
    assert!(exported["text"].as_str().is_some_and(|t| t.contains("\"name\": \"Site\"")));

    // The profiles survive a restart.
    let mut b = app();
    b.load_profile(&a.profile_json());
    assert_eq!(b.run("options.profile", json!({})).unwrap(), a.run("options.profile", json!({})).unwrap());
    b.run("options.profile", json!({"action": "set", "name": "Site"})).unwrap();
    assert_eq!(b.session.settings.pickbox, 7.0);
    // Reset puts a profile back to the defaults.
    b.run("options.profile", json!({"action": "reset"})).unwrap();
    assert_eq!(b.session.settings.pickbox, 3.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_saved_profiles_are_capped_and_skipped() {
    let many: serde_json::Map<String, serde_json::Value> = (0..200).map(|i| (format!("P{i}"), json!({"PICKBOX": 3}))).collect();
    let mut a = app();
    a.load_profile(&json!({"profileName": "\n", "profiles": many}).to_string());
    assert_eq!(a.profiles.current, UNNAMED, "a bad name is skipped");
    assert_eq!(a.profiles.names().len(), cadcraft_ui_egui::options::MAX_PROFILES);
    let mut b = app();
    b.load_profile(r#"{"profiles": {"A": "not settings", "": {}, "B": [1]}}"#);
    assert_eq!(b.profiles.names(), vec![UNNAMED.to_string()]);
    // A saved profile with a bad setting still applies the rest.
    let mut c = app();
    c.load_profile(r#"{"profiles": {"Odd": {"PICKBOX": 9, "GRIPSIZE": "big", "theme": "sepia"}}}"#);
    c.run("options.profile", json!({"action": "set", "name": "Odd"})).unwrap();
    assert_eq!(c.session.settings.pickbox, 9.0);
}

/// With `CADCRAFT_OPTIONS_SHOTS` set to a folder, the dialog is rendered there for a look.
const SHOTS: &str = "CADCRAFT_OPTIONS_SHOTS";

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let builder = Harness::builder().with_size(egui::vec2(1400.0, 900.0));
    let builder = if std::env::var_os(SHOTS).is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn click(h: &mut Harness<'static, CadApp>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(3);
}

/// Type `name` into the profile name field labelled `label` and press its OK.
fn type_name(h: &mut Harness<'static, CadApp>, label: &str, name: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_role_and_label(Role::TextInput, label).type_text(name);
    h.run_steps(2);
    click(h, "OK");
}

fn shot(h: &mut Harness<'static, CadApp>, name: &str) {
    if let Some(dir) = std::env::var_os(SHOTS).map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}

#[test]
fn the_profiles_tab_drives_every_action() {
    let dir = temp_dir("options_profiles_tab");
    let (out, back) = (dir.join("dialog.json"), dir.join("dialog.json"));
    let services = Services {
        pick_save_file: Some(Box::new(move |name: &str, _: &str, exts: &[&str]| {
            assert!(name.ends_with(".json") && exts == ["json"]);
            Some(out.to_string_lossy().to_string())
        })),
        pick_file: Some(Box::new(move |_: &str, _: &[&str]| Some(back.to_string_lossy().to_string()))),
        ..Services::default()
    };
    let mut a = CadApp::new(Session::new(), services);
    a.run("options", json!({"PICKBOX": 7})).unwrap();
    menus::activate(&mut a, "options");
    a.ui.dialog = Some("options:profiles".into());
    let mut h = harness(a);
    h.get_by_label("Current profile:");
    assert_eq!(h.get_all_by_label(UNNAMED).count(), 2, "above the tabs and in the list");

    // Add to List... asks for a name and copies the current settings.
    click(&mut h, "Add to List...");
    type_name(&mut h, "New profile name:", "Night");
    assert!(h.state().profiles.names().contains(&"Night".to_string()), "{:?}", h.state().profiles.names());
    // The new profile stays selected: Set Current makes it current.
    click(&mut h, "Set Current");
    assert_eq!(h.state().profiles.current, "Night");
    assert_eq!(h.state().session.settings.pickbox, 7.0);

    // Rename... the selected profile.
    click(&mut h, "Rename...");
    type_name(&mut h, "New name:", "Evening");
    assert_eq!(h.state().profiles.current, "Evening");
    shot(&mut h, "options-profiles.png");

    // Export... and Import... through the host's pickers.
    click(&mut h, "Export...");
    assert!(dir.join("dialog.json").is_file());
    click(&mut h, "Import...");
    assert!(h.state().profiles.names().contains(&"Evening (2)".to_string()), "{:?}", h.state().profiles.names());
    h.get_by_label("Imported as \"Evening (2)\"");

    // The imported profile is selected; Delete removes it. The current one can't be deleted.
    click(&mut h, "Delete");
    assert!(!h.state().profiles.names().contains(&"Evening (2)".to_string()));
    assert!(h.get_by_label("Delete").accesskit_node().is_disabled(), "the current profile is selected again");

    // Back to the first profile.
    click(&mut h, UNNAMED);
    let unnamed_in_list = h.get_all_by_label(UNNAMED).count();
    assert_eq!(unnamed_in_list, 1, "only in the list while Evening is current");
    click(&mut h, "Set Current");
    assert_eq!(h.state().profiles.current, UNNAMED);
    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed controls: {unnamed:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
