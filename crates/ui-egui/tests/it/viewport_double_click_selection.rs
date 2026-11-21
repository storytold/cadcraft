use cadcraft_doc::{Common, EntityKind, Space, Viewport};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use serde_json::json;

#[test]
fn double_click_viewport_does_not_leave_pending_window() {
    let mut session = Session::new();
    session.new_drawing(true);
    session.viewport_px = (1600.0, 1000.0);

    let paper = Space::Paper("Layout1".into());
    let vp = Viewport {
        id: 2,
        center: cadcraft_geom::Vec3::new(70.0, 60.0, 0.0),
        width: 100.0,
        height: 80.0,
        view_center: Vec2::new(500.0, 250.0),
        view_height: 500.0,
        ..Default::default()
    };
    session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    session.execute("layout.set", &json!({"name": "Layout1"})).unwrap();

    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;

    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);

    // Simulate double click at viewport center
    let screen_pos = h.state().canvas.xf.unwrap().to_screen(Vec2::new(70.0, 60.0));
    // First click sets pending window
    h.event(egui::Event::PointerMoved(screen_pos));
    h.event(egui::Event::PointerButton { pos: screen_pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: screen_pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);

    // Second click (egui triggers double click)
    h.event(egui::Event::PointerButton { pos: screen_pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: screen_pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(3);

    assert!(h.state().session.pending_window.is_none(), "double clicking a viewport must not leave a pending selection window");
}
