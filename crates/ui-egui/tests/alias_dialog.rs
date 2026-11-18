//! ALIASEDIT typed or started from a menu opens the Command Aliases dialog (#1381): the user's
//! aliases next to the built-in ones, Add, Edit and Remove, and OK, Cancel and Apply. JSON calls
//! keep the JSON form. The aliases are process-wide, so these tests run one at a time and use
//! their own names.

use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::resolve_alias;
use cadcraft_ui_egui::aliasdlg::Form;
use cadcraft_ui_egui::{CadApp, Services, dialogs, theme};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn harness(app: CadApp, render: bool) -> Harness<'static, CadApp> {
    let builder = Harness::builder().with_size(egui::vec2(900.0, 700.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h
}

fn opened() -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.cmdline("aliasedit");
    app
}

fn fill(h: &mut Harness<'_, CadApp>, alias: &str, command: &str) {
    let form = h.state_mut().alias_dlg.as_mut().and_then(|s| s.form.as_mut()).expect("the alias form");
    form.alias = alias.into();
    form.command = command.into();
    h.run_steps(2);
    h.get_by_label("Save alias").click();
    h.run_steps(2);
}

fn user(h: &Harness<'_, CadApp>) -> Vec<(String, String)> {
    h.state().alias_dlg.as_ref().map(|s| s.user.clone().into_iter().collect()).unwrap_or_default()
}

#[test]
fn typed_and_menu_aliasedit_open_the_dialog_but_json_does_not() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = CadApp::new(Session::new(), Services::default());
    for typed in ["aliasedit", "ALIASEDIT"] {
        app.ui.dialog = None;
        app.cmdline(typed);
        assert_eq!(app.ui.dialog.as_deref(), Some("aliasedit"), "{typed}");
    }
    app.ui.dialog = None;
    app.start("aliasedit");
    assert_eq!(app.ui.dialog.as_deref(), Some("aliasedit"), "menu");
    app.ui.dialog = None;
    let listing = app.run("aliasedit", json!({})).unwrap();
    assert!(listing.get("aliases").is_some());
    app.cmdline("aliasedit {}");
    assert_eq!(app.ui.dialog, None);
}

#[test]
fn add_then_ok_defines_the_alias_and_cancel_drops_it() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = harness(opened(), false);
    h.get_by_label("Add…").click();
    h.run_steps(2);
    fill(&mut h, "ZZAQ", "qsave");
    assert!(user(&h).contains(&("zzaq".into(), "qsave".into())));
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.dialog, None);
    assert_eq!(resolve_alias("zzaq"), "zzaq", "Cancel applies nothing");

    h.state_mut().cmdline("aliasedit");
    h.run_steps(2);
    h.get_by_label("Add…").click();
    h.run_steps(2);
    fill(&mut h, "zzaq", "QSAVE");
    h.get_by_label("Apply").click();
    h.run_steps(2);
    assert_eq!(resolve_alias("zzaq"), "qsave", "Apply applies and keeps the dialog");
    assert_eq!(h.state().ui.dialog.as_deref(), Some("aliasedit"));
    h.state_mut().alias_dlg.as_mut().unwrap().selected = Some("zzaq".into());
    h.run_steps(2);
    h.get_by_label("Edit…").click();
    h.run_steps(2);
    fill(&mut h, "zzaq", "circle");
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.dialog, None);
    assert_eq!(resolve_alias("zzaq"), "circle");
    h.state_mut().run("aliasedit", json!({ "remove": ["zzaq"] })).unwrap();
}

#[test]
fn aliasedit_typed_while_open_keeps_the_changes_not_applied() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = harness(opened(), false);
    h.get_by_label("Add…").click();
    h.run_steps(2);
    fill(&mut h, "zzkeep", "line");
    h.state_mut().cmdline("aliasedit");
    h.run_steps(2);
    assert!(user(&h).contains(&("zzkeep".into(), "line".into())));
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(resolve_alias("zzkeep"), "zzkeep");
}

#[test]
fn remove_is_offered_for_user_aliases_only() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("aliasedit", json!({ "set": { "zzrm": "move" } })).unwrap();
    app.cmdline("aliasedit");
    let mut h = harness(app, false);
    // L is built in (LINE): the dialog lists it but can't remove it.
    h.state_mut().alias_dlg.as_mut().unwrap().selected = Some("l".into());
    h.run_steps(2);
    assert!(h.get_by_label("Remove").accesskit_node().is_disabled(), "Remove on a built-in alias");
    h.state_mut().alias_dlg.as_mut().unwrap().selected = Some("zzrm".into());
    h.run_steps(2);
    assert!(!h.get_by_label("Remove").accesskit_node().is_disabled());
    // A selected row the filter hides is not edited or removed.
    h.state_mut().alias_dlg.as_mut().unwrap().filter = "qsave".into();
    h.run_steps(2);
    assert!(h.get_by_label("Remove").accesskit_node().is_disabled(), "Remove on a hidden row");
    assert!(h.get_by_label("Edit…").accesskit_node().is_disabled(), "Edit on a hidden row");
    h.state_mut().alias_dlg.as_mut().unwrap().filter.clear();
    h.run_steps(2);
    h.get_by_label("Remove").click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(resolve_alias("zzrm"), "zzrm");
    assert_eq!(resolve_alias("l"), "line");
}

#[test]
fn edit_on_a_built_in_alias_overrides_it() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = harness(opened(), false);
    h.state_mut().alias_dlg.as_mut().unwrap().selected = Some("l".into());
    h.run_steps(2);
    h.get_by_label("Edit…").click();
    h.run_steps(2);
    let form = h.state().alias_dlg.as_ref().unwrap().form.clone().unwrap();
    assert_eq!((form.alias.as_str(), form.command.as_str()), ("L", "LINE"));
    fill(&mut h, "L", "pline");
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert_eq!(resolve_alias("l"), "pline");
    h.state_mut().run("aliasedit", json!({ "remove": ["l"] })).unwrap();
    assert_eq!(resolve_alias("l"), "line");
}

#[test]
fn refused_aliases_stay_out_of_the_list_and_say_why() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = harness(opened(), false);
    h.get_by_label("Add…").click();
    h.run_steps(2);
    fill(&mut h, "zzdup", "line");
    let before = user(&h);
    for (alias, command, why) in [
        ("", "line", "Type an alias"),
        ("  ", "line", "Type an alias"),
        ("LINE", "circle", "is a command name"),
        ("zz q", "line", "can't be an alias"),
        ("zz{", "line", "can't be an alias"),
        ("zzdup", "circle", "already defined"),
        ("zznew", "nosuchcommand", "unknown command"),
        ("zznew", "", "Choose the command"),
    ] {
        h.state_mut().alias_dlg.as_mut().unwrap().form = Some(Form::default());
        fill(&mut h, alias, command);
        assert_eq!(user(&h), before, "{alias:?} → {command:?}");
        assert!(h.state().alias_dlg.as_ref().unwrap().form.is_some(), "the form stays open for {alias:?}");
        assert!(h.query_by_label_contains(why).is_some(), "{alias:?} → {command:?}: no message containing {why:?}");
    }
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(resolve_alias("zzdup"), "zzdup");
}

#[test]
fn the_dialog_names_the_alias_file_reinit_reads() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("cadcraft-aliasdlg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mine.pgp");
    std::fs::write(&file, "ZZFC, *CIRCLE\n").unwrap();
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("aliasedit", json!({ "file": file.to_string_lossy() })).unwrap();
    app.cmdline("aliasedit");
    let directory = std::env::var_os("CADCRAFT_ALIAS_SHOTS").map(std::path::PathBuf::from);
    let mut h = harness(app, directory.is_some());
    assert!(h.query_by_label_contains("mine.pgp").is_some(), "the loaded file is shown");
    assert!(user(&h).contains(&("zzfc".into(), "circle".into())));
    if let Some(directory) = directory {
        let st = h.state_mut().alias_dlg.as_mut().unwrap();
        st.selected = Some("zzfc".into());
        st.filter = "circle".into();
        h.run_steps(3);
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(directory.join("cadcraft-aliasedit.png")).unwrap();
    }
    h.state_mut().run("aliasedit", json!({ "clear": true })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
