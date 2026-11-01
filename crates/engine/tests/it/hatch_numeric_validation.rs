//! HATCHEDIT refuses an explicit bad `scale` or `angle` before changing anything: no partial
//! edit, no undo step, redo history kept.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::{Value, json};

fn fixture() -> (Session, String) {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 10]})).unwrap();
    let handle = s.execute("hatch", &json!({"points": [[5, 5]]})).unwrap()["handle"].as_str().unwrap().to_owned();
    // One redo entry, which a rejected edit must leave alone.
    s.execute("circle", &json!({"center": [20, 20], "radius": 1})).unwrap();
    s.undo().unwrap();
    assert_eq!(s.state().unwrap().redo.len(), 1);
    (s, handle)
}

fn hatch_props(s: &Session) -> (f64, f64) {
    let hatches: Vec<(f64, f64)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some((h.scale, h.angle.to_degrees())) } else { None })
        .collect();
    assert_eq!(hatches.len(), 1);
    hatches[0]
}

fn assert_props(s: &Session, scale: f64, angle: f64) {
    let (sc, an) = hatch_props(s);
    assert!((sc - scale).abs() < 1e-9 && (an - angle).abs() < 1e-9, "scale {sc}, angle {an}");
}

fn edit(s: &mut Session, handle: &str, extra: Value) -> cadcraft_engine::Result<Value> {
    let mut params = json!({ "handles": [handle] });
    for (k, v) in extra.as_object().unwrap() {
        params[k] = v.clone();
    }
    s.execute("hatchedit", &params)
}

#[test]
fn bad_scale_or_angle_is_refused_and_changes_nothing() {
    let cases = [
        (json!({"scale": 0}), "scale"),
        (json!({"scale": -1}), "scale"),
        (json!({"scale": "x"}), "scale"),
        (json!({"angle": "x"}), "angle"),
        (json!({"scale": 0, "angle": 45}), "scale"),
        (json!({"scale": 2, "angle": "x"}), "angle"),
        (json!({"scale": 0, "recreate": "polyline"}), "scale"),
        (json!({"angle": "x", "recreate": "polyline"}), "angle"),
    ];
    for (extra, key) in cases {
        let (mut s, handle) = fixture();
        let before = s.doc().unwrap().clone();
        let (undo, redo, revision) = {
            let st = s.state().unwrap();
            (st.undo.len(), st.redo.len(), st.revision)
        };
        let err = edit(&mut s, &handle, extra.clone()).expect_err(&format!("{extra} should be refused"));
        assert!(err.to_string().contains(&format!("`{key}`")), "{extra}: {err}");
        assert_eq!(s.doc().unwrap(), &before, "{extra}: the drawing is untouched");
        let st = s.state().unwrap();
        assert_eq!((st.undo.len(), st.redo.len(), st.revision), (undo, redo, revision), "{extra}: history is untouched");
    }
}

#[test]
fn valid_omitted_and_null_values_still_edit() {
    let (mut s, handle) = fixture();
    edit(&mut s, &handle, json!({"scale": 2, "angle": 45})).unwrap();
    assert_props(&s, 2.0, 45.0);
    // Omitted or null numeric fields keep what the hatch has while another property changes.
    edit(&mut s, &handle, json!({"style": "outer"})).unwrap();
    assert_props(&s, 2.0, 45.0);
    edit(&mut s, &handle, json!({"scale": null, "angle": null, "style": "normal"})).unwrap();
    assert_props(&s, 2.0, 45.0);
    // Zero and negative angles are angles.
    edit(&mut s, &handle, json!({"angle": 0})).unwrap();
    assert_props(&s, 2.0, 0.0);
    edit(&mut s, &handle, json!({"angle": -30})).unwrap();
    assert_props(&s, 2.0, -30.0);
}
