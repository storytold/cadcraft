//! MEASUREGEOM's Quick option: what the cursor's axes meet is measured as it moves (lengths,
//! radii, the gap between parallel lines, the angle of joined lines), and a click measures the
//! closed area under it, Shift+click adding areas.

use cadcraft_engine::doc::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

/// A session showing a 10 x 5 rectangle at the origin, in a view of 1200 x 800 pixels.
fn rectangle(extra: &[&str]) -> Session {
    let mut s = Session::new();
    s.viewport_px = (1200.0, 800.0);
    s.cmdline("rectang 0,0 10,5").unwrap();
    for l in extra {
        s.cmdline(l).unwrap();
    }
    s.cmdline("zoom e").unwrap();
    s
}

fn readings(s: &mut Session, at: [f64; 2]) -> Vec<Value> {
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "at": at })).unwrap();
    r["readings"].as_array().cloned().unwrap_or_default()
}

fn of_type<'a>(rs: &'a [Value], t: &str) -> Vec<&'a Value> {
    rs.iter().filter(|r| r["type"] == t).collect()
}

fn close(a: &Value, b: f64) -> bool {
    a.as_f64().is_some_and(|a| (a - b).abs() < 1e-9)
}

#[test]
fn inside_a_rectangle_the_axes_give_its_sides_gaps_and_right_angles() {
    let mut s = rectangle(&[]);
    let rs = readings(&mut s, [4.0, 2.0]);
    let lengths: Vec<f64> = of_type(&rs, "length").iter().filter_map(|r| r["length"].as_f64()).collect();
    assert_eq!(lengths.len(), 4, "{rs:?}");
    assert_eq!(lengths.iter().filter(|l| (**l - 10.0).abs() < 1e-9).count(), 2, "{rs:?}");
    assert_eq!(lengths.iter().filter(|l| (**l - 5.0).abs() < 1e-9).count(), 2, "{rs:?}");
    // The two pairs of opposite sides are parallel: 10 apart across, 5 apart up and down.
    let gaps = of_type(&rs, "gap");
    assert_eq!(gaps.len(), 2, "{rs:?}");
    assert!(gaps.iter().any(|g| close(&g["distance"], 10.0)) && gaps.iter().any(|g| close(&g["distance"], 5.0)), "{gaps:?}");
    // The four corners join a side met across and one met up or down: right angles.
    let angles = of_type(&rs, "angle");
    assert_eq!(angles.len(), 4, "{rs:?}");
    assert!(angles.iter().all(|a| close(&a["degrees"], 90.0) && a["right"] == true), "{angles:?}");
    // The axes stop at the sides.
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "at": [4, 2] })).unwrap();
    assert_eq!(r["axes"], json!([[[0.0, 2.0], [10.0, 2.0]], [[4.0, 0.0], [4.0, 5.0]]]));
}

#[test]
fn an_arc_gives_its_radius_and_lines_that_do_not_meet_give_no_angle_or_gap() {
    // A circle beside the rectangle, and a slanted line above it that touches nothing.
    let mut s = rectangle(&["circle 20,2 3", "line 12,8 16,12 "]);
    let rs = readings(&mut s, [13.0, 2.0]);
    let radii = of_type(&rs, "radius");
    assert_eq!(radii.len(), 1, "{rs:?}");
    assert!(close(&radii[0]["radius"], 3.0) && radii[0]["at"] == json!([17.0, 2.0]), "{radii:?}");
    // Across: the rectangle's right side and the circle (not two parallel lines): no gap.
    assert!(of_type(&rs, "gap").iter().all(|g| !close(&g["distance"], 10.0)), "{rs:?}");
    // Up: the slanted line, which joins nothing the horizontal axis meets.
    let lengths = of_type(&rs, "length");
    assert!(lengths.iter().any(|l| close(&l["length"], 32f64.sqrt())), "{rs:?}");
    assert!(of_type(&rs, "angle").is_empty(), "{rs:?}");
}

#[test]
fn nothing_around_the_cursor_measures_nothing() {
    let mut s = rectangle(&[]);
    s.cmdline("zoom c 500,500 10").unwrap();
    assert!(readings(&mut s, [500.0, 500.0]).is_empty());
    let err = s.execute("measuregeom", &json!({ "mode": "quick" })).unwrap_err().to_string();
    assert!(err.contains("`at`"), "{err}");
}

#[test]
fn areas_add_up_and_islands_count() {
    // An island: a circle of radius 1 inside the rectangle.
    let mut s = rectangle(&["circle 7,2.5 1"]);
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "areas": [[2, 2]] })).unwrap();
    let pi = std::f64::consts::PI;
    assert!(close(&r["area"], 50.0 - pi), "{r}");
    assert!(close(&r["perimeter"], 30.0 + 2.0 * pi), "{r}");
    // The island and the rest of the rectangle together: the whole rectangle's area.
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "areas": [[2, 2], [7, 2.5]] })).unwrap();
    assert!(close(&r["area"], 50.0), "{r}");
    assert!(r["message"].as_str().is_some_and(|m| m.contains("Total area = 50.0000")), "{r}");
    let err = s.execute("measuregeom", &json!({ "mode": "quick", "areas": [[30, 30]] })).unwrap_err().to_string();
    assert!(err.contains("no closed area"), "{err}");
}

#[test]
fn at_the_command_line_clicks_measure_areas_and_shift_adds_them() {
    let mut s = rectangle(&["circle 7,2.5 1"]);
    s.cmdline("measuregeom").unwrap();
    s.cmdline("q").unwrap();
    assert!(s.prompt_text().contains("Move cursor"), "{}", s.prompt_text());
    // The readout follows the cursor, in its own colour.
    let marks = s.preview(Vec2::new(2.0, 2.0));
    assert!(marks.len() > 4, "{marks:?}");
    assert!(marks.iter().all(|e| matches!(e.common.color, cadcraft_engine::doc::color::Color::True(_))), "{marks:?}");

    s.input(Input::Point(Vec2::new(2.0, 2.0))).unwrap();
    assert!(s.log.iter().any(|l| l.contains("Area = 46.8584, Perimeter = 36.2832")), "{:?}", s.log);
    assert!((s.area - (50.0 - std::f64::consts::PI)).abs() < 1e-9);
    // Shift+click the island: both areas, and their total.
    s.shift_down = true;
    s.input(Input::Point(Vec2::new(7.0, 2.5))).unwrap();
    assert!(s.log.iter().any(|l| l.contains("Total area = 50.0000")), "{:?}", s.log);
    // Shift+click it again: taken away.
    s.input(Input::Point(Vec2::new(7.0, 2.5))).unwrap();
    assert!((s.area - (50.0 - std::f64::consts::PI)).abs() < 1e-9, "{}", s.area);
    s.shift_down = false;
    // A click outside every closed area is said so.
    s.input(Input::Point(Vec2::new(40.0, 40.0))).unwrap();
    assert!(s.log.iter().any(|l| l.contains("No closed area there.")), "{:?}", s.log);
    // Its options switch to the other measurements; Enter ends.
    s.cmdline("d").unwrap();
    assert!(s.prompt_text().contains("first point"), "{}", s.prompt_text());
    s.cancel();
    s.start("measure.quick").unwrap();
    assert!(s.prompt_text().contains("Move cursor"), "{}", s.prompt_text());
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.iter().count(), 2, "measuring adds nothing to the drawing");
}
