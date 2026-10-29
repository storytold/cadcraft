//! The style and settings dialogs (#126): typed or menu-invoked commands open them, JSON calls run
//! the command without one.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus, theme};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
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

    // The Dimension Style Manager draws over the changed style without panicking.
    let mut out = ctx.run_ui(Default::default(), |ui| dialogs::show(&mut app, ui.ctx()));
    out.textures_delta.clear();
    assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"));
}

#[test]
fn text_style_dialog_exposes_font_picker_and_updates_the_style_font() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.dialog = Some("textstyle".into());
    let mut h = Harness::builder().with_size(egui::vec2(800.0, 600.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            theme::apply(ui.ctx(), egui::Theme::Dark, false);
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(4);
    assert!(h.unnamed_widgets().is_empty(), "unnamed widgets: {:?}", h.unnamed_widgets());

    let combo = h.get_by_role_and_label(Role::ComboBox, "Font");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("CADCraft Stroke"));
    combo.click();
    h.run_steps(2);
    if let Some(dir) = std::env::var_os("CADCRAFT_STYLE_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join("text-style-font-open.png")).unwrap();
    }
    h.get_by_label("Arial").click();
    h.run_steps(2);
    if let Some(dir) = std::env::var_os("CADCRAFT_STYLE_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        h.render().unwrap().save(dir.join("text-style-font-picked.png")).unwrap();
    }

    let d = h.state().session.doc().unwrap();
    assert_eq!(d.text_style("Standard").unwrap().font, "Arial");
    let combo = h.get_by_role_and_label(Role::ComboBox, "Font");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("Arial"));

    // A style with an SHX or custom font not in the system font list still shows its font name.
    h.state_mut().run("style", json!({ "name": "Standard", "font": "romans.shx" })).unwrap();
    h.run_steps(2);
    let combo = h.get_by_role_and_label(Role::ComboBox, "Font");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("romans.shx"));
    assert_eq!(h.state().session.doc().unwrap().text_style("Standard").unwrap().font, "romans.shx");

    // Hostile or unmapped characters in a drawing's style font fall back cleanly without panicking.
    h.state_mut().run("style", json!({ "name": "Standard", "font": "bad\u{E000}font" })).unwrap();
    h.run_steps(2);
    let combo = h.get_by_role_and_label(Role::ComboBox, "Font");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("CADCraft Stroke"));
}
