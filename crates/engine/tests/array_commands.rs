//! ARRAY, ARRAYRECT and ARRAYPOLAR at the command line: the type prompt and the edit-array
//! option loops. Each input is its own `cmdline` call (one typed line each).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn typed(s: &mut Session, inputs: &[&str]) {
    for t in inputs {
        s.cmdline(t).unwrap();
    }
}

fn circle_centers(s: &Session) -> Vec<Vec2> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Circle(c) => Some(c.center.xy()),
            _ => None,
        })
        .collect()
}

fn has(pts: &[Vec2], x: f64, y: f64) -> bool {
    pts.iter().any(|p| p.near(Vec2::new(x, y), 1e-6))
}

fn prompt(s: &Session) -> String {
    s.prompt_text()
}

#[test]
fn array_asks_for_the_type_and_continues_as_that_array() {
    // ARRAY ▸ POlar.
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [10, 0], "radius": 1})).unwrap();
    typed(&mut s, &["array", "l", ""]);
    assert!(prompt(&s).contains("Enter array type or [Rectangular/PAth/POlar] <Rectangular>"), "{}", prompt(&s));
    typed(&mut s, &["po", "0,0", "i", "4", "x"]);
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    let c = circle_centers(&s);
    assert_eq!(c.len(), 4);
    assert!(has(&c, 0.0, 10.0) && has(&c, -10.0, 0.0) && has(&c, 0.0, -10.0));

    // AR (alias), Enter takes the default Rectangular.
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    typed(&mut s, &["ar", "l", "", ""]);
    assert!(prompt(&s).contains("COUnt"), "{}", prompt(&s));
    typed(&mut s, &["cou", "2", "1", "s", "5", "5", ""]);
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    let c = circle_centers(&s);
    assert_eq!(c.len(), 2);
    assert!(has(&c, 5.0, 0.0));

    // ARRAY ▸ PAth continues at the path prompt with the selected objects.
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 1], "radius": 0.25})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    typed(&mut s, &["array", "0.25,1", "", "pa", "5,0", "3"]);
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    assert_eq!(circle_centers(&s).len(), 3);

    // The JSON form dispatches on `type`.
    let mut s = Session::new();
    let r = s.execute("circle", &json!({"center": [10, 0], "radius": 1})).unwrap();
    let h = r["handle"].clone();
    let r = s.execute("array", &json!({"type": "polar", "handles": [h], "center": [0, 0], "count": 3})).unwrap();
    assert_eq!(r["created"], 2);
    assert!(s.execute("array", &json!({"type": "spiral", "handles": [h]})).is_err());
}

#[test]
fn arrayrect_edits_the_array_in_a_loop_until_exit() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 0.5})).unwrap();
    typed(&mut s, &["arrayrect", "l", ""]);
    let p = prompt(&s);
    assert!(p.contains("Select grip to edit array or [ASsociative/Base point/COUnt/Spacing/COLumns/Rows/Levels/eXit] <eXit>"), "{p}");
    // Nothing is created before eXit.
    assert_eq!(circle_centers(&s).len(), 1);
    typed(&mut s, &["cou", "2", "3", "s", "10", "20"]);
    // Unavailable options report and return to the loop without taking the next input.
    typed(&mut s, &["as", "l", "cou", "2", "3"]);
    assert!(prompt(&s).contains("Select grip to edit array"), "{}", prompt(&s));
    typed(&mut s, &["x"]);
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    let c = circle_centers(&s);
    assert_eq!(c.len(), 6, "{c:?}");
    for (x, y) in [(10.0, 0.0), (0.0, 20.0), (10.0, 40.0)] {
        assert!(has(&c, x, y), "{c:?}");
    }

    // COLumns with a Total distance, Rows with spacing and elevation.
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 0.5})).unwrap();
    typed(&mut s, &["arrayrect", "l", "", "col", "3", "t", "20", "r", "2", "5", "0", "x"]);
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    let c = circle_centers(&s);
    assert_eq!(c.len(), 6, "{c:?}");
    assert!(has(&c, 20.0, 0.0) && has(&c, 10.0, 5.0), "{c:?}");

    // Invalid counts re-prompt.
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 0.5})).unwrap();
    typed(&mut s, &["arrayrect", "l", "", "cou", "0"]);
    assert!(prompt(&s).contains("number of columns"), "{}", prompt(&s));
    typed(&mut s, &["2", "1", "x"]);
    assert_eq!(circle_centers(&s).len(), 2);

    // The JSON form is unchanged.
    let mut s = Session::new();
    let r = s.execute("circle", &json!({"center": [0, 0], "radius": 0.5})).unwrap();
    let r = s.execute("arrayrect", &json!({"handles": [r["handle"]], "rows": 2, "cols": 3, "rowSpacing": 4, "colSpacing": 2})).unwrap();
    assert_eq!(r["created"], 5);
    assert!(has(&circle_centers(&s), 4.0, 4.0));
}

#[test]
fn arraypolar_asks_for_the_center_then_edits_items_and_angles() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [10, 0], "radius": 1})).unwrap();
    typed(&mut s, &["arraypolar", "l", ""]);
    assert!(prompt(&s).contains("Specify center point of array or [Base point/Axis of rotation]"), "{}", prompt(&s));
    typed(&mut s, &["0,0"]);
    let p = prompt(&s);
    assert!(
        p.contains("Select grip to edit array or [ASsociative/Base point/Items/Angle between/Fill angle/ROWs/Levels/ROTate items/eXit] <eXit>"),
        "{p}"
    );
    typed(&mut s, &["i", "4", "x"]);
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    // X ended ARRAYPOLAR; it did not start EXPLODE.
    assert!(!s.log.iter().any(|l| l.contains("EXPLODE")));
    let c = circle_centers(&s);
    assert_eq!(c.len(), 4);
    assert!(has(&c, 0.0, 10.0) && has(&c, -10.0, 0.0) && has(&c, 0.0, -10.0));

    // Angle between with 4 items: 30° apart, filling 90°.
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [10, 0], "radius": 1})).unwrap();
    s.script("ARRAYPOLAR L\n\n0,0 I 4 A 30 X\n").unwrap();
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    let c = circle_centers(&s);
    assert_eq!(c.len(), 4);
    assert!(has(&c, 0.0, 10.0) && !has(&c, -10.0, 0.0), "{c:?}");

    // Fill angle 180 with 3 items, items not rotated: the copies only move.
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[10, 0], [12, 0]]})).unwrap();
    typed(&mut s, &["arraypolar", "l", "", "0,0", "i", "3", "f", "180", "rot", "n", "x"]);
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    let lines: Vec<(Vec2, Vec2)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 3);
    // The middle copy: the line's centre (11,0) moved to (0,11), still horizontal.
    assert!(lines.iter().any(|(a, b)| a.near(Vec2::new(-1.0, 11.0), 1e-6) && b.near(Vec2::new(1.0, 11.0), 1e-6)), "{lines:?}");
}

#[test]
fn arrays_over_maxarray_are_refused_not_shrunk() {
    // JSON: a million × a million rows/cols (or items) is refused up front, nothing is created.
    let mut s = Session::new();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    let h = c["handle"].clone();
    let before = s.doc().unwrap().model.iter().count();
    let e = s.execute("arrayrect", &json!({"handles": [h], "rows": 1_000_000, "cols": 1_000_000})).unwrap_err();
    assert!(e.to_string().contains("MAXARRAY"), "{e}");
    assert!(s.execute("arraypolar", &json!({"handles": [h], "center": [5, 5], "count": 200_000})).unwrap_err().to_string().contains("MAXARRAY"));
    let path = s.execute("line", &json!({"points": [[0, 5], [100, 5]]})).unwrap()["handles"][0].clone();
    let e = s.execute("arraypath", &json!({"handles": [h], "path": path, "spacing": 1e-6})).unwrap_err();
    assert!(e.to_string().contains("MAXARRAY"), "{e}");
    assert_eq!(s.doc().unwrap().model.iter().count(), before + 1);

    // The limit counts every selected object, and MAXARRAY is a settable system variable.
    s.execute("setvar", &json!({"name": "MAXARRAY", "value": 100})).unwrap();
    let h2 = s.execute("circle", &json!({"center": [3, 0], "radius": 1})).unwrap()["handle"].clone();
    assert!(s.execute("arrayrect", &json!({"handles": [h, h2], "rows": 10, "cols": 6})).is_err());
    assert_eq!(s.execute("arrayrect", &json!({"handles": [h, h2], "rows": 10, "cols": 5})).unwrap()["created"], 98);

    // Interactive: a count over the limit is rejected at its prompt and the loop keeps going.
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    typed(&mut s, &["arrayrect", "l", "", "cou", "1000", "1000"]);
    assert!(s.log.iter().any(|l| l.contains("MAXARRAY")), "{:?}", s.log);
    assert!(prompt(&s).contains("rows"), "{}", prompt(&s));
    typed(&mut s, &["2", ""]);
    assert!(s.current_prompt().is_none(), "{}", prompt(&s));
    assert_eq!(circle_centers(&s).len(), 2000);
}
