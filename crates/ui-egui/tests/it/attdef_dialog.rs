//! The Attribute Definition dialog: ATTDEF typed opens it; OK with a typed point makes the
//! definition; "Specify on-screen" starts -ATTDEF asking only for the start point; -ATTDEF keeps
//! the command line; the Properties palette lists a definition's rows.

use cadcraft_engine::doc::{Attrib, EntityKind, HAlign, VAlign};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services, attdefdlg, menus};
use serde_json::json;

fn attdefs(app: &CadApp) -> Vec<Attrib> {
    app.session
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(a) => Some(a.clone()),
            _ => None,
        })
        .collect()
}

/// The text painted in one frame.
fn painted(app: &mut CadApp, ctx: &egui::Context) -> String {
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(s) => {
                text.push_str(s.galley.text());
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let mut text = String::new();
    for s in &output.shapes {
        collect(&s.shape, &mut text);
    }
    text
}

#[test]
fn typed_attdef_opens_the_dialog_and_ok_makes_the_definition() {
    let mut app = CadApp::new(Session::new(), Services::default());
    menus::activate(&mut app, "attdef");
    assert_eq!(app.ui.dialog.as_deref(), Some("attdef"));
    assert!(app.session.running.is_none(), "the dialog, not the command-line prompts");
    assert!(app.attdef.on_screen && app.attdef.justification == "Left" && app.attdef.style == "Standard");

    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    for s in [
        "Attribute Definition",
        "Mode",
        "Invisible",
        "Lock position",
        "Tag",
        "Prompt",
        "Default",
        "Insertion Point",
        "Text Settings",
        "Justification",
        "Text style",
        "OK",
    ] {
        assert!(text.contains(s), "{s} missing from:\n{text}");
    }

    let st = &mut app.attdef;
    st.tag = "no".into();
    st.prompt = "Number".into();
    st.default = "1".into();
    st.on_screen = false;
    st.x = "10".into();
    st.y = "20".into();
    st.justification = "Middle center".into();
    st.height = "2.5".into();
    st.rotation = "90".into();
    st.invisible = true;
    st.lock_position = true;
    assert!(attdefdlg::confirm(&mut app), "{:?}", app.attdef.error);
    let a = &attdefs(&app)[0];
    assert_eq!((a.tag.as_str(), a.prompt.as_str(), a.text.value.as_str()), ("NO", "Number", "1"));
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(a.text.align_pt.map(|p| (p.x, p.y)), Some((10.0, 20.0)));
    assert_eq!(a.text.height, 2.5);
    assert!((a.text.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert!(a.invisible && a.modes.lock_position && !a.constant && !a.modes.verify);

    // A tag with a space and a bad height are refused; the dialog stays with the reason.
    app.attdef.tag = "two words".into();
    assert!(!attdefdlg::confirm(&mut app));
    assert!(app.attdef.error.as_deref().is_some_and(|e| e.contains("tag")));
    app.attdef.tag = "ok".into();
    app.attdef.height = "big".into();
    assert!(!attdefdlg::confirm(&mut app));
    assert!(app.attdef.error.as_deref().is_some_and(|e| e.contains("height")), "{:?}", app.attdef.error);
    assert_eq!(attdefs(&app).len(), 1);
}

#[test]
fn specify_on_screen_starts_attdef_asking_only_for_the_point() {
    let mut app = CadApp::new(Session::new(), Services::default());
    attdefdlg::open(&mut app);
    let st = &mut app.attdef;
    st.tag = "A".into();
    st.default = "x".into();
    st.justification = "Top right".into();
    st.height = "3".into();
    st.constant = true;
    assert!(attdefdlg::confirm(&mut app), "{:?}", app.attdef.error);
    assert!(app.session.running.as_ref().is_some_and(|r| r.id == "attdef"));
    assert!(app.session.prompt_text().contains("start point"), "{}", app.session.prompt_text());
    assert!(app.session.attdef_answers.is_none(), "taken by the command");
    app.session.input(Input::Point(Vec2::new(4.0, 5.0))).unwrap();
    assert!(app.session.running.is_none(), "nothing more is asked: {}", app.session.prompt_text());
    let a = &attdefs(&app)[0];
    assert_eq!((a.tag.as_str(), a.text.value.as_str(), a.constant), ("A", "x", true));
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Right, VAlign::Top));
    assert_eq!(a.text.align_pt.map(|p| (p.x, p.y)), Some((4.0, 5.0)));
    assert_eq!(a.text.height, 3.0);

    // Align below the previous definition: no point, the previous one's text settings.
    attdefdlg::open(&mut app);
    app.attdef.tag = "B".into();
    app.attdef.align_below = true;
    assert!(attdefdlg::confirm(&mut app), "{:?}", app.attdef.error);
    assert!(app.session.running.is_none());
    let b = &attdefs(&app)[1];
    assert_eq!((b.text.halign, b.text.valign, b.text.height), (HAlign::Right, VAlign::Top, 3.0));
    let at = b.text.justify_point();
    assert!((at.x - 4.0).abs() < 1e-9 && (at.y - 0.0).abs() < 1e-9, "{at:?}: one line (5/3 of the height) below");
}

#[test]
fn dash_attdef_keeps_the_command_line() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.cmdline("-attdef");
    assert!(app.ui.dialog.is_none());
    assert!(app.session.running.as_ref().is_some_and(|r| r.id == "attdef"));
    assert!(app.session.prompt_text().contains("attribute modes"), "{}", app.session.prompt_text());
    app.session.cancel();

    // JSON calls never open the dialog.
    app.run("attdef", json!({ "tag": "J", "at": [0, 0] })).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(attdefs(&app).len(), 1);
}

#[test]
fn the_properties_palette_lists_a_definitions_rows() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let r = app.run("attdef", json!({ "tag": "NO", "prompt": "Number", "default": "1", "at": [0, 0], "justify": "TR" })).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.set_selection(vec![h]);
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    // (Drop-down and field contents are not painted shapes here, so only the row labels are checked.)
    for s in [
        "Tag",
        "Prompt",
        "Default",
        "Text style",
        "Justify",
        "Height",
        "Rotation",
        "Width factor",
        "Invisible",
        "Constant",
        "Verify",
        "Preset",
        "Lock position",
    ] {
        assert!(text.lines().any(|l| l == s), "{s} missing from:\n{text}");
    }
}
