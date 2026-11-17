//! PEDIT's main prompt has options and no default: Enter ends the command, as typed at the
//! keyboard (Dynamic Input on), and does not pick the first option (#2329 review).

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::Harness;
use serde_json::json;

#[test]
fn enter_at_pedit_ends_it_and_leaves_the_polyline_open() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.session.settings.dynmode = true;
    let r = app.run("pline", json!({ "points": [[0, 0], [10, 0], [10, 10]] })).unwrap();
    let h = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    app.start("pedit");
    app.session.input(Input::Point(cadcraft_geom::Vec2::new(5.0, 0.0))).unwrap();
    let mut h_ = Harness::builder().with_size(vec2(1280.0, 860.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h_.run_steps(5);
    assert!(h_.state().session.prompt_text().contains("Enter an option"), "{}", h_.state().session.prompt_text());
    h_.event(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h_.event(Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h_.run_steps(3);
    assert!(h_.state().session.running.is_none(), "{}", h_.state().session.prompt_text());
    let d = h_.state().session.doc().unwrap();
    let closed = d.model.iter().find(|e| e.handle.hex() == h).map(|e| matches!(&e.kind, EntityKind::LwPolyline(p) if p.closed));
    assert_eq!(closed, Some(false), "Enter didn't close it");
}
