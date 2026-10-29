//! Tab cycling through object snaps: a crowd of candidates is capped, hostile cursors and
//! apertures give no candidates, and the list never repeats a snap.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Space;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::pointmod::snap_candidates;
use cadcraft_engine::snap::mode;
use serde_json::json;

#[test]
fn a_crowd_of_snaps_is_capped_and_never_repeats() {
    let mut s = Session::new();
    // 600 short lines fanning out from points packed round the origin: 1,200 endpoints.
    for i in 0..600 {
        let a = f64::from(i) * 0.01;
        let p = [1e-3 * a.cos(), 1e-3 * a.sin()];
        s.execute("line", &json!({ "points": [p, [a.cos(), a.sin()]] })).unwrap();
    }
    let d = s.doc().unwrap();
    let cands = snap_candidates(d, &Space::Model, Vec2::ZERO, 0.5, mode::END | mode::INT | mode::NEA, None, false);
    assert!(!cands.is_empty());
    assert!(cands.len() <= 257, "{} candidates", cands.len());
    for (i, a) in cands.iter().enumerate() {
        assert!(a.point.is_finite());
        assert!(!cands.iter().skip(i + 1).any(|b| b.mode == a.mode && b.point == a.point), "{a:?} repeats");
    }
}

#[test]
fn hostile_cursor_and_aperture_give_no_candidates() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let d = s.doc().unwrap();
    let space = Space::Model;
    let all = mode::END | mode::MID | mode::INT | mode::NEA | mode::PER;
    for (c, ap) in [
        (Vec2::new(f64::NAN, 0.0), 0.5),
        (Vec2::new(0.0, f64::INFINITY), 0.5),
        (Vec2::ZERO, f64::NAN),
        (Vec2::ZERO, 0.0),
        (Vec2::ZERO, -1.0),
        (Vec2::ZERO, f64::INFINITY),
    ] {
        assert!(snap_candidates(d, &space, c, ap, all, None, false).is_empty(), "{c:?} {ap}");
        assert!(snap_candidates(d, &space, c, ap, all, Some(Vec2::new(f64::NAN, 1.0)), true).is_empty(), "{c:?} {ap}");
    }
    // A huge but finite aperture still answers, from the objects there are.
    let far = snap_candidates(d, &space, Vec2::ZERO, 1e300, mode::END, None, false);
    assert!(far.iter().all(|h| h.point.is_finite()));
    // Object snaps off (or only the OFF bit) offer nothing.
    assert!(snap_candidates(d, &space, Vec2::ZERO, 0.5, 0, None, false).is_empty());
    assert!(snap_candidates(d, &space, Vec2::ZERO, 0.5, mode::END | mode::OFF, None, false).is_empty());
}

#[test]
fn every_endpoint_under_the_aperture_is_offered_not_one_per_mode() {
    let mut s = Session::new();
    // Three line ends close together, all within the aperture of the cursor between them.
    for (a, b) in [([0.0, 0.0], [10.0, 0.0]), ([0.06, 0.05], [0.06, 10.0]), ([0.03, -0.06], [-10.0, -10.0])] {
        s.execute("line", &json!({ "points": [a, b] })).unwrap();
    }
    let d = s.doc().unwrap();
    let cands = snap_candidates(d, &Space::Model, Vec2::new(0.03, 0.0), 0.125, mode::END, None, false);
    let ends: Vec<Vec2> = cands.iter().filter(|h| h.mode == mode::END).map(|h| h.point).collect();
    assert_eq!(ends.len(), 3, "{cands:?}");
    for want in [Vec2::new(0.0, 0.0), Vec2::new(0.06, 0.05), Vec2::new(0.03, -0.06)] {
        assert!(ends.iter().any(|p| p.near(want, 1e-12)), "{want:?} in {ends:?}");
    }
    // The first is the snap a click would take.
    let first = snap_candidates(d, &Space::Model, Vec2::new(0.03, 0.0), 0.125, mode::END, None, false).first().map(|h| h.point);
    assert_eq!(first, cadcraft_engine::snap::osnap(d, &Space::Model, Vec2::new(0.03, 0.0), 0.125, mode::END, None, false).map(|h| h.point));
}
