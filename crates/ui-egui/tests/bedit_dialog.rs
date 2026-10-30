//! The Edit Block Definition dialog: BEDIT typed or from a menu with no block reference selected
//! opens it; OK opens the Block Editor on the block named, or on a new block for a new name.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, bedit};
use serde_json::json;

fn app_with_blocks() -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());
    for (name, y) in [("Window", 0.0), ("door", 5.0)] {
        let l = app.run("line", json!({ "points": [[0, y], [2, y]] })).unwrap()["handles"][0].clone();
        app.run("block", json!({ "name": name, "base": [0, y], "handles": [l], "description": format!("{name} symbol") })).unwrap();
    }
    app.session.set_selection(Vec::new());
    app
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
fn bedit_without_a_selected_reference_opens_the_dialog() {
    let mut app = app_with_blocks();
    app.cmdline("BEDIT");
    assert_eq!(app.ui.dialog.as_deref(), Some("bedit"));
    // The drawing itself, then the blocks sorted by name, case-insensitively; the first is offered.
    assert_eq!(bedit::editable_blocks(app.session.doc().unwrap()), ["<Current Drawing>", "door", "Window"]);
    assert_eq!(app.bedit.name, "<Current Drawing>");
    assert!(app.session.state().unwrap().block_editor().is_none(), "the dialog changes nothing by itself");

    app.bedit.name = "door".into();
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    for s in ["Edit Block Definition", "Block to create or edit", "Window", "Preview", "door symbol"] {
        assert!(text.contains(s), "{s} missing from:\n{text}");
    }

    // OK edits the block named, in any case.
    app.bedit.name = "WINDOW".into();
    assert!(bedit::confirm(&mut app));
    assert_eq!(app.session.state().unwrap().block_editor(), Some("Window"));
    app.run("bclose", json!({})).unwrap();

    // Opened again, it starts on the block last edited.
    app.start("be");
    assert_eq!((app.ui.dialog.as_deref(), app.bedit.name.as_str()), (Some("bedit"), "WINDOW"));
}

#[test]
fn a_new_name_starts_a_new_block_and_a_bad_one_keeps_the_dialog() {
    let mut app = app_with_blocks();
    app.start("bedit");
    app.bedit.name = "a*b".into();
    assert!(!bedit::confirm(&mut app));
    assert!(app.bedit.error.as_deref().is_some_and(|e| e.contains("not a valid block name")), "{:?}", app.bedit.error);

    app.bedit.name = " Skylight ".into();
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    assert!(painted(&mut app, &ctx).contains("New block"));
    assert!(bedit::confirm(&mut app));
    assert_eq!(app.session.state().unwrap().block_editor(), Some("Skylight"));
    app.run("line", json!({ "points": [[0, 0], [1, 1]] })).unwrap();
    app.run("bclose", json!({})).unwrap();
    assert_eq!(app.session.doc().unwrap().block("Skylight").unwrap().entities.len(), 1);
}

#[test]
fn a_selected_reference_and_json_calls_skip_the_dialog() {
    let mut app = app_with_blocks();
    let refs: Vec<_> = app.session.doc().unwrap().model.handles();
    app.session.set_selection(vec![refs[0]]);
    app.start("bedit");
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.session.state().unwrap().block_editor(), Some("Window"));
    // In the editor BEDIT opens the dialog, to switch to another block.
    app.start("bedit");
    assert_eq!(app.ui.dialog.as_deref(), Some("bedit"));
    app.ui.dialog = None;
    app.run("bclose", json!({})).unwrap();

    app.run("bedit", json!({ "name": "door" })).unwrap();
    assert!(app.ui.dialog.is_none(), "JSON calls never open dialogs");
    assert_eq!(app.session.state().unwrap().block_editor(), Some("door"));
}

/// What the stand-in reader opens: for a file saying "authoring", a dynamic block's own drawing
/// as AutoCAD saves one (model space's block record has an extension dictionary holding an
/// `ACAD_ENHANCEDBLOCK` evaluation graph); otherwise a plain drawing. Both have one line.
fn read(bytes: &[u8], _name: &str) -> Result<cadcraft_engine::doc::Drawing, String> {
    use cadcraft_engine::doc::foreign::{Foreign, ForeignExtra, ForeignObject};
    use cadcraft_engine::doc::{Common, EntityKind, Handle, Line, RawTag, Space};
    use cadcraft_engine::geom::Vec3;
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) })).map_err(|e| e.to_string())?;
    if bytes == b"authoring" {
        let mut f = Foreign::default();
        let tag = |code, value: &str| RawTag { code, value: value.into() };
        f.records.insert(("BLOCK_RECORD".into(), "*MODEL_SPACE".into()), ForeignExtra { xdict: Some(Handle(0xF00)), ..Default::default() });
        f.objects.push(ForeignObject { handle: Handle(0xF00), kind: "DICTIONARY".into(), tags: vec![tag(3, "ACAD_ENHANCEDBLOCK"), tag(360, "F01")] });
        d.foreign = std::sync::Arc::new(f);
    }
    Ok(d)
}

#[test]
fn opening_a_drawing_with_authoring_elements_offers_the_block_editor() {
    use cadcraft_engine::cmd::file::{IoHooks, set_io};
    set_io(IoHooks { read, write: |_, _| Ok(Vec::new()), plot: None });
    let dir = std::env::temp_dir().join(format!("cadcraft-authoring-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let symbol = dir.join("Symbol.dxf");
    std::fs::write(&symbol, "authoring").unwrap();
    let mut app = CadApp::new(Session::new(), Services::default());
    app.open_path(&symbol);
    // The file is read in the background (#797): wait for it to be added.
    let t = std::time::Instant::now();
    while app.opening.busy() && t.elapsed() < std::time::Duration::from_secs(10) {
        app.finish_opening();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.finish_opening();
    assert_eq!(app.ui.dialog.as_deref(), Some("authoring"));
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    assert!(text.contains("Symbol.dxf contains authoring elements. Open in Block Editor?"), "{text}");
    for s in ["Yes", "No", "Cancel"] {
        assert!(
            text.contains(s),
            "{s} missing from:
{text}"
        );
    }
    // Yes is BEDIT on the drawing itself.
    app.run("bedit", json!({ "name": "<Current Drawing>" })).unwrap();
    assert_eq!(app.session.state().unwrap().block_editor(), Some("<Current Drawing>"));
    assert_eq!(app.session.doc().unwrap().model.len(), 1);

    // A drawing without authoring elements opens without asking.
    let plain = dir.join("Plain.dxf");
    std::fs::write(&plain, "plain").unwrap();
    let mut app = CadApp::new(Session::new(), Services::default());
    app.open_path(&plain);
    // The file is read in the background (#797): wait for it to be added.
    let t = std::time::Instant::now();
    while app.opening.busy() && t.elapsed() < std::time::Duration::from_secs(10) {
        app.finish_opening();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.finish_opening();
    assert!(app.session.state().unwrap().path.is_some(), "opened");
    assert!(app.ui.dialog.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A name the Block Editor can't switch to is refused before the dialog asks about the changes not
/// saved, so the question never traps the user.
#[test]
fn a_bad_name_is_refused_before_the_dialog_asks_to_save() {
    let mut app = app_with_blocks();
    app.run("bedit", json!({ "name": "door" })).unwrap();
    app.run("line", json!({ "points": [[0, 0], [0, 4]] })).unwrap();
    app.start("bedit");
    app.bedit.name = "*U1".into();
    assert!(!bedit::confirm(&mut app));
    assert!(!app.bedit.ask_save, "no question");
    assert!(app.bedit.error.is_some(), "the dialog says why");
    assert!(app.session.state().unwrap().block_unsaved(), "nothing lost");
}

/// In the Block Editor, BEDIT opens the dialog on the block being edited; OK on another block
/// switches to it, first asking whether to save the changes not saved yet (Cancel goes back).
#[test]
fn in_the_block_editor_the_dialog_switches_blocks_asking_to_save_first() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    let mut app = app_with_blocks();
    app.run("bedit", json!({ "name": "door" })).unwrap();
    app.start("bedit");
    assert_eq!((app.ui.dialog.as_deref(), app.bedit.name.as_str()), (Some("bedit"), "door"), "starts on the block edited");
    // Nothing changed: OK switches at once.
    app.bedit.name = "Window".into();
    assert!(bedit::confirm(&mut app));
    assert_eq!(app.session.state().unwrap().block_editor(), Some("Window"));

    app.run("line", json!({ "points": [[0, 0], [0, 4]] })).unwrap();
    app.start("bedit");
    app.bedit.name = "door".into();
    assert!(!bedit::confirm(&mut app), "asks first");
    assert!(app.bedit.ask_save);
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(2);
    assert!(h.query_by_label_contains("Save changes to block \"Window\"").is_some());
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(!h.state().bedit.ask_save, "back to the list");
    assert_eq!(h.state().session.state().unwrap().block_editor(), Some("Window"));
    assert!(h.state().session.state().unwrap().block_unsaved(), "nothing lost");

    assert!(!bedit::confirm(h.state_mut()));
    h.run_steps(2);
    h.get_by_label("Yes").click();
    h.run_steps(2);
    let st = h.state().session.state().unwrap();
    assert_eq!(st.block_editor(), Some("door"));
    assert!(h.state().ui.dialog.is_none(), "the dialog closed");
    let window = h.state().session.doc().unwrap().block("Window").map(|b| b.entities.len());
    assert_eq!(window, Some(2), "Window saved with its new line before switching");
}
