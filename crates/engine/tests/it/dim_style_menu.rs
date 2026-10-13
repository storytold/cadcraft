//! The dimension shortcut menu's style commands: Remove Style Overrides clears the selected
//! dimensions' overrides, and Save as New Style makes a style of a dimension's settings (its
//! overrides included) and gives it to the dimension.

use cadcraft_engine::doc::{Dimension, EntityKind, Handle};
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn dimension(s: &mut Session, y: f64) -> Handle {
    s.execute("dimlinear", &json!({ "p1": [0, y], "p2": [10, y], "at": [5, y + 3.0] })).unwrap();
    s.doc().unwrap().model.last().unwrap().handle
}

fn dim(s: &Session, h: Handle) -> Dimension {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        k => panic!("not a dimension: {k:?}"),
    }
}

#[test]
fn remove_style_overrides_clears_the_selected_dimensions() {
    let mut s = Session::new();
    let (a, b) = (dimension(&mut s, 0.0), dimension(&mut s, 10.0));
    s.execute("properties.set", &json!({ "handles": [a.hex(), b.hex()], "textHeight": 0.3 })).unwrap();
    s.execute("dimoverride.clear", &json!({ "handles": [a.hex()] })).unwrap();
    assert!(dim(&s, a).overrides.is_empty() && !dim(&s, b).overrides.is_empty());
    // From the menu: the selected dimensions, in one undo step.
    s.set_selection(vec![b]);
    s.start("dimoverride.clear").unwrap();
    assert!(s.running.is_none() && dim(&s, b).overrides.is_empty());
    s.undo().unwrap();
    assert_eq!(dim(&s, b).overrides.get("textHeight"), Some(&json!(0.3)));
}

#[test]
fn save_as_new_style_takes_the_dimensions_settings() {
    let mut s = Session::new();
    let h = dimension(&mut s, 0.0);
    s.execute("properties.set", &json!({ "handles": [h.hex()], "textHeight": 0.3, "DIMDEC": 2 })).unwrap();
    let r = s.execute("dimstyle.saveas", &json!({ "handle": h.hex(), "name": "Detail" })).unwrap();
    assert_eq!(r["style"], "Detail");
    let d = dim(&s, h);
    assert!(d.style == "Detail" && d.overrides.is_empty(), "{d:?}");
    let st = s.execute("dimstyle.list", &json!({ "name": "Detail" })).unwrap();
    assert_eq!((st["variables"]["DIMTXT"].as_f64(), st["variables"]["DIMDEC"].as_u64()), (Some(0.3), Some(2)));
    // The dimension looks the same, and the current style is still Standard.
    let g = cadcraft_engine::render::dimension_in(s.doc().unwrap(), &d);
    assert!((g.text_height - 0.3).abs() < 1e-12 && g.value == "10.00", "{} {}", g.text_height, g.value);
    assert_eq!(s.doc().unwrap().header.str("DIMSTYLE", "Standard"), "Standard");
    // A new name that a style can have.
    for name in ["detail", "  ", "a<b"] {
        assert!(s.execute("dimstyle.saveas", &json!({ "handle": h.hex(), "name": name })).is_err(), "{name}");
    }
    // Only dimensions.
    s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
    let c = s.doc().unwrap().model.last().unwrap().handle;
    assert!(s.execute("dimstyle.saveas", &json!({ "handle": c.hex(), "name": "Other" })).is_err());
}

#[test]
fn save_as_new_style_from_the_menu_asks_for_a_name() {
    let mut s = Session::new();
    let h = dimension(&mut s, 0.0);
    s.execute("properties.set", &json!({ "handles": [h.hex()], "textHeight": 0.3 })).unwrap();
    s.set_selection(vec![h]);
    s.start("dimstyle.saveas").unwrap();
    assert!(s.prompt_text().contains("name of new dimension style"), "{}", s.prompt_text());
    // A name already taken is refused, said why, and asked again.
    let _ = s.input(Input::Text("Standard".into()));
    assert!(s.running.is_some(), "a taken name asks again");
    assert!(s.log.iter().any(|l| l.contains("already exists")), "{:?}", s.log);
    s.input(Input::Text("Detail".into())).unwrap();
    assert!(s.running.is_none());
    assert_eq!(dim(&s, h).style, "Detail");
}

#[test]
fn save_as_new_style_picks_a_dimension_when_none_is_selected() {
    let mut s = Session::new();
    let h = dimension(&mut s, 0.0);
    s.start("dimstyle.saveas").unwrap();
    assert!(s.prompt_text().contains("Select a dimension"), "{}", s.prompt_text());
    // The dimension line of the dimension.
    s.input(Input::Point(cadcraft_engine::geom::Vec2::new(5.0, 3.0))).unwrap();
    s.input(Input::Text("Picked".into())).unwrap();
    assert_eq!(dim(&s, h).style, "Picked");
}
