//! ROTATE Reference: the reference angle (typed, or two points), then the new angle (typed, a
//! point measured from the base point, or two points after `P`).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;

fn session_with_line(l: &str) -> Session {
    let mut s = Session::new();
    s.cmdline(&format!("LINE {l}")).unwrap();
    s.cmdline("").unwrap();
    s
}

/// The end point of the only line.
fn end(s: &Session) -> (f64, f64) {
    let e = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Line(l) = &e.kind { Some((l.b.x, l.b.y)) } else { None });
    e.unwrap()
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

#[test]
fn rotate_reference_typed_angles() {
    // Reference 30, new angle 120: rotates by 90 degrees.
    let mut s = session_with_line("0,0 10,0");
    s.script("ROTATE ALL  0,0 R 30 120\nCIRCLE 0,0 1\n").unwrap();
    assert!(near(end(&s), (0.0, 10.0)), "{:?} {:?}", end(&s), s.log);
    assert_eq!(s.doc().unwrap().model.len(), 2, "the tokens after ROTATE are not run as commands");
}

#[test]
fn rotate_reference_by_points() {
    // Reference from the line itself (0 degrees), new angle towards 0,10: 90 degrees.
    let mut s = session_with_line("0,0 10,0");
    s.script("ROTATE ALL  0,0 R 0,0 10,0 0,10\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(near(end(&s), (0.0, 10.0)), "{:?}", end(&s));
    // New angle by two points (`P`): from 5,5 to 5,-5 is -90 degrees.
    let mut s = session_with_line("0,0 10,0");
    s.script("ROTATE ALL  0,0 R 0 P 5,5 5,-5\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(near(end(&s), (0.0, -10.0)), "{:?}", end(&s));
}
