//! Block attributes from the UI: Edit Attributes on a selected block reference opens the Enhanced
//! Attribute Editor, and the Properties palette lists the attributes in their own group.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_ui_egui::{CadApp, Services, attedit, context_menu, menus};
use serde_json::json;

/// A block "Tag" with attributes NO (prompt "Number") and NOTE, and one reference to it.
fn app_with_reference() -> (CadApp, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let h = add_reference(&mut app);
    (app, h)
}

/// Block "Tag" and its reference, as [`app_with_reference`], in the active drawing (selected).
fn add_reference(app: &mut CadApp) -> Handle {
    let mut hs = vec![app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone()];
    hs.push(app.run("attdef", json!({ "tag": "NO", "prompt": "Number", "default": "1", "at": [0, 1] })).unwrap()["handle"].clone());
    hs.push(app.run("attdef", json!({ "tag": "NOTE", "default": "n", "at": [0, 2] })).unwrap()["handle"].clone());
    app.run("block", json!({ "name": "Tag", "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
    let r = app.run("insert", json!({ "name": "Tag", "at": [100, 0], "attribs": { "NO": "7" } })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.set_selection(vec![h]);
    h
}

fn value(app: &CadApp, h: Handle, tag: &str) -> String {
    match app.session.doc().unwrap().entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => i.attribs.iter().find(|a| a.tag == tag).unwrap().text.value.clone(),
        other => panic!("{other:?}"),
    }
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
fn edit_attributes_on_a_selected_reference_opens_the_dialog() {
    let (mut app, h) = app_with_reference();
    let labels: Vec<_> = context_menu::object_entries(&app)
        .into_iter()
        .filter_map(|e| match e {
            menus::Entry::Item { label, .. } => Some(label),
            menus::Entry::Sub { .. } => None,
        })
        .collect();
    assert!(labels.iter().any(|l| l == "Edit Attributes..."), "{labels:?}");

    menus::activate(&mut app, "attedit");
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    assert_eq!(app.attedit.block, "Tag");
    let rows: Vec<_> = app.attedit.rows.iter().map(|r| (r.tag.as_str(), r.prompt.as_str(), r.value.as_str())).collect();
    assert_eq!(rows, [("NO", "Number", "7"), ("NOTE", "NOTE", "n")]);
    assert!(app.session.running.is_none(), "the dialog, not the command-line prompts");

    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    for s in ["Enhanced Attribute Editor", "Number", "NOTE", "OK", "Apply"] {
        assert!(text.contains(s), "{s} missing from:\n{text}");
    }

    // Apply changes only what was typed, in one undo step, and keeps the dialog open.
    app.attedit.rows[1].typed = "a note".into();
    assert!(attedit::apply(&mut app));
    assert_eq!((value(&app, h, "NO"), value(&app, h, "NOTE")), ("7".into(), "a note".into()));
    assert_eq!(app.attedit.rows[1].value, "a note", "rows reload after Apply");
    app.run("undo", json!({})).unwrap();
    assert_eq!(value(&app, h, "NOTE"), "n");
}

/// `attedit::open` (a double-click on an attribute) opens the dialog on a reference with a
/// chosen attribute current; its value box takes the focus in the first frame.
#[test]
fn open_makes_the_chosen_attribute_current() {
    let (mut app, h) = app_with_reference();
    app.session.set_selection(Vec::new());
    assert!(attedit::open(&mut app, h, Some(1)));
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    assert_eq!((app.attedit.current, app.attedit.focus), (Some(1), true));
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    assert!(!app.attedit.focus, "the focus was given in the first frame");
    assert!(ctx.memory(|m| m.focused().is_some()), "a value box has the focus");
    assert_eq!(app.attedit.current, Some(1));

    // Apply keeps the current attribute.
    app.attedit.rows[1].typed = "x".into();
    assert!(attedit::apply(&mut app));
    assert_eq!(app.attedit.current, Some(1));

    // No attribute, or one out of range: the first that can be changed.
    assert!(attedit::open(&mut app, h, None));
    assert_eq!(app.attedit.current, Some(0));
    assert!(attedit::open(&mut app, h, Some(99)));
    assert_eq!(app.attedit.current, Some(0));

    // Not a block reference with attributes: nothing opens.
    app.ui.dialog = None;
    let line = app.run("line", json!({ "points": [[0, 0], [1, 1]] })).unwrap()["handles"][0].as_str().map(str::to_string).unwrap();
    assert!(!attedit::open(&mut app, Handle::parse_hex(&line).unwrap(), Some(0)));
    assert!(!attedit::open(&mut app, Handle(0xFFFF_FFFF), None));
    assert!(app.ui.dialog.is_none());
}

#[test]
fn without_a_selected_reference_attedit_asks_at_the_command_line() {
    let (mut app, _) = app_with_reference();
    app.session.set_selection(Vec::new());
    app.start("eattedit");
    assert!(app.ui.dialog.is_none());
    assert!(app.session.prompt_text().contains("Select block reference"), "{}", app.session.prompt_text());
    app.session.cancel();

    // JSON calls never open the dialog.
    let (mut app, h) = app_with_reference();
    app.run("attedit", json!({ "handle": h.hex(), "values": { "NO": "9" } })).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(value(&app, h, "NO"), "9");
}

#[test]
fn the_properties_palette_lists_the_attributes() {
    let (mut app, _) = app_with_reference();
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    // (Text fields' contents are not painted shapes here, so only the labels are checked.)
    for s in ["Attributes", "NO", "NOTE"] {
        assert!(text.lines().any(|l| l == s), "{s} missing from:\n{text}");
    }
}

/// The dialog belongs to the drawing it was opened for: with another drawing's tab chosen (where
/// the same handle is a reference with the same tags), Apply changes nothing and the dialog
/// closes.
#[test]
fn the_dialog_closes_in_another_drawing() {
    let (mut app, h) = app_with_reference();
    menus::activate(&mut app, "attedit");
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    app.attedit.rows[1].typed = "a note".into();

    app.session.new_drawing(false);
    assert_eq!(add_reference(&mut app), h, "the same handle in the other drawing");
    assert!(!attedit::apply(&mut app));
    assert_eq!(value(&app, h, "NOTE"), "n", "the other drawing's reference is untouched");
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    assert!(app.ui.dialog.is_none(), "closed");

    // Back in the first drawing, its reference is as it was too.
    app.session.active = 0;
    assert_eq!(value(&app, h, "NOTE"), "n");
}
