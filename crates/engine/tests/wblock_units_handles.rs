//! WBLOCK writes the source's units, only the blocks the written objects use, and handles that
//! never repeat.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use serde_json::json;

/// Writes "insunits;block names;handles;distinct handles" so the test can read what would be saved.
fn write(d: &Drawing, _path: &str) -> Result<Vec<u8>, String> {
    let mut handles: Vec<u64> = d.model.iter().map(|e| e.handle.0).collect();
    let mut names = Vec::new();
    for b in d.blocks.values() {
        names.push(b.name.clone());
        handles.extend(b.entities.iter().map(|e| e.handle.0));
    }
    let total = handles.len();
    handles.sort_unstable();
    handles.dedup();
    Ok(format!("{};{};{};{}", d.header.i64("INSUNITS", -1), names.join(","), total, handles.len()).into_bytes())
}

fn written(s: &mut Session, p: serde_json::Value) -> String {
    let path = std::env::temp_dir().join(format!("cadcraft-wblock-units-{}.dxf", std::process::id()));
    let mut p = p;
    p["path"] = json!(path.to_string_lossy());
    s.execute("wblock", &p).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    text
}

fn line(s: &mut Session, y: f64) -> String {
    s.execute("line", &json!({ "points": [[0, y], [1, y]] })).unwrap()["handles"][0].as_str().unwrap().to_string()
}

#[test]
fn wblock_keeps_units_drops_unused_blocks_and_repeats_no_handle() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write, plot: None });
    let mut s = Session::new();
    s.execute("units", &json!({ "insunits": 6 })).unwrap();
    // Block "Part" is made first, so its line has one of the drawing's lowest handles.
    let p = line(&mut s, 0.0);
    let ins = s.execute("block", &json!({ "name": "Part", "handles": [p], "keep": "convert" })).unwrap()["insert"].as_str().unwrap().to_string();
    let mut picked = vec![ins];
    for i in 1..300 {
        picked.push(line(&mut s, f64::from(i) * 0.01));
    }
    let u = line(&mut s, 5.0);
    s.execute("block", &json!({ "name": "Unused", "handles": [u], "keep": "delete" })).unwrap();

    // 300 objects (the insert and 299 lines) plus Part's line: the source's units, no Unused,
    // and every handle distinct although the new objects are numbered from the start again.
    assert_eq!(written(&mut s, json!({ "handles": picked })), "6;Part;301;301");
    // A block written by name: its objects go to model space, its definition stays behind.
    assert_eq!(written(&mut s, json!({ "name": "Unused" })), "6;;1;1");
}
