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
fn area_object_on_an_open_polyline_closes_the_area_but_not_the_perimeter() {
    // Open, 160 long; the closing side back to 0,0 would add 116.619.
    let (_, log) = run(&["pline", "0,0", "100,0", "100,60", "", "area", "o", "50,0"]);
    assert!(has(&log, "Area = 3000.0000, Perimeter = 160.0000"), "{log:?}");
}

#[test]
fn area_unsupported_keywords_say_so_and_length_extends_the_last_segment() {
    let (s, log) = run(&["area", "0,0", "a"]);
    assert!(has(&log, "not available yet"), "{log:?}");
    assert!(s.running.is_some());
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
