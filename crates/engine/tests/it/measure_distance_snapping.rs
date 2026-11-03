//! DIST snaps to the model geometry displayed in paper-space viewports (#516).

use cadcraft_engine::doc::{Block, Circle, Common, Drawing, Entity, EntityKind, Handle, Insert, Line, Space, Viewport};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::snap::{SnapCurve, SnapHit, mode, osnap};
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn drawing() -> (Drawing, Space) {
    let mut d = Drawing::new_imperial();
    let paper = Space::Paper("Layout1".into());
    for (a, b) in [(Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0)), (Vec2::new(10.0, -10.0), Vec2::new(10.0, 10.0))] {
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) })).unwrap();
    }
    d.add(
        &paper,
        Common::default(),
        EntityKind::Viewport(Viewport {
            center: Vec3::new(100.0, 50.0, 0.0),
            width: 20.0,
            height: 20.0,
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
    (d, paper)
}

fn viewport(d: &Drawing, paper: &Space) -> Handle {
    d.space(paper).unwrap().iter().find(|e| matches!(e.kind, EntityKind::Viewport(_))).unwrap().handle
}

fn edit_viewport(d: &mut Drawing, paper: &Space, edit: impl FnOnce(&mut Viewport)) {
    d.modify_entity(viewport(d, paper), |e| match &mut e.kind {
        EntityKind::Viewport(vp) => edit(vp),
        _ => panic!("viewport fixture"),
    })
    .unwrap();
}

fn session(d: Drawing) -> Session {
    let mut s = Session::empty();
    s.open_drawing(d, "Drawing", None);
    s.execute("layout.set", &json!({"name":"Layout1"})).unwrap();
    s.execute("view.set", &json!({"center":[100,50],"height":30})).unwrap();
    s.settings.osmode = mode::END | mode::INT;
    s
}

fn pick(s: &Session, at: Vec2, modes: u32) -> SnapHit {
    osnap(s.doc().unwrap(), &s.space(), at + Vec2::new(0.05, 0.04), 0.5, modes, None, false).expect("model snap through viewport")
}

#[test]
fn paper_space_snaps_to_visible_model_endpoints_and_intersections() {
    let (d, paper) = drawing();
    for (cursor, at, modes, expected) in [
        (Vec2::new(95.1, 50.1), Vec2::new(95.0, 50.0), mode::END | mode::INT, mode::END),
        (Vec2::new(100.1, 50.1), Vec2::new(100.0, 50.0), mode::INT, mode::INT),
    ] {
        let hit = osnap(&d, &paper, cursor, 0.5, modes, None, false).expect("visible model geometry must snap from PAPER");
        assert_eq!(hit.mode, expected);
        assert!(hit.point.near(at, 1e-9));
        let source = hit.viewport.unwrap();
        assert_eq!(source.scale, 0.5);
        assert!(source.point.near(Vec2::new((at.x - 100.0) * 2.0 + 10.0, (at.y - 50.0) * 2.0), 1e-9));
    }
}

#[test]
fn model_space_running_snaps_and_disabled_snaps_keep_their_behavior() {
    let (d, paper) = drawing();
    let hit = osnap(&d, &Space::Model, Vec2::new(0.1, 0.1), 0.5, mode::END, None, false).unwrap();
    assert_eq!(hit.point, Vec2::ZERO);
    assert!(hit.viewport.is_none());
    for modes in [0, mode::END | mode::INT | mode::OFF] {
        assert!(osnap(&d, &paper, Vec2::new(95.1, 50.1), 0.5, modes, None, false).is_none());
    }
}

#[test]
fn distance_uses_model_units_for_two_snaps_in_the_same_viewport() {
    let (d, _) = drawing();
    let mut s = session(d);
    let before = s.state().unwrap().is_dirty();
    s.start("dist").unwrap();
    s.input_snapped(pick(&s, Vec2::new(95.0, 50.0), mode::END)).unwrap();
    assert_eq!(s.current_prompt().unwrap().base, Some(Vec2::new(95.0, 50.0)), "preview remains in paper coordinates");
    s.input_snapped(pick(&s, Vec2::new(100.0, 50.0), mode::INT)).unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l.starts_with("Distance = 10.0000,")), "{:?}", s.log);
    assert!(s.log.iter().any(|l| l.starts_with("Delta X = 10.0000,  Delta Y = 0.0000")));
    assert_eq!(s.state().unwrap().is_dirty(), before, "inquiry must not edit the drawing");
}

#[test]
fn typed_and_programmatic_points_are_measured_in_paper_units() {
    let (d, _) = drawing();
    let mut s = session(d);
    for snapped_first in [true, false] {
        s.start("dist").unwrap();
        if snapped_first {
            s.input_snapped(pick(&s, Vec2::new(95.0, 50.0), mode::END)).unwrap();
            s.input(Input::Point(Vec2::new(100.0, 50.0))).unwrap();
        } else {
            s.input(Input::Point(Vec2::new(95.0, 50.0))).unwrap();
            s.input_snapped(pick(&s, Vec2::new(100.0, 50.0), mode::INT)).unwrap();
        }
        assert!(s.log.iter().any(|l| l.starts_with("Distance = 5.0000,")), "{:?}", s.log);
        s.log.clear();
    }
    assert_eq!(s.execute("dist", &json!({"p1":[95,50],"p2":[100,50]})).unwrap()["distance"], json!(5.0));
}

#[test]
fn points_from_different_viewports_use_paper_units() {
    let (mut d, paper) = drawing();
    let mut vp = match &d.entity(viewport(&d, &paper)).unwrap().kind {
        EntityKind::Viewport(vp) => vp.clone(),
        _ => panic!("viewport fixture"),
    };
    vp.center.x = 120.0;
    vp.view_height = 20.0;
    vp.id = 3;
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    let mut s = session(d);
    s.start("dist").unwrap();
    let first = pick(&s, Vec2::new(95.0, 50.0), mode::END);
    let second = pick(&s, Vec2::new(110.0, 50.0), mode::END);
    assert_ne!(first.viewport.unwrap().viewport, second.viewport.unwrap().viewport);
    assert_eq!(first.viewport.unwrap().point, second.viewport.unwrap().point);
    s.input_snapped(first).unwrap();
    s.input_snapped(second).unwrap();
    assert!(s.log.iter().any(|l| l.starts_with("Distance = 15.0000,")), "{:?}", s.log);
}

#[test]
fn multiple_points_length_and_undo_keep_the_measurement_units() {
    let (d, _) = drawing();
    let mut s = session(d);
    s.start("dist").unwrap();
    s.input_snapped(pick(&s, Vec2::new(95.0, 50.0), mode::END)).unwrap();
    s.input(Input::Keyword("Multiple points".into())).unwrap();
    s.input_snapped(pick(&s, Vec2::new(100.0, 50.0), mode::INT)).unwrap();
    s.input_snapped(pick(&s, Vec2::new(105.0, 50.0), mode::END)).unwrap();
    s.input(Input::Keyword("Length".into())).unwrap();
    s.input(Input::Text("5".into())).unwrap();
    assert_eq!(s.log.last().unwrap(), "Distance = 25.0000");
    assert_eq!(s.current_prompt().unwrap().base, Some(Vec2::new(107.5, 50.0)));
    s.input(Input::Keyword("Undo".into())).unwrap();
    s.input(Input::Point(Vec2::new(102.0, 52.0))).unwrap();
    s.input(Input::Keyword("Undo".into())).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.log.last().unwrap(), "Distance = 20.0000");
}

#[test]
fn multiple_points_arc_options_keep_the_measurement_units() {
    let (d, _) = drawing();
    let mut s = session(d);
    s.start("dist").unwrap();
    s.input_snapped(pick(&s, Vec2::new(95.0, 50.0), mode::END)).unwrap();
    s.input(Input::Keyword("Multiple points".into())).unwrap();
    s.input_snapped(pick(&s, Vec2::new(100.0, 50.0), mode::INT)).unwrap();
    // A half circle of radius 2.5 on paper, ended by its included angle, is radius 5 in the
    // model: 10 + 5 pi.
    s.input(Input::Keyword("Arc".into())).unwrap();
    s.input(Input::Keyword("CEnter".into())).unwrap();
    s.input(Input::Point(Vec2::new(100.0, 52.5))).unwrap();
    s.input(Input::Keyword("Angle".into())).unwrap();
    s.input(Input::Text("180".into())).unwrap();
    assert_eq!(s.log.last().unwrap(), "Distance = 25.7080");
    let base = s.current_prompt().unwrap().base.unwrap();
    assert!(base.near(Vec2::new(100.0, 55.0), 1e-9), "the path stays in paper coordinates: {base:?}");
    s.input(Input::Enter).unwrap();
    assert_eq!(s.log.last().unwrap(), "Distance = 25.7080");

    // A typed radius is in model units: 5 is 2.5 on paper, the same half circle.
    let start = |s: &mut Session| {
        s.start("dist").unwrap();
        s.input_snapped(pick(s, Vec2::new(95.0, 50.0), mode::END)).unwrap();
        s.input(Input::Keyword("Multiple points".into())).unwrap();
        s.input_snapped(pick(s, Vec2::new(100.0, 50.0), mode::INT)).unwrap();
        s.input(Input::Keyword("Arc".into())).unwrap();
    };
    start(&mut s);
    for i in [Input::Keyword("Radius".into()), Input::Text("5".into()), Input::Keyword("Angle".into()), Input::Text("180".into()), Input::Enter] {
        s.input(i).unwrap();
    }
    assert_eq!(s.log.last().unwrap(), "Distance = 25.7080");

    // A snapped end point off the circle gives the direction of the end: the model end is the
    // arc's, not the snapped point (0,0). Radius 5 from angle -90 to the angle of (-2,-1).
    start(&mut s);
    s.input(Input::Keyword("CEnter".into())).unwrap();
    s.input(Input::Point(Vec2::new(100.0, 52.5))).unwrap();
    s.input_snapped(pick(&s, Vec2::new(95.0, 50.0), mode::END)).unwrap();
    let sweep = (-1.0f64).atan2(-2.0) + std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU;
    assert_eq!(s.log.last().unwrap(), &format!("Distance = {:.4}", 10.0 + 5.0 * sweep));
    s.input(Input::Enter).unwrap();

    // A typed end point is a paper point, as in line mode: the path is measured in paper units.
    start(&mut s);
    s.input(Input::Keyword("CEnter".into())).unwrap();
    s.input(Input::Point(Vec2::new(100.0, 52.5))).unwrap();
    s.input(Input::Point(Vec2::new(100.0, 55.0))).unwrap();
    assert_eq!(s.log.last().unwrap(), "Distance = 12.8540");
}

#[test]
fn an_arc_end_picked_through_another_viewport_uses_paper_units() {
    let (mut d, paper) = drawing();
    let mut vp = match &d.entity(viewport(&d, &paper)).unwrap().kind {
        EntityKind::Viewport(vp) => vp.clone(),
        _ => panic!("viewport fixture"),
    };
    vp.center.x = 120.0;
    vp.view_height = 20.0;
    vp.id = 3;
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    let mut s = session(d);
    let mut measure = |snap: bool| {
        s.start("dist").unwrap();
        let first = pick(&s, Vec2::new(95.0, 50.0), mode::END);
        let second = pick(&s, Vec2::new(100.0, 50.0), mode::INT);
        let end = pick(&s, Vec2::new(110.0, 50.0), mode::END);
        assert_ne!(first.viewport.unwrap().viewport, end.viewport.unwrap().viewport);
        for (n, hit) in [first, second].into_iter().enumerate() {
            if n == 1 {
                s.input(Input::Keyword("Multiple points".into())).unwrap();
            }
            if snap { s.input_snapped(hit).unwrap() } else { s.input(Input::Point(hit.point)).unwrap() }
        }
        s.input(Input::Keyword("Arc".into())).unwrap();
        s.input(Input::Keyword("Second pt".into())).unwrap();
        s.input(Input::Point(Vec2::new(105.0, 52.0))).unwrap();
        if snap {
            s.input_snapped(end).unwrap()
        } else {
            s.input(Input::Point(end.point)).unwrap()
        }
        s.input(Input::Enter).unwrap();
        s.log.last().unwrap().clone()
    };
    let typed = measure(false);
    assert_eq!(typed, "Distance = 16.0347", "5 along X, then the arc through 105,52 to 110,50");
    assert_eq!(measure(true), typed);
}

#[test]
fn explicit_endpoint_and_intersection_overrides_work_from_the_command_line() {
    let (d, _) = drawing();
    let mut s = session(d);
    s.settings.osmode = mode::OFF;
    s.cmdline("dist").unwrap();
    s.cmdline("end").unwrap();
    s.cmdline("95.05,50.04").unwrap();
    s.cmdline("int").unwrap();
    s.cmdline("100.05,50.04").unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l.starts_with("Distance = 10.0000,")), "{:?}", s.log);
}

#[test]
fn clipped_endpoints_do_not_hide_a_visible_candidate() {
    let (mut d, paper) = drawing();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(30.2, 2.0, 0.0), b: Vec3::new(29.0, 2.0, 0.0) })).unwrap();
    let hit = osnap(&d, &paper, Vec2::new(110.11, 51.0), 0.8, mode::END, None, false).unwrap();
    assert_eq!(hit.point, Vec2::new(109.5, 51.0), "only the endpoint inside the viewport is visible");
    assert!(osnap(&d, &paper, Vec2::new(110.11, 51.0), 0.2, mode::END, None, false).is_none());
}

#[test]
fn viewport_frozen_layers_do_not_hide_paper_space_geometry() {
    let (mut d, paper) = drawing();
    edit_viewport(&mut d, &paper, |vp| vp.frozen_layers.push("0".into()));
    assert!(osnap(&d, &paper, Vec2::new(95.1, 50.1), 0.5, mode::END, None, false).is_none());
    d.add(&paper, Common::default(), EntityKind::Line(Line { a: Vec3::new(99.0, 51.0, 0.0), b: Vec3::new(101.0, 51.0, 0.0) })).unwrap();
    let hit = osnap(&d, &paper, Vec2::new(99.1, 51.1), 0.5, mode::END, None, false).unwrap();
    assert_eq!(hit.point, Vec2::new(99.0, 51.0));
    assert!(hit.viewport.is_none());
}

#[test]
fn viewport_frozen_layers_apply_inside_nested_block_references() {
    let (mut d, paper) = drawing();
    d.layers.push(cadcraft_engine::doc::Layer { name: "Detail".into(), ..Default::default() });
    let mut inner = Block::new("Inner");
    inner.entities.push(Entity {
        handle: d.new_handle(),
        common: Common::default(),
        kind: EntityKind::Line(Line { a: Vec3::new(0.0, 4.0, 0.0), b: Vec3::new(2.0, 4.0, 0.0) }),
    });
    d.blocks.insert(inner.name.clone(), std::sync::Arc::new(inner));
    let insert = |block: &str| {
        EntityKind::Insert(Insert {
            block: block.into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        })
    };
    let mut outer = Block::new("Outer");
    outer.entities.push(Entity { handle: d.new_handle(), common: Common { layer: "Detail".into(), ..Default::default() }, kind: insert("Inner") });
    d.blocks.insert(outer.name.clone(), std::sync::Arc::new(outer));
    d.add(&Space::Model, Common::default(), insert("Outer")).unwrap();
    let cursor = Vec2::new(95.05, 52.04);
    assert_eq!(osnap(&d, &paper, cursor, 0.5, mode::END, None, false).unwrap().point, Vec2::new(95.0, 52.0));
    edit_viewport(&mut d, &paper, |vp| vp.frozen_layers.push("detail".into()));
    assert!(osnap(&d, &paper, cursor, 0.5, mode::END, None, false).is_none());
    assert_eq!(osnap(&d, &Space::Model, Vec2::new(0.1, 4.1), 0.5, mode::END, None, false).unwrap().point, Vec2::new(0.0, 4.0));
}

#[test]
fn clipped_deferred_curves_do_not_hide_a_visible_candidate() {
    let (mut d, paper) = drawing();
    // Hide the viewport border so its perpendicular snap does not win this query.
    d.layers.push(cadcraft_engine::doc::Layer { name: "Viewports".into(), on: false, ..Default::default() });
    d.modify_entity(viewport(&d, &paper), |e| e.common.layer = "Viewports".into()).unwrap();
    for x in [30.2, 29.0] {
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 1.0, 0.0), b: Vec3::new(x, 3.0, 0.0) })).unwrap();
    }
    let hit = osnap(&d, &paper, Vec2::new(110.11, 51.0), 0.8, mode::PER, None, true).unwrap();
    assert_eq!(hit.point, Vec2::new(109.5, 51.0));
    assert!(hit.deferred.is_some());
    assert!(hit.viewport.is_some());
}

#[test]
fn off_viewport_borders_keep_contents_but_frozen_or_hidden_viewports_do_not() {
    let (mut d, paper) = drawing();
    d.layer_mut("0").unwrap().on = false;
    // Put model geometry on a different, visible layer.
    d.layers.push(cadcraft_engine::doc::Layer { name: "Geometry".into(), ..Default::default() });
    let handles: Vec<Handle> = d.model.iter().map(|e| e.handle).collect();
    for h in handles {
        d.modify_entity(h, |e| e.common.layer = "Geometry".into()).unwrap();
    }
    assert!(osnap(&d, &paper, Vec2::new(95.1, 50.1), 0.5, mode::END, None, false).is_some());
    d.layer_mut("0").unwrap().frozen = true;
    assert!(osnap(&d, &paper, Vec2::new(95.1, 50.1), 0.5, mode::END, None, false).is_none());
    d.layer_mut("0").unwrap().frozen = false;
    let h = d.space(&paper).unwrap().iter().find(|e| matches!(e.kind, EntityKind::Viewport(_))).unwrap().handle;
    d.modify_entity(h, |e| e.common.visible = false).unwrap();
    assert!(osnap(&d, &paper, Vec2::new(95.1, 50.1), 0.5, mode::END, None, false).is_none());
}

#[test]
fn deferred_curves_are_projected_into_paper_coordinates() {
    let (mut d, paper) = drawing();
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(10.0, 4.0, 0.0), radius: 2.0 })).unwrap();
    let hit = osnap(&d, &paper, Vec2::new(101.1, 52.0), 0.5, mode::TAN, None, true).unwrap();
    assert!(hit.point.near(Vec2::new(101.0, 52.0), 1e-9));
    match hit.deferred.unwrap().curve {
        SnapCurve::Circle { center, radius } => {
            assert_eq!(center, Vec2::new(100.0, 52.0));
            assert_eq!(radius, 1.0);
        }
        _ => panic!("circle tangent"),
    }
}

#[test]
fn malformed_and_paper_space_viewports_are_not_snap_sources() {
    for (width, height, view_height, id) in [
        (20.0, 20.0, 40.0, 1),
        (-20.0, 20.0, 40.0, 2),
        (20.0, 20.0, 0.0, 2),
        (f64::NAN, 20.0, 40.0, 2),
        (20.0, f64::INFINITY, 40.0, 2),
        (20.0, 1e308, 1e-308, 2),
    ] {
        let (mut d, paper) = drawing();
        edit_viewport(&mut d, &paper, |vp| (vp.width, vp.height, vp.view_height, vp.id) = (width, height, view_height, id));
        assert!(osnap(&d, &paper, Vec2::new(95.1, 50.1), 0.5, mode::END, None, false).is_none());
    }
}
