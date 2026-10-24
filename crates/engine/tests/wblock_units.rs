//! WBLOCK writes the source drawing's units: `$INSUNITS` and `$MEASUREMENT` of the written file
//! are the source's, for selected objects and for a block, as AutoCAD's `-WBLOCK` does.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use serde_json::json;

/// Writes "INSUNITS;MEASUREMENT" so the test can read the units of the file that would be saved.
fn write(d: &Drawing, _path: &str) -> Result<Vec<u8>, String> {
    Ok(format!("{};{}", d.header.i64("INSUNITS", -1), d.header.i64("MEASUREMENT", -1)).into_bytes())
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

fn line(s: &mut Session) -> String {
    s.execute("line", &json!({ "points": [[0, 0], [2, 0]] })).unwrap()["handles"][0].as_str().unwrap().to_string()
}

#[test]
fn wblock_keeps_the_source_drawing_units() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write, plot: None });
    let mut s = Session::new();
    // A metric drawing in metres, like the issue's metres.dxf (`--metric` then `insunits 6`).
    s.new_drawing(true);
    s.execute("units", &json!({ "insunits": 6 })).unwrap();
    let b = line(&mut s);
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [b], "keep": "delete" })).unwrap();
    let c = line(&mut s);

    // Selected objects and a block by name both keep the source's units, not inches.
    assert_eq!(written(&mut s, json!({ "handles": [c] })), "6;1");
    assert_eq!(written(&mut s, json!({ "name": "B" })), "6;1");

    // The metric default (millimetres) and an imperial drawing keep their units too.
    s.new_drawing(true);
    let m = line(&mut s);
    assert_eq!(written(&mut s, json!({ "handles": [m] })), "4;1");
    s.new_drawing(false);
    let i = line(&mut s);
    assert_eq!(written(&mut s, json!({ "handles": [i] })), "1;0");
}
