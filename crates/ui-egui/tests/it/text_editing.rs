//! The in-place text editor on existing objects: one line of text finishes with Enter, a table
//! cell moves on with Tab, a dimension keeps `<>` for its measured value, and Options ▸ Fonts
//! lists font folders.

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::{EditorKind, Input, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

fn harness(setup: impl FnOnce(&mut CadApp)) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    setup(&mut app);
    let mut h = Harness::builder().with_size(vec2(1280.0, 860.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn entity(h: &Harness<'_, CadApp>, hex: &str) -> EntityKind {
    let doc = h.state().session.doc().unwrap();
    doc.model.iter().find(|e| e.handle.hex() == hex).unwrap().kind.clone()
}

fn key(h: &mut Harness<'_, CadApp>, key: Key, modifiers: Modifiers) {
    h.event(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    h.event(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
    h.run_steps(3);
}

fn type_text(h: &mut Harness<'_, CadApp>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run_steps(3);
}

#[test]
fn one_line_of_text_is_edited_in_place_and_enter_finishes() {
    let mut t = String::new();
    let mut h = harness(|app| {
        t = handle(&app.run("text", json!({ "at": [0, 0], "text": "Plan", "height": 2.0 })).unwrap());
        app.start("textedit.object");
        app.session.input(Input::Point(cadcraft_geom::Vec2::new(0.5, 0.5))).unwrap();
    });
    assert_eq!(h.state().session.text_editor().map(|e| e.kind), Some(EditorKind::Line));
    // The text opens selected: End goes past it, then the typing adds to it.
    key(&mut h, Key::End, Modifiers::NONE);
    type_text(&mut h, " B");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().session.running.is_none(), "{}", h.state().session.prompt_text());
    assert!(matches!(entity(&h, &t), EntityKind::Text(x) if x.value == "Plan B"));
}

#[test]
fn tab_keeps_a_cell_and_moves_to_the_next() {
    let mut t = String::new();
    let mut h = harness(|app| {
        t = handle(&app.run("table", json!({ "at": [0, 10], "rows": 2, "cols": 2, "colWidth": 6, "rowHeight": 2 })).unwrap());
        app.start("tabledit.cell");
        app.session.input(Input::Point(cadcraft_geom::Vec2::new(3.0, 9.0))).unwrap();
    });
    type_text(&mut h, "A");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert!(matches!(h.state().session.text_editor().map(|e| e.kind), Some(EditorKind::TableCell { name }) if name == "B1"));
    type_text(&mut h, "B");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert!(h.state().session.running.is_none(), "{}", h.state().session.prompt_text());
    let EntityKind::Table(tb) = entity(&h, &t) else { panic!() };
    assert_eq!((tb.cells[0][0].text.as_str(), tb.cells[0][1].text.as_str()), ("A", "B"));
}

#[test]
fn a_dimension_shows_its_measured_value_and_keeps_the_placeholder() {
    let mut d = String::new();
    let mut h = harness(|app| {
        d = handle(&app.run("dimlinear", json!({ "p1": [0, 0], "p2": [25, 0], "at": [12, 4] })).unwrap());
        let doc = app.session.doc().unwrap();
        let Some(EntityKind::Dimension(dm)) = doc.model.iter().find(|e| e.handle.hex() == d).map(|e| e.kind.clone()) else { panic!() };
        let at = cadcraft_render::dimension_in(doc, &dm).text_pos;
        app.start("textedit.object");
        app.session.input(Input::Point(at)).unwrap();
    });
    assert!(h.query_by_label_contains("<> = 25").is_some(), "the measured value is shown");
    key(&mut h, Key::Home, Modifiers::NONE);
    type_text(&mut h, "L=");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert!(matches!(entity(&h, &d), EntityKind::Dimension(x) if x.text == "L=<>"), "{:?}", entity(&h, &d));
}

#[test]
fn options_lists_font_folders_and_the_drawings_missing_fonts() {
    let mut h = harness(|app| {
        app.run("style", json!({ "name": "Gost", "font": "gost_cadcraft_test.shx" })).unwrap();
        app.ui.dialog = Some("options:fonts".into());
    });
    h.run_steps(3);
    assert!(h.query_by_label("Font folders").is_some());
    assert!(h.query_by_label_contains("gost_cadcraft_test.shx").is_some(), "the missing font is listed");
    let dir = std::env::temp_dir().join(format!("cadcraft-ui-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.to_string_lossy().to_string();
    let r = h.state_mut().run("options", json!({ "fontFolders": [path.clone(), path.clone(), ""] })).unwrap();
    assert_eq!(r["fontFolders"], json!([path]));
    assert!(h.state_mut().run("options", json!({ "fontFolders": "nope" })).is_err());
    let saved = h.state().profile_json();
    h.state_mut().run("options", json!({ "fontFolders": [] })).unwrap();
    h.state_mut().load_profile(&saved);
    assert_eq!(cadcraft_fonts::ttf::font_dirs(), vec![path]);
    h.state_mut().run("options", json!({ "fontFolders": [] })).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_options_dialog_keeps_its_place_and_size_on_every_tab() {
    let mut h = harness(|app| app.ui.dialog = Some("options".into()));
    let mut rects = Vec::new();
    for tab in ["display", "files", "fonts", "drafting", "selection", "preferences", "profiles"] {
        h.state_mut().ui.dialog = Some(format!("options:{tab}"));
        h.run_steps(4);
        rects.push((tab, h.get_by_label("Close").rect()));
    }
    let first = rects[0].1;
    for (tab, r) in &rects {
        assert!((r.min - first.min).length() < 0.5, "{tab}: {r:?} vs {first:?}");
    }
}

#[test]
fn the_application_button_opens_the_file_menu_and_cannot_be_removed() {
    let mut h = harness(|_| {});
    h.get_by_label("CADCraft").click();
    h.run_steps(3);
    for label in ["New", "Open...", "Save", "Save As...", "Import...", "Export...", "Print...", "Batch Print...", "Close", "Options..."] {
        assert!(h.query_all_by_label_contains(label).next().is_some(), "{label}");
    }
    h.query_all_by_label_contains("Options...").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("options"));
    // Not one of the bar's items: the customizable list never holds it.
    assert!(!cadcraft_ui_egui::toolbar::default_items().iter().any(|i| i.contains("app")));
    h.state_mut().run("ui.toolbar.reset", json!({})).unwrap();
    h.state_mut().ui.toolbar_items.clear();
    h.run_steps(3);
    assert!(h.query_by_label("CADCraft").is_some(), "an empty bar still has the application button");
}
