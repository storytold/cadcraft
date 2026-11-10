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

/// A session with `lines` drawn in a view of `px` pixels, zoomed by the last line.
fn drawing(px: (f64, f64), lines: &[&str]) -> Session {
    let mut s = Session::new();
    s.viewport_px = px;
    for l in lines {
        s.cmdline(l).unwrap();
    }
    s
}

/// Review on #2776: both axes meeting the same slanted segment is no angle (it gave 0°); the
/// corners the axes do reach give theirs.
#[test]
fn one_segment_met_by_both_axes_gives_no_angle() {
    let mut s = drawing((1200.0, 800.0), &["pline 0,0 10,0 0,10 c", "zoom e"]);
    let rs = readings(&mut s, [2.0, 2.0]);
    let mut angles: Vec<(f64, f64, f64)> = of_type(&rs, "angle")
        .iter()
        .map(|a| (a["degrees"].as_f64().unwrap(), a["vertex"][0].as_f64().unwrap(), a["vertex"][1].as_f64().unwrap()))
        .collect();
    angles.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    assert_eq!(angles.len(), 3, "{rs:?}");
    assert!((angles[0].0 - 45.0).abs() < 1e-9 && (angles[0].1, angles[0].2) == (0.0, 10.0), "{angles:?}");
    assert!((angles[1].0 - 45.0).abs() < 1e-9 && (angles[1].1, angles[1].2) == (10.0, 0.0), "{angles:?}");
    assert!((angles[2].0 - 90.0).abs() < 1e-9 && (angles[2].1, angles[2].2) == (0.0, 0.0), "{angles:?}");
}

/// Review on #2776: an ellipse (or spline) stops the axes but gives no reading: its flattened
/// pieces are not lengths of the drawing.
#[test]
fn an_ellipse_stops_the_axes_but_gives_no_reading() {
    let mut s = drawing((1200.0, 800.0), &["ellipse c 0,0 5,0 2", "zoom e"]);
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "at": [0.5, 0.3] })).unwrap();
    assert_eq!(r["readings"], json!([]), "{r}");
    let right = r["axes"][0][1][0].as_f64().unwrap();
    assert!(right > 4.9 && right < 5.0 + 1e-6, "the axis stops at the ellipse: {r}");
}

/// Review on #2776: an island inside another island is in its hole already (a counterbore), and
/// islands that overlap leave out their union once.
#[test]
fn nested_islands_count_once_and_overlapping_ones_by_their_union() {
    let pi = std::f64::consts::PI;
    let mut s = rectangle(&["circle 5,2.5 2", "circle 5,2.5 1"]);
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "areas": [[1, 1]] })).unwrap();
    assert!(close(&r["area"], 50.0 - 4.0 * pi), "{r}");
    assert!(close(&r["perimeter"], 30.0 + 4.0 * pi), "{r}");

    // Two circles of radius 1.5, 2 apart: the hole is their union, not both whole.
    let mut s = rectangle(&["circle 3,2.5 1.5", "circle 5,2.5 1.5"]);
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "areas": [[0.5, 0.5]] })).unwrap();
    let lens = 2.0 * 2.25 * (2.0f64 / 3.0).acos() - 5.0f64.sqrt();
    let union = 2.0 * pi * 2.25 - lens;
    let area = r["area"].as_f64().unwrap();
    assert!((area - (50.0 - union)).abs() < 1e-2, "{area} vs {}", 50.0 - union);
}

/// Review on #2776: two islands crossed like a plus sign, with no corner of either in the other,
/// still leave out their union once.
#[test]
fn islands_crossed_like_a_plus_leave_out_their_union() {
    let mut s = drawing((1200.0, 800.0), &["rectang 0,0 20,20", "rectang 8,2 12,18", "rectang 2,8 18,12", "zoom e"]);
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "areas": [[1, 1]] })).unwrap();
    // 400 less the plus: two 4 x 16 strips sharing a 4 x 4 square.
    assert!(close(&r["area"], 400.0 - 112.0), "{r}");
    assert!(close(&r["perimeter"], 80.0 + 64.0), "{r}");

    // Strips of different sizes: the smaller one crosses the larger, it isn't inside it.
    let mut s = drawing((1200.0, 800.0), &["rectang 0,0 20,20", "rectang 8,3 12,17", "rectang 2,8 18,12", "zoom e"]);
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "areas": [[1, 1]] })).unwrap();
    // 4 x 14 and 16 x 4 sharing a 4 x 4 square: 104, with an outline of 36 + 40 - 16.
    assert!(close(&r["area"], 400.0 - 104.0), "{r}");
    assert!(close(&r["perimeter"], 80.0 + 60.0), "{r}");
}

/// Review on #2776: a circle met by several axes gives its radius once.
#[test]
fn a_circle_round_the_cursor_gives_its_radius_once() {
    let mut s = rectangle(&["circle 20,2 3"]);
    let rs = readings(&mut s, [20.0, 2.0]);
    assert_eq!(of_type(&rs, "radius").len(), 1, "{rs:?}");
}

/// Review on #2776: the axes reach the edge of a view much wider than tall.
#[test]
fn the_axes_cross_a_wide_view() {
    // 30 wide and 10 high: a line near the left edge, the cursor near the right one.
    let mut s = drawing((1500.0, 500.0), &["line -14,-3 -14,3 ", "zoom c 0,0 10"]);
    let rs = readings(&mut s, [14.0, 0.0]);
    assert!(of_type(&rs, "length").iter().any(|l| close(&l["length"], 6.0)), "{rs:?}");
}

/// Review on #2776: a click a snap moved onto an edge is measured where the mouse is, inside.
#[test]
fn a_click_snapped_onto_an_edge_measures_the_area_the_mouse_is_in() {
    let mut s = rectangle(&[]);
    s.start("measure.quick").unwrap();
    s.pointer = Some(Vec2::new(0.02, 2.0));
    s.input(Input::Point(Vec2::new(0.0, 2.0))).unwrap();
    assert!(s.log.iter().any(|l| l.contains("Area = 50.0000")), "{:?}", s.log);
    // A point far from the mouse is measured where it was given.
    s.input(Input::Point(Vec2::new(40.0, 40.0))).unwrap();
    assert!(s.log.iter().any(|l| l.contains("No closed area there.")), "{:?}", s.log);
    // A point typed just outside, next to the mouse just inside: the typed point is measured.
    let refused = |s: &Session| s.log.iter().filter(|l| l.contains("No closed area there.")).count();
    let before = refused(&s);
    s.cmdline("#-0.02,2").unwrap();
    assert_eq!(refused(&s), before + 1, "{:?}", s.log);
    // A click just outside, snapped onto the edge, measures nothing: not the area behind the edge.
    s.pointer = Some(Vec2::new(-0.02, 2.0));
    s.input(Input::Point(Vec2::new(0.0, 2.0))).unwrap();
    assert_eq!(refused(&s), before + 2, "{:?}", s.log);
    s.cancel();
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
    // Two points in one area, or one point twice, measure it once: they don't cancel out.
    let r = s.execute("measuregeom", &json!({ "mode": "quick", "areas": [[2, 2], [3, 3], [2, 2]] })).unwrap();
    assert!(close(&r["area"], 50.0 - pi), "{r}");
    assert_eq!(r["areas"].as_array().map(Vec::len), Some(1), "{r}");
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
