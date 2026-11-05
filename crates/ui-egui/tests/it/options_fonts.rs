//! Options ▸ Fonts lists the font folders and the drawing's missing fonts, and keeps its place.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

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
