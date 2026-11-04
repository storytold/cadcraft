//! Dynamic Input (#519): DYNPROMPT and the options a prompt offers at the cursor.

use cadcraft_engine::{Input, Session, sysvars};
use serde_json::json;

#[test]
fn dynprompt_is_a_setting_on_by_default() {
    let mut s = Session::new();
    assert!(s.settings.dynprompt);
    assert_eq!(sysvars::get(&s, "DYNPROMPT"), Some(json!(1)));
    s.execute("setvar", &json!({"name": "dynprompt", "value": 0})).unwrap();
    assert!(!s.settings.dynprompt);
    assert_eq!(sysvars::get(&s, "dynprompt"), Some(json!(0)));
    s.cmdline("setvar dynprompt 1").unwrap();
    assert!(s.settings.dynprompt);
    assert!(s.execute("setvar", &json!({"name": "DYNPROMPT", "value": "x"})).is_err());
    assert!(s.settings.dynprompt);
}

#[test]
fn an_option_picked_at_the_cursor_answers_the_prompt() {
    // The options menu lists the prompt's keywords and sends the picked one, as a click on it
    // at the command line does.
    let mut s = Session::new();
    s.cmdline("circle").unwrap();
    let kws = s.current_prompt().unwrap().keywords;
    let three_p = kws.iter().find(|k| k.eq_ignore_ascii_case("3P")).cloned().expect("CIRCLE offers 3P");
    s.input(Input::Keyword(three_p)).unwrap();
    for p in [[0.0, 0.0], [10.0, 0.0], [5.0, 5.0]] {
        s.input(Input::Point(cadcraft_geom::Vec2::new(p[0], p[1]))).unwrap();
    }
    assert!(s.running.is_none());
    let doc = s.doc().unwrap();
    let circle = doc.model.iter().find_map(|e| match &e.kind {
        cadcraft_doc::EntityKind::Circle(c) => Some(c.clone()),
        _ => None,
    });
    let c = circle.expect("a three-point circle");
    assert!((c.center.x - 5.0).abs() < 1e-9 && c.center.y.abs() < 1e-9 && (c.radius - 5.0).abs() < 1e-9, "{c:?}");
}
