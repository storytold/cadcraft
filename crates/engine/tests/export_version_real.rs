//! #3213, with the real DXF reader and writer: a drawing read from a 2013 DXF (AC1027) exported
//! as a 2000 DXF says AC1015, and a plain export keeps the drawing's own version.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, base64_decode, set_io, set_write_as};
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

fn install() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    set_write_as(|d, name, o| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
}

/// The `$ACADVER` an exported DXF declares.
fn acadver(r: &serde_json::Value) -> String {
    let text = String::from_utf8(base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();
    let mut lines = text.lines().map(str::trim);
    while let Some(l) = lines.next() {
        if l == "$ACADVER" {
            lines.next();
            return lines.next().unwrap_or_default().to_string();
        }
    }
    String::new()
}

#[test]
fn a_2013_drawing_exports_as_a_2000_dxf() {
    install();
    // Removed when the test ends, even when an assertion fails.
    let dir = TempDir::new("export-version");
    let file = dir.path_str("plan.dxf");
    let mut s = Session::new();
    s.cmdline("circle 0,0 5").unwrap();
    s.execute("saveas", &json!({ "path": file, "version": "2013" })).unwrap();
    s.execute("open", &json!({ "path": file })).unwrap();

    assert_eq!(acadver(&s.execute("export", &json!({ "format": "dxf" })).unwrap()), "AC1027");
    assert_eq!(acadver(&s.execute("export", &json!({ "format": "dxf", "version": "2000" })).unwrap()), "AC1015");
}
