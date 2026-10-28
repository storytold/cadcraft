//! Read-only variables that report the session's state: AREA, PERIMETER and DISTANCE (the last
//! measurement), LASTPROMPT, LOGINNAME, TEMPPREFIX, PRODUCT, PROGRAM, VERNUM and SCREENSIZE; the
//! USERS1-5 text variables; and TILEMODE, which switches between the Model tab and a layout.

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn getvar(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

fn setvar(s: &mut Session, name: &str, value: Value) -> bool {
    s.execute("setvar", &json!({ "name": name, "value": value })).is_ok()
}

const READ_ONLY: [&str; 10] =
    ["AREA", "PERIMETER", "DISTANCE", "LASTPROMPT", "LOGINNAME", "TEMPPREFIX", "PRODUCT", "PROGRAM", "VERNUM", "SCREENSIZE"];

#[test]
fn read_only_variables_report_and_refuse_changes() {
    let mut s = Session::new();
    for n in READ_ONLY {
        assert!(cadcraft_engine::sysvars::is_read_only(n), "{n}");
        let before = getvar(&mut s, n);
        assert!(!setvar(&mut s, n, json!(1)), "{n} refused");
        if n != "LASTPROMPT" {
            assert_eq!(getvar(&mut s, n), before, "{n} unchanged");
        }
    }
    assert_eq!(getvar(&mut s, "PRODUCT"), json!("CADCraft"));
    assert_eq!(getvar(&mut s, "PROGRAM"), json!("cadcraft"));
    assert!(!getvar(&mut s, "VERNUM").as_str().unwrap().is_empty());
    let temp = getvar(&mut s, "TEMPPREFIX").as_str().unwrap().to_string();
    assert!(temp.ends_with(std::path::MAIN_SEPARATOR), "{temp}");
    assert!(getvar(&mut s, "LOGINNAME").is_string());
    s.viewport_px = (1024.0, 600.0);
    assert_eq!(getvar(&mut s, "SCREENSIZE"), json!([1024.0, 600.0]));
    // Typed: SETVAR says it is read-only.
    s.cmdline("setvar product").unwrap();
    assert!(s.log.iter().any(|l| l.contains("PRODUCT") && l.contains("read only")), "{:?}", s.log);
    // LASTPROMPT is the last line shown.
    s.echo("Hello there");
    assert_eq!(getvar(&mut s, "LASTPROMPT"), json!("Hello there"));
    // All listed by SETVAR ?.
    let all = cadcraft_engine::sysvars::list(&s);
    for n in READ_ONLY.iter().chain(&["USERS1", "USERS5", "TILEMODE"]) {
        assert!(all.get(*n).is_some(), "{n} listed");
    }
}

#[test]
fn area_perimeter_and_distance_keep_the_last_measurement() {
    let mut s = Session::new();
    assert_eq!(getvar(&mut s, "AREA"), json!(0.0));
    s.execute("area", &json!({ "points": [[0, 0], [4, 0], [4, 3], [0, 3]] })).unwrap();
    assert_eq!(getvar(&mut s, "AREA"), json!(12.0));
    assert_eq!(getvar(&mut s, "PERIMETER"), json!(14.0));
    s.execute("dist", &json!({ "p1": [0, 0], "p2": [3, 4] })).unwrap();
    assert_eq!(getvar(&mut s, "DISTANCE"), json!(5.0));
    // Typed AREA and DIST too.
    s.script("AREA 0,0 2,0 2,2 0,2\n\n").unwrap();
    assert_eq!(getvar(&mut s, "AREA"), json!(4.0));
    assert_eq!(getvar(&mut s, "PERIMETER"), json!(8.0));
    s.script("DIST 0,0 6,8\n").unwrap();
    assert_eq!(getvar(&mut s, "DISTANCE"), json!(10.0));
    // MEASUREGEOM reaches them through DIST and AREA.
    s.execute("measuregeom", &json!({ "mode": "distance", "p1": [0, 0], "p2": [0, 7] })).unwrap();
    assert_eq!(getvar(&mut s, "DISTANCE"), json!(7.0));
    // A measurement too large to report leaves the last one.
    assert!(s.execute("area", &json!({ "points": [[0, 0], [1e300, 0], [1e300, 1e300], [0, 1e300]] })).is_err());
    assert_eq!(getvar(&mut s, "AREA"), json!(4.0));
}

#[test]
fn users_variables_keep_text_for_the_session() {
    let mut s = Session::new();
    assert_eq!(getvar(&mut s, "USERS1"), json!(""));
    assert!(setvar(&mut s, "USERS1", json!("Job 42")));
    assert!(setvar(&mut s, "users5", json!(7)));
    assert_eq!(getvar(&mut s, "USERS1"), json!("Job 42"));
    assert_eq!(getvar(&mut s, "USERS5"), json!("7"));
    s.cmdline("setvar users2 hello").unwrap();
    assert_eq!(getvar(&mut s, "USERS2"), json!("hello"));
    for v in [json!(null), json!([1]), json!({ "a": 1 }), json!(true), json!("x".repeat(5000))] {
        assert!(!setvar(&mut s, "USERS3", v.clone()), "{v}");
    }
    assert_eq!(getvar(&mut s, "USERS3"), json!(""));
    assert!(s.execute("getvar", &json!({ "name": "USERS6" })).is_err());
    assert!(s.execute("getvar", &json!({ "name": "USERS0" })).is_err());
}

#[test]
fn tilemode_switches_between_model_and_a_layout() {
    let mut s = Session::new();
    assert_eq!(getvar(&mut s, "TILEMODE"), json!(1));
    s.cmdline("tilemode 0").unwrap();
    assert_eq!(getvar(&mut s, "TILEMODE"), json!(0));
    assert_eq!(getvar(&mut s, "CTAB"), json!("Layout1"));
    assert!(setvar(&mut s, "TILEMODE", json!(1)));
    assert_eq!(getvar(&mut s, "CTAB"), json!("Model"));
    // From a layout, 0 stays on it.
    s.execute("layout", &json!({ "option": "new", "name": "Sheet B" })).unwrap();
    s.execute("layout.set", &json!({ "name": "Sheet B" })).unwrap();
    assert!(setvar(&mut s, "TILEMODE", json!(0)));
    assert_eq!(getvar(&mut s, "CTAB"), json!("Sheet B"));
    for v in [json!(2), json!(-1), json!("x"), json!(null), json!(f64::MAX)] {
        assert!(!setvar(&mut s, "TILEMODE", v.clone()), "{v}");
    }
    assert_eq!(getvar(&mut s, "CTAB"), json!("Sheet B"));
}

#[test]
fn hostile_values_never_panic() {
    let mut s = Session::new();
    let hostile = [
        json!(f64::MAX),
        json!(i64::MIN),
        json!(u64::MAX),
        json!("NaN"),
        json!("1e400"),
        json!(""),
        json!(null),
        json!([1, 2]),
        json!({ "x": 1 }),
        json!("9".repeat(400)),
    ];
    let names = READ_ONLY.iter().chain(&["USERS1", "USERS2", "USERS3", "USERS4", "USERS5", "TILEMODE"]);
    for n in names {
        for v in &hostile {
            let _ = setvar(&mut s, n, v.clone());
            let _ = getvar(&mut s, n);
        }
    }
    // Non-finite measurements never reach the variables.
    s.viewport_px = (f64::NAN, f64::INFINITY);
    let _ = s.execute("getvar", &json!({ "name": "SCREENSIZE" }));
    assert!(getvar(&mut s, "AREA").as_f64().unwrap().is_finite());
}
