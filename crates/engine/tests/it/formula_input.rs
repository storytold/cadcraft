//! A formula typed where a distance or number is asked (`=1200+350*2`) is worked out first.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;

#[test]
fn a_formula_answers_a_distance_prompt() {
    let mut s = Session::new();
    s.cmdline("circle 0,0").unwrap();
    s.cmdline("=(10+2)*3/4").unwrap();
    let r = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Circle(c) = &e.kind { Some(c.radius) } else { None }).unwrap();
    assert!((r - 9.0).abs() < 1e-12, "{r}");
}

#[test]
fn a_bad_formula_is_reported_and_asks_again() {
    let mut s = Session::new();
    s.cmdline("circle 0,0").unwrap();
    s.cmdline("=10+").unwrap();
    assert!(s.running.is_some(), "the prompt is still waiting");
    s.cmdline("=5").unwrap();
    assert!(s.running.is_none());
}
