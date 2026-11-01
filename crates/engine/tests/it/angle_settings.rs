//! Typed angles follow the drawing's angle settings: AUNITS for bare numbers, directions
//! measured from ANGBASE in the ANGDIR sense, `<<` / `<<<` overrides.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::lisp::value::Value;
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
    // A surveyor's bearing is an angle like any other, from ANGBASE in the ANGDIR sense: AutoCAD
    // 2024 draws `@10<N45dE` at 45 degrees here, and `@10<N` east.
    setvar(&mut s, "AUNITS", json!(0));
    let h = 10.0 / 2f64.sqrt();
    assert_near(line_end(&mut s, "@10<N45dE"), h, h, "@10<N45dE");
    assert_near(line_end(&mut s, "@10<N"), 10.0, 0.0, "@10<N");
}

#[test]
fn bearings_and_compass_points_turn_with_angbase_and_angdir() {
    // AutoCAD 2024 (accoreconsole), LINE 0,0 to the point: (input, AUNITS, ANGBASE in degrees,
    // ANGDIR, the direction of the line in degrees counterclockwise from east). A bearing or a
    // compass point is an angle like any other, in any AUNITS; `<<<` measures it from east.
    let cases = [
        ("@10<N", 0, 0.0, 0, 90.0),
        ("@10<N", 4, 0.0, 0, 90.0),
        ("@10<E", 0, 0.0, 0, 0.0),
        ("@10<W", 4, 0.0, 0, 180.0),
        ("@10<S", 1, 0.0, 0, 270.0),
        ("@10<N45dE", 0, 0.0, 0, 45.0),
        ("@10<N", 4, 90.0, 1, 0.0),
        ("@10<N45dE", 0, 90.0, 1, 45.0),
        ("@10<N45dE", 0, 0.0, 1, 315.0),
        ("@10<N45dE", 0, 90.0, 0, 135.0),
        ("@10<N", 0, 90.0, 0, 180.0),
        ("@10<E", 0, 0.0, 1, 0.0),
        ("@10<N", 0, 0.0, 1, 270.0),
        // ANGBASE 0.5 radians: AutoCAD drew it at 253.64789 degrees.
        ("@10<S45dW", 0, 0.5f64.to_degrees(), 0, 225.0 + 0.5f64.to_degrees()),
        ("@10<<<N45dE", 4, 90.0, 1, 45.0),
        ("@10<<<N45dE", 0, 90.0, 1, 45.0),
        ("@10<<<N", 4, 90.0, 1, 90.0),
        ("@10<<<N", 0, 90.0, 1, 90.0),
    ];
    for (to, aunits, angbase, angdir, deg) in cases {
        let mut s = Session::new();
        setvar(&mut s, "AUNITS", json!(aunits));
        setvar(&mut s, "ANGBASE", json!(angbase));
        setvar(&mut s, "ANGDIR", json!(angdir));
        let a = f64::to_radians(deg);
        assert_near(line_end(&mut s, to), 10.0 * a.cos(), 10.0 * a.sin(), &format!("{to} AUNITS {aunits} ANGBASE {angbase} ANGDIR {angdir}"));
    }
    // `<<` is in degrees only: AutoCAD refuses a bearing or a compass point there.
    let settings = AngleSettings { aunits: 0, angbase: std::f64::consts::FRAC_PI_2, clockwise: true };
    assert_eq!(settings.direction("<<N45dE"), None);
    assert_eq!(settings.direction("<<N"), None);
    assert!(settings.direction("<<45").is_some_and(|a| (a - std::f64::consts::FRAC_PI_4).abs() < 1e-12));
    // A compass point as an amount turns as ANGDIR says: ROTATE by N with ANGDIR 1 turns a line
    // along +X to -Y in AutoCAD 2024.
    let mut s = Session::new();
    setvar(&mut s, "ANGDIR", json!(1));
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    for t in ["rotate", "all", "", "0,0", "N"] {
        s.cmdline(t).unwrap();
    }
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::Line(l) => assert_near(l.b.xy(), 0.0, -10.0, "ROTATE by N with ANGDIR 1"),
        k => panic!("expected a line, got {k:?}"),
    }
    // ANGTOF reads them the same way: AutoCAD 2024 returns 0 for "N" with ANGBASE 90, ANGDIR 1
    // and AUNITS 3, and 4.18879020478639 for "S 30d W".
    let mut s = Session::new();
    setvar(&mut s, "AUNITS", json!(3));
    setvar(&mut s, "ANGBASE", json!(90));
    setvar(&mut s, "ANGDIR", json!(1));
    let n = s.lisp_eval("(angtof \"N\")", false, true, Vec::new()).unwrap();
    assert!(matches!(n, Some(Ok(Value::Real(a))) if a.abs() < 1e-12), "{n:?}");
    let mut s = Session::new();
    setvar(&mut s, "AUNITS", json!(4));
    let sw = s.lisp_eval("(angtof \"S 30d W\")", false, true, Vec::new()).unwrap();
    assert!(matches!(sw, Some(Ok(Value::Real(a))) if (a - 4.18879020478639).abs() < 1e-12), "{sw:?}");
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
