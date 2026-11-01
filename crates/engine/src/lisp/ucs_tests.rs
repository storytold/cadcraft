//! AutoLISP in a UCS (#1106): `getpoint` and friends answer in the current UCS, `command` takes
//! UCS points, `trans` converts between the WCS, the UCS, display coordinates and an object's
//! OCS, and entity lists stay in world coordinates.

#![allow(clippy::unwrap_used)]

use cadcraft_geom::Vec2;
use serde_json::json;

use super::value::to_text;
use crate::Session;
use crate::cmd::file::{EntityCodec, set_entity_codec};

fn session() -> Session {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let mut s = Session::new();
    s.new_drawing(false);
    s
}

fn ok(s: &mut Session, code: &str) -> String {
    match s.lisp_eval(code, false, true, Vec::new()) {
        Ok(Some(Ok(v))) => to_text(&v, true),
        other => panic!("{code}: {other:?}"),
    }
}

fn err(s: &mut Session, code: &str) -> bool {
    matches!(s.lisp_eval(code, false, true, Vec::new()), Ok(Some(Err(_))) | Err(_))
}

/// The three numbers of a printed point.
fn pt(text: &str) -> [f64; 3] {
    let v: Vec<f64> = text.trim_matches(['(', ')']).split_whitespace().map(|x| x.parse().unwrap()).collect();
    [v[0], v[1], v.get(2).copied().unwrap_or(0.0)]
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// The UCS at (100, 50) turned 90° (UCS X is world +Y).
fn turned(s: &mut Session) {
    s.execute("ucs", &json!({"option": "origin", "origin": [100, 50]})).unwrap();
    s.execute("ucs", &json!({"option": "z", "angle": 90})).unwrap();
}

#[test]
fn trans_between_world_ucs_and_display() {
    let mut s = session();
    turned(&mut s);
    // UCS (10, 0) is world (100, 60).
    assert!(close(pt(&ok(&mut s, "(trans '(10 0 0) 1 0)")), [100.0, 60.0, 0.0]));
    assert!(close(pt(&ok(&mut s, "(trans '(100 60 0) 0 1)")), [10.0, 0.0, 0.0]));
    // A 2D point is at Z 0.
    assert!(close(pt(&ok(&mut s, "(trans '(10 0) 1 0)")), [100.0, 60.0, 0.0]));
    // Displacements turn but don't move.
    assert!(close(pt(&ok(&mut s, "(trans '(10 0 0) 1 0 T)")), [0.0, 10.0, 0.0]));
    // The display system lines up with the world one in a plan view.
    assert!(close(pt(&ok(&mut s, "(trans '(10 0 0) 1 2)")), [100.0, 60.0, 0.0]));
    assert!(close(pt(&ok(&mut s, "(trans '(3 4 5) 2 0)")), [3.0, 4.0, 5.0]));
    // UCS → UCS and back is the identity.
    assert!(close(pt(&ok(&mut s, "(trans (trans '(7 -3 2) 1 0) 0 1)")), [7.0, -3.0, 2.0]));
}

#[test]
fn trans_to_and_from_an_objects_ocs() {
    let mut s = session();
    // A circle in the world XY plane: its OCS is the WCS.
    ok(&mut s, "(command \"_.CIRCLE\" '(5 5) 2)");
    assert!(close(pt(&ok(&mut s, "(trans '(5 5 0) 0 (entlast))")), [5.0, 5.0, 0.0]));
    // An extrusion direction straight up is the WCS too; one along +X follows the arbitrary
    // axis algorithm (OCS X = world Y × N = (0, 0, -1)... the DXF Reference's rule).
    assert!(close(pt(&ok(&mut s, "(trans '(1 2 3) '(0 0 1) 0)")), [1.0, 2.0, 3.0]));
    let w = pt(&ok(&mut s, "(trans '(1 0 0) '(1 0 0) 0)"));
    assert!(close(w, [0.0, 1.0, 0.0]), "{w:?}");
    let back = pt(&ok(&mut s, "(trans (trans '(1 2 3) '(1 0 0) 0) 0 '(1 0 0))"));
    assert!(close(back, [1.0, 2.0, 3.0]), "{back:?}");
    // Bad systems and points are errors, not crashes.
    for bad in [
        "(trans '(1 2) 7 0)",
        "(trans '(1 2) \"x\" 0)",
        "(trans '(1 2) '(0 0 0) 0)",
        "(trans \"p\" 0 1)",
        "(trans '(1 2) 0)",
        "(trans '(1e308 1e308) '(1e-320 0 0) 0)",
    ] {
        assert!(err(&mut s, bad), "{bad}");
    }
}

#[test]
fn getpoint_answers_in_the_ucs_and_command_reads_ucs_points() {
    let mut s = session();
    turned(&mut s);
    s.cmdline("(setq p (getpoint))").unwrap();
    // The user clicks world (100, 60): the routine gets UCS (10, 0).
    s.input(crate::Input::Point(Vec2::new(100.0, 60.0))).unwrap();
    assert!(close(pt(&to_text(&s.lisp_var("P"), true)), [10.0, 0.0, 0.0]), "{}", to_text(&s.lisp_var("P"), true));
    // Typed at getpoint: read in the UCS, given back in it.
    s.cmdline("(setq q (getpoint))").unwrap();
    s.cmdline("3,4").unwrap();
    assert!(close(pt(&to_text(&s.lisp_var("Q"), true)), [3.0, 4.0, 0.0]));
    // `command` takes those points as UCS points: the line lands at the world places clicked.
    ok(&mut s, "(command \"_.LINE\" p q \"\")");
    let d = s.doc().unwrap();
    let l = d.model.iter().find_map(|e| if let cadcraft_doc::EntityKind::Line(l) = &e.kind { Some(l.clone()) } else { None }).unwrap();
    assert!(l.a.xy().dist(Vec2::new(100.0, 60.0)) < 1e-9 && l.b.xy().dist(Vec2::new(96.0, 53.0)) < 1e-9, "{l:?}");
    // Entity lists stay in world coordinates.
    let g = ok(&mut s, "(cdr (assoc 10 (entget (entlast))))");
    assert!(close(pt(&g), [100.0, 60.0, 0.0]), "{g}");
    // `!expr` at the command line is a UCS point too.
    s.cmdline("line").unwrap();
    s.cmdline("!p").unwrap();
    s.cmdline("!q").unwrap();
    s.cmdline("").unwrap();
    let n = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, cadcraft_doc::EntityKind::Line(_))).count();
    assert_eq!(n, 2);
    let d = s.doc().unwrap();
    let last = d.model.iter().filter_map(|e| if let cadcraft_doc::EntityKind::Line(l) = &e.kind { Some(l.clone()) } else { None }).last().unwrap();
    assert!(last.a.xy().dist(Vec2::new(100.0, 60.0)) < 1e-9 && last.b.xy().dist(Vec2::new(96.0, 53.0)) < 1e-9, "{last:?}");
}

/// The rubber band of `getpoint`/`getcorner` with a base point starts where that UCS point is in
/// the world, and `getcorner`'s rectangle runs along the UCS axes.
#[test]
fn getpoint_and_getcorner_previews_start_at_the_ucs_base() {
    let ends = |k: &cadcraft_doc::EntityKind| match k {
        cadcraft_doc::EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        other => panic!("{other:?}"),
    };
    let near = |a: Vec2, b: Vec2| a.dist(b) < 1e-9;
    let mut s = session();
    turned(&mut s);
    // UCS (0, 0) is world (100, 50); the cursor is at world (90, 70), UCS (20, 10).
    let cursor = Vec2::new(90.0, 70.0);
    s.cmdline("(setq p (getpoint '(0 0)))").unwrap();
    let band = s.lisp_preview(cursor).unwrap();
    assert_eq!(band.len(), 1);
    let (a, b) = ends(&band[0]);
    assert!(near(a, Vec2::new(100.0, 50.0)) && near(b, cursor), "{a:?} {b:?}");
    s.input(crate::Input::Point(cursor)).unwrap();
    s.cmdline("(setq c (getcorner '(0 0)))").unwrap();
    let rect: Vec<(Vec2, Vec2)> = s.lisp_preview(cursor).unwrap().iter().map(ends).collect();
    // Along UCS X (world +Y) to UCS (20, 0), the cursor, UCS (0, 10) (world (90, 50)), back.
    let want = [(100.0, 50.0), (100.0, 70.0), (90.0, 70.0), (90.0, 50.0)].map(|(x, y)| Vec2::new(x, y));
    assert_eq!(rect.len(), 4);
    for (i, (a, b)) in rect.iter().enumerate() {
        assert!(near(*a, want[i]) && near(*b, want[(i + 1) % 4]), "side {i}: {a:?} {b:?}");
    }
    s.input(crate::Input::Point(cursor)).unwrap();
    assert!(close(pt(&to_text(&s.lisp_var("C"), true)), [20.0, 10.0, 0.0]), "the corner it returns, in the UCS");
}

#[test]
fn getangle_and_getdist_measure_in_the_ucs() {
    let mut s = session();
    turned(&mut s);
    // Two clicks one above the other in the world: along the UCS X axis, angle 0.
    s.cmdline("(setq a (getangle))").unwrap();
    s.input(crate::Input::Point(Vec2::new(100.0, 50.0))).unwrap();
    s.input(crate::Input::Point(Vec2::new(100.0, 70.0))).unwrap();
    let a: f64 = to_text(&s.lisp_var("A"), true).parse().unwrap();
    assert!(a.abs() < 1e-6 || (a - std::f64::consts::TAU).abs() < 1e-6, "{a}");
    // A typed angle is measured in the UCS as well.
    s.cmdline("(setq b (getangle))").unwrap();
    s.cmdline("45").unwrap();
    let b: f64 = to_text(&s.lisp_var("B"), true).parse().unwrap();
    assert!((b - std::f64::consts::FRAC_PI_4).abs() < 1e-6, "{b} (printed to 6 places)");
    // Distances don't depend on the UCS.
    s.cmdline("(setq d (getdist '(0 0)))").unwrap();
    s.cmdline("3,4").unwrap();
    assert_eq!(to_text(&s.lisp_var("D"), true), "5.0");
    // The UCS variables read from LISP.
    assert!(close(pt(&ok(&mut s, "(getvar \"UCSORG\")")), [100.0, 50.0, 0.0]));
    assert!(close(pt(&ok(&mut s, "(getvar \"UCSXDIR\")")), [0.0, 1.0, 0.0]));
    assert_eq!(ok(&mut s, "(getvar \"WORLDUCS\")"), "0");
}

#[test]
fn entsel_and_getcorner_answer_in_the_ucs() {
    let mut s = session();
    s.execute("line", &json!({"points": [[100, 60], [100, 80]]})).unwrap();
    turned(&mut s);
    // entsel: the object, and the pick point in the UCS.
    s.cmdline("(setq e (entsel))").unwrap();
    s.input(crate::Input::Point(Vec2::new(100.0, 70.0))).unwrap();
    let v = to_text(&s.lisp_var("E"), true);
    assert!(v.contains("<Entity name"), "{v}");
    let p = ok(&mut s, "(cadr e)");
    assert!(close(pt(&p), [20.0, 0.0, 0.0]), "{p}");
    // entsel with the point typed in the UCS picks the object at its world place.
    s.cmdline("(setq e2 (entsel))").unwrap();
    s.cmdline("15,0").unwrap();
    assert!(to_text(&s.lisp_var("E2"), true).contains("<Entity name"), "{}", to_text(&s.lisp_var("E2"), true));
    // getcorner: the base point is a UCS point; the answer is in the UCS.
    s.cmdline("(setq c (getcorner '(0 0) \"Corner: \"))").unwrap();
    s.input(crate::Input::Point(Vec2::new(95.0, 60.0))).unwrap();
    assert!(close(pt(&to_text(&s.lisp_var("C"), true)), [10.0, 5.0, 0.0]), "{}", to_text(&s.lisp_var("C"), true));
    // A miss asks again rather than answering.
    s.cmdline("(setq e3 (entsel))").unwrap();
    s.input(crate::Input::Point(Vec2::new(500.0, 500.0))).unwrap();
    assert!(s.lisp_running(), "Missed, try again");
    s.cancel();
}
