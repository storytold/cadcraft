//! Nearest (NEA) object snap picks the closest point of all objects in the aperture, whatever
//! order they were drawn in.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Space;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::{self, mode};
use serde_json::json;

fn nearest(s: &Session, cursor: Vec2) -> Vec2 {
    let hit = snap::osnap(s.doc().unwrap(), &Space::Model, cursor, 1.0, mode::NEA, None, false).expect("a nearest snap");
    assert_eq!(hit.mode, mode::NEA);
    hit.point
}

#[test]
fn nearest_prefers_the_closer_object_in_any_drawing_order() {
    // A line along y = 0 and a circle whose edge passes y = 0.8 above the cursor at (0, 0.1):
    // the line (0.1 away) is closer than the circle (0.7 away).
    for circle_first in [false, true] {
        let mut s = Session::new();
        let line = || json!({ "points": [[-5, 0], [5, 0]] });
        let circle = || json!({ "center": [0, 3.8], "radius": 3 });
        if circle_first {
            s.execute("circle", &circle()).unwrap();
            s.execute("line", &line()).unwrap();
        } else {
            s.execute("line", &line()).unwrap();
            s.execute("circle", &circle()).unwrap();
        }
        let p = nearest(&s, Vec2::new(0.0, 0.1));
        assert!(p.near(Vec2::new(0.0, 0.0), 1e-6), "circle drawn first: {circle_first}: snapped to {p:?}");
    }
    // And the circle wins where it is the closer one.
    let mut s = Session::new();
    s.execute("circle", &json!({ "center": [0, 3.8], "radius": 3 })).unwrap();
    s.execute("line", &json!({ "points": [[-5, 0], [5, 0]] })).unwrap();
    let p = nearest(&s, Vec2::new(0.0, 0.7));
    assert!(p.near(Vec2::new(0.0, 0.8), 1e-6), "snapped to {p:?}");
}
