//! Resting the cursor on a grip with options opens the grip menu (here a dimension's text grip).

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(setup: impl FnOnce(&mut CadApp)) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    setup(&mut app);
    let mut h = Harness::builder().with_size(vec2(1280.0, 900.0)).build_ui_state(
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
fn resting_on_the_text_grip_opens_its_menu() {
    let mut hx = String::new();
    let mut h = harness(|app| {
        hx = app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap()["handle"].as_str().unwrap().to_string();
        app.run("view.set", json!({ "center": [5, 3], "height": 20 })).unwrap();
        let handle = cadcraft_engine::doc::Handle::parse_hex(&hx).unwrap();
        app.session.set_selection(vec![handle]);
    });
    h.run_steps(3);
    let handle = cadcraft_engine::doc::Handle::parse_hex(&hx).unwrap();
    let grip = h.state().session.grips_of(handle).unwrap()[3];
    let at: Pos2 = h.state().canvas.xf.unwrap().to_screen(grip);
    h.event(Event::PointerMoved(at));
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    h.run_steps(2);
    assert!(h.query_by_label("Move Text Only").is_some(), "the menu opened on hover");
    let item = h.get_by_label("Move Text Only").rect().center();
    h.event(Event::PointerMoved(item));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: item, button: PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.event(Event::PointerButton { pos: item, button: PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
    assert!(h.state().session.running.is_some(), "the grip is hot, asking for the text location");
    assert!(h.state().session.prompt_text().contains("MOVE TEXT ONLY"), "{}", h.state().session.prompt_text());
}
