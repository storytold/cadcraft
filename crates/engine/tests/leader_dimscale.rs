//! LEADER annotation uses the dimension style's overall scale (#1056).
use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

#[test]
fn leader_annotation_height_follows_dimscale() {
    for (scale, expected) in [(25, 6.25), (1, 0.25), (0, 0.25)] {
        let mut s = Session::new();
        s.execute("dimstyle", &json!({"name": "S25", "current": true, "DIMSCALE": scale, "DIMTXT": 0.25})).unwrap();
        s.execute("leader", &json!({"points": [[0, 0], [20, 20], [30, 20]], "text": "NOTE"})).unwrap();
        let EntityKind::MText(t) = &s.doc().unwrap().model.last().unwrap().kind else { panic!("expected annotation") };
        assert_eq!(t.height, expected, "DIMSCALE {scale}");
    }
}
