//! The `entities` query (control channel `engine.execute`, MCP `query_entities`) rejects malformed
//! filters instead of ignoring them and returning everything.

use cadcraft_engine::Session;
use serde_json::json;

#[test]
fn malformed_filters_are_errors() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.execute("circle", &json!({"center": [100, 100], "radius": 1})).unwrap();
    for p in [
        json!({"window": [-5, -5, 5, 5]}),
        json!({"window": [[-5, -5]]}),
        json!({"window": [[-5, -5], [5, 5], [9, 9]]}),
        json!({"window": {"min": [-5, -5], "max": [5, 5]}}),
        json!({"window": "x"}),
        json!({"type": 7}),
        json!({"layer": ["0"]}),
        json!({"limit": -1}),
        json!({"offset": "1"}),
        json!({"crossing": "yes"}),
        json!({"selected": "true"}),
        json!({"selected": 1}),
        json!({"selected": []}),
        json!({"selected": {}}),
    ] {
        assert!(s.execute("entities", &p).is_err(), "{p} must be rejected");
    }
    let r = s.execute("entities", &json!({"window": [[-5, -5], [5, 5]]})).unwrap();
    assert_eq!(r["count"], 1, "{r}");
}

#[test]
fn selection_filter_combines_before_paging_without_changing_state() {
    let mut s = Session::new();
    s.execute("layer.new", &json!({"name": "측정", "current": true})).unwrap();
    for x in [0, 10, 20] {
        s.execute("circle", &json!({"center": [x, 0], "radius": 2})).unwrap();
    }
    s.execute("line", &json!({"points": [[0, 0], [3, 4]]})).unwrap();
    let all = s.execute("entities", &json!({})).unwrap();
    let hs: Vec<_> = all["entities"].as_array().unwrap().iter().map(|e| e["handle"].clone()).collect();
    // Leave a redo entry to prove the query doesn't clear redo or create an undo entry.
    s.execute("circle", &json!({"center": [99, 99], "radius": 1})).unwrap();
    s.undo().unwrap();
    s.execute("select", &json!({"handles": [hs[0], hs[2], hs[3]]})).unwrap();
    let before = s.state().unwrap().clone();
    let report = s.execute("drawing.inspect", &json!({})).unwrap();
    let selected = s.execute("entities", &json!({"selected": true})).unwrap();
    assert_eq!(selected["count"], 3);
    assert_eq!(selected["entities"][0]["handle"], hs[0]);
    assert_eq!(selected["entities"][1]["handle"], hs[2]);
    assert_eq!(s.execute("entities", &json!({"selected": false})).unwrap(), all);
    assert_eq!(s.execute("entities", &json!({"selected": null})).unwrap(), all);
    assert_eq!(s.execute("entities", &json!({})).unwrap(), all);
    let page = s.execute("entities", &json!({"selected": true, "type": "circle", "layer": "측정", "limit": 1, "offset": 1})).unwrap();
    assert_eq!(page, json!({"count": 2, "returned": 1, "offset": 1, "entities": [all["entities"][2]]}));
    let window = json!({"selected": true, "type": "circle", "layer": "측정", "window": [[-3, -3], [3, 3]], "crossing": false});
    assert_eq!(s.execute("entities", &window).unwrap()["count"], 1);
    assert_eq!(s.execute("entities", &json!({"selected": true, "layer": "other"})).unwrap()["count"], 0);
    assert_eq!(s.execute("entities", &json!({"selected": true, "limit": 0})).unwrap()["returned"], 0);
    assert_eq!(s.execute("entities", &json!({"selected": true, "offset": 99})).unwrap()["returned"], 0);
    assert!(s.execute("entities", &json!({"selected": "true"})).is_err());
    assert_eq!(s.execute("drawing.inspect", &json!({})).unwrap(), report);
    let after = s.state().unwrap();
    assert!(std::sync::Arc::ptr_eq(&before.doc, &after.doc));
    assert!(std::sync::Arc::ptr_eq(&before.saved, &after.saved));
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.selection, after.selection);
    assert_eq!(before.previous_selection, after.previous_selection);
    assert_eq!(before.is_dirty(), after.is_dirty());
    for (a, b) in [(&before.undo, &after.undo), (&before.redo, &after.redo)] {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert_eq!(a.label, b.label);
            assert_eq!(a.selection, b.selection);
            assert!(std::sync::Arc::ptr_eq(&a.doc, &b.doc));
        }
    }
}

#[test]
fn selection_is_empty_by_default_and_limited_to_active_space() {
    use cadcraft_doc::{Handle, Space};
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
    let model = s.doc().unwrap().model.iter().next().unwrap().handle;
    assert_eq!(s.execute("entities", &json!({"selected": true})).unwrap()["count"], 0);
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.execute("circle", &json!({"center": [10, 10], "radius": 2})).unwrap();
    let paper = s.doc().unwrap().space(&Space::Paper("Layout1".into())).unwrap().iter().last().unwrap().handle;
    // Even stale, duplicate and foreign-space selection handles cannot leak entities.
    s.state_mut().unwrap().selection = vec![model, paper, paper, Handle(u64::MAX)];
    let page = s.execute("entities", &json!({"selected": true})).unwrap();
    assert_eq!(page["count"], 1);
    assert_eq!(page["entities"][0]["handle"], paper.hex());
    s.state_mut().unwrap().space = Space::Model;
    let page = s.execute("entities", &json!({"selected": true})).unwrap();
    assert_eq!(page["count"], 1);
    assert_eq!(page["entities"][0]["handle"], model.hex());
    s.state_mut().unwrap().selection.clear();
    assert_eq!(s.execute("entities", &json!({"selected": true})).unwrap()["entities"], json!([]));
}
