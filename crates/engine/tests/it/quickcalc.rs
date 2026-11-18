//! CAL and the engine side of the QuickCalc palette: points and vector functions, unit
//! conversion, values picked from the drawing (#563).

use cadcraft_engine::Session;
use serde_json::json;

#[test]
fn cal_evaluates_points_and_vector_functions() {
    let mut s = Session::new();
    let r = s.execute("cal", &json!({"expr": "(3+4)*2^2"})).unwrap();
    assert_eq!((r["value"].as_f64(), r["message"].as_str()), (Some(28.0), Some("28")));
    let r = s.execute("cal", &json!({"expr": "plt([0,0],[10,0],0.25)+[0,1]"})).unwrap();
    assert_eq!(r["point"], json!([2.5, 1.0]), "{r}");
    assert_eq!(r["message"], "2.5,1");
    let r = s.execute("cal", &json!({"expr": "dist(p, [3,4]) + k", "vars": {"P": [0, 0], "k": 1}})).unwrap();
    assert_eq!(r["value"], 6.0, "variables are case-insensitive and may be points: {r}");
    let r = s.execute("cal", &json!({"expr": "ang([0,0],[0,5])"})).unwrap();
    assert_eq!(r["value"], 90.0);
    let r = s.execute("cal", &json!({"expr": "[10<90]"})).unwrap();
    assert!(r["point"][0].as_f64().unwrap().abs() < 1e-9 && (r["point"][1].as_f64().unwrap() - 10.0).abs() < 1e-9, "{r}");
    for bad in ["[1,2]+1", "dist([0,0])", "1/0", "sqrt(-1)", "x", "ill([0,0],[1,0],[0,1],[1,1])"] {
        assert!(s.execute("cal", &json!({"expr": bad})).is_err(), "{bad}");
    }
}

#[test]
fn cal_converts_units() {
    let mut s = Session::new();
    let conv = |s: &mut Session, t: &str, from: &str, to: &str, v: serde_json::Value| {
        s.execute("cal", &json!({"convert": {"type": t, "from": from, "to": to, "value": v}})).map(|r| r["value"].as_f64().unwrap())
    };
    assert!((conv(&mut s, "length", "in", "mm", json!(1)).unwrap() - 25.4).abs() < 1e-9);
    assert!((conv(&mut s, "area", "m2", "ft²", json!(1)).unwrap() - 10.763910416709722).abs() < 1e-9);
    assert!((conv(&mut s, "volume", "l", "galus", json!("2*1.5")).unwrap() - 0.792516157).abs() < 1e-6);
    assert!((conv(&mut s, "angle", "rad", "deg", json!("pi")).unwrap() - 180.0).abs() < 1e-9);
    // The type may be left out when the units say it.
    let r = s.execute("cal", &json!({"convert": {"from": "ft", "to": "in", "value": 2}})).unwrap();
    assert_eq!((r["value"].as_f64(), r["message"].as_str()), (Some(24.0), Some("2 ft = 24 in")));
    assert!(conv(&mut s, "length", "m", "m2", json!(1)).is_err(), "metres don't convert to square metres");
    assert!(conv(&mut s, "mass", "kg", "lb", json!(1)).is_err());
    assert!(conv(&mut s, "length", "m", "mm", json!(true)).is_err());
    let r = s.execute("cal", &json!({"units": true})).unwrap();
    for kind in ["length", "area", "volume", "angle"] {
        assert!(r["units"][kind].as_array().is_some_and(|u| u.len() >= 4), "{kind}: {r}");
    }
}

#[test]
fn picks_read_values_from_points() {
    let mut s = Session::new();
    let r = s.execute("cal", &json!({"pick": "distance", "points": [[0, 0], [3, 4]]})).unwrap();
    assert_eq!(r["value"], 5.0);
    let r = s.execute("quickcalc.pick", &json!({"mode": "intersection", "points": [[0, 0], [2, 2], [0, 2], [2, 0]]})).unwrap();
    assert_eq!(r["point"], json!([1.0, 1.0]));
    assert!(s.execute("quickcalc.pick", &json!({"mode": "angle", "points": [[0, 0]]})).is_err(), "two points are needed");
    assert!(s.execute("quickcalc.pick", &json!({"mode": "nope", "points": []})).is_err());
}

#[test]
fn cal_and_picks_at_the_command_line() {
    let mut s = Session::new();
    s.cmdline("cal").unwrap();
    s.cmdline("(3+4)*2").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("14"));
    s.start("quickcalc.pick").unwrap();
    s.cmdline("d").unwrap();
    s.cmdline("0,0").unwrap();
    s.cmdline("3,4").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("QuickCalc: 5"));
    // A point at the first prompt picks coordinates.
    s.start("quickcalc.pick").unwrap();
    s.cmdline("2,7").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("QuickCalc: 2,7"));
    s.start("quickcalc.pick").unwrap();
    for t in ["i", "0,0", "4,4", "0,4", "4,0"] {
        s.cmdline(t).unwrap();
    }
    assert_eq!(s.log.last().map(String::as_str), Some("QuickCalc: 2,2"));
    assert!(s.running.is_none());
}

#[test]
fn hostile_expressions_never_panic() {
    let mut s = Session::new();
    for e in [
        "[".repeat(10_000),
        "(".repeat(300) + "1",
        "max(1,2,3,4,5,6,7,8,9)".into(),
        "1e999".into(),
        "[1e308,1e308]*1e10".into(),
        "ang()".into(),
        "\u{0}".into(),
    ] {
        assert!(s.execute("cal", &json!({"expr": e})).is_err(), "{e:.40}");
    }
    assert!(s.execute("cal", &json!({"expr": "x", "vars": {"x": [1]}})).is_err());
    assert!(s.execute("cal", &json!({"pick": "point", "points": [[1e308, "a"]]})).is_err());
}
