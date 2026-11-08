//! LAYNEWSEL: a new layer for the selected objects, the objects moved onto it, one undo step.

use cadcraft_engine::Session;
use serde_json::json;

fn two_lines() -> (Session, Vec<cadcraft_engine::doc::Handle>) {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 9], [10, 9]] })).unwrap();
    let hs = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    (s, hs)
}

#[test]
fn the_selection_lands_on_the_new_layer() {
    let (mut s, hs) = two_lines();
    s.set_selection(hs[..2].to_vec());
    let undo = s.state().unwrap().undo.len();
    let r = s.execute("laynewsel", &json!({ "name": "  Walls " })).unwrap();
    assert_eq!(r["changed"], 2);
    assert_eq!(r["layer"], "Walls", "the name is trimmed");
    let d = s.doc().unwrap();
    assert!(d.layer("Walls").is_some());
    let layers: Vec<String> = d.model.iter().map(|e| e.common.layer.clone()).collect();
    assert_eq!(layers, ["Walls", "Walls", "0"]);
    assert_eq!(d.header.str("CLAYER", "0"), "0", "the current layer stays");
    assert_eq!(s.state().unwrap().undo.len(), undo + 1, "one undo step");
    s.undo().unwrap();
    let d = s.doc().unwrap();
    assert!(d.layer("Walls").is_none());
    assert!(d.model.iter().all(|e| e.common.layer == "0"));
}

#[test]
fn nothing_selected_or_a_taken_name_changes_nothing() {
    let (mut s, hs) = two_lines();
    let e = s.execute("laynewsel", &json!({ "name": "Walls" })).unwrap_err().to_string();
    assert!(e.contains("select"), "{e}");
    assert!(s.doc().unwrap().layer("Walls").is_none(), "no empty layer left behind");
    s.set_selection(vec![hs[0]]);
    let undo = s.state().unwrap().undo.len();
    for bad in ["0", "", "a/b"] {
        assert!(s.execute("laynewsel", &json!({ "name": bad })).is_err(), "{bad:?}");
    }
    assert!(s.doc().unwrap().model.iter().all(|e| e.common.layer == "0"));
    assert_eq!(s.state().unwrap().undo.len(), undo, "a refusal leaves no undo step");
    // A handle that isn't in the drawing: refused before anything changes, the valid ones included.
    let e = s.execute("laynewsel", &json!({ "name": "Fresh", "handles": [hs[0].hex(), "FFFFF"] })).unwrap_err().to_string();
    assert!(e.contains("FFFFF"), "{e}");
    let d = s.doc().unwrap();
    assert!(d.layer("Fresh").is_none(), "no layer made");
    assert_eq!(d.entity(hs[0]).unwrap().common.layer, "0", "nothing moved");
    assert_eq!(s.state().unwrap().undo.len(), undo, "no undo step");
    s.execute("laynewsel", &json!({ "name": "Doors", "handles": [hs[2].hex()], "current": true })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.entity(hs[2]).unwrap().common.layer, "Doors", "named handles win over the selection");
    assert_eq!(d.entity(hs[0]).unwrap().common.layer, "0");
    assert_eq!(d.header.str("CLAYER", "0"), "Doors");
}

#[test]
fn delete_empty_layers_keeps_the_used_and_the_fixed_ones() {
    let (mut s, hs) = two_lines();
    s.execute("layer.new", &json!({ "name": "Empty" })).unwrap();
    s.execute("layer.new", &json!({ "name": "Cur", "current": true })).unwrap();
    s.set_selection(vec![hs[0]]);
    s.execute("laynewsel", &json!({ "name": "Walls" })).unwrap();
    let r = s.execute("purge", &json!({ "type": "layers" })).unwrap();
    let names: Vec<String> = s.doc().unwrap().layers.iter().map(|l| l.name.clone()).collect();
    assert!(names.contains(&"Walls".to_string()) && names.contains(&"0".to_string()) && names.contains(&"Cur".to_string()), "{names:?}");
    assert!(!names.contains(&"Empty".to_string()), "{names:?} {r}");
    let again = s.execute("purge", &json!({ "type": "layers" })).unwrap();
    assert_eq!(again["layers"], 0, "a second run finds nothing: {again}");
}
