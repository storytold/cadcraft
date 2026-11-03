//! Panning and zooming inside the active MSPACE viewport (#688). Its contents are drawn apart
//! from the layout's list, built for an area around the viewport and moved to the current view,
//! so realtime navigation redraws them without rebuilding anything. They used to stay frozen:
//! realtime navigation changes the viewport's view without a new revision, and the layout's list
//! kept showing the old view until some command rebuilt it.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Circle, Common, Drawing, EntityKind, Line, Space, Viewport};
use cadcraft_engine::geom::{Bounds2, Vec2, Vec3};
use cadcraft_engine::render::VIEWPORT_CONTENT;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use serde_json::json;

/// Model space: a line from (0,0) to (20,0). Layout1: an 8×6 viewport centred at (5,4) on paper,
/// looking at model (10,0) 40 units high (0.15 paper units per model unit), so the line spans
/// paper x 3.5..6.5 at y 4. `clipped`: the viewport is clipped by a circle of radius 2.9.
fn harness(clipped: bool) -> Harness<'static, CadApp> {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(20.0, 0.0, 0.0) })).unwrap();
    let paper = Space::Paper("Layout1".into());
    let clip =
        clipped.then(|| d.add(&paper, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(5.0, 4.0, 0.0), radius: 2.9 })).unwrap());
    d.add(
        &paper,
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
            clip,
            on: true,
        }),
    )
    .unwrap();
    let mut session = Session::empty();
    session.open_drawing(d, "MSPACE view regression", None);
    session.execute("layout.set", &json!({"name":"Layout1"})).unwrap();
    session.execute("view.set", &json!({"center":[5,4],"height":10})).unwrap();
    session.execute("mspace", &json!({})).unwrap();
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
    // The first view change after opening copies the drawing, still shared with its saved
    // state; the copy is a new document to the canvas and builds once. Get it out of the way.
    {
        let st = h.state_mut().session.state_mut().unwrap();
        let v = st.view();
        st.set_view_quiet(v);
    }
    h.run_steps(2);
    h
}

/// Where the viewport's contents are on paper, and how many times they were built.
fn content(h: &Harness<'_, CadApp>) -> (Bounds2, u64) {
    h.state().canvas.viewport_content().expect("the active viewport's contents")
}

/// The model geometry the layout's own list draws through viewports, on paper.
fn in_layout_list(h: &Harness<'_, CadApp>) -> Bounds2 {
    let list = h.state().canvas.list.as_ref().expect("a display list");
    list.prims
        .iter()
        .filter(|p| p.handle == VIEWPORT_CONTENT)
        .fold(Bounds2::EMPTY, |b, p| b.union(&Bounds2::from_points(list.points(p).iter().copied())))
}

fn revision(h: &Harness<'_, CadApp>) -> u64 {
    h.state().session.state().unwrap().revision
}

/// A middle-drag or trackpad pan step: the quiet view change of realtime navigation.
fn pan(h: &mut Harness<'_, CadApp>, dx: f64) {
    let st = h.state_mut().session.state_mut().unwrap();
    let v = st.view();
    st.set_view_quiet(cadcraft_engine::View { center: v.center - Vec2::new(dx, 0.0), height: v.height });
}

/// A wheel step zooming in by `f` about the viewport's view centre.
fn zoom(h: &mut Harness<'_, CadApp>, f: f64) {
    let about = h.state().session.state().unwrap().active_viewport().unwrap().1.view_center;
    h.state_mut().session.zoom_about(f, about).unwrap();
}

/// The line, on paper at y 4, spans x `min_x..max_x`.
fn spans(b: Bounds2, min_x: f64, max_x: f64) -> bool {
    (b.min.x - min_x).abs() < 1e-9 && (b.max.x - max_x).abs() < 1e-9 && (b.min.y - 4.0).abs() < 1e-9
}

#[test]
fn panning_inside_the_active_viewport_moves_its_contents_without_rebuilding() {
    let mut h = harness(false);
    let (before, builds) = content(&h);
    assert!(spans(before, 3.5, 6.5), "{before:?}");
    // The layout's own list leaves the active viewport's contents out.
    assert!(in_layout_list(&h).is_empty());
    let rev = revision(&h);
    // Panning the model view 4 units left moves the contents 4 × 0.15 = 0.6 paper units right.
    for _ in 0..2 {
        pan(&mut h, 2.0);
        h.run_steps(2);
    }
    assert_eq!(revision(&h), rev, "a realtime pan makes no new revision");
    let (after, after_builds) = content(&h);
    assert!(spans(after, 4.1, 7.1), "{after:?}");
    assert_eq!(after_builds, builds, "panning within the built area only moves the geometry");
    // Panning past the area built around the viewport (half its larger side, 4 paper units =
    // 26.7 model units) builds it again around the new view.
    pan(&mut h, 30.0);
    h.run_steps(2);
    let (far, far_builds) = content(&h);
    assert!(far_builds > builds);
    assert!(spans(far, 8.6, 11.6), "{far:?}");
}

#[test]
fn zooming_inside_the_active_viewport_rebuilds_per_2x_and_once_it_settles() {
    let mut h = harness(false);
    let (_, builds) = content(&h);
    let rev = revision(&h);
    // One wheel step of √2 scales the built geometry: the line spans 5 ± 1.5√2 on paper.
    zoom(&mut h, std::f64::consts::SQRT_2);
    h.run_steps(2);
    let half = 1.5 * std::f64::consts::SQRT_2;
    let (b, n) = content(&h);
    assert!(spans(b, 5.0 - half, 5.0 + half), "{b:?}");
    assert_eq!(n, builds, "zooming within 2x of the built view only scales it");
    // At rest it is built again at the exact view (dash lengths and curve detail follow the
    // viewport's scale); the geometry stays where it is.
    std::thread::sleep(std::time::Duration::from_millis(200));
    h.run_steps(2);
    let (b, n) = content(&h);
    assert_eq!(n, builds + 1);
    assert!(spans(b, 5.0 - half, 5.0 + half), "{b:?}");
    // Two quick steps of √2 (2x from the built view) build again straight away: the line
    // doubles to 5 ± 3√2 (the viewport spans 1..9).
    zoom(&mut h, std::f64::consts::SQRT_2);
    h.run_steps(1);
    zoom(&mut h, std::f64::consts::SQRT_2);
    h.run_steps(1);
    let (b, n2) = content(&h);
    assert_eq!(n2, n + 1);
    assert!(spans(b, 5.0 - 2.0 * half, 5.0 + 2.0 * half), "{b:?}");
    assert_eq!(revision(&h), rev);
}

#[test]
fn leaving_the_viewport_puts_its_contents_back_in_the_layout() {
    let mut h = harness(false);
    pan(&mut h, 4.0);
    h.run_steps(2);
    h.state_mut().session.execute("pspace", &json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().canvas.viewport_content().is_none());
    let b = in_layout_list(&h);
    assert!(spans(b, 4.1, 7.1), "{b:?}");
}

#[test]
fn without_a_stencil_buffer_a_clipped_viewport_follows_realtime_navigation_in_the_layout_list() {
    let mut h = harness(true);
    // No stencil buffer to clip its outline with (the CPU canvas, the web): the layout's list
    // keeps drawing its contents. On the GPU see `gpu_viewport_stencil.rs`.
    assert!(h.state().canvas.viewport_content().is_none());
    assert!(spans(in_layout_list(&h), 3.5, 6.5));
    // Realtime navigation makes no new revision; the list follows the viewport's view anyway.
    for _ in 0..2 {
        pan(&mut h, 2.0);
        h.run_steps(2);
    }
    let b = in_layout_list(&h);
    assert!(spans(b, 4.1, 7.1), "{b:?}");
}
