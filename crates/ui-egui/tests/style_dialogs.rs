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
        ("style", "textstyle"),
        ("dimstyle", "dimstyle"),
        ("dimstyle.dimension", "dimstyle"),
        ("tablestyle", "tablestyle"),
        ("mleaderstyle", "mleaderstyle"),
        ("ddptype", "ptype"),
        ("constraintsettings", "csettings"),
        ("plot", "plot"),
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
    app.ui.dialog = None;
    assert!(app.cmdline("PTYPE").is_none(), "bare PTYPE is not an error");
    assert_eq!(app.ui.dialog.as_deref(), Some("ptype"), "PTYPE opens the Point Style dialog");
    for typed in ["style", "ST"] {
        app.ui.dialog = None;
        assert!(app.cmdline(typed).is_none(), "bare {typed} is not an error");
        assert_eq!(app.ui.dialog.as_deref(), Some("textstyle"), "{typed}");
    }
    for typed in ["dimstyle", "D", "DST", "DDIM"] {
        app.ui.dialog = None;
        assert!(app.cmdline(typed).is_none(), "bare {typed} is not an error");
        assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"), "{typed}");
    }
}

#[test]
fn json_calls_never_open_the_dialogs() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("ddptype", json!({ "pdmode": 34, "pdsize": -5 })).unwrap();
    app.run("linetype", json!({ "load": "DASHED", "current": "DASHED" })).unwrap();
    app.run("tablestyle", json!({ "name": "Schedule", "current": false })).unwrap();
    app.run("style", json!({ "name": "SCHEM", "font": "Arial" })).unwrap();
    app.run("dimstyle", json!({ "name": "Architectural", "scale": 2.5 })).unwrap();
    assert!(app.session.doc().unwrap().text_styles.iter().any(|s| s.name == "SCHEM"));
    assert!(app.session.doc().unwrap().dim_styles.iter().any(|s| s.name == "Architectural" && s.scale == 2.5));
    assert!(app.ui.dialog.is_none());
    let d = app.session.doc().unwrap();
    assert_eq!((d.header.i64("PDMODE", 0), d.header.f64("PDSIZE", 0.0)), (34, -5.0));
    assert_eq!(d.header.str("CELTYPE", ""), "DASHED");
    assert!(d.table_styles.iter().any(|s| s.name == "Schedule"));
}

#[test]
fn dimstyle_dialog_renders_and_preserves_state() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    app.cmdline("D");
    assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"));

    let mut out = ctx.run_ui(Default::default(), |ui| dialogs::show(&mut app, ui.ctx()));
    out.textures_delta.clear();
    assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"));

    let d = app.session.doc().unwrap();
    assert!(d.dim_styles.iter().any(|s| s.name.eq_ignore_ascii_case("Standard")));
}

#[test]
fn dimstyle_refuses_invalid_values_with_diagnostic() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let r = app.run("dimstyle", json!({ "name": "Standard", "textHeight": 2.5, "decimals": 4, "current": false })).unwrap();
    assert!(r.get("ignored").is_none());

    let d = app.session.doc().unwrap();
    let st = d.dim_style("Standard").unwrap();
    assert_eq!(st.text_height, 2.5);
    assert_eq!(st.decimals, 4);

    // Text height 0, precision 200, and negative precision -1 must be refused.
    let r = app.run("dimstyle", json!({ "name": "Standard", "textHeight": 0.0, "decimals": 200, "current": false })).unwrap();
    let ignored = r["ignored"].as_array().expect("ignored list");
    assert!(ignored.iter().any(|v| v == "textHeight"));
    assert!(ignored.iter().any(|v| v == "decimals"));
    assert!(r["message"].as_str().unwrap().contains("Ignored invalid value"));

    let r = app.run("dimstyle", json!({ "name": "Standard", "decimals": -1, "current": false })).unwrap();
    let ignored = r["ignored"].as_array().expect("ignored list");
    assert!(ignored.iter().any(|v| v == "decimals"));

    // Style retains valid values.
    let d = app.session.doc().unwrap();
    let st = d.dim_style("Standard").unwrap();
    assert_eq!(st.text_height, 2.5);
    assert_eq!(st.decimals, 4);
}

#[test]
fn dimstyle_dialog_action_updates_style_properties() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    menus::activate(&mut app, "dimstyle");
    assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"));

    // Execute field update through the action mechanism.
    app.run("dimstyle", json!({ "name": "Standard", "arrowSize": 3.5, "extExtend": 1.75, "current": false })).unwrap();
    let d = app.session.doc().unwrap();
    let st = d.dim_style("Standard").unwrap();
    assert_eq!(st.arrow_size, 3.5);
    assert_eq!(st.ext_extend, 1.75);

    // Rendering frame with error present in context data displays warning without panicking.
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("dimstyle_err"), "Text height must be greater than 0".to_string()));
    let mut out = ctx.run_ui(Default::default(), |ui| dialogs::show(&mut app, ui.ctx()));
    out.textures_delta.clear();
    assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"));

    // Verify error message stored as Option<String> is consumed and carried forward into subsequent frames.
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("dimstyle_err"), Some("Text height must be greater than 0".to_string())));
    // Frame 1: dialog reads the stored Option<String> diagnostic and keeps it active.
    let mut out1 = ctx.run_ui(Default::default(), |ui| dialogs::show(&mut app, ui.ctx()));
    out1.textures_delta.clear();
    let err_after_f1: Option<Option<String>> = ctx.data_mut(|d| d.get_temp(egui::Id::new("dimstyle_err")));
    assert_eq!(err_after_f1, Some(Some("Text height must be greater than 0".to_string())));

    // Frame 2: subsequent render without re-inserting proves the dialog preserved the message across frame boundaries.
    let mut out2 = ctx.run_ui(Default::default(), |ui| dialogs::show(&mut app, ui.ctx()));
    out2.textures_delta.clear();
    let err_after_f2: Option<Option<String>> = ctx.data_mut(|d| d.get_temp(egui::Id::new("dimstyle_err")));
    assert_eq!(err_after_f2, Some(Some("Text height must be greater than 0".to_string())));
}
