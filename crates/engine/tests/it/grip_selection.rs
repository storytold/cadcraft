//! Grip editing acts on the whole selection, as in AutoCAD: a stretched grip drags the
//! coincident grips of the other selected objects with it, and Move/Rotate/Scale/Mirror
//! transform every selected object.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::grips::GripMode;
use serde_json::json;

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
}

fn ends(s: &Session, h: Handle) -> (Vec2, Vec2) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        k => panic!("not a line: {k:?}"),
    }
}

fn near(p: Vec2, x: f64, y: f64) -> bool {
    p.near(Vec2::new(x, y), 1e-9)
}

#[test]
fn grip_edits_act_on_the_selection() {
    // Two connected lines and a third one elsewhere, all selected.
    let mut s = Session::new();
    let a = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut s, [10.0, 0.0], [10.0, 10.0]);
    let c = line(&mut s, [20.0, 0.0], [30.0, 0.0]);
    s.set_selection(vec![a, b, c]);
    let undo = s.state().unwrap().undo.len();

    // Stretching the shared corner grip of `a` moves `b`'s start with it; `c` stays.
    s.grip_edit(a, 2, Vec2::new(12.0, 3.0), GripMode::Stretch).unwrap();
    assert!(near(ends(&s, a).1, 12.0, 3.0));
    assert!(near(ends(&s, b).0, 12.0, 3.0) && near(ends(&s, b).1, 10.0, 10.0), "{:?}", ends(&s, b));
    assert!(near(ends(&s, c).0, 20.0, 0.0) && near(ends(&s, c).1, 30.0, 0.0));
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step");

    // Move about a grip of `a`: every selected object moves.
    s.grip_edit(a, 0, Vec2::new(1.0, 1.0), GripMode::Move).unwrap();
    assert!(near(ends(&s, a).0, 1.0, 1.0));
    assert!(near(ends(&s, b).1, 11.0, 11.0));
    assert!(near(ends(&s, c).0, 21.0, 1.0));

    // Rotate 90° about the same grip: `c` turns too.
    s.grip_edit(a, 0, Vec2::new(1.0, 5.0), GripMode::Rotate).unwrap();
    assert!(near(ends(&s, c).0, 1.0, 21.0), "{:?}", ends(&s, c));

    // An object that isn't selected is only edited through its own grip.
    s.set_selection(vec![a]);
    let before = ends(&s, b);
    let g = s.grips_of(a).unwrap()[2];
    s.grip_edit(a, 2, g + Vec2::new(1.0, 0.0), GripMode::Stretch).unwrap();
    assert_eq!(ends(&s, b), before);
}
