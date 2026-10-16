//! Grip acquisition and palette shortcuts through the real canvas/input pipeline.
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services, docking::Panel};
use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness(scale: f32) -> Harness<'static, CadApp> {
    let mut session = Session::new();
    session.new_drawing(false);
    session.execute("rectang", &json!({"p1":[30,30],"p2":[70,70]})).unwrap();
    session.execute("view.set", &json!({"center":[50,50],"height":100})).unwrap();
    let handle = session.doc().unwrap().model.iter().next().unwrap().handle;
    session.set_selection(vec![handle]);
    session.settings.osmode = 0;
    session.settings.snapmode = false;
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_pixels_per_point(scale).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn key(h: &mut Harness<'_, CadApp>) {
    let modifiers = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
    h.input_mut().events.extend([
        Event::ModifiersChanged(modifiers),
        Event::Key { key: Key::Num1, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key: Key::Num1, physical_key: None, pressed: false, repeat: false, modifiers },
        Event::ModifiersChanged(Modifiers::NONE),
    ]);
    h.run_steps(2);
}

#[test]
fn vertex_and_midpoint_acquire_exact_grip_and_click_starts_edit() {
    for scale in [1.0, 2.0] {
        for point in [Vec2::new(30.0, 30.0), Vec2::new(50.0, 70.0)] {
            let mut h = harness(scale);
            let xf = h.state().canvas.xf.unwrap();
            let near = xf.to_screen(point) + vec2(3.0, 2.0);
            let before = h.state().session.doc().unwrap().clone();
            h.event(Event::PointerMoved(near));
            h.run_steps(2);
            assert!(h.state().canvas.cursor.unwrap().near(point, 1e-9));
            assert_eq!(h.state().session.doc().unwrap(), &before);
            if let Ok(dir) = std::env::var("CRAFT_GRIP_VISUAL_DIR") {
                std::fs::create_dir_all(&dir).unwrap();
                h.render().unwrap().save(format!("{dir}/grip-{scale}-{}.png", point.x)).unwrap();
            }
            for pressed in [true, false] {
                h.event(Event::PointerButton { pos: near, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
                h.run_steps(1);
            }
            assert_eq!(h.state().session.running.as_ref().unwrap().id, "grip");
            assert!(h.state().session.pending_window.is_none());
        }
    }
}

#[test]
fn grip_snap_releases_outside_tolerance_and_does_not_capture_window_or_drawing_points() {
    let mut h = harness(1.0);
    let xf = h.state().canvas.xf.unwrap();
    let point = Vec2::new(30.0, 30.0);
    let far = xf.to_screen(point) + vec2(20.0, 0.0);
    h.event(Event::PointerMoved(far));
    h.run_steps(2);
    assert!(h.state().canvas.cursor.unwrap().near(xf.to_world(far), 1e-9));
    let near = xf.to_screen(point) + vec2(3.0, 2.0);
    h.state_mut().session.begin_window(Vec2::new(10.0, 10.0));
    h.event(Event::PointerMoved(near));
    h.run_steps(2);
    assert!(h.state().canvas.cursor.unwrap().near(xf.to_world(near), 1e-9));
    h.state_mut().session.pending_window = None;
    h.state_mut().start("line");
    h.run_steps(2);
    assert!(h.state().canvas.cursor.unwrap().near(xf.to_world(near), 1e-9));
}

#[test]
fn ctrl_one_toggles_only_properties_and_keeps_active_command() {
    let mut h = harness(1.0);
    h.state_mut().start("line");
    let drawing = h.state().session.doc().unwrap().clone();
    for visible in [false, true, false, true] {
        key(&mut h);
        assert_eq!(h.state().ui.docking.layout.contains(&Panel::Properties), visible);
        assert!(h.state().ui.docking.layout.contains(&Panel::Layers));
        assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
        assert_eq!(h.state().session.doc().unwrap(), &drawing);
    }
    h.state_mut().run("ui.panel.properties", json!({})).unwrap();
    assert!(h.state().ui.docking.layout.contains(&Panel::Properties), "explicit open remains idempotent");
}

#[test]
fn hidden_grips_do_not_capture_cursor() {
    let mut h = harness(1.0);
    h.state_mut().session.settings.grips = 0;
    let xf = h.state().canvas.xf.unwrap();
    let near = xf.to_screen(Vec2::new(30.0, 30.0)) + vec2(3.0, 2.0);
    h.event(Event::PointerMoved(near));
    h.run_steps(2);
    assert!(h.state().canvas.cursor.unwrap().near(xf.to_world(near), 1e-9));
}

#[test]
fn properties_toggle_restores_floating_location() {
    let mut h = harness(1.0);
    h.state_mut().run("ui.dock", json!({"operation":"float","panel":"properties","rect":[450,200,300,350]})).unwrap();
    h.run_steps(2);
    let before = h.state().ui.docking.layout.location(&Panel::Properties).unwrap();
    key(&mut h);
    assert!(!h.state().ui.docking.layout.contains(&Panel::Properties));
    key(&mut h);
    assert_eq!(h.state().ui.docking.layout.location(&Panel::Properties).unwrap(), before);
}

#[test]
fn ctrl_one_works_with_text_focus_without_changing_text() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.cmd.buffer = "keep this text".into();
    let mut h = Harness::builder().build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            ui.text_edit_singleline(&mut app.cmd.buffer).request_focus();
        },
        app,
    );
    h.run_steps(3);
    for visible in [false, true] {
        key(&mut h);
        assert_eq!(h.state().ui.docking.layout.contains(&Panel::Properties), visible);
        assert_eq!(h.state().cmd.buffer, "keep this text");
    }
}
