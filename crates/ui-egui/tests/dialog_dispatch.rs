//! Which calls open dialogs and file pickers: typed and menu-invoked commands do, JSON calls never.

use std::cell::Cell;
use std::rc::Rc;

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, menus};
use serde_json::json;

/// An app whose file pickers count their calls and cancel.
fn app() -> (CadApp, Rc<Cell<u32>>) {
    let picks = Rc::new(Cell::new(0));
    let (a, b) = (picks.clone(), picks.clone());
    let services = Services {
        pick_open: Some(Box::new(move || {
            a.set(a.get() + 1);
            None
        })),
        pick_save: Some(Box::new(move |_| {
            b.set(b.get() + 1);
            None
        })),
    };
    (CadApp::new(Session::new(), services), picks)
}

#[test]
fn drafting_settings_menu_opens_the_dialog() {
    let (mut app, _) = app();
    menus::activate(&mut app, "dsettings");
    assert_eq!(app.ui.dialog.as_deref(), Some("dsettings"));
    app.ui.dialog = None;
    app.cmdline("DS");
    assert_eq!(app.ui.dialog.as_deref(), Some("dsettings"));
    app.ui.dialog = None;
    app.run("dsettings", json!({ "gridmajor": 10 })).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.session.settings.gridmajor, 10);
}

#[test]
fn properties_menu_shows_the_palette() {
    let (mut app, _) = app();
    app.ui.show_palettes = false;
    app.run("properties", json!({})).unwrap();
    assert!(!app.ui.show_palettes, "JSON calls only read properties");
    menus::activate(&mut app, "properties");
    assert!(app.ui.show_palettes);
}

#[test]
fn json_file_commands_never_open_pickers() {
    let (mut app, picks) = app();
    for c in ["open", "saveas", "qsave"] {
        assert!(app.run(c, json!({})).is_err(), "{c} {{}} reports the missing path");
    }
    app.cmdline("open {}");
    assert_eq!(picks.get(), 0);
    // Typed, chosen from the menu or the ui.* shortcut commands: the pickers.
    for c in ["open", "saveas", "qsave"] {
        app.start(c);
    }
    assert_eq!(picks.get(), 3);
    app.run("ui.open", json!({})).unwrap();
    app.run("ui.saveas", json!({})).unwrap();
    assert_eq!(picks.get(), 5);
}

#[test]
fn typed_json_form_runs_the_command() {
    let (mut app, _) = app();
    app.cmdline("LAYER");
    assert_eq!(app.ui.dialog.as_deref(), Some("layers"));
    app.ui.dialog = None;
    let n = app.session.log.len();
    app.cmdline("layer {}");
    assert!(app.ui.dialog.is_none());
    assert!(app.session.log[n..].iter().any(|l| l.contains("\"layers\"")), "the layer list is echoed");
    app.cmdline(r#"dsettings {"gridmajor": 7}"#);
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.session.settings.gridmajor, 7);
}
