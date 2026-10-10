//! The style and settings dialogs (#126): typed or menu-invoked commands open them, JSON calls run
//! the command without one.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use serde_json::json;

#[test]
fn menu_items_open_their_dialogs() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    for (cmd, dialog) in [
        ("pagesetup", "pagesetup"),
        ("linetype", "linetype"),
        ("tablestyle", "tablestyle"),
        ("mleaderstyle", "mleaderstyle"),
        ("ddptype", "ptype"),
        ("constraintsettings", "csettings"),
    ] {
        app.ui.dialog = None;
        menus::activate(&mut app, cmd);
        assert_eq!(app.ui.dialog.as_deref(), Some(dialog), "{cmd}");
        let mut out = ctx.run_ui(Default::default(), |ui| dialogs::show(&mut app, ui.ctx()));
        out.textures_delta.clear();
        assert_eq!(app.ui.dialog.as_deref(), Some(dialog), "{cmd} draws and stays open");
    }
    app.ui.dialog = None;
    app.cmdline("LT");
    assert_eq!(app.ui.dialog.as_deref(), Some("linetype"), "typed aliases open it too");
}

#[test]
fn json_calls_never_open_the_dialogs() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("ddptype", json!({ "pdmode": 34, "pdsize": -5 })).unwrap();
    app.run("linetype", json!({ "load": "DASHED", "current": "DASHED" })).unwrap();
    app.run("tablestyle", json!({ "name": "Schedule", "current": false })).unwrap();
    assert!(app.ui.dialog.is_none());
    let d = app.session.doc().unwrap();
    assert_eq!((d.header.i64("PDMODE", 0), d.header.f64("PDSIZE", 0.0)), (34, -5.0));
    assert_eq!(d.header.str("CELTYPE", ""), "DASHED");
    assert!(d.table_styles.iter().any(|s| s.name == "Schedule"));
}
