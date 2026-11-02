//! Snap/grid spacing belongs to each drawing, including command edits and undo (#607).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Drawing, HVal};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::tracking::CursorQuery;
use serde_json::{Value, json};

fn get(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({"name": name})).unwrap()["value"].clone()
}

#[test]
fn spacing_stays_with_each_drawing_and_new_drawings_use_defaults() {
    let mut s = Session::new();
    let first = s.active;
    s.execute("setvar", &json!({"name": "SNAPUNIT", "value": [0.25, 0.125]})).unwrap();
    s.execute("setvar", &json!({"name": "GRIDUNIT", "value": [0.75, 1.5]})).unwrap();
    s.execute("setvar", &json!({"name": "OSMODE", "value": 0})).unwrap();
    assert!(s.state().unwrap().is_dirty(), "spacing changes must be saved");
    let second = s.new_drawing(true);
    assert_eq!(get(&mut s, "SNAPUNIT"), json!([0.5, 0.5]));
    assert_eq!(get(&mut s, "GRIDUNIT"), json!([0.5, 0.5]));
    assert_eq!(get(&mut s, "OSMODE"), json!(0), "profile settings stay in the session");
    s.execute("document.switch", &json!({"index": first})).unwrap();
    assert_eq!(get(&mut s, "SNAPUNIT"), json!([0.25, 0.125]));
    assert_eq!(get(&mut s, "GRIDUNIT"), json!([0.75, 1.5]));
    s.execute("close", &json!({"index": first})).unwrap();
    assert_eq!(s.active, second - 1);
    assert_eq!(get(&mut s, "SNAPUNIT"), json!([0.5, 0.5]));
}

#[test]
fn loaded_spacing_is_used_by_getvar_settings_snapshots_and_grid_snap() {
    let mut d = Drawing::new_imperial();
    d.header.set("SNAPUNIT", HVal::Point(Vec2::new(0.25, 0.125).to3(0.0)));
    d.header.set("GRIDUNIT", HVal::Point(Vec2::new(0.75, 1.5).to3(0.0)));
    let mut s = Session::empty();
    s.open_drawing(d, "loaded", None);
    assert!(!s.state().unwrap().is_dirty());
    assert_eq!(get(&mut s, "SNAPUNIT"), json!([0.25, 0.125]));
    let listed = s.execute("sysvars", &json!({})).unwrap();
    assert_eq!(listed["SNAPUNIT"], json!([0.25, 0.125]));
    assert_eq!(listed["GRIDUNIT"], json!([0.75, 1.5]));
    assert_eq!(s.summary()["settings"]["snapunit"], serde_json::to_value(Vec2::new(0.25, 0.125)).unwrap());
    s.settings.snapmode = true;
    s.settings.osmode = 0;
    s.settings.polarmode = false;
    let q = CursorQuery { base: None, deferred: false, corner: false, aperture: 0.1, tol: 0.1, now: 0.0, shift: false };
    assert_eq!(s.snap_cursor(Vec2::new(0.27, 0.15), &q).point, Vec2::new(0.25, 0.125));
}

#[test]
fn drafting_settings_edits_are_drawing_changes_with_undo_and_redo() {
    let mut s = Session::new();
    let response = s.execute("dsettings", &json!({"snapunit": [0.25, 0.125], "gridunit": [0.75, 1.5]})).unwrap();
    assert_eq!(response["snapunit"], serde_json::to_value(Vec2::new(0.25, 0.125)).unwrap());
    assert_eq!(response["gridunit"], serde_json::to_value(Vec2::new(0.75, 1.5)).unwrap());
    assert!(s.state().unwrap().is_dirty());
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(get(&mut s, "SNAPUNIT"), json!([0.5, 0.5]));
    assert_eq!(get(&mut s, "GRIDUNIT"), json!([0.5, 0.5]));
    assert!(!s.state().unwrap().is_dirty());
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(get(&mut s, "SNAPUNIT"), json!([0.25, 0.125]));
    assert_eq!(get(&mut s, "GRIDUNIT"), json!([0.75, 1.5]));
}

#[test]
fn invalid_spacing_is_refused_without_changing_the_drawing() {
    let mut s = Session::new();
    for name in ["SNAPUNIT", "GRIDUNIT"] {
        for value in [json!([-1, 0.25]), json!([0.25, -1]), json!("NaN"), json!("inf"), Value::Null] {
            assert!(s.execute("setvar", &json!({"name": name, "value": value})).is_err(), "{name}: {value}");
        }
    }
    assert!(s.execute("setvar", &json!({"name": "SNAPUNIT", "value": [0, 0.25]})).is_err());
    assert!(!s.state().unwrap().is_dirty());
    s.execute("setvar", &json!({"name": "GRIDUNIT", "value": [0, 0]})).unwrap();
    assert_eq!(get(&mut s, "GRIDUNIT"), json!([0.0, 0.0]));
}
