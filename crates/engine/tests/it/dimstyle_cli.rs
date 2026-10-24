//! -DIMSTYLE at the command line and as JSON (save, annotative, restore, status, variables,
//! apply, list), and `dimstyle.overrides`, which sets the current style's overrides.

use cadcraft_engine::doc::{Dimension, EntityKind, Handle};
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn dimension(s: &mut Session) -> Handle {
    s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    s.doc().unwrap().model.last().unwrap().handle
}

fn dim(s: &Session, h: Handle) -> Dimension {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        k => panic!("not a dimension: {k:?}"),
    }
}

fn current(s: &Session) -> String {
    s.doc().unwrap().header.str("DIMSTYLE", "")
}

#[test]
fn save_restore_status_and_variables_as_json() {
    let mut s = Session::new();
    // A current-style override (SETVAR) goes into the saved style.
    s.execute("setvar", &json!({ "name": "DIMTXT", "value": 0.25 })).unwrap();
    let r = s.execute("-dimstyle", &json!({ "option": "save", "name": "Plan" })).unwrap();
    assert_eq!(r["style"], "Plan");
    assert_eq!(current(&s), "Plan");
    let d = s.doc().unwrap();
    assert!(d.dim_overrides().is_empty(), "saved into the style");
    assert_eq!(d.dim_style("Plan").unwrap().text_height, 0.25);
    // A name in use is redefined only when asked to.
    assert!(s.execute("-dimstyle", &json!({ "option": "save", "name": "plan" })).is_err());
    s.execute("-dimstyle", &json!({ "option": "save", "name": "plan", "replace": true })).unwrap();
    assert_eq!(s.doc().unwrap().dim_styles.iter().filter(|st| st.name.eq_ignore_ascii_case("plan")).count(), 1);
    s.execute("-dimstyle", &json!({ "option": "restore", "name": "standard" })).unwrap();
    assert_eq!(current(&s), "Standard");
    let status = s.execute("-dimstyle", &json!({ "option": "status" })).unwrap();
    assert_eq!(status["variables"]["DIMTXT"], 0.18);
    let vars = s.execute("-dimstyle", &json!({ "option": "variables", "name": "Plan" })).unwrap();
    assert_eq!(vars["variables"]["DIMTXT"], 0.25);
    s.execute("-dimstyle", &json!({ "option": "annotative", "name": "Anno" })).unwrap();
    assert!(s.doc().unwrap().dim_style("Anno").unwrap().annotative);
    let list = s.execute("-dimstyle", &json!({ "option": "list", "pattern": "p*" })).unwrap();
    assert_eq!(list["styles"], json!(["Plan"]));
    // A dimension's style becomes current; its variables include its overrides.
    let h = dimension(&mut s);
    s.execute("properties.set", &json!({ "handles": [h.hex()], "textHeight": 0.4 })).unwrap();
    let vars = s.execute("-dimstyle", &json!({ "option": "variables", "handle": h.hex() })).unwrap();
    assert_eq!(vars["variables"]["DIMTXT"], 0.4);
    s.execute("-dimstyle", &json!({ "option": "restore", "handle": h.hex() })).unwrap();
    assert_eq!(current(&s), "Anno");
}

#[test]
fn apply_gives_dimensions_the_current_settings() {
    let mut s = Session::new();
    let h = dimension(&mut s);
    s.execute("dimstyle", &json!({ "name": "Plan", "DIMTXT": 0.3 })).unwrap();
    s.execute("setvar", &json!({ "name": "DIMDEC", "value": 1 })).unwrap();
    let r = s.execute("-dimstyle", &json!({ "option": "apply", "handles": [h.hex()] })).unwrap();
    assert_eq!(r["changed"], 1);
    let d = dim(&s, h);
    assert_eq!(d.style, "Plan");
    assert_eq!(d.overrides.get("decimals"), Some(&json!(1)));
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &d);
    assert_eq!(g.value, "10.0");
}

#[test]
fn the_command_line_asks_like_autocad() {
    let mut s = Session::new();
    s.cmdline("-DIMSTYLE").unwrap();
    assert!(s.prompt_text().contains("ANnotative"), "{}", s.prompt_text());
    s.cmdline("S").unwrap();
    s.cmdline("Plan").unwrap();
    assert!(s.running.is_none());
    assert_eq!(current(&s), "Plan");
    s.cmdline("-DIMSTYLE").unwrap();
    s.cmdline("R").unwrap();
    s.cmdline("Standard").unwrap();
    assert_eq!(current(&s), "Standard");
    let before = s.log.len();
    s.cmdline("-DIMSTYLE").unwrap();
    s.cmdline("ST").unwrap();
    assert!(s.log[before..].iter().any(|l| l.starts_with("DIMTXT") && l.contains("Text height")), "{:?}", &s.log[before..]);
    // A name in use asks to redefine it.
    s.cmdline("-DIMSTYLE").unwrap();
    s.cmdline("S").unwrap();
    s.cmdline("Plan").unwrap();
    assert!(s.prompt_text().contains("redefine"), "{}", s.prompt_text());
    s.cmdline("Y").unwrap();
    assert!(s.running.is_none());
    // Apply: the objects to give the current settings.
    s.cmdline("-DIMSTYLE").unwrap();
    s.cmdline("R").unwrap();
    s.cmdline("Standard").unwrap();
    let h = dimension(&mut s);
    s.execute("dimstyle.current", &json!({ "name": "Plan" })).unwrap();
    s.cmdline("-DIMSTYLE").unwrap();
    s.cmdline("A").unwrap();
    s.input(Input::Pick(vec![h])).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(dim(&s, h).style, "Plan");
    // ~name compares a style with the current settings.
    s.cmdline("-DIMSTYLE").unwrap();
    s.cmdline("R").unwrap();
    let before = s.log.len();
    s.cmdline("~Standard").unwrap();
    assert!(s.log[before..].iter().any(|l| l.contains("Differences")), "{:?}", &s.log[before..]);
    s.cancel();
}

#[test]
fn style_overrides_are_set_in_one_step() {
    let mut s = Session::new();
    s.execute("dimstyle.overrides", &json!({ "textHeight": 0.3, "DIMCLRT": 1 })).unwrap();
    let o = s.doc().unwrap().dim_overrides();
    assert_eq!(o.get("textHeight"), Some(&json!(0.3)));
    assert!(o.contains_key("textColor"), "{o:?}");
    s.undo().unwrap();
    assert!(s.doc().unwrap().dim_overrides().is_empty(), "one undo step");
    // Unusable values change nothing; names that aren't variables are refused.
    assert!(s.execute("dimstyle.overrides", &json!({ "textHeight": -1, "DIMDEC": 2 })).is_err());
    assert!(s.execute("dimstyle.overrides", &json!({ "nope": 1 })).is_err());
    assert!(s.doc().unwrap().dim_overrides().is_empty());
    s.execute("dimstyle.overrides", &json!({ "DIMDEC": 2 })).unwrap();
    // Clear drops them only when every value is usable.
    assert!(s.execute("dimstyle.overrides", &json!({ "clear": true, "textHeight": -1 })).is_err());
    assert!(s.execute("dimstyle.overrides", &json!({ "clear": true, "nope": 1 })).is_err());
    assert_eq!(s.doc().unwrap().dim_overrides().get("decimals"), Some(&json!(2)));
    s.execute("dimstyle.overrides", &json!({ "clear": true, "textGap": 0.05 })).unwrap();
    let o = s.doc().unwrap().dim_overrides();
    assert_eq!((o.len(), o.get("textGap")), (1, Some(&json!(0.05))));
    s.execute("dimstyle.overrides", &json!({ "clear": true })).unwrap();
    assert!(s.doc().unwrap().dim_overrides().is_empty());
    // A lineweight by name goes in as the number the header variable holds.
    s.execute("dimstyle.overrides", &json!({ "clear": true, "DIMLWD": "bylayer" })).unwrap();
    assert_eq!(s.doc().unwrap().header.i64("DIMLWD", 0), -1);
}

#[test]
fn refused_calls_leave_no_undo_step() {
    let mut s = Session::new();
    s.execute("-dimstyle", &json!({ "option": "save", "name": "Plan" })).unwrap();
    assert!(s.execute("-dimstyle", &json!({ "option": "save", "name": "plan" })).is_err());
    assert!(s.execute("-dimstyle", &json!({ "option": "restore", "name": "Nope" })).is_err());
    // Apply to objects none of which is a dimension changes nothing, and makes no step either.
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let line = s.doc().unwrap().model.last().unwrap().handle.hex();
    s.undo().unwrap();
    assert_eq!(s.execute("-dimstyle", &json!({ "option": "apply", "handles": [line] })).unwrap()["changed"], 0);
    // The one undo step is the save.
    s.undo().unwrap();
    assert!(s.doc().unwrap().dim_style("Plan").is_none());
}

#[test]
fn new_and_renamed_styles_take_the_names_dimstyle_takes() {
    let mut s = Session::new();
    for name in ["~Plan", "a*b", "x?"] {
        assert!(s.execute("dimstyle", &json!({ "name": name })).is_err(), "{name}");
    }
    s.execute("dimstyle", &json!({ "name": "Plan", "current": false })).unwrap();
    assert!(s.execute("dimstyle.rename", &json!({ "from": "Plan", "to": "~Plan" })).is_err());
    s.execute("dimstyle.rename", &json!({ "from": "Plan", "to": "Site" })).unwrap();
    assert!(s.doc().unwrap().dim_style("Site").is_some());
}

#[test]
fn a_refused_answer_asks_the_same_question() {
    let mut s = Session::new();
    s.cmdline("-DIMSTYLE").unwrap();
    s.cmdline("R").unwrap();
    // A style to compare with that doesn't exist.
    let _ = s.cmdline("~Nope");
    assert!(s.prompt_text().contains("Enter a dimension style name"), "{}", s.prompt_text());
    // Words at "Select dimension".
    s.cmdline("").unwrap();
    let _ = s.cmdline("Standard");
    assert!(s.prompt_text().contains("Select dimension"), "{}", s.prompt_text());
    s.cancel();
}

#[test]
fn listing_with_many_stars_takes_no_time() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "a".repeat(200), "current": false })).unwrap();
    let start = std::time::Instant::now();
    for pattern in ["*".repeat(255) + "x", "*a".repeat(120) + "b"] {
        let r = s.execute("-dimstyle", &json!({ "option": "list", "pattern": pattern })).unwrap();
        assert_eq!(r["styles"], json!([]));
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
    let r = s.execute("-dimstyle", &json!({ "option": "list", "pattern": "a*a?" })).unwrap();
    assert_eq!(r["styles"], json!(["a".repeat(200)]));
}

#[test]
fn hostile_requests_never_panic() {
    let mut s = Session::new();
    let h = dimension(&mut s);
    for p in [
        json!({ "option": "save", "name": "" }),
        json!({ "option": "save", "name": "a*b" }),
        json!({ "option": "restore", "handle": "zz" }),
        json!({ "option": "variables", "handle": 1e300 }),
        json!({ "option": "apply", "handles": [h.hex(), "1", 5] }),
        json!({ "option": "list", "pattern": "*".repeat(100_000) }),
        json!({ "option": 5 }),
    ] {
        let _ = s.execute("-dimstyle", &p);
    }
    for p in [json!({ "DIMTXT": "x" }), json!({ "DIMBLK": [1] }), json!({ "DIMCLRT": { "a": 1 } }), json!({ "clear": "yes" })] {
        let _ = s.execute("dimstyle.overrides", &p);
    }
    for line in ["-DIMSTYLE", "?", "", "-DIMSTYLE", "V", "", "-DIMSTYLE", "AN", "N", "?", "Bad/Name", "Good"] {
        let _ = s.cmdline(line);
    }
    s.cancel();
}
