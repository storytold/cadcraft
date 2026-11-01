//! MIRROR refuses a mirror line whose points are closer than the geometry tolerance.

use cadcraft_engine::Session;
use serde_json::json;

#[test]
fn nearly_coincident_mirror_points_are_refused() {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    let before = s.doc().unwrap().model.iter().count();
    for erase in [false, true] {
        let r = s.execute("mirror", &json!({ "handles": ["100"], "p1": [0, 0], "p2": [5e-10, 0], "erase": erase }));
        let err = r.expect_err("mirror line is degenerate").to_string();
        assert!(err.contains("mirror line points coincide"), "{err}");
    }
    assert_eq!(s.doc().unwrap().model.iter().count(), before);
}
