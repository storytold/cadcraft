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

/// Entities come in world coordinates with their bounding box; `extrusion` is only reported when it
/// is not +Z, so an agent can tell an object mirrored in its source file from a plain one.
#[test]
fn entities_report_bbox_and_only_unusual_extrusions() {
    // Two circles of radius 1: one plain at (0,0), one mirrored (extrusion 0,0,-1) at OCS (-10,5),
    // which is WCS (10,5).
    let dxf = "0\nSECTION\n2\nENTITIES\n\
               0\nCIRCLE\n5\n1A\n8\n0\n10\n0\n20\n0\n30\n0\n40\n1\n\
               0\nCIRCLE\n5\n1B\n8\n0\n10\n-10\n20\n5\n30\n0\n40\n1\n210\n0\n220\n0\n230\n-1\n\
               0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let mut s = Session::new();
    s.open_drawing(d, "mirrored", None);
    let r = s.execute("entities", &json!({"type": "Circle"})).unwrap();
    let es = r["entities"].as_array().unwrap();
    assert_eq!(es.len(), 2, "{r}");
    let plain = es.iter().find(|e| e["geometry"]["center"]["x"] == 0.0).unwrap();
    let mirrored = es.iter().find(|e| e["geometry"]["center"]["x"] == 10.0).unwrap();
    assert!(plain.get("extrusion").is_none(), "{plain}");
    assert_eq!(plain["bbox"], json!([[-1.0, -1.0], [1.0, 1.0]]));
    assert_eq!(mirrored["extrusion"], json!([0.0, 0.0, -1.0]), "{mirrored}");
    assert_eq!(mirrored["bbox"], json!([[9.0, 4.0], [11.0, 6.0]]));
    // drawing.inspect serializes entities the same way and names the insertion units.
    let i = s.execute("drawing.inspect", &json!({})).unwrap();
    assert!(i["entities"].as_array().unwrap().iter().any(|e| e["extrusion"] == json!([0.0, 0.0, -1.0])), "{i}");
    assert!(i["insunits"]["code"].is_i64() && i["insunits"]["name"].is_string(), "{i}");
}

/// A direction `Mat4::ocs` treats as the default plane is not reported: a zero extrusion (as a
/// damaged file may hold) or a longer +Z.
#[test]
fn zero_or_scaled_plus_z_extrusion_is_not_reported() {
    let dxf = "0\nSECTION\n2\nENTITIES\n\
               0\nCIRCLE\n5\n1A\n8\n0\n10\n0\n20\n0\n30\n0\n40\n1\n210\n0\n220\n0\n230\n0\n\
               0\nCIRCLE\n5\n1B\n8\n0\n10\n5\n20\n0\n30\n0\n40\n1\n210\n0\n220\n0\n230\n2\n\
               0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let mut s = Session::new();
    s.open_drawing(d, "zero", None);
    let r = s.execute("entities", &json!({})).unwrap();
    let es = r["entities"].as_array().unwrap();
    assert_eq!(es.len(), 2, "{r}");
    assert!(es.iter().all(|e| e.get("extrusion").is_none()), "{r}");
}

/// A block reference's `bbox` is where its contents are drawn: here a line from (0,0) to (10,0) in
/// the block, inserted at (100,50), spans (100,50)..(110,50).
#[test]
fn block_reference_bbox_follows_its_contents() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let line = s.execute("entities", &json!({"type": "Line"})).unwrap()["entities"][0]["handle"].clone();
    s.execute("block", &json!({"name": "L", "base": [0, 0], "handles": [line], "keep": "delete"})).unwrap();
    s.execute("insert", &json!({"name": "L", "at": [100, 50]})).unwrap();
    let r = s.execute("entities", &json!({"type": "Insert"})).unwrap();
    assert_eq!(r["count"], 1, "{r}");
    assert_eq!(r["entities"][0]["bbox"], json!([[100.0, 50.0], [110.0, 50.0]]), "{r}");
}
