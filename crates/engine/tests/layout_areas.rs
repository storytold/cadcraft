//! `layout.areas` and `layout.setarea`: the model area each layout viewport shows, read and edited.

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn session() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [100, 0]]})).unwrap();
    s.execute("circle", &json!({"center": [50, 25], "radius": 20})).unwrap();
    s
}

fn areas(s: &mut Session) -> Vec<Value> {
    s.execute("layout.areas", &json!({})).unwrap().as_array().unwrap().clone()
}

fn corner(a: &Value, i: usize, axis: usize) -> f64 {
    a["window"][i][axis].as_f64().unwrap()
}

/// The viewport of the layout named `name`.
fn area_of(s: &mut Session, name: &str) -> Value {
    areas(s).into_iter().find(|a| a["layout"] == name).unwrap()
}

#[test]
fn every_layout_viewport_has_an_area_in_tab_order() {
    let mut s = session();
    assert!(areas(&mut s).is_empty(), "no layout, no area");
    s.execute("layout.new", &json!({"name": "B", "window": [[10, 10], [30, 20]]})).unwrap();
    s.execute("layout.new", &json!({"name": "A"})).unwrap();
    s.execute("layout.new", &json!({"name": "C", "viewport": false})).unwrap();
    let a = areas(&mut s);
    let names: Vec<&str> = a.iter().map(|x| x["layout"].as_str().unwrap()).collect();
    assert_eq!(names, ["B", "A"], "tab order; a layout without a viewport has no area");
    // B was made to show [10,10]-[30,20]: centred on it, the whole window in view, no more than the 2 % margin.
    let (cx, cy) = ((corner(&a[0], 0, 0) + corner(&a[0], 1, 0)) / 2.0, (corner(&a[0], 0, 1) + corner(&a[0], 1, 1)) / 2.0);
    assert!((cx - 20.0).abs() < 1e-9 && (cy - 15.0).abs() < 1e-9, "centre {cx},{cy}");
    assert!(corner(&a[0], 0, 0) <= 10.0 && corner(&a[0], 1, 0) >= 30.0 && corner(&a[0], 0, 1) <= 10.0 && corner(&a[0], 1, 1) >= 20.0);
    // The model extents, for the layout made without a window, hold the drawing.
    assert!(corner(&a[1], 0, 0) <= 0.0 && corner(&a[1], 1, 0) >= 100.0);
}

#[test]
fn setarea_moves_and_resizes_keeping_the_shape() {
    let mut s = session();
    s.execute("layout.new", &json!({"name": "A", "window": [[0, 0], [40, 20]]})).unwrap();
    let before = area_of(&mut s, "A");
    let h = before["handle"].as_str().unwrap().to_string();
    let w0 = corner(&before, 1, 0) - corner(&before, 0, 0);
    let h0 = corner(&before, 1, 1) - corner(&before, 0, 1);

    s.execute("layout.setarea", &json!({"handle": h, "center": [100, 200]})).unwrap();
    let moved = area_of(&mut s, "A");
    assert!(((corner(&moved, 0, 0) + corner(&moved, 1, 0)) / 2.0 - 100.0).abs() < 1e-9);
    assert!(((corner(&moved, 0, 1) + corner(&moved, 1, 1)) / 2.0 - 200.0).abs() < 1e-9);
    assert!(((corner(&moved, 1, 0) - corner(&moved, 0, 0)) - w0).abs() < 1e-9, "a move keeps the size");

    s.execute("layout.setarea", &json!({"handle": h, "height": h0 * 2.0})).unwrap();
    let grown = area_of(&mut s, "A");
    let (w1, h1) = (corner(&grown, 1, 0) - corner(&grown, 0, 0), corner(&grown, 1, 1) - corner(&grown, 0, 1));
    assert!((h1 - h0 * 2.0).abs() < 1e-9 && (w1 - w0 * 2.0).abs() < 1e-9, "the width follows the height: {w1}x{h1}");
    assert!(((corner(&grown, 0, 0) + corner(&grown, 1, 0)) / 2.0 - 100.0).abs() < 1e-9, "a resize keeps the centre");
}

#[test]
fn setarea_is_one_undo_step() {
    let mut s = session();
    s.execute("layout.new", &json!({"name": "A", "window": [[0, 0], [40, 20]]})).unwrap();
    let before = area_of(&mut s, "A");
    let h = before["handle"].as_str().unwrap();
    s.execute("layout.setarea", &json!({"handle": h, "center": [7, 8], "height": 99})).unwrap();
    assert_ne!(area_of(&mut s, "A")["window"], before["window"]);
    s.undo().unwrap();
    assert_eq!(area_of(&mut s, "A")["window"], before["window"], "one undo restores the area");
}

#[test]
fn setarea_refuses_what_is_not_an_area_and_changes_nothing() {
    let mut s = session();
    s.execute("layout.new", &json!({"name": "A", "window": [[0, 0], [40, 20]]})).unwrap();
    let before = areas(&mut s);
    let h = before[0]["handle"].as_str().unwrap().to_string();
    let model_entity = s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap();
    let model_handle = model_entity["handle"].clone();
    let paper_viewport = {
        let d = s.doc().unwrap();
        let l = d.layout("A").unwrap();
        l.entities.iter().find_map(|e| matches!(&e.kind, cadcraft_doc::EntityKind::Viewport(v) if v.id == 1).then(|| e.handle.hex()))
    };
    let bad: Vec<Value> = vec![
        json!({}),
        json!({"handle": "FFFFFF"}),
        json!({"handle": 12345678}),
        json!({"handle": "not a handle"}),
        json!({"handle": model_handle, "center": [1, 1]}),
        json!({"handle": h, "center": "x"}),
        json!({"handle": h, "center": [1]}),
        json!({"handle": h, "center": [1, "a"]}),
        json!({"handle": h, "center": [1e300, 0]}),
        json!({"handle": h, "center": [0, -1e300]}),
        json!({"handle": h, "height": 0}),
        json!({"handle": h, "height": -5}),
        json!({"handle": h, "height": 1e-12}),
        json!({"handle": h, "height": 1e300}),
        json!({"handle": h, "height": "tall"}),
        json!({"handle": h, "height": [1]}),
    ];
    for p in &bad {
        assert!(s.execute("layout.setarea", p).is_err(), "{p}");
    }
    if let Some(ps) = paper_viewport {
        assert!(s.execute("layout.setarea", &json!({"handle": ps, "center": [1, 1]})).is_err(), "the paper viewport is not an area");
    }
    assert_eq!(areas(&mut s), before, "nothing changed");
}

#[test]
fn a_locked_viewport_keeps_its_area() {
    let mut s = session();
    s.execute("layout.new", &json!({"name": "A", "window": [[0, 0], [40, 20]]})).unwrap();
    let before = areas(&mut s);
    let h = before[0]["handle"].as_str().unwrap().to_string();
    s.execute("mview", &json!({"option": "lock", "handles": [h], "locked": true, "layout": "A"})).unwrap();
    assert_eq!(areas(&mut s)[0]["locked"], true);
    assert!(s.execute("layout.setarea", &json!({"handle": h, "center": [5, 5]})).is_err());
    assert_eq!(areas(&mut s)[0]["window"], before[0]["window"]);
}

#[test]
fn an_off_viewport_is_listed_as_off() {
    let mut s = session();
    s.execute("layout.new", &json!({"name": "A"})).unwrap();
    let h = areas(&mut s)[0]["handle"].as_str().unwrap().to_string();
    s.execute("mview", &json!({"option": "off", "handles": [h], "layout": "A"})).unwrap();
    assert_eq!(areas(&mut s)[0]["on"], false);
}

#[test]
fn renaming_and_deleting_layouts_shows_in_the_areas() {
    let mut s = session();
    s.execute("layout.new", &json!({"name": "A"})).unwrap();
    s.execute("layout.new", &json!({"name": "B"})).unwrap();
    s.execute("layout.rename", &json!({"from": "A", "to": "Sheet 1"})).unwrap();
    s.execute("layout.delete", &json!({"name": "B"})).unwrap();
    let names: Vec<String> = areas(&mut s).iter().map(|a| a["layout"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["Sheet 1"]);
    s.undo().unwrap();
    assert_eq!(areas(&mut s).len(), 2, "undoing the delete brings the layout's area back");
}

#[test]
fn listing_areas_does_not_touch_the_undo_history() {
    let mut s = session();
    s.execute("layout.new", &json!({"name": "A"})).unwrap();
    let rev = s.state().unwrap().revision;
    areas(&mut s);
    assert_eq!(s.state().unwrap().revision, rev);
    s.undo().unwrap();
    assert!(s.doc().unwrap().layout("A").is_none(), "the undo step is still the layout's creation");
}
