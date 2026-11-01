//! Measure Distance uses real pointer snaps in MODEL and PAPER, including viewport scale (#516).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Line, Space, Viewport};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::snap::mode;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(paper: bool, render: bool) -> Harness<'static, CadApp> {
    let mut d = Drawing::new_imperial();
    for (a, b) in [(Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0)), (Vec2::new(10.0, -10.0), Vec2::new(10.0, 10.0))] {
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) })).unwrap();
    }
    d.add(
        &Space::Paper("Layout1".into()),
        Common::default(),
        EntityKind::Viewport(Viewport {
            center: Vec3::new(5.0, 4.0, 0.0),
            width: 8.0,
            height: 6.0,
            view_center: Vec2::new(10.0, 0.0),
            view_height: 40.0,
            id: 2,
            locked: false,
            frozen_layers: Vec::new(),
            layer_colors: Vec::new(),
            clip: None,
            on: true,
        }),
    )
    .unwrap();
    let mut session = Session::empty();
    session.open_drawing(d, "Measure snap regression", None);
    if paper {
        session.execute("layout.set", &json!({"name":"Layout1"})).unwrap();
    }
    session.execute("view.set", &if paper { json!({"center":[5,4],"height":10}) } else { json!({"center":[10,0],"height":30}) }).unwrap();
    session.settings.osmode = mode::END | mode::INT;
    session.settings.snapmode = true;
    session.settings.orthomode = true;
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let builder = Harness::builder().with_size(vec2(1600.0, 1000.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn hover(h: &mut Harness<'_, CadApp>, point: Vec2) -> Pos2 {
    let pos = h.state().canvas.xf.unwrap().to_screen(point) + vec2(3.0, 2.0);
    h.event(Event::PointerMoved(pos));
    h.run_steps(3);
    pos
}

fn click(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

fn measure(paper: bool) {
    let shots = std::env::var_os("CADCRAFT_MEASURE_SHOTS").map(std::path::PathBuf::from);
    let mut h = harness(paper, shots.is_some());
    let dirty_before = h.state().session.state().unwrap().is_dirty();
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Measure Distance").click();
    h.run_steps(3);
    assert_eq!(h.state().session.running.as_ref().unwrap().id, "dist");
    let (endpoint, intersection) = if paper { (Vec2::new(3.5, 4.0), Vec2::new(5.0, 4.0)) } else { (Vec2::ZERO, Vec2::new(10.0, 0.0)) };
    let pos = hover(&mut h, endpoint);
    let hit = h.state().canvas.snap.expect("endpoint marker");
    assert_eq!(hit.mode, mode::END);
    assert!(hit.point.near(endpoint, 1e-6));
    assert_eq!(hit.viewport.is_some(), paper);
    if let Some(path) = &shots {
        std::fs::create_dir_all(path).unwrap();
        h.render().unwrap().save(path.join(if paper { "paper-endpoint.png" } else { "model-endpoint.png" })).unwrap();
    }
    click(&mut h, pos);
    assert!(h.state().session.current_prompt().unwrap().base.unwrap().near(endpoint, 1e-6));
    let pos = hover(&mut h, intersection);
    let hit = h.state().canvas.snap.expect("intersection marker");
    assert_eq!(hit.mode, mode::INT);
    assert!(hit.point.near(intersection, 1e-6));
    if let Some(path) = &shots {
        h.render().unwrap().save(path.join(if paper { "paper-intersection.png" } else { "model-intersection.png" })).unwrap();
    }
    click(&mut h, pos);
    let app = h.state();
    assert!(app.session.running.is_none());
    assert!(app.session.log.iter().any(|l| l.starts_with("Distance = 10.0000,")), "{:?}", app.session.log);
    assert_eq!(app.session.state().unwrap().is_dirty(), dirty_before);
    if let Some(path) = &shots {
        h.render().unwrap().save(path.join(if paper { "paper-distance.png" } else { "model-distance.png" })).unwrap();
    }
}

#[test]
fn measure_distance_toolbar_snaps_to_model_geometry_through_paper_viewports() {
    measure(true);
}

#[test]
fn measure_distance_toolbar_keeps_model_space_snaps_working() {
    measure(false);
}
