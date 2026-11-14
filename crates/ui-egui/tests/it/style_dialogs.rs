//! The style and settings dialogs (#126): typed or menu-invoked commands open them, JSON calls run
//! the command without one.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus, theme};
use egui::accesskit::{Action, ActionRequest, Role};
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
}

#[test]
fn json_calls_never_open_the_dialogs() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("ddptype", json!({ "pdmode": 34, "pdsize": -5 })).unwrap();
    app.run("linetype", json!({ "load": "DASHED", "current": "DASHED" })).unwrap();
    app.run("tablestyle", json!({ "name": "Schedule", "current": false })).unwrap();
    app.run("style", json!({ "name": "SCHEM", "font": "Arial" })).unwrap();
    assert!(app.session.doc().unwrap().text_styles.iter().any(|s| s.name == "SCHEM"));
    assert!(app.ui.dialog.is_none());
    let d = app.session.doc().unwrap();
    assert_eq!((d.header.i64("PDMODE", 0), d.header.f64("PDSIZE", 0.0)), (34, -5.0));
    assert_eq!(d.header.str("CELTYPE", ""), "DASHED");
    assert!(d.table_styles.iter().any(|s| s.name == "Schedule"));
}

fn sfnt_with_names(family: &str, subfamily: &str) -> Vec<u8> {
    let fam = family.as_bytes();
    let sub = subfamily.as_bytes();
    let mut name_tbl = Vec::new();
    name_tbl.extend_from_slice(&0u16.to_be_bytes());
    name_tbl.extend_from_slice(&2u16.to_be_bytes());
    name_tbl.extend_from_slice(&30u16.to_be_bytes());
    for (name_id, len, off) in [(1u16, fam.len() as u16, 0u16), (2u16, sub.len() as u16, fam.len() as u16)] {
        name_tbl.extend_from_slice(&1u16.to_be_bytes());
        name_tbl.extend_from_slice(&0u16.to_be_bytes());
        name_tbl.extend_from_slice(&0u16.to_be_bytes());
        name_tbl.extend_from_slice(&name_id.to_be_bytes());
        name_tbl.extend_from_slice(&len.to_be_bytes());
        name_tbl.extend_from_slice(&off.to_be_bytes());
    }
    name_tbl.extend_from_slice(fam);
    name_tbl.extend_from_slice(sub);

    let mut sfnt = vec![0, 1, 0, 0, 0, 1, 0, 16, 0, 0, 0, 0];
    sfnt.extend_from_slice(b"name");
    sfnt.extend_from_slice(&0u32.to_be_bytes());
    sfnt.extend_from_slice(&28u32.to_be_bytes());
    sfnt.extend_from_slice(&(name_tbl.len() as u32).to_be_bytes());
    sfnt.extend_from_slice(&name_tbl);
    sfnt
}

#[test]
fn text_style_dialog_exposes_font_picker_and_updates_the_style_font() {
    let test_family = "00 CADCraft Test";
    cadcraft_fonts::ttf::register("ccpickertest-r.ttf", sfnt_with_names(test_family, "Regular"));
    cadcraft_fonts::ttf::register("ccpickertest-b.ttf", sfnt_with_names(test_family, "Bold"));

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

    let combo = h.get_by_role_and_label(Role::ComboBox, "Font Name:");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("CADCraft Stroke"));
    combo.click();
    h.run_steps(2);
    if let Some(dir) = std::env::var_os("CADCRAFT_STYLE_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join("text-style-font-open.png")).unwrap();
    }
    let (target_node, target_tree) = h.get_by_label(test_family).accesskit_node().locate();
    for action in [Action::ScrollIntoView, Action::Click] {
        h.event(egui::Event::AccessKitActionRequest(ActionRequest { action, target_node, target_tree, data: None }));
        h.run_steps(2);
    }
    if let Some(dir) = std::env::var_os("CADCRAFT_STYLE_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        h.render().unwrap().save(dir.join("text-style-font-picked.png")).unwrap();
    }

    let d = h.state().session.doc().unwrap();
    assert_eq!(d.text_style("Standard").unwrap().font, "ccpickertest-r.ttf");
    let combo = h.get_by_role_and_label(Role::ComboBox, "Font Name:");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some(test_family));

    // Selecting a Font Style updates the style's font face file name.
    let style_combo = h.get_by_role_and_label(Role::ComboBox, "Font Style:");
    assert_eq!(style_combo.accesskit_node().value().as_deref(), Some("Regular"));
    style_combo.click();
    h.run_steps(2);
    h.get_by_label("Bold").click();
    h.run_steps(2);
    assert_eq!(h.state().session.doc().unwrap().text_style("Standard").unwrap().font, "ccpickertest-b.ttf");
    let style_combo = h.get_by_role_and_label(Role::ComboBox, "Font Style:");
    assert_eq!(style_combo.accesskit_node().value().as_deref(), Some("Bold"));

    // Effects and Size checkboxes update the TextStyle.
    h.get_by_role_and_label(Role::CheckBox, "Annotative").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Upside down").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Backwards").click();
    h.run_steps(2);
    let st = h.state().session.doc().unwrap().text_style("Standard").unwrap().clone();
    assert!(st.annotative && st.upside_down && st.backwards);

    // A style with an SHX or custom font not in the system font list still shows its font name.
    h.state_mut().run("style", json!({ "name": "Standard", "font": "romans.shx" })).unwrap();
    h.run_steps(2);
    let combo = h.get_by_role_and_label(Role::ComboBox, "Font Name:");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("romans.shx"));
    assert_eq!(h.state().session.doc().unwrap().text_style("Standard").unwrap().font, "romans.shx");

    // Hostile or unmapped characters in a drawing's style font fall back cleanly without panicking.
    h.state_mut().run("style", json!({ "name": "Standard", "font": "bad\u{E000}font" })).unwrap();
    h.run_steps(2);
    let combo = h.get_by_role_and_label(Role::ComboBox, "Font Name:");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("CADCraft Stroke"));

    // Edits outside the dialog while it stays open invalidate the (uid, revision) in-use cache:
    // a newly created unused style can be deleted until an MTEXT entity references it.
    h.state_mut().run("style", json!({ "name": "Notes", "font": "ccpickertest-r.ttf", "current": false })).unwrap();
    h.run_steps(2);
    h.get_by_label("Notes").click();
    h.run_steps(2);
    assert!(!h.get_by_role_and_label(Role::Button, "Delete").accesskit_node().is_disabled());
    h.state_mut().run("mtext", json!({ "at": [0.0, 0.0], "height": 2.5, "text": "Note", "style": "Notes" })).unwrap();
    h.run_steps(2);
    assert!(h.get_by_role_and_label(Role::Button, "Delete").accesskit_node().is_disabled());

    // Switching drawings with the same number of styles uses the active drawing's (uid, revision):
    // a newly created "Notes" in the second drawing is not in use even though it was in the first.
    h.state_mut().session.new_drawing(false);
    h.state_mut().run("style", json!({ "name": "Notes", "font": "ccpickertest-r.ttf", "current": false })).unwrap();
    h.run_steps(2);
    h.get_by_label("Notes").click();
    h.run_steps(2);
    assert!(!h.get_by_role_and_label(Role::Button, "Delete").accesskit_node().is_disabled());
}
