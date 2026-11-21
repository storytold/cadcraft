//! CONSTRAINTINFER: constraints inferred for what a drawing or editing command places with
//! object snaps, ortho or polar (#546).

use cadcraft_doc::{ConstraintKind as K, EntityKind, Handle, Sub};
use cadcraft_engine::Session;
use serde_json::json;

/// A session with a circle at 20,20 radius 3 (not constrained) and CONSTRAINTINFER on.
fn session() -> Session {
    let mut s = Session::new();
    s.script("CIRCLE 20,20 3\n").unwrap();
    s.cmdline("constraintinfer 1").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(s.execute("getvar", &json!({ "name": "CONSTRAINTINFER" })).unwrap()["value"], json!(1));
    s
}

fn kinds(s: &Session) -> Vec<K> {
    s.doc().unwrap().constraints.iter().map(|c| c.kind).collect()
}

fn count(s: &Session, k: K) -> usize {
    kinds(s).iter().filter(|x| **x == k).count()
}

fn lines(s: &Session) -> Vec<Handle> {
    s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).map(|e| e.handle).collect()
}

fn circle(s: &Session) -> Handle {
    s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Circle(_))).map(|e| e.handle).unwrap()
}

#[test]
fn off_by_default_and_settable_by_setvar() {
    let mut s = Session::new();
    s.script("LINE 0,0 10,0 10,5\n\n").unwrap();
    assert!(kinds(&s).is_empty());
    assert_eq!(s.execute("getvar", &json!({ "name": "constraintinfer" })).unwrap()["value"], json!(0));
    s.execute("setvar", &json!({ "name": "CONSTRAINTINFER", "value": 1 })).unwrap();
    assert!(s.doc().unwrap().parametric.settings.infer);
    assert!(s.execute("setvar", &json!({ "name": "CONSTRAINTINFER", "value": "yes" })).is_err());
    for bad in [json!({ "value": 2 }), json!({ "value": "x" }), json!({ "value": [] }), json!({ "value": f64::MAX })] {
        assert!(s.execute("constraintinfer", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.execute("constraintinfer", &json!({})).unwrap()["value"], json!(1));
    s.execute("constraintinfer", &json!({ "value": false })).unwrap();
    assert!(!s.doc().unwrap().parametric.settings.infer);
    // At the command line: Enter keeps the value, a bad one re-prompts.
    s.cmdline("constraintinfer").unwrap();
    assert!(s.prompt_text().contains("CONSTRAINTINFER"), "{}", s.prompt_text());
    s.cmdline("7").unwrap();
    assert!(s.running.is_some());
    s.cmdline("1").unwrap();
    assert!(s.running.is_none() && s.doc().unwrap().parametric.settings.infer);
}

#[test]
fn line_segments_join_and_ortho_directions_are_kept() {
    let mut s = session();
    s.script("LINE 0,0 10,0 10,5\n\n").unwrap();
    // The joint, and the horizontal and vertical segments.
    assert_eq!(count(&s, K::Coincident), 1, "{:?}", kinds(&s));
    assert_eq!(count(&s, K::Horizontal), 1, "{:?}", kinds(&s));
    assert_eq!(count(&s, K::Vertical), 1, "{:?}", kinds(&s));
    // A sloped line drawn exactly parallel to another (a parallel snap) is kept parallel.
    s.script("LINE 0,10 6,13\n\nLINE 0,15 @6,3\n\n").unwrap();
    assert_eq!(count(&s, K::Parallel), 1, "{:?}", kinds(&s));
    // Hand-placed points that touch nothing infer nothing.
    let before = kinds(&s).len();
    s.script("LINE 30.3,0.2 37.1,3.3\n\n").unwrap();
    assert_eq!(kinds(&s).len(), before);
    // Undo takes the inferred constraints with the line.
    s.execute("u", &json!({})).unwrap();
    assert_eq!(lines(&s).len(), 4);
    assert_eq!(kinds(&s).len(), before);
}

#[test]
fn snapped_points_become_coincident_and_tangent() {
    let mut s = session();
    let c = circle(&s);
    // Centre snap, then an endpoint snap back to the first line.
    s.script("LINE 0,0 9,1\n\nLINE CEN 20.05,19.98 END 8.98,1.01\n\n").unwrap();
    let d = s.doc().unwrap();
    let centre = d.constraints.iter().find(|k| k.refs.iter().any(|r| r.handle == c && r.sub == Sub::Center));
    assert!(centre.is_some_and(|k| k.kind == K::Coincident), "{:?}", d.constraints);
    let ls = lines(&s);
    let joined = d.constraints.iter().any(|k| k.kind == K::Coincident && k.refs.iter().all(|r| ls.contains(&r.handle)));
    assert!(joined, "{:?}", d.constraints);
    // A tangent snap to the circle: on it and tangent to it.
    s.script("LINE 40,20 TAN 20.45,22.97\n\n").unwrap();
    let last = *lines(&s).last().unwrap();
    let d = s.doc().unwrap();
    let with = |k: K| d.constraints.iter().any(|x| x.kind == k && x.refs.iter().any(|r| r.handle == last) && x.refs.iter().any(|r| r.handle == c));
    assert!(with(K::Tangent), "{:?}", d.constraints);
    // A perpendicular snap onto the first line, ending on it.
    s.script("LINE 3,8 PER 3.84,0.43\n\n").unwrap();
    let last = *lines(&s).last().unwrap();
    let first = *lines(&s).first().unwrap();
    let d = s.doc().unwrap();
    let pair =
        |k: K| d.constraints.iter().any(|x| x.kind == k && x.refs.iter().any(|r| r.handle == last) && x.refs.iter().any(|r| r.handle == first));
    assert!(pair(K::Perpendicular) && pair(K::Coincident), "{:?}", d.constraints);
    assert_eq!(s.execute("constraints.inspect", &json!({})).unwrap()["satisfied"], json!(true));
}

#[test]
fn editing_with_snaps_infers_and_constraint_commands_do_not() {
    let mut s = session();
    s.script("LINE 0,0 10,0\n\nLINE 30,7 34,9\n\n").unwrap();
    let ls = lines(&s);
    let n0 = kinds(&s).len();
    // MOVE the second line's start exactly onto the first line's end.
    s.execute("move", &json!({ "handles": [ls[1].hex()], "from": [30, 7], "to": [10, 0] })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.constraints.iter().skip(n0).any(|k| k.kind == K::Coincident), "{:?}", d.constraints);
    // A constraint command adds only its own constraint.
    let n = kinds(&s).len();
    s.execute("gcequal", &json!({ "h1": ls[0].hex(), "h2": ls[1].hex() })).unwrap();
    assert_eq!(kinds(&s).len(), n + 1);
    // Many objects at once (an array) are not inferred.
    let n = kinds(&s).len();
    let before = s.doc().unwrap().model.len();
    s.execute("arrayrect", &json!({ "handles": [ls[0].hex()], "rows": 1, "cols": 60, "rowSpacing": 1, "colSpacing": 0.5, "associative": false }))
        .unwrap();
    assert!(s.doc().unwrap().model.len() > before + 50);
    assert_eq!(kinds(&s).len(), n);
}
