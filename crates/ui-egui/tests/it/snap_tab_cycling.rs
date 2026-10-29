//! Tab at a point prompt steps through every object snap under the aperture (each endpoint of
//! each object nearby), not just the closest snap of each running mode, and comes back round.

use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, vec2};
use egui_kittest::Harness;
use serde_json::json;

#[test]
fn tab_cycles_through_every_endpoint_under_the_aperture() {
    let mut s = Session::new();
    // Three line ends close together (within the aperture of the cursor between them).
    for (a, b) in [([0.0, 0.0], [10.0, 0.0]), ([0.06, 0.05], [0.06, 10.0]), ([0.03, -0.06], [-10.0, -10.0])] {
        s.execute("line", &json!({ "points": [a, b] })).unwrap();
    }
    s.execute("setvar", &json!({ "name": "OSMODE", "value": 1 })).unwrap();
    s.execute("view.set", &json!({ "center": [0, 0], "height": 20 })).unwrap();
    let mut app = CadApp::new(s, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.state_mut().session.start("line").unwrap();
    h.run_steps(2);
    let xf = h.state().canvas.xf.unwrap();
    let at = xf.to_screen(Vec2::new(0.03, 0.0));
    h.event(Event::PointerMoved(at));
    h.run_steps(2);
    let mut seen: Vec<Vec2> = Vec::new();
    for i in 0..4 {
        let p = h.state().canvas.snap.map(|s| s.point).unwrap();
        if i < 3 {
            assert!(!seen.iter().any(|q| q.near(p, 1e-9)), "Tab {i} repeats {p:?} ({seen:?})");
        } else {
            assert!(seen.first().is_some_and(|q| q.near(p, 1e-9)), "the fourth Tab comes back round");
        }
        seen.push(p);
        h.event(Event::Text("\t".into()));
        h.run_steps(2);
    }
    for want in [Vec2::new(0.0, 0.0), Vec2::new(0.06, 0.05), Vec2::new(0.03, -0.06)] {
        assert!(seen.iter().any(|q| q.near(want, 1e-9)), "{want:?} in {seen:?}");
    }
}
