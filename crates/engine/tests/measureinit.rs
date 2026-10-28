//! MEASUREINIT: the units of a new drawing made without a template (issue #1182). It is a profile
//! setting, so a metric choice survives restarts; an explicit `metric` or template still wins.

use cadcraft_engine::Session;
use serde_json::json;

fn var(s: &mut Session, name: &str) -> serde_json::Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

#[test]
fn imperial_by_default() {
    let mut s = Session::empty();
    assert_eq!(var(&mut s, "MEASUREINIT"), json!(0));
    s.execute("new", &json!({})).unwrap();
    assert_eq!(var(&mut s, "INSUNITS"), json!(1));
    assert_eq!(var(&mut s, "MEASUREMENT"), json!(0));
}

#[test]
fn new_follows_measureinit() {
    let mut s = Session::empty();
    s.execute("options", &json!({ "MEASUREINIT": 1 })).unwrap();
    s.execute("new", &json!({})).unwrap();
    assert_eq!(var(&mut s, "INSUNITS"), json!(4));
    assert_eq!(var(&mut s, "MEASUREMENT"), json!(1));
    assert_eq!(var(&mut s, "DIMSTYLE"), json!("ISO-25"));
    // An explicit choice still wins.
    s.execute("new", &json!({ "metric": false })).unwrap();
    assert_eq!(var(&mut s, "INSUNITS"), json!(1));
}

#[test]
fn only_zero_or_one() {
    let mut s = Session::empty();
    for bad in [json!(2), json!(-1), json!("metric")] {
        assert!(s.execute("options", &json!({ "MEASUREINIT": bad })).is_err(), "{bad}");
    }
    assert_eq!(var(&mut s, "MEASUREINIT"), json!(0));
}

#[test]
fn kept_in_the_profile() {
    let mut s = Session::empty();
    s.execute("options", &json!({ "MEASUREINIT": 1 })).unwrap();
    let saved = cadcraft_engine::cmd::options::profile(&s);
    assert_eq!(saved["MEASUREINIT"], json!(1));
    let mut restarted = Session::empty();
    cadcraft_engine::cmd::options::restore(&mut restarted, &saved);
    assert_eq!(var(&mut restarted, "MEASUREINIT"), json!(1));
}
