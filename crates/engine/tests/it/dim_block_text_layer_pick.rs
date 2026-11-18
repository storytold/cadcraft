//! A dimension drawn from a block is picked on the text of that block only where the text is
//! drawn: text on a layer frozen for the whole drawing, or turned off, is not there to pick (#3218).
//! What the block draws on a drawn layer is still picked, and with the layer thawed or on the text
//! is picked as before.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

/// What the LISP `code` evaluates to, as LISP prints it.
fn lisp(s: &mut Session, code: &str) -> String {
    let r = s.execute("lisp", &json!({ "code": code })).unwrap();
    r["value"].as_str().unwrap().to_string()
}

/// Whether the pick at `p` finds an object.
fn picks(s: &mut Session, p: [f64; 2]) -> bool {
    lisp(s, &format!("(ssget (list {} {}))", p[0], p[1])) != "nil"
}

fn handle(r: &Value) -> Handle {
    r["handle"].as_str().or_else(|| r["handles"][0].as_str()).and_then(Handle::parse_hex).unwrap()
}

/// A drawing with a dimension drawn from the block "DT", as AutoCAD saves one: the block holds the
/// dimension's line and its text, and the text alone is on the layer "Hidden".
fn dimension_with_block_text() -> (Session, Handle) {
    // Metric, as the issue draws it: the ISO-25 text height makes the dimension's box reach the
    // block's text.
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("layer.new", &json!({ "name": "Hidden" })).unwrap();
    let line = handle(&s.execute("line", &json!({ "points": [[0, 40], [40, 40]] })).unwrap());
    s.execute("mtext", &json!({ "at": [20, 44], "text": "40", "height": 3, "attach": 5 })).unwrap();
    let text = s.doc().unwrap().model.last().unwrap().handle;
    s.execute("properties.set", &json!({ "handles": [text.hex()], "layer": "Hidden" })).unwrap();
    assert_eq!(s.doc().unwrap().entity(text).unwrap().common.layer, "Hidden");
    s.execute("block", &json!({ "name": "DT", "base": [0, 0], "handles": [line.hex(), text.hex()], "keep": "delete" })).unwrap();
    let dim = handle(&s.execute("dimlinear", &json!({ "p1": [0, 5], "p2": [40, 15], "at": [20, 40] })).unwrap());
    // The block the dimension draws from, as DXF group 2 of a dimension carries it.
    s.doc_mut()
        .unwrap()
        .modify_entity(dim, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.block = Some("DT".into());
            }
        })
        .unwrap();
    (s, dim)
}

/// Sets the layer "Hidden" frozen or thawed, and off or on.
fn layer_state(s: &mut Session, frozen: bool, on: bool) {
    s.execute("layer.set", &json!({ "name": "Hidden", "frozen": frozen })).unwrap();
    s.execute("layer.set", &json!({ "name": "Hidden", "on": on })).unwrap();
}

#[test]
fn a_pick_inside_a_dimension_blocks_text_follows_the_text_layers_state() {
    let (mut s, dim) = dimension_with_block_text();
    // Thawed: the pick in the middle of the text finds the dimension, as the text is drawn.
    layer_state(&mut s, false, true);
    assert!(picks(&mut s, [20.0, 44.0]), "the text is drawn, so the pick inside it finds the dimension");
    // Beside the text's box, and on the block's line, are as they were.
    assert!(!picks(&mut s, [30.0, 44.0]), "the pick beside the text's box is on nothing");
    assert!(picks(&mut s, [10.0, 40.0]), "the dimension line the block draws is picked");

    // Frozen for the whole drawing: the text is not drawn, so the pick inside it finds nothing.
    layer_state(&mut s, true, true);
    assert!(!picks(&mut s, [20.0, 44.0]), "the text on a frozen layer is not picked");
    assert!(picks(&mut s, [10.0, 40.0]), "the line the block draws on a drawn layer is still picked");
    assert_eq!(s.doc().unwrap().entity(dim).unwrap().common.layer, "0", "the dimension itself stays on a drawn layer");

    // Turned off: the same.
    layer_state(&mut s, false, false);
    assert!(!picks(&mut s, [20.0, 44.0]), "the text on a layer turned off is not picked");
    assert!(picks(&mut s, [10.0, 40.0]), "the line the block draws on a drawn layer is still picked");
}
