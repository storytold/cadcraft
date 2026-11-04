//! AREA's Object / Add area / Subtract area, DIST's Multiple points and MEASUREGEOM's Angle and
//! Volume, typed as at the command line.

use cadcraft_engine::Session;
use serde_json::json;

fn run(lines: &[&str]) -> (Session, Vec<String>) {
    let mut s = Session::new();
    for l in lines {
        s.cmdline(l).unwrap();
    }
    let log = s.log.clone();
    (s, log)
}

fn has(log: &[String], text: &str) -> bool {
    log.iter().any(|l| l.contains(text))
}

#[test]
fn area_object_add_and_subtract_keep_a_running_total() {
    // A 10 x 10 square polyline and a circle of radius 1 inside it.
    let setup = ["rectang 0,0 10,10", "circle 5,5 1"];

    // Object (the default): pick the square by its edge.
    let (s, log) = run(&[setup[0], setup[1], "area", "o", "10,5"]);
    assert!(has(&log, "Area = 100.0000, Perimeter = 40.0000"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // Add the square by points, subtract the circle as an object.
    let (s, log) = run(&[setup[0], setup[1], "area", "a", "0,0", "10,0", "10,10", "0,10", "", "s", "o", "6,5", "", ""]);
    assert!(has(&log, "Total area = 100.0000"), "{log:?}");
    assert!(has(&log, "Total area = 96.8584"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());
}

#[test]
fn area_too_large_to_hold_is_refused() {
    // A 2e154 square has an area of 4e308, past f64's range; its perimeter fits.
    let big = json!([[0, 0], [2e154, 0], [2e154, 2e154], [0, 2e154]]);
    let mut s = Session::new();
    let err = s.execute("area", &json!({ "points": big })).unwrap_err();
    assert!(err.to_string().contains("the area is too large"), "{err}");
    // Two sides of 1e308: the area fits, the perimeter does not.
    let long = json!([[0, 0], [1e308, 0], [1e308, 1e-300]]);
    let err = s.execute("area", &json!({ "points": long })).unwrap_err();
    assert!(err.to_string().contains("the perimeter is too large"), "{err}");

    // Typed, nothing is reported.
    let (_, log) = run(&["area", "0,0", "2e154,0", "2e154,2e154", "0,2e154", ""]);
    assert!(has(&log, "The area is too large."), "{log:?}");
    assert!(!has(&log, "Area = "), "{log:?}");

    // Areas of 8.1e307 that each fit overflow the running total at the third; the total keeps two.
    let sq = ["0,0", "9e153,0", "9e153,9e153", "0,9e153", ""];
    for mode in ["a", "s"] {
        let mut lines = vec!["area", mode];
        for _ in 0..3 {
            lines.extend(sq);
        }
        let (s, log) = run(&lines);
        assert!(has(&log, "The total area is too large."), "{mode}: {log:?}");
        assert_eq!(log.iter().filter(|l| l.contains("Total area = ")).count(), 2, "{mode}: {log:?}");
        assert!(s.running.is_some(), "{mode}: the command goes on");
    }

    // MEASUREGEOM's Volume option does not keep such an area waiting for a height: the next area is measured.
    let mut lines = vec!["measuregeom", "v", "a", "0,0", "2e154,0", "2e154,2e154", "0,2e154", ""];
    lines.extend(["0,0", "1,0", "1,1", "0,1", "", "2"]);
    let (s, log) = run(&lines);
    assert!(has(&log, "The area is too large."), "{log:?}");
    assert!(has(&log, "Total volume = 2"), "{log:?}");
    assert!(s.running.is_some(), "the command goes on");
}

#[test]
fn area_object_on_an_open_polyline_closes_the_area_but_not_the_perimeter() {
    // Open, 160 long; the closing side back to 0,0 would add 116.619.
    let (_, log) = run(&["pline", "0,0", "100,0", "100,60", "", "area", "o", "50,0"]);
    assert!(has(&log, "Area = 3000.0000, Perimeter = 160.0000"), "{log:?}");
}

#[test]
fn area_length_extends_the_last_segment() {
    // Length continues along the previous segment: 0,0 -> 4,0 -> +6 -> 10,0, then 10,10.
    let (_, log) = run(&["area", "0,0", "4,0", "l", "6", "10,10", ""]);
    assert!(has(&log, "Area = 50.0000"), "{log:?}");
}

#[test]
fn dist_multiple_points_sums_the_segments() {
    let (s, log) = run(&["dist", "0,0", "m", "3,4", "3,10", ""]);
    assert!(has(&log, "Distance = 11.0000"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());
}

#[test]
fn area_arc_option_measures_arc_segments() {
    // A 10 x 10 square whose right side is a half circle of radius 5 bulging out:
    // area 100 + 12.5 pi, perimeter 30 + 5 pi.
    let expected = "Area = 139.2699, Perimeter = 45.7080";

    // Arc mode: a point ends an arc that continues the last segment; Line goes back to lines.
    let (s, _) = run(&["area", "0,0", "10,0", "a"]);
    assert!(s.prompt_text().contains("Specify endpoint of arc"), "{}", s.prompt_text());
    assert!(s.prompt_text().contains("CEnter"), "{}", s.prompt_text());
    let (s, log) = run(&["area", "0,0", "10,0", "a", "10,10", "l", "0,10", ""]);
    assert!(has(&log, expected), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // The same half circle by its center, by its included angle and by a second point.
    for arc in [&["ce", "10,5", "10,10"][..], &["a", "180", "10,10"], &["s", "15,5", "10,10"]] {
        let mut lines = vec!["area", "0,0", "10,0", "a"];
        lines.extend_from_slice(arc);
        lines.extend(["l", "0,10", ""]);
        let (_, log) = run(&lines);
        assert!(has(&log, expected), "{arc:?}: {log:?}");
    }

    // After the arc, Length continues along its end tangent (-X): 10 to 0,10.
    let (_, log) = run(&["area", "0,0", "10,0", "a", "10,10", "l", "l", "10", ""]);
    assert!(has(&log, expected), "{log:?}");

    // Undo removes the arc segment; arc mode stays on.
    let (s, _) = run(&["area", "0,0", "10,0", "a", "10,10", "u"]);
    assert!(s.prompt_text().contains("Specify endpoint of arc"), "{}", s.prompt_text());

    // CLose ends with an arc back to the first point: center 0,10, radius 10, 270 degrees.
    let (s, log) = run(&["area", "0,0", "10,0", "10,10", "a", "cl"]);
    assert!(has(&log, "Area = 335.6194, Perimeter = 67.1239"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());
    // No arc continues the last segment back to the first point when it lies straight behind.
    let (s, log) = run(&["area", "0,0", "10,0", "a", "cl"]);
    assert!(has(&log, "Invalid arc endpoint."), "{log:?}");
    assert!(s.prompt_text().contains("Specify endpoint of arc"), "{}", s.prompt_text());

    // Add mode keeps its prefix in arc mode and adds the curved area to the total.
    let (s, _) = run(&["area", "a", "0,0", "10,0", "a"]);
    assert!(s.prompt_text().contains("(ADD mode) Specify endpoint of arc"), "{}", s.prompt_text());
    let (_, log) = run(&["area", "a", "0,0", "10,0", "a", "10,10", "l", "0,10", ""]);
    assert!(has(&log, "Total area = 139.2699"), "{log:?}");

    // MEASUREGEOM's Volume option measures the same area times the height.
    let (_, log) = run(&["measuregeom", "v", "0,0", "10,0", "a", "10,10", "l", "0,10", "", "2"]);
    assert!(has(&log, "Volume = 278.5398"), "{log:?}");
}

#[test]
fn dist_multiple_points_arc_option_adds_arc_lengths() {
    // 10 along X, then a half circle of radius 5 up to 10,10: 10 + 5 pi.
    let (s, log) = run(&["dist", "0,0", "m", "10,0", "a", "10,10"]);
    assert!(has(&log, "Distance = 25.7080"), "{log:?}");
    assert!(s.prompt_text().contains("Specify endpoint of arc"), "{}", s.prompt_text());

    // Line, then Length continues along the tangent at the arc's end (-X): 5 more.
    let (s, log) = run(&["dist", "0,0", "m", "10,0", "a", "10,10", "l", "l", "5", ""]);
    assert!(has(&log, "Distance = 30.7080"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // Undo removes the arc segment.
    let (_, log) = run(&["dist", "0,0", "m", "10,0", "a", "10,10", "u", ""]);
    assert_eq!(log.iter().filter(|l| l.contains("Distance = 10.0000")).count(), 2, "{log:?}");

    // CLose adds an arc back to the first point and ends: 20 + 15 pi.
    let (s, log) = run(&["dist", "0,0", "m", "10,0", "10,10", "a", "cl"]);
    assert!(has(&log, "Distance = 67.1239"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // CLose needs a second point.
    let (s, log) = run(&["dist", "0,0", "m", "a", "cl"]);
    assert!(has(&log, "CLose needs at least two points."), "{log:?}");
    assert!(s.running.is_some());

    // No arc continues the last segment to a point straight behind it.
    for end in ["5,0", "cl"] {
        let (s, log) = run(&["dist", "0,0", "m", "10,0", "a", end]);
        assert!(has(&log, "Invalid arc endpoint."), "{end}: {log:?}");
        assert!(s.prompt_text().contains("Specify endpoint of arc"), "{end}: {}", s.prompt_text());
    }
}

#[test]
fn area_of_an_open_elliptical_arc_is_refused_and_a_full_ellipse_is_measured() {
    for end in [90, 45] {
        let mut s = Session::new();
        let r = s.execute("ellipse", &json!({"center": [0, 0], "major": [4, 0], "ratio": 0.5, "start": 0, "end": end})).unwrap();
        assert!(s.execute("area", &json!({"handle": r["handle"]})).is_err());
    }
    let mut s = Session::new();
    let r = s.execute("ellipse", &json!({"center": [0, 0], "major": [4, 0], "ratio": 0.5})).unwrap();
    let a = s.execute("area", &json!({"handle": r["handle"]})).unwrap();
    assert!((a["area"].as_f64().unwrap() - 8.0 * std::f64::consts::PI).abs() < 1e-9, "{a}");
    // Object selection of the quarter ellipse says it has no area.
    let (_, log) = run(&["ellipse a c 0,0 4,0 2 0 90", "area", "o", "4,0"]);
    assert!(has(&log, "no area"), "{log:?}");
    assert!(!has(&log, "Area = "), "{log:?}");
}

#[test]
fn tools_inquiry_radius_asks_for_the_object() {
    // Nothing selected: the menu item asks for the arc or circle.
    let mut s = Session::new();
    s.cmdline("circle 0,0 2").unwrap();
    s.start("radius.measure").unwrap();
    assert!(s.prompt_text().contains("Select arc or circle"), "{}", s.prompt_text());
    s.cmdline("2,0").unwrap();
    assert!(has(&s.log, "Radius = 2.0000"), "{:?}", s.log);
    assert!(has(&s.log, "Diameter = 4.0000"), "{:?}", s.log);
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // A selected arc is measured at once.
    let mut s = Session::new();
    let r = s.execute("arc", &json!({"center": [0, 0], "radius": 3, "start": 0, "end": 90})).unwrap();
    s.set_selection(vec![cadcraft_doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()]);
    s.start("radius.measure").unwrap();
    assert!(has(&s.log, "Radius = 3.0000"), "{:?}", s.log);
    assert!(s.running.is_none(), "{}", s.prompt_text());
}

#[test]
fn measuregeom_radius_measures_a_polyline_arc_segment() {
    // A line from 0,0 to 10,0, then a half circle of radius 5 to 10,10 through 15,5.
    let pline = json!({"vertices": [[0, 0], {"p": [10, 0], "bulge": 1}, [10, 10]]});
    let mut s = Session::new();
    s.execute("pline", &pline).unwrap();
    s.cmdline("measuregeom").unwrap();
    s.cmdline("r").unwrap();
    // A pick on the line segment keeps the prompt.
    s.cmdline("5,0").unwrap();
    assert!(has(&s.log, "Select an arc, circle or polyline arc."), "{:?}", s.log);
    assert!(s.running.is_some());
    s.cmdline("15,5").unwrap();
    assert!(has(&s.log, "Radius = 5.0000"), "{:?}", s.log);
    assert!(has(&s.log, "Diameter = 10.0000"), "{:?}", s.log);
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // The same pick from Tools > Inquiry > Radius, and the JSON form with the point on the arc.
    let mut s = Session::new();
    let h = s.execute("pline", &pline).unwrap()["handle"].clone();
    s.start("radius.measure").unwrap();
    s.cmdline("15,5").unwrap();
    assert!(has(&s.log, "Radius = 5.0000"), "{:?}", s.log);
    let r = s.execute("radius.measure", &json!({"handle": h, "point": [15, 5]})).unwrap();
    assert_eq!(r["radius"].as_f64(), Some(5.0), "{r}");
    assert!(s.execute("radius.measure", &json!({"handle": h, "point": [5, 0]})).is_err());
    assert!(s.execute("radius.measure", &json!({"handle": h})).is_err());
}

#[test]
fn measuregeom_typed_asks_for_the_option_then_the_points() {
    // Distance is the default option (#1103: typed, it used to fail asking for `p1`).
    let (s, log) = run(&["measuregeom", "", "0,0", "3,4"]);
    assert!(has(&log, "Distance = 5.0000"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());

    let (_, log) = run(&["circle 0,0 2", "measuregeom", "r", "2,0"]);
    assert!(has(&log, "Radius = 2.0000"), "{log:?}");

    let (_, log) = run(&["measuregeom", "ar", "0,0", "4,0", "4,3", ""]);
    assert!(has(&log, "Area = 6.0000"), "{log:?}");
}

#[test]
fn measuregeom_angle_measures_an_arc_a_circle_two_lines_and_a_vertex() {
    let typed = |s: &mut Session, lines: &[&str]| {
        for l in lines {
            s.cmdline(l).unwrap();
        }
        assert!(s.running.is_none(), "{}", s.prompt_text());
        s.log.clone()
    };

    // Arc: the angle between its endpoints about its center.
    let mut s = Session::new();
    s.execute("arc", &json!({"center": [0, 0], "radius": 2, "start": 0, "end": 270})).unwrap();
    assert!(has(&typed(&mut s, &["measuregeom", "a", "-2,0"]), "Angle = 270.0000°"));

    // Circle: the pick on the circle and a second point, about the center.
    let log = typed(&mut Session::new(), &["circle 0,0 2", "measuregeom", "a", "2,0", "0,5"]);
    assert!(has(&log, "Angle = 90.0000°"), "{log:?}");

    // Two lines that do not meet, and two segments of one polyline.
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[1, 0], [10, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 1], [5, 6]]})).unwrap();
    assert!(has(&typed(&mut s, &["measuregeom", "a", "5,0", "2,3"]), "Angle = 45.0000°"), "{:?}", s.log);
    let mut s = Session::new();
    s.execute("pline", &json!({"vertices": [[0, 0], [10, 0], [10, 10]]})).unwrap();
    assert!(has(&typed(&mut s, &["measuregeom", "a", "5,0", "10,5"]), "Angle = 90.0000°"), "{:?}", s.log);
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.execute("line", &json!({"points": [[0, 5], [10, 5]]})).unwrap();
    assert!(has(&typed(&mut s, &["measuregeom", "a", "5,0", "5,5"]), "The lines are parallel."), "{:?}", s.log);

    // Enter: a vertex and two endpoints.
    let log = typed(&mut Session::new(), &["measuregeom", "a", "", "0,0", "1,0", "-1,1"]);
    assert!(has(&log, "Angle = 135.0000°"), "{log:?}");

    // Empty space is not an object, and the prompt stays.
    let (s, log) = run(&["measuregeom", "a", "50,50"]);
    assert!(has(&log, "*Invalid selection*"), "{log:?}");
    assert!(s.prompt_text().contains("Select arc, circle, line"), "{}", s.prompt_text());
}

#[test]
fn tools_inquiry_angle_asks_for_the_objects() {
    let spec = cadcraft_engine::find_command("angle.measure").expect("no command angle.measure");
    assert_eq!(spec.menu, ["Tools", "Inquiry", "Angle"]);
    let mut s = Session::new();
    s.cmdline("circle 0,0 2").unwrap();
    s.start("angle.measure").unwrap();
    assert!(s.prompt_text().contains("Select arc, circle, line"), "{}", s.prompt_text());
    for l in ["0,2", "-3,0"] {
        s.cmdline(l).unwrap();
    }
    assert!(has(&s.log, "Angle = 90.0000°"), "{:?}", s.log);
    assert!(s.running.is_none(), "{}", s.prompt_text());
}

#[test]
fn area_of_an_ellipse_reports_its_exact_perimeter() {
    // References from the arithmetic-geometric mean (Gauss–Kummer), to 12 decimals. A closed-form
    // estimate gives 46.025625 for the 10 × 4 and 39.874135 for the 10 × 0.1.
    for (a, b, want) in
        [(10.0, 4.0, 46.026_225_191_330), (4.0, 2.0, 19.376_896_441_095), (10.0, 0.1, 40.010_983_297_227), (5.0, 5.0, 31.415_926_535_898)]
    {
        let mut s = Session::new();
        let r = s.execute("ellipse", &json!({"center": [3, -2], "major": [0, a], "ratio": b / a})).unwrap();
        let got = s.execute("area", &json!({"handle": r["handle"]})).unwrap()["perimeter"].as_f64().unwrap();
        assert!((got - want).abs() < 1e-9 * want, "{a} x {b}: {got} vs {want}");
    }
    let (_, log) = run(&["ellipse c 0,0 10,0 4", "area", "o", "10,0"]);
    assert!(has(&log, "Perimeter = 46.0262"), "{log:?}");
}

#[test]
fn measuregeom_volume_multiplies_an_area_by_a_height() {
    // Points: a 4 x 3 triangle's area is 6, times a height of 10.
    let (s, log) = run(&["measuregeom", "v", "0,0", "4,0", "4,3", "", "10"]);
    assert!(has(&log, "Volume = 60.0000"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // Object (the default): a 10 x 10 square polyline, height 2.
    let (s, log) = run(&["rectang 0,0 10,10", "measuregeom", "v", "", "10,5", "2"]);
    assert!(has(&log, "Volume = 200.0000"), "{log:?}");
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // A height that is not a distance is asked for again.
    let (s, log) = run(&["measuregeom", "v", "0,0", "4,0", "4,3", "", "tall"]);
    assert!(has(&log, "Requires a distance."), "{log:?}");
    assert!(s.prompt_text().contains("Specify height"), "{}", s.prompt_text());

    // Enter is not a height; two points give the height as their distance.
    let (mut s, log) = run(&["measuregeom", "v", "0,0", "4,0", "4,3", "", "", "1,1"]);
    assert!(has(&log, "Requires a height."), "{log:?}");
    assert!(s.prompt_text().contains("Specify second point"), "{}", s.prompt_text());
    s.cmdline("7,9").unwrap();
    assert!(has(&s.log, "Volume = 60.0000"), "{:?}", s.log);
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // The JSON forms, by points and by handle.
    let mut s = Session::new();
    let r = s.execute("measuregeom", &json!({"mode": "volume", "points": [[0, 0], [4, 0], [4, 3]], "height": 10})).unwrap();
    assert_eq!(r["volume"], json!(60.0));
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    let r = s.execute("volume.measure", &json!({"handle": c["handle"], "height": -3})).unwrap();
    assert!((r["volume"].as_f64().unwrap() - 3.0 * std::f64::consts::PI).abs() < 1e-12, "{r}");
    assert!(s.execute("volume.measure", &json!({"points": [[0, 0], [1, 0]], "height": 1})).is_err());
    assert!(s.execute("volume.measure", &json!({"points": [[0, 0], [1, 0], [1, 1]]})).is_err());
}

#[test]
fn measure_with_points_that_are_not_points_is_an_error_not_the_selection() {
    let mut s = Session::new();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.set_selection(vec![cadcraft_engine::doc::Handle::parse_hex(c["handle"].as_str().unwrap()).unwrap()]);
    for bad in [json!([[0, 0], [4, 0], "north"]), json!([[0, 0], [4, 0], [4]]), json!(7)] {
        assert!(s.execute("volume.measure", &json!({"points": bad, "height": 10})).is_err(), "{bad}");
        assert!(s.execute("area", &json!({"points": bad})).is_err(), "{bad}");
    }
    // Without `points`, both measure the selected circle.
    let r = s.execute("volume.measure", &json!({"height": 1})).unwrap();
    assert!((r["volume"].as_f64().unwrap() - std::f64::consts::PI).abs() < 1e-12, "{r}");
    let r = s.execute("area", &json!({"points": null})).unwrap();
    assert!((r["area"].as_f64().unwrap() - std::f64::consts::PI).abs() < 1e-12, "{r}");
}

#[test]
fn measuregeom_volume_too_large_to_hold_is_refused() {
    // A 2e150 square has an area of 4e300; times 1e10 it is past f64's range.
    let p = json!([[0, 0], [2e150, 0], [2e150, 2e150], [0, 2e150]]);
    let mut s = Session::new();
    assert!(s.execute("volume.measure", &json!({"points": p, "height": 1e10})).is_err());
    assert!(s.execute("volume.measure", &json!({"points": p, "height": 1})).is_ok());

    // Typed, the height is asked for again.
    let (s, log) = run(&["measuregeom", "v", "0,0", "2e150,0", "2e150,2e150", "0,2e150", "", "1e10"]);
    assert!(has(&log, "The volume is too large."), "{log:?}");
    assert!(!has(&log, "Volume = "), "{log:?}");
    assert!(s.prompt_text().contains("Specify height"), "{}", s.prompt_text());

    // Two volumes that each fit can overflow the running total; the second is asked for again.
    let sq = ["0,0", "1,0", "1,1", "0,1", ""];
    let mut lines = vec!["measuregeom", "v", "a"];
    lines.extend(sq);
    lines.push("1e308");
    lines.extend(sq);
    lines.push("1e308");
    let (s, log) = run(&lines);
    assert!(has(&log, "The volume is too large."), "{log:?}");
    assert!(!has(&log, "Total volume = 0"), "{log:?}");
    assert!(s.prompt_text().contains("Specify height"), "{}", s.prompt_text());
}

#[test]
fn tools_inquiry_volume_adds_and_subtracts_a_running_total() {
    let spec = cadcraft_engine::find_command("volume.measure").expect("no command volume.measure");
    assert_eq!(spec.menu, ["Tools", "Inquiry", "Volume"]);

    // Add a 10 x 10 square by points at height 3, subtract a 2 x 2 square object at height 5.
    let mut s = Session::new();
    s.cmdline("rectang 20,0 22,2").unwrap();
    s.start("volume.measure").unwrap();
    for l in ["a", "0,0", "10,0", "10,10", "0,10", "", "3", "s", "o", "21,0", "5", "", ""] {
        s.cmdline(l).unwrap();
    }
    assert!(has(&s.log, "Total volume = 300.0000"), "{:?}", s.log);
    assert!(has(&s.log, "Total volume = 280.0000"), "{:?}", s.log);
    assert!(s.running.is_none(), "{}", s.prompt_text());

    // Architectural units show the volume as a number, not as a feet-and-inches length.
    let (_, log) = run(&["setvar lunits 4", "measuregeom", "v", "0,0", "10,0", "10,10", "0,10", "", "12"]);
    assert!(has(&log, "Volume = 1200"), "{log:?}");
    assert!(!has(&log, "'-"), "{log:?}");
}
