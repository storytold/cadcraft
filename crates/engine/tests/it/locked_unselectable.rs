//! Objects on locked layers can be left unselectable (lockedLayersUnselectable), and are still
//! snapped to.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Space;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::{self, mode};
use serde_json::json;

#[test]
fn locked_layers_can_be_left_unselectable_but_still_snap() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "Base", "current": true })).unwrap();
    let r = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let l = cadcraft_engine::doc::Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
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
