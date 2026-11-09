//! Original 2D drafting acceptance fixtures: coordinate entry, edits and DXF persistence.
//! Expected results come from analytic geometry, without requiring a reference application.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, EntityKind};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn segments(d: &Drawing) -> Vec<[f64; 4]> {
    d.model
        .iter()
        .map(|e| match &e.kind {
            EntityKind::Line(l) => [l.a.x, l.a.y, l.b.x, l.b.y],
            other => panic!("expected only lines, got {other:?}"),
        })
        .collect()
}

fn assert_segments(d: &Drawing, expected: &[[f64; 4]]) {
    let actual = segments(d);
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.iter().zip(expected) {
        for (a, b) in a.iter().zip(b) {
            assert!((a - b).abs() < 1e-9, "actual {actual:?}, expected {expected:?}");
        }
    }
}

fn assert_round_trips(d: &Drawing, expected: &[[f64; 4]]) {
    let mut d = d.clone();
    for _ in 0..2 {
        d = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
        assert_segments(&d, expected);
    }
}

#[test]
fn absolute_relative_and_polar_paths_make_the_same_closed_outline() {
    // A translated 18 by 12 outline crosses both axes. Negative deltas and a nonzero
    // starting point distinguish relative input from accidentally absolute input.
    let expected = [[-7., -5., 11., -5.], [11., -5., 11., 7.], [11., 7., -7., 7.], [-7., 7., -7., -5.]];
    for script in [
        "LINE -7,-5 11,-5 11,7 -7,7 C\n",
        "LINE -7,-5 @18,0 @0,12 @-18,0 C\n",
        "LINE -7,-5 @18<0 @12<90 @18<180 C\n",
        "LINE #-7,-5 @18,0 #11,7 @-18,0 C\n",
    ] {
        let mut s = Session::new();
        s.script(script).unwrap();
        assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
        assert_segments(s.doc().unwrap(), &expected);
        assert_round_trips(s.doc().unwrap(), &expected);
    }
}

#[test]
fn a_relative_move_can_be_undone_redone_and_saved() {
    let mut s = Session::new();
    s.script("LINE -7,-5 @18,0\n\n").unwrap();
    let original = [[-7., -5., 11., -5.]];
    assert_segments(s.doc().unwrap(), &original);
    // Select the line, finish selection, then give a base point and relative destination.
    s.script("MOVE ALL\n\n2,3 @-4,9\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let moved = [[-11., 4., 7., 4.]];
    assert_segments(s.doc().unwrap(), &moved);
    s.execute("undo", &json!({})).unwrap();
    assert_segments(s.doc().unwrap(), &original);
    s.execute("redo", &json!({})).unwrap();
    assert_segments(s.doc().unwrap(), &moved);
    assert_round_trips(s.doc().unwrap(), &moved);
}

#[test]
fn direct_distance_follows_the_cursor_with_ortho() {
    let mut s = Session::new();
    s.settings.orthomode = true;
    s.cmdline("LINE -7,-5").unwrap();
    s.cursor = Vec2::new(30., -4.);
    s.cmdline("18").unwrap();
    s.cursor = Vec2::new(12., 40.);
    s.cmdline("12").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    let expected = [[-7., -5., 11., -5.], [11., -5., 11., 7.]];
    assert_segments(s.doc().unwrap(), &expected);
    assert_round_trips(s.doc().unwrap(), &expected);
}
