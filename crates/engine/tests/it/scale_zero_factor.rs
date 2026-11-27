//! SCALE: a factor point on the base point is rejected and the prompt stays (#3475).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;

fn length(s: &Session) -> f64 {
    s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.a.xy().dist(l.b.xy())) } else { None }).unwrap()
}

#[test]
fn scale_factor_point_on_base_point_keeps_the_prompt() {
    let mut s = Session::new();
    s.cmdline("LINE 0,0 10,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("SCALE").unwrap();
    s.cmdline("ALL").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("0,0").unwrap();
    let before = length(&s);
    let _ = s.cmdline("0,0");
    assert!(s.running.is_some(), "the command ended: {:?}", s.log);
    assert_eq!(length(&s), before);
    assert!(!s.selection().is_empty(), "the selection was cleared");
    s.cmdline("0,3").unwrap();
    assert!(length(&s) > before, "line not scaled: {}", length(&s));
}
