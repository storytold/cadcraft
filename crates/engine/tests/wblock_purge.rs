//! WBLOCK writes what the block or the selection uses, not the source drawing's whole tables
//! (#2423): AutoCAD's WBLOCK of a 5-object symbol writes its layer, its style and the defaults,
//! not the other 40 layers and 20 blocks of the drawing it came from.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use serde_json::{Value, json};

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

/// Writes the written drawing's table names, one table a line: layers, text styles, blocks.
fn write(d: &Drawing, _path: &str) -> Result<Vec<u8>, String> {
    let mut blocks: Vec<&str> = d.blocks.values().map(|b| b.name.as_str()).collect();
    blocks.sort_unstable();
    let layers: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
    let styles: Vec<&str> = d.text_styles.iter().map(|t| t.name.as_str()).collect();
    Ok(format!("{}\n{}\n{}", layers.join(","), styles.join(","), blocks.join(",")).into_bytes())
}

/// The layers, text styles and blocks of the file WBLOCK writes with `p`.
fn written(s: &mut Session, mut p: Value) -> (String, String, String) {
    let dir = TempDir::new("wblock-purge");
    let path = dir.path_str("out.dxf");
    p["path"] = json!(path);
    s.execute("wblock", &p).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines = text.lines().map(str::to_string);
    (lines.next().unwrap_or_default(), lines.next().unwrap_or_default(), lines.next().unwrap_or_default())
}

fn line_on(s: &mut Session, layer: &str, y: f64) -> String {
    let h = s.execute("line", &json!({ "points": [[0, y], [5, y]] })).unwrap()["handles"][0].as_str().unwrap().to_string();
    s.execute("properties.set", &json!({ "handles": [h], "layer": layer })).unwrap();
    h
}

/// Block SYM: a line on SYM-LAYER and a reference to INNER, whose line is on INNER-LAYER. The
/// drawing also has a layer, a text style and a block that SYM doesn't use.
fn drawing() -> Session {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write, plot: None });
    let mut s = Session::new();
    for name in ["SYM-LAYER", "INNER-LAYER", "UNUSED-LAYER", "OTHER-LAYER"] {
        s.execute("layer.new", &json!({ "name": name })).unwrap();
    }
    s.execute("style", &json!({ "name": "UNUSED-STYLE" })).unwrap();
    s.execute("style.current", &json!({ "name": "Standard" })).unwrap();
    let inner = line_on(&mut s, "INNER-LAYER", 0.0);
    s.execute("block", &json!({ "name": "INNER", "base": [0, 0], "handles": [inner], "keep": "delete" })).unwrap();
    let r = s.execute("insert", &json!({ "name": "INNER", "at": [0, 5] })).unwrap();
    let inner_ref = r["handle"].as_str().unwrap().to_string();
    let own = line_on(&mut s, "SYM-LAYER", 10.0);
    s.execute("block", &json!({ "name": "SYM", "base": [0, 0], "handles": [own, inner_ref], "keep": "delete" })).unwrap();
    let other = line_on(&mut s, "OTHER-LAYER", 20.0);
    s.execute("block", &json!({ "name": "OTHER", "base": [0, 0], "handles": [other], "keep": "delete" })).unwrap();
    s
}

#[test]
fn a_block_is_written_with_what_it_uses() {
    let mut s = drawing();
    let (layers, styles, blocks) = written(&mut s, json!({ "name": "SYM" }));
    let layers: Vec<&str> = layers.split(',').collect();
    for used in ["0", "SYM-LAYER", "INNER-LAYER"] {
        assert!(layers.contains(&used), "{used} in {layers:?}");
    }
    for unused in ["UNUSED-LAYER", "OTHER-LAYER"] {
        assert!(!layers.contains(&unused), "{unused} in {layers:?}");
    }
    assert!(!styles.split(',').any(|n| n == "UNUSED-STYLE"), "{styles}");
    assert!(styles.split(',').any(|n| n == "Standard"), "{styles}");
    // The nested block comes along; the block being written is the file itself, and OTHER stays out.
    assert_eq!(blocks, "INNER");
    // The source drawing is untouched.
    let d = s.doc().unwrap();
    assert!(d.layer("UNUSED-LAYER").is_some() && d.block("OTHER").is_some());
}

#[test]
fn selected_objects_are_written_with_what_they_use() {
    let mut s = drawing();
    let r = s.execute("insert", &json!({ "name": "INNER", "at": [50, 50] })).unwrap();
    let h = r["handle"].as_str().unwrap().to_string();
    let (layers, _, blocks) = written(&mut s, json!({ "handles": [h] }));
    assert_eq!(blocks, "INNER", "only the block the selection references");
    let layers: Vec<&str> = layers.split(',').collect();
    assert!(layers.contains(&"INNER-LAYER") && !layers.contains(&"SYM-LAYER"), "{layers:?}");
}
