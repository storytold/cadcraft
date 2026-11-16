//! A hidden object that is back in the selection takes no part in a grip edit: Undo, `sssetfirst`
//! or a control call can restore it there, and the grips it shares with a visible object must not
//! drag it along.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

/// Draws a line from `a` to `b` and returns its handle.
fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
}

/// The two ends of the line `h`.
fn ends(s: &Session, h: Handle) -> (Vec2, Vec2) {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
        k => panic!("not a line: {k:?}"),
    }
}

/// Two lines sharing the end (0, 0), the second one hidden, both in the selection.
fn shared_corner() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    let shown = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let hidden = line(&mut s, [0.0, 0.0], [0.0, 10.0]);
    s.execute("hideobjects", &json!({ "handles": [hidden.hex()] })).unwrap();
    let d = s.doc().unwrap();
    assert!(!d.is_visible(d.entity(hidden).unwrap()), "the second line must be hidden");
    s.set_selection(vec![shown, hidden]);
    (s, shown, hidden)
}

/// The index of the grip of `h` at `p`.
fn grip_at(s: &Session, h: Handle, p: Vec2) -> usize {
    s.grips_of(h).unwrap().iter().position(|g| g.near(p, 1e-9)).unwrap()
}

/// Stretching the shared corner moves the visible line only; the hidden one stays where it was.
#[test]
fn a_hidden_selected_line_is_not_stretched_with_a_shared_grip() {
    let (mut s, shown, hidden) = shared_corner();
    let i = grip_at(&s, shown, Vec2::new(0.0, 0.0));
    s.start_grip_edit(&[(shown, i)]).unwrap();
    s.cmdline("3,4").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (start, end) = ends(&s, shown);
    assert!(start.near(Vec2::new(3.0, 4.0), 1e-9) && end.near(Vec2::new(10.0, 0.0), 1e-9), "{start:?} {end:?}");
    let (start, end) = ends(&s, hidden);
    assert!(start.near(Vec2::new(0.0, 0.0), 1e-9) && end.near(Vec2::new(0.0, 10.0), 1e-9), "the hidden line moved: {start:?} {end:?}");
}

/// The Move mode of a grip edit moves the visible selection only.
#[test]
fn a_hidden_selected_line_is_not_moved_by_the_move_mode_of_a_grip_edit() {
    let (mut s, shown, hidden) = shared_corner();
    let i = grip_at(&s, shown, Vec2::new(10.0, 0.0));
    s.start_grip_edit(&[(shown, i)]).unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("** MOVE **"), "{}", s.prompt_text());
    s.cmdline("12,5").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (start, end) = ends(&s, shown);
    assert!(start.near(Vec2::new(2.0, 5.0), 1e-9) && end.near(Vec2::new(12.0, 5.0), 1e-9), "{start:?} {end:?}");
    let (start, end) = ends(&s, hidden);
    assert!(start.near(Vec2::new(0.0, 0.0), 1e-9) && end.near(Vec2::new(0.0, 10.0), 1e-9), "the hidden line moved: {start:?} {end:?}");
}

/// The grip of a hidden object is never made hot, so a grip edit cannot start on it.
#[test]
fn the_grip_of_a_hidden_object_cannot_be_made_hot() {
    let (mut s, _, hidden) = shared_corner();
    let before = s.state().unwrap().doc.clone();
    let err = s.start_grip_edit(&[(hidden, 0)]).unwrap_err().to_string();
    assert!(err.contains("no grip"), "{err}");
    assert!(s.running.is_none());
    assert!(std::sync::Arc::ptr_eq(&before, &s.state().unwrap().doc), "nothing may change");
}

/// A hidden object's grip among the hot grips is left out, wherever it stands in the list: the
/// first grip the drawing shows is the base grip, and only the visible line is stretched.
#[test]
fn a_hidden_grip_among_the_hot_grips_is_left_out() {
    for hidden_first in [false, true] {
        let (mut s, shown, hidden) = shared_corner();
        let mut hot = vec![(shown, grip_at(&s, shown, Vec2::new(0.0, 0.0))), (hidden, grip_at(&s, hidden, Vec2::new(0.0, 10.0)))];
        if hidden_first {
            hot.reverse();
        }
        s.start_grip_edit(&hot).unwrap();
        s.cmdline("3,4").unwrap();
        assert!(s.running.is_none(), "{}", s.prompt_text());
        let (start, end) = ends(&s, shown);
        assert!(start.near(Vec2::new(3.0, 4.0), 1e-9) && end.near(Vec2::new(10.0, 0.0), 1e-9), "hidden first: {hidden_first}: {start:?} {end:?}");
        let (start, end) = ends(&s, hidden);
        assert!(
            start.near(Vec2::new(0.0, 0.0), 1e-9) && end.near(Vec2::new(0.0, 10.0), 1e-9),
            "hidden first: {hidden_first}: the hidden line moved: {start:?} {end:?}"
        );
    }
}

/// Only what the drawing does not show is left out: a grip of an object that does not exist is
/// still an error, also after a good one.
#[test]
fn a_missing_object_among_the_hot_grips_is_still_an_error() {
    let (mut s, shown, _) = shared_corner();
    let i = grip_at(&s, shown, Vec2::new(0.0, 0.0));
    let err = s.start_grip_edit(&[(shown, i), (Handle(0xFFFF), 0)]).unwrap_err().to_string();
    assert!(err.contains("no such object"), "{err}");
    assert!(s.running.is_none());
}

/// A selected line on a layer that is off is not shown either, and is not stretched with a
/// shared grip.
#[test]
fn a_selected_line_on_a_layer_that_is_off_is_not_stretched_with_a_shared_grip() {
    let mut s = Session::new();
    let shown = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("layer.new", &json!({ "name": "Guides", "current": true })).unwrap();
    let off = line(&mut s, [0.0, 0.0], [0.0, 10.0]);
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    s.execute("layer.set", &json!({ "name": "Guides", "on": false })).unwrap();
    let d = s.doc().unwrap();
    assert!(!d.is_visible(d.entity(off).unwrap()), "the second line must be on a layer that is off");
    s.set_selection(vec![shown, off]);
    let i = grip_at(&s, shown, Vec2::new(0.0, 0.0));
    s.start_grip_edit(&[(shown, i)]).unwrap();
    s.cmdline("3,4").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (start, end) = ends(&s, shown);
    assert!(start.near(Vec2::new(3.0, 4.0), 1e-9) && end.near(Vec2::new(10.0, 0.0), 1e-9), "{start:?} {end:?}");
    let (start, end) = ends(&s, off);
    assert!(
        start.near(Vec2::new(0.0, 0.0), 1e-9) && end.near(Vec2::new(0.0, 10.0), 1e-9),
        "the line on the layer that is off moved: {start:?} {end:?}"
    );
}

/// The typed GRIP command finds no grip where only a hidden object has one, and at the shared
/// corner it makes only the visible line's grip hot.
#[test]
fn the_typed_grip_command_finds_no_grip_of_a_hidden_object() {
    let (mut s, shown, hidden) = shared_corner();
    s.cmdline("grip").unwrap();
    assert!(s.prompt_text().contains("Select grip"), "{}", s.prompt_text());
    // Only the hidden line has a grip at (0, 10): the command says so and asks again.
    s.cmdline("0,10").unwrap();
    assert!(s.log.iter().any(|l| l.contains("No grip of a selected object there")), "{:?}", s.log);
    s.cmdline("0,0").unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("** STRETCH **"), "{}", s.prompt_text());
    s.cmdline("3,4").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (start, end) = ends(&s, shown);
    assert!(start.near(Vec2::new(3.0, 4.0), 1e-9) && end.near(Vec2::new(10.0, 0.0), 1e-9), "{start:?} {end:?}");
    let (start, end) = ends(&s, hidden);
    assert!(start.near(Vec2::new(0.0, 0.0), 1e-9) && end.near(Vec2::new(0.0, 10.0), 1e-9), "the hidden line moved: {start:?} {end:?}");
}
