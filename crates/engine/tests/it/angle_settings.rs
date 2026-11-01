//! Typed angles follow the drawing's angle settings: AUNITS for bare numbers, directions
//! measured from ANGBASE in the ANGDIR sense, `<<` / `<<<` overrides.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::units::AngleSettings;
use serde_json::json;

fn setvar(s: &mut Session, name: &str, value: serde_json::Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

/// Draws LINE 0,0 `to` at the command line; returns the end point.
fn line_end(s: &mut Session, to: &str) -> Vec2 {
    for t in ["line", "0,0", to, ""] {
        s.cmdline(t).unwrap();
    }
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::Line(l) => l.b.xy(),
        k => panic!("expected a line, got {k:?}"),
    }
}

fn assert_near(p: Vec2, x: f64, y: f64, what: &str) {
    assert!(p.near(Vec2::new(x, y), 1e-9), "{what}: got {p:?}, expected ({x}, {y})");
}

#[test]
fn polar_angles_measure_from_angbase_in_angdir() {
    let mut s = Session::new();
    // 0 = north, clockwise (as in surveying).
    setvar(&mut s, "ANGBASE", json!(90));
    setvar(&mut s, "ANGDIR", json!(1));
    assert!((s.doc().unwrap().header.f64("ANGBASE", 0.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-12, "ANGBASE is held in radians");
    assert_near(line_end(&mut s, "@10<90"), 10.0, 0.0, "@10<90");
    assert_near(line_end(&mut s, "@10<0"), 0.0, 10.0, "@10<0");
    assert_near(line_end(&mut s, "10<180"), 0.0, -10.0, "10<180");
    // The overrides measure from +X counterclockwise: `<<` in degrees, `<<<` in AUNITS.
    assert_near(line_end(&mut s, "@10<<90"), 0.0, 10.0, "@10<<90");
    setvar(&mut s, "AUNITS", json!(3));
    assert_near(line_end(&mut s, "@10<<<3.141592653589793"), -10.0, 0.0, "@10<<<pi");
    // A surveyor's bearing is absolute.
    assert_near(line_end(&mut s, "@10<N90dE"), 10.0, 0.0, "@10<N90dE");
}

#[test]
fn bare_numbers_are_in_aunits_and_suffixes_override() {
    let mut s = Session::new();
    setvar(&mut s, "AUNITS", json!(3));
    assert_near(line_end(&mut s, "@10<1.5707963267948966"), 0.0, 10.0, "radians");
    assert_near(line_end(&mut s, "@10<180d"), -10.0, 0.0, "180d under radians");
    setvar(&mut s, "AUNITS", json!(2));
    assert_near(line_end(&mut s, "@10<100"), 0.0, 10.0, "grads");
    assert_near(line_end(&mut s, "@10<0.5r"), 10.0 * 0.5f64.cos(), 10.0 * 0.5f64.sin(), "0.5r under grads");
    setvar(&mut s, "AUNITS", json!(1));
    assert_near(line_end(&mut s, "@10<89d60'"), 0.0, 10.0, "deg/min");
    assert_near(line_end(&mut s, "@10<-89d59'60\""), 0.0, -10.0, "negative deg/min/sec");
}

#[test]
fn rotate_reference_angles_follow_angbase_and_angdir() {
    let mut s = Session::new();
    setvar(&mut s, "ANGBASE", json!(45));
    setvar(&mut s, "ANGDIR", json!(1));
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    // Reference 0 (= 45° here) to new angle 90 (= -45°): a quarter turn clockwise.
    for t in ["rotate", "all", "", "0,0", "r", "0", "90"] {
        s.cmdline(t).unwrap();
    }
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::Line(l) => assert_near(l.b.xy(), 0.0, -10.0, "ROTATE Reference 0 to 90 with ANGDIR 1"),
        k => panic!("expected a line, got {k:?}"),
    }
}

#[test]
fn json_and_default_parsing_keep_degrees_from_east() {
    let mut s = Session::new();
    setvar(&mut s, "ANGBASE", json!(90));
    setvar(&mut s, "ANGDIR", json!(1));
    setvar(&mut s, "AUNITS", json!(3));
    // JSON params are unaffected by the drawing's angle settings.
    let r = s.execute("line", &json!({ "points": ["0,0", "@10<90"] })).unwrap();
    assert!(r.is_object());
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::Line(l) => assert_near(l.b.xy(), 0.0, 10.0, "JSON @10<90"),
        k => panic!("expected a line, got {k:?}"),
    }
    assert!((cadcraft_engine::units::parse_angle("90").unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    // SETVAR reports ANGBASE in degrees.
    let v = cadcraft_engine::sysvars::get(&s, "ANGBASE").unwrap();
    assert!((v.as_f64().unwrap() - 90.0).abs() < 1e-9, "{v}");
    let a = AngleSettings { aunits: 0, angbase: 0.0, clockwise: false };
    assert_eq!(a.direction(""), None);
    assert_eq!(a.direction("nan"), None);
    assert_eq!(a.direction("1e400"), None);
}
