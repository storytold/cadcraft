//! HATCHGENERATEBOUNDARY (#2801): closed polylines around existing hatches, which stay as they are.

use cadcraft_engine::doc::{EntityKind, Handle, Hatch, LwPolyline};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn hatch(s: &Session, h: &str) -> Hatch {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::Hatch(x) => x.clone(),
        k => panic!("not a hatch: {k:?}"),
    }
}

fn poly(s: &Session, h: &str) -> LwPolyline {
    match &s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).unwrap().kind {
        EntityKind::LwPolyline(p) => p.clone(),
        k => panic!("not a polyline: {k:?}"),
    }
}

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

fn handles(v: &Value) -> Vec<String> {
    v["handles"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect()
}

fn rect(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> String {
    handle(&s.execute("rectang", &json!({ "p1": a, "p2": b })).unwrap())
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.iter().count()
}

#[test]
fn generates_a_closed_polyline_around_a_hatch_without_its_outline() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r.clone()] })).unwrap());
    s.execute("erase", &json!({ "handles": [r] })).unwrap();
    let before = hatch(&s, &h);
    let v = s.execute("hatchgenerateboundary", &json!({ "handles": [h.clone()] })).unwrap();
    let new = handles(&v);
    assert_eq!(new.len(), 1);
    let p = poly(&s, &new[0]);
    assert!(p.closed);
    assert_eq!(p.vertices, before.loops[0].vertices);
    assert_eq!(hatch(&s, &h), before, "the hatch is not changed");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(count(&s), 1, "one undo step removes what was generated");
}

#[test]
fn islands_get_their_own_polylines() {
    let mut s = Session::new();
    rect(&mut s, [0.0, 0.0], [20.0, 20.0]);
    rect(&mut s, [5.0, 5.0], [10.0, 10.0]);
    let h = handle(&s.execute("hatch", &json!({ "points": [[2, 2]] })).unwrap());
    assert_eq!(hatch(&s, &h).loops.len(), 2);
    let v = s.execute("hatchgenerateboundary", &json!({ "handles": [h] })).unwrap();
    assert_eq!(handles(&v).len(), 2);
}

#[test]
fn every_selected_hatch_gets_its_boundary() {
    let mut s = Session::new();
    let a = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let b = rect(&mut s, [10.0, 0.0], [14.0, 2.0]);
    let ha = handle(&s.execute("hatch", &json!({ "handles": [a] })).unwrap());
    let hb = handle(&s.execute("hatch", &json!({ "handles": [b] })).unwrap());
    let v = s.execute("hatchgenerateboundary", &json!({ "handles": [ha, hb] })).unwrap();
    assert_eq!(handles(&v).len(), 2);
}

#[test]
fn an_associative_hatch_stays_linked_to_its_own_boundary() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r.clone()], "associative": true })).unwrap());
    let v = s.execute("hatchgenerateboundary", &json!({ "handles": [h.clone()] })).unwrap();
    let p = handles(&v)[0].clone();
    // Moving the generated polyline leaves the hatch where it is.
    s.execute("move", &json!({ "handles": [p], "delta": [0, 10] })).unwrap();
    assert_eq!(hatch(&s, &h).loops[0].vertices[0].p.y, 0.0);
    // Moving the original rectangle still moves the hatch.
    s.execute("move", &json!({ "handles": [r], "delta": [10, 0] })).unwrap();
    assert!(hatch(&s, &h).associative);
    assert_eq!(hatch(&s, &h).loops[0].vertices[0].p.x, 10.0);
}

#[test]
fn boundaries_keep_the_hatch_elevation() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r] })).unwrap());
    s.doc_mut()
        .unwrap()
        .modify_entity(Handle::parse_hex(&h).unwrap(), |e| {
            if let EntityKind::Hatch(x) = &mut e.kind {
                x.elevation = 300.0;
            }
        })
        .unwrap();
    let v = s.execute("hatchgenerateboundary", &json!({ "handles": [h] })).unwrap();
    assert_eq!(poly(&s, &handles(&v)[0]).elevation, 300.0);
}

#[test]
fn refuses_a_hatch_in_another_space_and_adds_nothing() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r] })).unwrap());
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let before = s.doc().unwrap().entity_count();
    let e = s.execute("hatchgenerateboundary", &json!({ "handles": [h] })).unwrap_err();
    assert!(e.to_string().contains("not in the current space"), "{e}");
    assert_eq!(s.doc().unwrap().entity_count(), before);
}

#[test]
fn refuses_more_boundaries_than_the_limit_and_adds_nothing() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r] })).unwrap());
    // One hatch with more loops than one call may turn into polylines.
    s.doc_mut()
        .unwrap()
        .modify_entity(Handle::parse_hex(&h).unwrap(), |e| {
            if let EntityKind::Hatch(x) = &mut e.kind {
                let l = x.loops[0].clone();
                x.loops = vec![l; 10_001];
            }
        })
        .unwrap();
    let before = count(&s);
    assert!(s.execute("hatchgenerateboundary", &json!({ "handles": [h] })).is_err());
    assert_eq!(count(&s), before);
}

#[test]
fn needs_a_hatch() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    assert!(s.execute("hatchgenerateboundary", &json!({ "handles": [r] })).is_err());
    assert_eq!(count(&s), 1, "nothing is added");
}

#[test]
fn runs_at_once_on_selected_hatches() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r] })).unwrap());
    s.set_selection(vec![Handle::parse_hex(&h).unwrap()]);
    s.start("hatchgenerateboundary").unwrap();
    assert!(s.current_prompt().is_none(), "a pickfirst selection needs no prompt");
    assert_eq!(count(&s), 3);
}

#[test]
fn asks_for_hatches_without_a_selection() {
    let mut s = Session::new();
    let r = rect(&mut s, [0.0, 0.0], [4.0, 2.0]);
    let h = handle(&s.execute("hatch", &json!({ "handles": [r] })).unwrap());
    s.set_selection(Vec::new());
    s.start("hatchgenerateboundary").unwrap();
    assert!(s.current_prompt().is_some());
    s.input(Input::Pick(vec![Handle::parse_hex(&h).unwrap()])).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.current_prompt().is_none());
    assert_eq!(count(&s), 3);
}
