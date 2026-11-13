//! Objects on locked layers can be left unselectable (lockedLayersUnselectable), and are still
//! snapped to.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Handle, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::{self, mode};
use serde_json::json;

#[test]
fn locked_layers_can_be_left_unselectable_but_still_snap() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "Base", "current": true })).unwrap();
    let r = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let l = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    s.execute("layer.set", &json!({ "name": "Base", "locked": true })).unwrap();
    let pick = |s: &Session| cadcraft_engine::select::pick(s.doc().unwrap(), &Space::Model, Vec2::new(5.0, 0.0), 0.5);
    assert_eq!(pick(&s), Some(l), "picked by default, only edits leave it alone");
    std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc).locked_unselectable = true;
    assert_eq!(pick(&s), None);
    let d = s.doc().unwrap();
    let window = cadcraft_engine::geom::Bounds2::from_points([Vec2::new(-1.0, -1.0), Vec2::new(11.0, 1.0)]);
    assert!(cadcraft_engine::select::select_window(d, &Space::Model, window, false).is_empty());
    let hit = snap::osnap(d, &Space::Model, Vec2::new(10.05, 0.0), 0.5, mode::END, None, false).unwrap();
    assert!(hit.point.near(Vec2::new(10.0, 0.0), 1e-9), "still snapped to");
}

/// A line on layer 0, then one on the locked layer "Base", drawn last; objects on locked layers
/// are left unselectable. Their handles, in that order.
fn last_on_locked_layer() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    let line = |s: &mut Session, y: f64| {
        let r = s.execute("line", &json!({ "points": [[0.0, y], [10.0, y]] })).unwrap();
        Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
    };
    let a = line(&mut s, 0.0);
    s.execute("layer.new", &json!({ "name": "Base", "current": true })).unwrap();
    let b = line(&mut s, 5.0);
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    s.execute("layer.set", &json!({ "name": "Base", "locked": true })).unwrap();
    std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc).locked_unselectable = true;
    (s, a, b)
}

#[test]
fn last_never_falls_back_to_an_earlier_object() {
    let (mut s, a, b) = last_on_locked_layer();
    s.script("ERASE\nL\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.doc().unwrap().entity(a).is_some(), "the earlier object is not erased");
    assert!(s.doc().unwrap().entity(b).is_some());
    // STRETCH ends at Enter when nothing was selected, rather than asking for a base point.
    s.script("STRETCH\nL\n\n").unwrap();
    assert!(s.running.is_none(), "STRETCH selected the earlier object: {}", s.prompt_text());
}

#[test]
fn groups_bring_no_members_on_layers_locked_against_selection() {
    let (mut s, a, b) = last_on_locked_layer();
    s.execute("group", &json!({ "name": "G", "handles": [a.hex(), b.hex()] })).unwrap();
    s.idle_click(Vec2::new(5.0, 0.0), false).unwrap();
    assert_eq!(s.selection(), vec![a], "a click");
    s.set_selection(Vec::new());
    s.idle_click(Vec2::new(-1.0, -1.0), false).unwrap();
    s.idle_click(Vec2::new(11.0, 6.0), false).unwrap();
    assert_eq!(s.selection(), vec![a], "a window");
    assert_eq!(cadcraft_engine::select::select_group(s.doc().unwrap(), &Space::Model, "G"), Some(vec![a]), "by name");
}
