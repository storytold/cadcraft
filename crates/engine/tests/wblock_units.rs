//! WBLOCK writes the source drawing's units: `$INSUNITS` and `$MEASUREMENT` of the written file
//! are the source's, for selected objects and for a block, as AutoCAD's `-WBLOCK` does. So are its
//! other settings and styles: a metric drawing's text size, dimension style and scale list.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

/// Writes "INSUNITS;MEASUREMENT" and, on a second line, the settings a metric drawing changes, so
/// the test can read them from the file that would be saved.
fn write(d: &Drawing, _path: &str) -> Result<Vec<u8>, String> {
    Ok(format!("{};{}\n{}", d.header.i64("INSUNITS", -1), d.header.i64("MEASUREMENT", -1), settings(d)).into_bytes())
}

fn settings(d: &Drawing) -> String {
    let mleader = d.mleader_styles.first().map(|m| format!("{m:?}")).unwrap_or_default();
    let table = d.table_styles.first().map(|t| format!("{t:?}")).unwrap_or_default();
    let limmax = d.header.point("LIMMAX");
    format!("{} {} {limmax:?} {} {mleader} {table}", d.header.str("DIMSTYLE", ""), d.header.f64("TEXTSIZE", -1.0), d.scales.len())
}

/// The file WBLOCK would save: line `n` of what [`write`] wrote. Each call writes into a folder
/// of its own, so tests running in parallel never read or delete each other's file.
fn wblock(s: &mut Session, p: serde_json::Value, n: usize) -> String {
    let dir = TempDir::new("wblock-units");
    let path = dir.path_str("out.dxf");
    let mut p = p;
    p["path"] = json!(path);
    s.execute("wblock", &p).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    text.lines().nth(n).unwrap_or("").to_string()
}

fn written(s: &mut Session, p: serde_json::Value) -> String {
    wblock(s, p, 0)
}

fn written_settings(s: &mut Session, p: serde_json::Value) -> String {
    wblock(s, p, 1)
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

#[test]
fn wblock_keeps_the_source_drawing_settings_and_styles() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write, plot: None });
    let mut s = Session::new();
    // A metric drawing: ISO-25, 2.5 mm text, A3 limits, metric multileader/table styles and scales.
    s.new_drawing(true);
    let source = settings(s.doc().unwrap());
    let l = line(&mut s);
    assert_eq!(written_settings(&mut s, json!({ "handles": [l] })), source);
    s.new_drawing(false);
    assert_ne!(settings(s.doc().unwrap()), source, "the imperial template differs, so the check means something");
}
