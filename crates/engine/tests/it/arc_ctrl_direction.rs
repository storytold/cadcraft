//! ARC's "hold Ctrl to switch direction": the UI writes `Session::ctrl_down` every frame and
//! the command reads it. Two cases, and they flip differently on purpose:
//!
//! * Continue (the tangent comes from the previous curve): negating the start tangent makes
//!   `arc_sed` pick the mirror arc through the same two endpoints.
//! * Center,Start,End (the two picks fix the chord): swapping the start/end angles walks the
//!   other way around the same circle.
//!
//! Both should land on the same circle with its two halves swapped, so the assertions below
//! are the same: same center, same radius, and the start/end angles are exchanged.

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};

/// `(center, radius, start angle, end angle)` of the last arc in the model.
fn last_arc(s: &Session) -> (Vec2, f64, f64, f64) {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::Arc(a) => (a.center.xy(), a.radius, a.start, a.end),
        k => panic!("expected an arc, got {k:?}"),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// CCW sweep from `start` to `end`, both normalised to `[0, TAU)`.
fn sweep(start: f64, end: f64) -> f64 {
    (end - start).rem_euclid(std::f64::consts::TAU)
}

/// Draw a line from the origin to (10,0), then `arc` + Enter continues from its end at (10,0)
/// with the outgoing tangent +X. The end point is picked at (20,5).
fn run_continue(ctrl: bool) -> (Vec2, f64, f64, f64) {
    let mut s = Session::new();
    s.cmdline("line").unwrap();
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(10.0, 0.0))).unwrap();
    s.input(Input::Enter).unwrap();
    s.cmdline("arc").unwrap();
    // Enter at the first point prompt continues the last curve.
    s.input(Input::Enter).unwrap();
    assert!(s.prompt_text().contains("hold Ctrl"), "Continue prompt should advertise Ctrl: {}", s.prompt_text());
    s.ctrl_down = ctrl;
    s.input(Input::Point(Vec2::new(20.0, 5.0))).unwrap();
    last_arc(&s)
}

/// ARC Center,Start,End: center (0,0), start (5,0) at 0°, end (0,5) at 90°.
fn run_cse(ctrl: bool) -> (Vec2, f64, f64, f64) {
    let mut s = Session::new();
    s.cmdline("arc c").unwrap();
    s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    s.input(Input::Point(Vec2::new(5.0, 0.0))).unwrap();
    assert!(s.prompt_text().contains("hold Ctrl"), "Cse end-point prompt should advertise Ctrl: {}", s.prompt_text());
    s.ctrl_down = ctrl;
    s.input(Input::Point(Vec2::new(0.0, 5.0))).unwrap();
    last_arc(&s)
}

#[test]
fn continue_ctrl_flips_the_arc_through_the_same_endpoints() {
    let plain = run_continue(false);
    let flipped = run_continue(true);
    assert!(plain.0.dist(flipped.0) < 1e-9, "center moved: {:?} vs {:?}", plain.0, flipped.0);
    assert!(near(plain.1, flipped.1), "radius changed: {} vs {}", plain.1, flipped.1);
    assert!(
        near(plain.2, flipped.3) && near(plain.3, flipped.2),
        "Ctrl must swap start/end angles: plain ({}, {}) vs flipped ({}, {})",
        plain.2,
        plain.3,
        flipped.2,
        flipped.3
    );
    let total = sweep(plain.2, plain.3) + sweep(flipped.2, flipped.3);
    assert!(
        near(total, std::f64::consts::TAU),
        "the two arcs must be the two halves of the circle: {} + {} != TAU",
        sweep(plain.2, plain.3),
        sweep(flipped.2, flipped.3)
    );
}

#[test]
fn cse_ctrl_reverses_by_swapping_the_endpoint_angles() {
    let plain = run_cse(false);
    let flipped = run_cse(true);
    assert!(plain.0.dist(flipped.0) < 1e-9, "center moved: {:?} vs {:?}", plain.0, flipped.0);
    assert!(near(plain.1, flipped.1), "radius changed: {} vs {}", plain.1, flipped.1);
    assert!(
        near(plain.2, flipped.3) && near(plain.3, flipped.2),
        "Ctrl must swap start/end angles: plain ({}, {}) vs flipped ({}, {})",
        plain.2,
        plain.3,
        flipped.2,
        flipped.3
    );
    let total = sweep(plain.2, plain.3) + sweep(flipped.2, flipped.3);
    assert!(near(total, std::f64::consts::TAU), "sum of sweeps must be TAU");
}

/// Sanity: without Ctrl the Continue arc uses the tangent as-is (center above the chord).
#[test]
fn no_ctrl_is_the_old_behavior() {
    let plain = run_continue(false);
    assert!(plain.0.y > 0.0, "tangent as-is puts the center above the chord: {:?}", plain.0);
}
