//! AREA's Object / Add area / Subtract area and DIST's Multiple points, typed as at the command line.

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
