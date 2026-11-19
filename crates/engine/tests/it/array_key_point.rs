//! The Key point option of a base point: ARRAYRECT, ARRAYPOLAR and ARRAYEDIT (Base point and
//! REPlace) take the source objects' key point (an end point, midpoint, centre…) nearest the
//! picked point as the base point; the JSON forms take `keyPoint`.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{ArrayKind, ArrayParams, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn source_line(s: &mut Session) -> String {
    let r = s.execute("line", &json!({"points": [[0, 0], [2, 1]]})).unwrap();
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string()
}

fn handle(v: &serde_json::Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

/// The array's insertion point (its base point) and parameters.
fn array(s: &Session, h: Handle) -> (Vec2, ArrayParams) {
    let d = s.doc().unwrap();
    let e = d.entity(h).unwrap();
    let EntityKind::Insert(i) = &e.kind else { panic!("not an array") };
    (i.insert.xy(), d.array_params(e).unwrap())
}

fn only_array(s: &Session) -> Handle {
    let d = s.doc().unwrap();
    let arrays: Vec<Handle> = d.model.iter().filter(|e| d.array_params(e).is_some()).map(|e| e.handle).collect();
    assert_eq!(arrays.len(), 1);
    arrays[0]
}

fn typed(s: &mut Session, inputs: &[&str]) {
    for t in inputs {
        s.cmdline(t).unwrap();
    }
}

#[test]
fn arrayrect_and_arraypolar_take_a_key_point_as_the_base_point() {
    let mut s = Session::new();
    let l = source_line(&mut s);
    let r = s.execute("arrayrect", &json!({"handles": [l], "cols": 2, "rows": 1, "colSpacing": 5, "keyPoint": [1.9, 1.05]})).unwrap();
    assert!(array(&s, handle(&r)).0.near(Vec2::new(2.0, 1.0), 1e-9));
    let mut s = Session::new();
    let l = source_line(&mut s);
    let r = s.execute("arraypolar", &json!({"handles": [l], "center": [-10, 0], "count": 4, "keyPoint": [1.1, 0.4]})).unwrap();
    assert!(array(&s, handle(&r)).0.near(Vec2::new(1.0, 0.5), 1e-9), "the line's midpoint");
}

#[test]
fn typed_key_points_at_arrayrect_and_arraypolar() {
    let mut s = Session::new();
    source_line(&mut s);
    typed(&mut s, &["arrayrect", "l", "", "b", "k", "1.9,0.95", "x"]);
    assert!(array(&s, only_array(&s)).0.near(Vec2::new(2.0, 1.0), 1e-9));
    // Enter at the key point prompt goes back to the base point prompt.
    let mut s = Session::new();
    source_line(&mut s);
    typed(&mut s, &["arraypolar", "l", "", "b", "k"]);
    assert!(s.current_prompt().unwrap().message.contains("key point"));
    typed(&mut s, &[""]);
    assert!(s.current_prompt().unwrap().message.contains("base point"));
    typed(&mut s, &["k", "0.1,0.1", "-10,0", "x"]);
    assert!(array(&s, only_array(&s)).0.near(Vec2::new(0.0, 0.0), 1e-9));
}

#[test]
fn arrayedit_moves_the_base_point_to_a_key_point_of_an_item() {
    let mut s = Session::new();
    let l = source_line(&mut s);
    let h = handle(&s.execute("arrayrect", &json!({"handles": [l], "cols": 3, "rows": 1, "colSpacing": 5, "base": [0, 0]})).unwrap());
    // Near the start of the third item: the base point goes to (10, 0), the items stay.
    s.execute("arrayedit", &json!({"handle": h.hex(), "keyPoint": [10.2, 0.05]})).unwrap();
    let (at, p) = array(&s, h);
    assert!(at.near(Vec2::new(10.0, 0.0), 1e-9), "{at:?}");
    assert!(matches!(p.kind, ArrayKind::Rect(r) if r.cols == 3 && r.col_spacing == 5.0));
    let items = s.execute("explode", &json!({"handles": [h.hex()]})).unwrap();
    assert!(items.to_string().len() > 2);
    let d = s.doc().unwrap();
    let mut starts: Vec<(i64, i64)> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some(((l.a.x * 10.0).round() as i64, (l.a.y * 10.0).round() as i64)),
            _ => None,
        })
        .collect();
    starts.sort();
    assert_eq!(starts, [(0, 0), (50, 0), (100, 0)]);
    // Not on an item: refused.
    let mut s = Session::new();
    let l = source_line(&mut s);
    let h = handle(&s.execute("arrayrect", &json!({"handles": [l], "cols": 3, "rows": 1, "colSpacing": 5, "base": [0, 0]})).unwrap());
    assert!(s.execute("arrayedit", &json!({"handle": h.hex(), "keyPoint": [3.5, 40]})).is_err());
    assert!(array(&s, h).0.near(Vec2::ZERO, 1e-9));
    // Typed: Base point, Key point, a point on the second item near its end.
    s.set_selection(vec![h]);
    typed(&mut s, &["arrayedit", "b", "k", "6.9,0.9", "x"]);
    assert!(array(&s, h).0.near(Vec2::new(7.0, 1.0), 1e-9));
}

#[test]
fn replacement_objects_take_a_key_point_as_their_base_point() {
    let mut s = Session::new();
    let l = source_line(&mut s);
    let h = handle(&s.execute("arrayrect", &json!({"handles": [l], "cols": 3, "rows": 1, "colSpacing": 5, "base": [0, 0]})).unwrap());
    let c = s.execute("circle", &json!({"center": [50, 50], "radius": 1})).unwrap()["handle"].clone();
    // The circle's right quadrant is the base point: on item 1 (at x = 5) its centre is at x = 4.
    s.execute("arrayedit", &json!({"handle": h.hex(), "replace": {"handles": [c], "keyPoint": [51.2, 50.1], "items": [[1, 0, 0]]}})).unwrap();
    s.execute("explode", &json!({"handles": [h.hex()]})).unwrap();
    let d = s.doc().unwrap();
    // The replaced item stays a reference to the replacement block.
    let centres: Vec<Vec2> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => d.block(&i.block).map(|b| (i.transform(b.base.xy()), b)),
            _ => None,
        })
        .flat_map(|(m, b)| {
            b.entities
                .iter()
                .filter_map(move |e| match &e.kind {
                    EntityKind::Circle(c) => Some(m.apply(c.center.xy())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(centres.iter().any(|c| c.near(Vec2::new(4.0, 0.0), 1e-9)), "{centres:?}");
}

#[test]
fn hostile_key_points_never_panic() {
    let mut s = Session::new();
    let l = source_line(&mut s);
    for kp in [json!(["a", 1]), json!([1e308, -1e308]), json!([]), json!("x"), json!(null)] {
        let _ = s.execute("arrayrect", &json!({"handles": [l], "cols": 2, "rows": 1, "keyPoint": kp}));
        let _ = s.execute("arraypolar", &json!({"handles": [l], "center": [5, 5], "count": 3, "keyPoint": kp}));
    }
    let mut s = Session::new();
    let l = source_line(&mut s);
    let h = handle(&s.execute("arrayrect", &json!({"handles": [l], "cols": 2, "rows": 1, "colSpacing": 5, "base": [0, 0]})).unwrap());
    for kp in [json!([1e308, 1e308]), json!([-1e308, 0]), json!(["x"]), json!([0.5, 0.25])] {
        let _ = s.execute("arrayedit", &json!({"handle": h.hex(), "keyPoint": kp}));
        let _ = s.execute("arrayedit", &json!({"handle": h.hex(), "replace": {"handles": [], "keyPoint": kp, "items": [[0, 0, 0]]}}));
    }
    // Typed key points far away or on nothing are refused without leaving the command broken.
    s.set_selection(vec![h]);
    typed(&mut s, &["arrayedit", "b", "k"]);
    assert!(s.cmdline("1e300,1e300").is_err() || s.running.is_some());
    s.cancel();
}
