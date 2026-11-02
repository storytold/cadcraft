//! A `handle`/`handles` param that can't be read is an error. It never falls back to the current
//! selection, and bad entries are never dropped without a word (#330).

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn circles(s: &mut Session) -> Vec<Value> {
    s.execute("entities", &json!({ "type": "Circle" })).unwrap()["entities"].as_array().cloned().unwrap_or_default()
}

#[test]
fn malformed_handles_are_rejected_instead_of_using_the_selection() {
    let mut s = Session::new();
    let a = s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("circle", &json!({ "center": [10, 0], "radius": 1 })).unwrap();
    s.execute("qselect", &json!({ "type": "Circle" })).unwrap();
    assert_eq!(s.selection().len(), 2);

    for (cmd, params) in [
        ("erase", json!({ "handle": format!("#{a}") })),
        ("erase", json!({ "handle": "" })),
        ("erase", json!({ "handle": -1 })),
        ("erase", json!({ "handles": a })),
        ("erase", json!({ "handles": [a, "#102"] })),
        ("move", json!({ "handles": a, "delta": [5, 5] })),
        ("move", json!({ "handle": "0x101", "delta": [5, 5] })),
    ] {
        let e = s.execute(cmd, &params).expect_err(&format!("{cmd} {params}"));
        assert!(e.to_string().contains("handle"), "{cmd} {params}: {e}");
        let c = circles(&mut s);
        assert_eq!(c.len(), 2, "{cmd} {params} must change nothing");
        assert!(c.iter().all(|e| e["geometry"]["center"]["y"] == json!(0.0)), "{cmd} {params} must move nothing");
    }

    // Well-formed handles act on exactly those objects; without either key the selection is used.
    assert_eq!(s.execute("move", &json!({ "handle": a, "delta": [0, 5] })).unwrap()["moved"], 1);
    assert_eq!(s.execute("erase", &json!({ "handles": [a] })).unwrap()["erased"], 1);
    s.execute("qselect", &json!({ "type": "Circle" })).unwrap();
    assert_eq!(s.execute("erase", &json!({ "handles": null })).unwrap()["erased"], 1);
    assert!(circles(&mut s).is_empty());
}
