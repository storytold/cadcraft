use std::collections::BTreeSet;

use cadcraft_doc::{Common, EntityKind, Space, Viewport};
use cadcraft_engine::Session;
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{vec2, Pos2, Rect, Shape};
use serde_json::json;

fn setup_two_viewport_layout() -> Session {
    let mut session = Session::new();
    session.new_drawing(true);
    session.viewport_px = (1600.0, 1000.0);
    assert!(session.settings.gridmode, "Model space should start with grid on");
    assert!(!session.settings.paper_gridmode, "Paper space should start with grid off");

    let paper = Space::Paper("Layout1".into());
    let vp1 = Viewport {
        id: 2,
        center: Vec3::new(50.0, 60.0, 0.0),
        width: 60.0,
        height: 60.0,
        view_center: Vec2::ZERO,
        view_height: 100.0,
        ..Viewport::default()
    };
    let vp2 = Viewport {
        id: 3,
        center: Vec3::new(150.0, 60.0, 0.0),
        width: 60.0,
        height: 60.0,
        view_center: Vec2::ZERO,
        view_height: 100.0,
        ..Viewport::default()
    };
    assert!(!vp1.grid, "new viewports must default to grid off");
    assert!(!vp2.grid, "new viewports must default to grid off");

    let doc = session.doc_mut().unwrap();
    doc.add(&paper, Common::default(), EntityKind::Viewport(vp1)).unwrap();
    doc.add(&paper, Common::default(), EntityKind::Viewport(vp2)).unwrap();
    session.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    session
}

fn step_frame(
    app: &mut CadApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    time: f64,
) -> Vec<egui::epaint::ClippedShape> {
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
        time: Some(time),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    out.shapes
}

fn dbl_click(app: &mut CadApp, ctx: &egui::Context, pos: Pos2, time: &mut f64) -> Vec<egui::epaint::ClippedShape> {
    *time += 1.0;
    step_frame(app, ctx, vec![egui::Event::PointerMoved(pos)], *time);
    for _ in 0..2 {
        for pressed in [true, false] {
            *time += 0.04;
            step_frame(
                app,
                ctx,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
                *time,
            );
        }
    }
    *time += 0.05;
    step_frame(app, ctx, Vec::new(), *time)
}

fn count_segments_in_shape(shape: &Shape, inner: Rect) -> usize {
    match shape {
        Shape::LineSegment { points, .. } => {
            let mid = points[0].lerp(points[1], 0.5);
            usize::from(inner.contains(mid))
        }
        Shape::Vec(v) => v.iter().map(|s| count_segments_in_shape(s, inner)).sum(),
        _ => 0,
    }
}

/// Counts painted `LineSegment` shapes clipped to `outer` whose midpoints cross `inner`.
fn painted_segments_inside(shapes: &[egui::epaint::ClippedShape], outer: Rect, inner: Rect) -> usize {
    shapes
        .iter()
        .filter(|cs| outer.contains_rect(cs.clip_rect))
        .map(|cs| count_segments_in_shape(&cs.shape, inner))
        .sum()
}

fn paper_viewport_rects(app: &CadApp, center_paper: Vec2, w: f64, h: f64) -> (Rect, Rect) {
    let xf = app.canvas.xf.expect("canvas xf");
    let half = Vec2::new(w * 0.5, h * 0.5);
    let min = xf.to_screen(center_paper - half);
    let max = xf.to_screen(center_paper + half);
    let exact = Rect::from_two_pos(min, max);
    (exact.expand(1.5), exact.shrink(4.0))
}

#[test]
fn new_mview_viewport_defaults_grid_off_and_respects_per_viewport_isolation() {
    let mut session = Session::new();
    session.new_drawing(true);
    session.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    session.execute("mview", &json!({"a": [10.0, 10.0], "b": [80.0, 80.0]})).unwrap();
    session.execute("mview", &json!({"a": [100.0, 10.0], "b": [170.0, 80.0]})).unwrap();

    let paper = Space::Paper("Layout1".into());
    let vps: Vec<Viewport> = session
        .doc()
        .unwrap()
        .space(&paper)
        .unwrap()
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) => Some(v.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(vps.len(), 2);
    assert!(vps.iter().all(|v| !v.grid), "mview should initialize viewports with grid off");
    assert!(!session.effective_gridmode(), "Paper space defaults to grid off");
    assert!(session.settings.gridmode, "Model space gridmode remains on");

    session.execute("mspace", &json!({})).unwrap();
    let active_id = session.state().unwrap().active_viewport().map(|(_, v)| v.id).expect("active vp");
    assert!(!session.effective_gridmode(), "active viewport initially has grid off");

    session.execute("grid", &json!({})).unwrap();
    assert!(session.effective_gridmode(), "GRID toggles active viewport on");

    session.execute("pspace", &json!({})).unwrap();
    assert!(!session.effective_gridmode(), "Paper space stays off after toggling active VP");

    let doc = session.doc().unwrap();
    let vps_after: Vec<Viewport> = doc
        .space(&paper)
        .unwrap()
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) => Some(v.clone()),
            _ => None,
        })
        .collect();
    let on_ids: BTreeSet<u32> = vps_after.iter().filter(|v| v.grid).map(|v| v.id).collect();
    assert_eq!(on_ids, BTreeSet::from([active_id]));
}

#[test]
fn grid_persists_per_viewport_and_renders_while_inactive_and_in_front_view() {
    let session = setup_two_viewport_layout();
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let ctx = egui::Context::default();
    let mut time = 0.0;

    let mut shapes = Vec::new();
    for _ in 0..3 {
        time += 0.05;
        shapes = step_frame(&mut app, &ctx, Vec::new(), time);
    }

    let xf_paper = app.canvas.xf.unwrap();
    let (vp1_outer, vp1_inner) = paper_viewport_rects(&app, Vec2::new(50.0, 60.0), 60.0, 60.0);
    let (vp2_outer, vp2_inner) = paper_viewport_rects(&app, Vec2::new(150.0, 60.0), 60.0, 60.0);

    // Initially in Paper Space with both viewports grid=false: no grid segments inside either VP.
    assert_eq!(painted_segments_inside(&shapes, vp1_outer, vp1_inner), 0, "VP1 starts without grid");
    assert_eq!(painted_segments_inside(&shapes, vp2_outer, vp2_inner), 0, "VP2 starts without grid");

    // Enter VP1 by double-clicking it and turn its grid ON via GRID command.
    dbl_click(&mut app, &ctx, xf_paper.to_screen(Vec2::new(50.0, 60.0)), &mut time);
    assert_eq!(app.session.state().unwrap().active_viewport().map(|(_, v)| v.id), Some(2));
    app.session.execute("grid", &json!({})).unwrap();
    time += 0.05;
    shapes = step_frame(&mut app, &ctx, Vec::new(), time);
    assert!(app.session.effective_gridmode());
    assert!(painted_segments_inside(&shapes, vp1_outer, vp1_inner) > 2, "Active VP1 should render grid lines");
    assert_eq!(painted_segments_inside(&shapes, vp2_outer, vp2_inner), 0, "Inactive VP2 remains without grid");

    // By default (GRIDORTHO = 0, AutoCAD style), Front view keeps the XY plane edge-on (no 2D wall grid).
    assert_eq!(app.session.execute("getvar", &json!({"name": "GRIDORTHO"})).unwrap()["value"], json!(0));
    app.session.execute("view.set", &json!({"preset": "front"})).unwrap();
    // Exit to Paper Space (no ViewCube/UCS overlay inside VP1): edge-on XY grid renders 0 grid lines.
    shapes = dbl_click(&mut app, &ctx, xf_paper.to_screen(Vec2::new(100.0, 120.0)), &mut time);
    assert!(app.session.state().unwrap().mspace.is_none());
    assert_eq!(
        painted_segments_inside(&shapes, vp1_outer, vp1_inner),
        0,
        "With GRIDORTHO=0 default, Front view keeps XY plane edge-on with no vertical wall grid"
    );

    // Enable GRIDORTHO = 1: inactive VP1 in Front view now renders the XZ principal-plane grid,
    // while VP2 stays empty and Paper Space gridmode stays off.
    app.session.execute("setvar", &json!({"name": "GRIDORTHO", "value": 1})).unwrap();
    time += 0.05;
    shapes = step_frame(&mut app, &ctx, Vec::new(), time);
    assert!(!app.session.effective_gridmode(), "Paper space grid remains off");
    assert!(
        painted_segments_inside(&shapes, vp1_outer, vp1_inner) > 2,
        "Inactive VP1 with grid=true and GRIDORTHO=1 must display its XZ Front-view grid in Paper Space"
    );
    assert_eq!(
        painted_segments_inside(&shapes, vp2_outer, vp2_inner),
        0,
        "Inactive VP2 with grid=false stays empty"
    );
}
