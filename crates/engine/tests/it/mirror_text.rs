//! MIRROR keeps an MTEXT reading forward, as it keeps a TEXT (#1169): the mirrored copy keeps the
//! source's rotation instead of turning upside down. MTEXT has no backwards or upside-down flags,
//! so MIRRTEXT 1 has nothing to mirror it with either.

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use serde_json::{Value, json};

/// The rotation in degrees of the TEXT or MTEXT with handle `h` (as a hex string).
fn rotation(s: &Session, h: &str) -> f64 {
    let d = s.doc().unwrap();
    let e = d.model.iter().find(|e| e.handle.hex() == h).unwrap();
    match &e.kind {
        EntityKind::Text(t) => t.rotation.to_degrees(),
        EntityKind::MText(t) => t.rotation.to_degrees(),
        k => panic!("not a text: {k:?}"),
    }
}

fn handle(v: &Value) -> String {
    v["handle"].as_str().unwrap().to_string()
}

/// The issue's case: a TEXT and an MTEXT mirrored together about a vertical line, with MIRRTEXT 0
/// and 1. Before the fix the MTEXT copy came out at 180 degrees, upside down.
#[test]
fn a_mirrored_mtext_reads_forward_like_a_text() {
    for mirrtext in [0, 1] {
        let mut s = Session::new();
        s.execute("setvar", &json!({ "name": "MIRRTEXT", "value": mirrtext })).unwrap();
        let text = handle(&s.execute("text", &json!({ "at": [10, 10], "text": "TEXT", "height": 5 })).unwrap());
        let mtext = handle(&s.execute("mtext", &json!({ "at": [10, 30], "text": "MTEXT", "height": 5, "width": 50 })).unwrap());
        let r = s.execute("mirror", &json!({ "handles": [text, mtext], "p1": [0, 0], "p2": [0, 100] })).unwrap();
        let copies: Vec<String> = r["handles"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
        assert_eq!(copies.len(), 2);
        for copy in &copies {
            let angle = rotation(&s, copy);
            assert!(angle.abs() < 1e-9, "MIRRTEXT {mirrtext}: copy {copy} at {angle} degrees");
        }
    }
}

/// A turned MTEXT keeps its own angle when mirrored, about a horizontal line too, while a rotation
/// still turns it.
#[test]
fn only_mirroring_leaves_an_mtext_angle_alone() {
    let mut s = Session::new();
    let mtext = handle(&s.execute("mtext", &json!({ "at": [10, 30], "text": "MTEXT", "height": 5, "width": 50, "rotation": 30 })).unwrap());
    assert!((rotation(&s, &mtext) - 30.0).abs() < 1e-9);
    for (p1, p2) in [([0, 0], [0, 100]), ([0, 0], [100, 0])] {
        let r = s.execute("mirror", &json!({ "handles": [mtext], "p1": p1, "p2": p2 })).unwrap();
        let copy = r["handles"][0].as_str().unwrap().to_string();
        assert!((rotation(&s, &copy) - 30.0).abs() < 1e-9, "mirrored about {p1:?}-{p2:?}: {}", rotation(&s, &copy));
    }
    s.execute("rotate", &json!({ "handles": [mtext], "base": [0, 0], "angle": 90 })).unwrap();
    assert!((rotation(&s, &mtext) - 120.0).abs() < 1e-9, "{}", rotation(&s, &mtext));
}
