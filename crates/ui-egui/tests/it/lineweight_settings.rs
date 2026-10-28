//! Lineweight Settings: LWEIGHT typed or chosen from Format ▸ Lineweight... opens it (a JSON call
//! never does), and so does Options ▸ User Preferences ▸ Lineweight Settings...; it sets the
//! current lineweight, LWUNITS, LWDISPLAY and LWDEFAULT, and LWUNITS lists lineweights in inches
//! across the interface.

use cadcraft_doc::Lineweight;
use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, lwsettings, menus};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

fn celweight(a: &CadApp) -> i64 {
    a.session.doc().unwrap().header.i64("CELWEIGHT", -1)
}

#[test]
fn typed_and_menu_lweight_open_the_window_json_never_does() {
    let mut a = app();
    for typed in ["LWEIGHT", "lw", "Lineweight"] {
        a.ui.dialog = None;
        a.cmdline(typed);
        assert_eq!(a.ui.dialog.as_deref(), Some(lwsettings::DIALOG), "{typed}");
    }
    a.ui.dialog = None;
    menus::activate(&mut a, "lweight");
    assert_eq!(a.ui.dialog.as_deref(), Some(lwsettings::DIALOG), "Format ▸ Lineweight...");

    a.ui.dialog = None;
    a.run("lweight", json!({"lineweight": 0.5})).unwrap();
    assert_eq!(celweight(&a), 50);
    a.cmdline(r#"lweight {"lineweight": "ByBlock"}"#);
    assert_eq!(celweight(&a), -2);
    a.run("options", json!({"LWUNITS": 0})).unwrap();
    assert!(a.ui.dialog.is_none(), "JSON calls never open dialogs");
    assert!(!a.session.settings.lwunits_mm);
}

#[test]
fn lineweights_are_listed_in_millimetres_or_inches() {
    assert_eq!(lwsettings::name(Lineweight::Mm100(25), true), "0.25 mm");
    assert_eq!(lwsettings::name(Lineweight::Mm100(25), false), "0.010\"");
    assert_eq!(lwsettings::name(Lineweight::Mm100(211), false), "0.083\"");
    assert_eq!(lwsettings::name(Lineweight::Mm100(0), false), "0.000\"");
    // A hostile value (read from a file) is listed as the widest lineweight, never panics.
    assert_eq!(lwsettings::name(Lineweight::Mm100(u16::MAX), false), "0.083\"");
    for lw in [Lineweight::ByLayer, Lineweight::ByBlock, Lineweight::Default] {
        assert_eq!(lwsettings::name(lw, false), lw.name());
    }
}

/// With `CADCRAFT_OPTIONS_SHOTS` set to a folder, the window is rendered there for a look.
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

fn shot(h: &mut Harness<'static, CadApp>, name: &str) {
    if let Some(dir) = std::env::var_os(SHOTS).map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}

#[test]
fn the_window_sets_the_current_lineweight_units_and_display() {
    let mut a = app();
    a.cmdline("LWEIGHT");
    let mut h = harness(a);
    h.get_by_label("Units for Listing");
    shot(&mut h, "lineweight-settings.png");

    h.get_by_label("0.13 mm").click();
    h.run_steps(3);
    assert_eq!(celweight(h.state()), 13, "a click makes a lineweight current");
    h.get_by_label("ByBlock").click();
    h.run_steps(3);
    assert_eq!(celweight(h.state()), -2);

    h.get_by_label("Display Lineweight").click();
    h.run_steps(3);
    assert!(h.state().session.settings.lwdisplay);

    h.get_by_label("Inches (in)").click();
    h.run_steps(3);
    assert!(!h.state().session.settings.lwunits_mm);
    h.get_by_label("0.005\"");
    assert!(h.query_by_label("0.13 mm").is_none(), "listed in inches");

    // The Default lineweight is the drawing's (LWDEFAULT): pick it from its list.
    h.get_by_label("Default lineweight:").click();
    h.run_steps(3);
    h.get_all_by_label("0.005\"").last().unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().header.i64("LWDEFAULT", 25), 13);
    h.state_mut().session.undo().unwrap();
    assert_eq!(h.state().session.doc().unwrap().header.i64("LWDEFAULT", 25), 25, "LWDEFAULT is the drawing's: undoable");
    h.run_steps(2);

    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed controls: {unnamed:?}");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.state().ui.dialog.is_none());

    // The Properties palette lists the current lineweight in inches too.
    h.state_mut().run("lweight", json!({"lineweight": 0.25})).unwrap();
    h.run_steps(3);
    h.get_by_label("0.010\"");
}

#[test]
fn options_user_preferences_opens_lineweight_settings_over_the_dialog() {
    let mut a = app();
    menus::activate(&mut a, "options");
    a.ui.dialog = Some("options:preferences".into());
    let mut h = harness(a);
    assert!(h.query_by_label("Units for Listing").is_none());
    h.get_by_label("Lineweight Settings...").click();
    h.run_steps(3);
    h.get_by_label("Units for Listing");
    assert_eq!(h.state().ui.dialog.as_deref(), Some("options:preferences"), "the Options dialog stays open");
    h.get_by_label("Inches (in)").click();
    h.run_steps(3);
    assert!(!h.state().session.settings.lwunits_mm);
    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed controls: {unnamed:?}");

    // The window's Close comes after the Options dialog's in the tree.
    h.get_all_by_label("Close").last().unwrap().click();
    h.run_steps(3);
    assert!(h.query_by_label("Units for Listing").is_none());
    assert_eq!(h.state().ui.dialog.as_deref(), Some("options:preferences"));
}
