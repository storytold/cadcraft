//! Angle prompts read typed angles with the drawing's settings (#353): AUNITS for bare numbers,
//! rotations turn clockwise when ANGDIR is 1, directions are measured from ANGBASE.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use cadcraft_engine::Session;
use cadcraft_engine::doc::{DimKind, EntityKind};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

const QUARTER: &str = "1.5707963267948966";
const EIGHTH: &str = "0.7853981633974483";

/// Radians, clockwise angles, 0 = north.
fn session() -> Session {
    let mut s = Session::new();
    for (name, value) in [("AUNITS", json!(3)), ("ANGDIR", json!(1)), ("ANGBASE", json!(90))] {
        s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
    }
    s
}

fn last(s: &Session) -> EntityKind {
    s.doc().unwrap().model.last().unwrap().kind.clone()
}

/// `a - b` turned into (-π, π].
fn diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(2.0 * PI);
    if d > PI { d - 2.0 * PI } else { d }
}

#[test]
fn rotate_angle_is_a_rotation_in_aunits() {
    let mut s = session();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.script(&format!("ROTATE ALL\n\n0,0 {QUARTER}\n")).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    // A quarter turn clockwise (ANGDIR 1), not 1.57 degrees counterclockwise.
    match last(&s) {
        EntityKind::Line(l) => assert!(l.b.xy().near(Vec2::new(0.0, -10.0), 1e-9), "{:?}", l.b),
        k => panic!("expected a line, got {k:?}"),
    }
}

#[test]
fn hatch_angle_is_a_rotation_in_aunits() {
    let mut s = session();
    s.script(&format!("HATCH A {QUARTER}\n\n")).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let a = s.doc().unwrap().header.f64("HPANG", 0.0);
    assert!(diff(a, -FRAC_PI_2).abs() < 1e-9, "HPANG {a}");
}

#[test]
fn dimension_line_and_text_angles_are_directions() {
    let mut s = session();
    // Rotated 0 points at ANGBASE (north); the text angle a quarter turn clockwise from north is east.
    s.script(&format!("DIMLINEAR 0,0 10,0 R 0 A {QUARTER} 5,5\n")).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    match last(&s) {
        EntityKind::Dimension(d) => {
            let DimKind::Linear { rotation } = d.kind else { panic!("expected a linear dimension, got {:?}", d.kind) };
            assert!(diff(rotation, FRAC_PI_2).abs() < 1e-9, "rotation {rotation}");
            assert!(diff(d.text_rotation, 0.0).abs() < 1e-9, "text rotation {}", d.text_rotation);
        }
        k => panic!("expected a dimension, got {k:?}"),
    }
}

#[test]
fn chamfer_and_mleader_angles_are_amounts_in_aunits() {
    let mut s = session();
    for l in ["0,0 12,0", "10,-2 10,12"] {
        s.cmdline(&format!("LINE {l}")).unwrap();
        s.cmdline("").unwrap();
    }
    // Length 2 on the first line at an eighth turn from it, whatever ANGBASE and ANGDIR say.
    s.script(&format!("CHAMFER A 2 {EIGHTH} 5,0 10,5\n")).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let d = s.doc().unwrap().header.f64("CHAMFERD", 0.0);
    assert!((d - FRAC_PI_4).abs() < 1e-9, "CHAMFERD {d}");
    let chamfer = s.doc().unwrap().model.iter().any(|e| match &e.kind {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            (a.near(Vec2::new(8.0, 0.0), 1e-6) && b.near(Vec2::new(10.0, 2.0), 1e-6))
                || (b.near(Vec2::new(8.0, 0.0), 1e-6) && a.near(Vec2::new(10.0, 2.0), 1e-6))
        }
        _ => false,
    });
    assert!(chamfer, "{:?}", s.doc().unwrap().model.iter().map(|e| &e.kind).collect::<Vec<_>>());

    // The first angle constraint snaps the leader to eighth-turn steps.
    s.script(&format!("MLEADER O F {EIGHTH} X 0,0 10,8 note\n")).unwrap();
    match last(&s) {
        EntityKind::MLeader(m) => {
            let r = Vec2::new(10.0, 8.0).len();
            assert!(m.landing.xy().near(Vec2::new(r, r) / 2f64.sqrt(), 1e-6), "{:?}", m.landing);
        }
        k => panic!("expected a multileader, got {k:?}"),
    }
}
