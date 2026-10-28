//! Array grips: the count and spacing arrows of an associative array, listed by `array.grips`,
//! set by `array.grip` (a point or a value, one undo step) and dragged with
//! `Session::start_array_grip` (the UI's click on a grip), with the rebuilt array as the preview.

use cadcraft_engine::doc::{ArrayKind, ArrayParams, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn rect_array(s: &mut Session) -> Handle {
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 0.5})).unwrap()["handle"].clone();
    let r = s.execute("arrayrect", &json!({"handles": [c], "cols": 4, "rows": 3, "colSpacing": 2, "rowSpacing": 3, "base": [0, 0]})).unwrap();
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

fn polar_array(s: &mut Session, fill: f64) -> Handle {
    let c = s.execute("circle", &json!({"center": [10, 0], "radius": 0.5})).unwrap()["handle"].clone();
    let r = s.execute("arraypolar", &json!({"handles": [c], "center": [0, 0], "count": 5, "angle": fill, "base": [10, 0]})).unwrap();
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

fn params(s: &Session, h: Handle) -> ArrayParams {
    let d = s.doc().unwrap();
    d.array_params(d.entity(h).unwrap()).unwrap()
}

fn rect(s: &Session, h: Handle) -> (u32, u32, f64, f64) {
    match params(s, h).kind {
        ArrayKind::Rect(r) => (r.cols, r.rows, r.col_spacing, r.row_spacing),
        ArrayKind::Polar(_) => panic!("not rectangular"),
    }
}

fn polar(s: &Session, h: Handle) -> (u32, f64) {
    match params(s, h).kind {
        ArrayKind::Polar(p) => (p.items, p.fill),
        ArrayKind::Rect(_) => panic!("not polar"),
    }
}

fn grips(s: &mut Session, h: Handle) -> Vec<(String, [f64; 2])> {
    let v = s.execute("array.grips", &json!({"handle": h.hex()})).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|g| (g["grip"].as_str().unwrap().to_string(), [g["at"][0].as_f64().unwrap(), g["at"][1].as_f64().unwrap()]))
        .collect()
}

#[test]
fn rectangular_grips_are_listed_and_set_in_one_undo_step() {
    let mut s = Session::new();
    let h = rect_array(&mut s);
    let g = grips(&mut s, h);
    let names: Vec<&str> = g.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["columnSpacing", "columnCount", "rowSpacing", "rowCount", "rowColumnCount"]);
    assert!(g.contains(&("columnCount".into(), [6.0, 0.0])) && g.contains(&("rowColumnCount".into(), [6.0, 6.0])));
    // Dragging the column count arrow to x = 10.2: six columns, spacing kept.
    s.execute("array.grip", &json!({"handle": h.hex(), "grip": "columnCount", "to": [10.2, 0.4]})).unwrap();
    assert_eq!(rect(&s, h), (6, 3, 2.0, 3.0));
    s.execute("array.grip", &json!({"handle": h.hex(), "grip": "rowSpacing", "value": 4.5})).unwrap();
    assert_eq!(rect(&s, h), (6, 3, 2.0, 4.5));
    s.execute("array.grip", &json!({"handle": h.hex(), "grip": "rowColumnCount", "to": [4.1, 9.2]})).unwrap();
    assert_eq!(rect(&s, h), (3, 3, 2.0, 4.5));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(rect(&s, h), (6, 3, 2.0, 4.5));
    s.execute("undo", &json!({})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(rect(&s, h), (4, 3, 2.0, 3.0));
}

#[test]
fn a_moved_and_rotated_array_takes_drawing_points() {
    let mut s = Session::new();
    let h = rect_array(&mut s);
    s.execute("rotate", &json!({"handles": [h.hex()], "base": [0, 0], "angle": 90})).unwrap();
    s.execute("move", &json!({"handles": [h.hex()], "delta": [100, 50]})).unwrap();
    let g = grips(&mut s, h);
    let at = g.iter().find(|(n, _)| n == "columnCount").map(|(_, a)| *a).unwrap();
    assert!((at[0] - 100.0).abs() < 1e-9 && (at[1] - 56.0).abs() < 1e-9, "{at:?}");
    // Two columns further up.
    s.execute("array.grip", &json!({"handle": h.hex(), "grip": "columnCount", "to": [99.0, 60.1]})).unwrap();
    assert_eq!(rect(&s, h).0, 6);
}

#[test]
fn polar_grips_change_the_angle_and_the_count() {
    let mut s = Session::new();
    let h = polar_array(&mut s, 180.0);
    let names: Vec<String> = grips(&mut s, h).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["itemAngle", "itemCount"]);
    // The last item dragged to straight down (270 degrees): 45 between, 7 items.
    s.execute("array.grip", &json!({"handle": h.hex(), "grip": "itemCount", "to": [0, -10]})).unwrap();
    let (n, fill) = polar(&s, h);
    assert!(n == 7 && (fill - 270.0).abs() < 1e-9, "{n} {fill}");
    s.execute("array.grip", &json!({"handle": h.hex(), "grip": "itemAngle", "value": 30})).unwrap();
    let (n, fill) = polar(&s, h);
    assert!(n == 7 && (fill - 180.0).abs() < 1e-9, "{n} {fill}");
    // A whole circle keeps whole: the angle sets the count.
    let full = polar_array(&mut s, 360.0);
    s.execute("array.grip", &json!({"handle": full.hex(), "grip": "itemAngle", "value": 40})).unwrap();
    assert_eq!(polar(&s, full), (9, 360.0));
    let names: Vec<String> = grips(&mut s, full).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["itemAngle"]);
}

#[test]
fn a_grip_drag_previews_the_array_and_takes_a_point_or_a_value() {
    let mut s = Session::new();
    let h = rect_array(&mut s);
    s.start_array_grip(h, cadcraft_engine::doc::ArrayGripKind::ColumnCount).unwrap();
    assert!(s.current_prompt().unwrap().message.contains("COLUMN COUNT"));
    // The preview is the array with six columns: 18 circles, plus the rubber band.
    let preview = s.preview(Vec2::new(10.9, 0.0));
    assert_eq!(preview.iter().filter(|e| matches!(e.kind, cadcraft_engine::doc::EntityKind::Circle(_))).count(), 18);
    s.input(Input::Point(Vec2::new(10.9, 0.0))).unwrap();
    assert!(s.running.is_none());
    assert_eq!(rect(&s, h).0, 6);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(rect(&s, h).0, 4);
    // A typed number is the count.
    s.start_array_grip(h, cadcraft_engine::doc::ArrayGripKind::ColumnCount).unwrap();
    s.input(Input::Text("9".into())).unwrap();
    assert_eq!(rect(&s, h).0, 9);
    // The corner takes only a point.
    s.start_array_grip(h, cadcraft_engine::doc::ArrayGripKind::RowColumnCount).unwrap();
    assert!(s.input(Input::Text("3".into())).is_err() || rect(&s, h).0 == 9);
    s.cancel();
    // No such grip on a polar array.
    let p = polar_array(&mut s, 360.0);
    assert!(s.start_array_grip(p, cadcraft_engine::doc::ArrayGripKind::ColumnCount).is_err());
}

#[test]
fn grips_respect_maxarray_and_locked_layers() {
    let mut s = Session::new();
    let h = rect_array(&mut s);
    s.execute("setvar", &json!({"name": "MAXARRAY", "value": 300})).unwrap();
    // Dragging far out stops at what MAXARRAY allows: 300 objects in 3 rows = 100 columns.
    s.execute("array.grip", &json!({"handle": h.hex(), "grip": "columnCount", "to": [1e6, 0]})).unwrap();
    assert_eq!(rect(&s, h).0, 100);
    // A typed count past it is refused, and nothing changes.
    assert!(s.execute("array.grip", &json!({"handle": h.hex(), "grip": "columnCount", "value": 101})).is_err());
    assert_eq!(rect(&s, h).0, 100);
    s.execute("layer", &json!({"name": "L", "make": true})).ok();
    s.execute("layer.set", &json!({"name": "0", "locked": true})).ok();
    if s.doc().unwrap().layer("0").is_some_and(|l| l.locked) {
        assert!(s.execute("array.grip", &json!({"handle": h.hex(), "grip": "columnCount", "value": 2})).is_err());
    }
}

#[test]
fn hostile_grip_params_never_panic() {
    let mut s = Session::new();
    let h = rect_array(&mut s);
    let line = s.execute("circle", &json!({"center": [40, 40], "radius": 1})).unwrap()["handle"].clone();
    let bad: Vec<Value> = vec![
        json!({}),
        json!({"handle": h.hex()}),
        json!({"handle": h.hex(), "grip": "nope", "value": 2}),
        json!({"handle": h.hex(), "grip": "columnCount"}),
        json!({"handle": h.hex(), "grip": "columnCount", "value": 2, "to": [1, 1]}),
        json!({"handle": h.hex(), "grip": "columnCount", "value": "x"}),
        json!({"handle": h.hex(), "grip": "columnCount", "value": -3}),
        json!({"handle": h.hex(), "grip": "columnCount", "value": 1e300}),
        json!({"handle": h.hex(), "grip": "columnCount", "value": 2.5}),
        json!({"handle": h.hex(), "grip": "columnSpacing", "value": 0}),
        json!({"handle": h.hex(), "grip": "columnSpacing", "value": 1e308}),
        json!({"handle": h.hex(), "grip": "rowSpacing", "to": [1e308, 1e308]}),
        json!({"handle": h.hex(), "grip": "columnCount", "to": ["a", 1]}),
        json!({"handle": h.hex(), "grip": "itemAngle", "value": 30}),
        json!({"handle": line, "grip": "columnCount", "value": 2}),
        json!({"handle": "ZZZZ", "grip": "columnCount", "value": 2}),
    ];
    for p in &bad {
        assert!(s.execute("array.grip", p).is_err(), "{p}");
    }
    assert_eq!(rect(&s, h), (4, 3, 2.0, 3.0));
    // Odd but valid: an enormous drag is capped, never a panic.
    let _ = s.execute("array.grip", &json!({"handle": h.hex(), "grip": "rowColumnCount", "to": [1e300, -1e300]}));
    assert!(s.execute("array.grips", &json!({"handle": line})).unwrap().as_array().unwrap().is_empty());
    assert!(s.execute("array.grips", &json!({})).is_err());
}
