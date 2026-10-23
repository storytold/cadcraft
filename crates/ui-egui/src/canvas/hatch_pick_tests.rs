use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use serde_json::json;

use crate::{CadApp, Services};

#[test]
fn hatch_at_finds_a_hatch_under_the_cursor_only() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap()["handle"].as_str().unwrap().to_string();
    app.session.execute("line", &json!({ "points": [[20, 0], [30, 0]] })).unwrap();
    assert_eq!(super::hatch_at(&app, Vec2::new(5.0, 5.0), 10.0).map(|h| h.hex()), Some(hx));
    assert_eq!(super::hatch_at(&app, Vec2::new(25.0, 0.0), 10.0), None, "a line is not a hatch");
    assert_eq!(super::hatch_at(&app, Vec2::new(60.0, 60.0), 10.0), None);
}

#[test]
fn double_clicking_a_hatch_opens_the_hatch_edit_dialog() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap()["handle"].as_str().unwrap().to_string();
    let _ = app.session.zoom_extents();
    let ctx = egui::Context::default();
    let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0)));
    let frame = |app: &mut CadApp, events: Vec<egui::Event>, time: f64| {
        app.logic(&ctx);
        let input = egui::RawInput { screen_rect: screen, events, time: Some(time), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| app.ui(ui));
        out.textures_delta.clear();
    };
    frame(&mut app, Vec::new(), 0.0);
    let at = app.canvas.xf.unwrap().to_screen(Vec2::new(5.0, 5.0));
    let button =
        |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
    frame(&mut app, vec![egui::Event::PointerMoved(at)], 0.1);
    frame(&mut app, vec![button(true), button(false)], 0.2);
    frame(&mut app, vec![button(true), button(false)], 0.3);
    frame(&mut app, Vec::new(), 0.4);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    assert_eq!(app.hatch_dlg.as_ref().map(|s| s.edit.iter().map(|h| h.hex()).collect::<Vec<_>>()), Some(vec![hx]));
    // The double-click (at the hatch's centre grip) only opens the dialog: no grip edit.
    assert!(app.session.running.is_none(), "{:?}", app.session.running.as_ref().map(|r| r.machine.name()));
}
