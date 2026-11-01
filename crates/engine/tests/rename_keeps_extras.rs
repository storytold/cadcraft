//! RENAME keeps what a renamed layer, block or style carries beyond what CADCraft models, as
//! AutoCAD does: other applications' xdata, its extension dictionary and reactors. A renamed
//! dynamic block stays dynamic, and saving writes the kept data on the new name.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Drawing;
use cadcraft_engine::doc::dynblock::dynamic_block;
use serde_json::json;

/// DXF text from lines of `code value` (the value may be empty).
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

/// The dynamic DOOR block drawing (its BLOCK_RECORD has an extension dictionary holding the
/// block's evaluation graph), plus a layer WALLS carrying another application's xdata.
fn session() -> Session {
    let layer = "0 TABLE\n2 LAYER\n5 2\n330 0\n100 AcDbSymbolTable\n70 1\n\
        0 LAYER\n5 10\n330 2\n100 AcDbSymbolTableRecord\n100 AcDbLayerTableRecord\n2 Walls\n70 0\n62 3\n6 Continuous\n\
        1001 ACME_TAGS\n1000 W-01\n1070 7\n0 ENDTAB\n";
    let src = include_str!("../../io/tests/data/dynblock_door.txt").replace("\r\n", "\n").replacen(
        "0 TABLE\n2 BLOCK_RECORD\n",
        &format!("{layer}0 TABLE\n2 BLOCK_RECORD\n"),
        1,
    );
    let d = cadcraft_io::read_dxf(dxf(&src).as_bytes()).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "rename", None);
    s
}

fn layer_xdata(d: &Drawing, name: &str) -> Vec<(i32, String)> {
    let key = ("LAYER".to_string(), name.to_string());
    d.foreign.records.get(&key).map(|x| x.xdata.iter().map(|t| (t.code, t.value.trim().to_string())).collect()).unwrap_or_default()
}

fn saved_and_read(s: &Session) -> Drawing {
    cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap()
}

fn acme() -> Vec<(i32, String)> {
    vec![(1001, "ACME_TAGS".into()), (1000, "W-01".into()), (1070, "7".into())]
}

#[test]
fn a_renamed_layer_keeps_its_xdata_through_a_save() {
    let mut s = session();
    assert_eq!(layer_xdata(s.doc().unwrap(), "WALLS"), acme());
    s.execute("rename", &json!({ "table": "layer", "from": "Walls", "to": "Partitions" })).unwrap();
    assert_eq!(layer_xdata(s.doc().unwrap(), "PARTITIONS"), acme());
    let d = saved_and_read(&s);
    assert!(d.layer("Partitions").is_some());
    assert_eq!(layer_xdata(&d, "PARTITIONS"), acme());
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(layer_xdata(s.doc().unwrap(), "WALLS"), acme(), "undo puts it back under the old name");
}

#[test]
fn a_renamed_dynamic_block_stays_dynamic_through_a_save() {
    let mut s = session();
    let before = dynamic_block(s.doc().unwrap(), "Door").expect("Door is a dynamic block");
    s.execute("rename", &json!({ "table": "block", "from": "Door", "to": "Gate" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.block("Gate").is_some() && d.block("Door").is_none());
    assert_eq!(dynamic_block(d, "Gate").map(|m| m.parameters.len()), Some(before.parameters.len()));
    let d = saved_and_read(&s);
    let after = dynamic_block(&d, "Gate").expect("Gate is still a dynamic block after a save");
    assert_eq!(after.parameters.len(), before.parameters.len());
    assert_eq!(after.actions.len(), before.actions.len());
}

#[test]
fn a_rename_that_only_changes_case_keeps_everything() {
    let mut s = session();
    s.execute("rename", &json!({ "table": "layer", "from": "Walls", "to": "WALLS" })).unwrap();
    s.execute("rename", &json!({ "table": "block", "from": "Door", "to": "DOOR" })).unwrap();
    let d = saved_and_read(&s);
    let layers: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).filter(|n| n.eq_ignore_ascii_case("walls")).collect();
    assert_eq!(layers, ["WALLS"], "the layer is saved under the new spelling");
    let blocks: Vec<(&str, &str)> =
        d.blocks.iter().filter(|(k, _)| k.eq_ignore_ascii_case("door")).map(|(k, b)| (k.as_str(), b.name.as_str())).collect();
    assert_eq!(blocks, [("DOOR", "DOOR")], "the block is saved under the new spelling");
    assert_eq!(layer_xdata(&d, "WALLS"), acme());
    assert!(dynamic_block(&d, "Door").is_some());
}
