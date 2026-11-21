//! SAVEAS at a chosen version and as binary DXF (#421): the options reach the host's writer, the
//! drawing keeps them for later saves, and bad values are errors.

use cadcraft_doc::{Drawing, HVal};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, SaveAs, base64_decode, base64_encode, set_io, set_write_as};
use serde_json::json;

/// The plain writer (no options).
fn plain(_: &Drawing, _: &str) -> Result<Vec<u8>, String> {
    Ok(b"plain".to_vec())
}

/// The writer with options: the bytes name the options it got.
fn with_options(_: &Drawing, _: &str, o: &SaveAs) -> Result<Vec<u8>, String> {
    Ok(format!("{}|{}", o.version.map(|v| v.year()).unwrap_or("-"), o.binary).into_bytes())
}

fn install() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: plain, plot: None });
    set_write_as(with_options);
}

fn acadver(s: &Session) -> Option<String> {
    s.state().unwrap().doc.header.get("ACADVER").and_then(HVal::as_str).map(str::to_string)
}

#[test]
fn saveas_version_and_binary_reach_the_writer_and_stay_with_the_drawing() {
    install();
    let dir = std::env::temp_dir().join(format!("cadcraft-save-version-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().to_string();
    let read = |name: &str| String::from_utf8(std::fs::read(dir.join(name)).unwrap()).unwrap();
    let mut s = Session::new();
    s.cmdline("circle 0,0 5").unwrap();
    assert_eq!(acadver(&s).as_deref(), Some("AC1032"), "a new drawing is 2018");

    let r = s.execute("saveas", &json!({ "path": path("a.dxf"), "version": "2010" })).unwrap();
    assert_eq!(read("a.dxf"), "2010|false");
    assert_eq!((r["version"].clone(), r["binary"].clone()), (json!("2010"), json!(false)));
    assert_eq!(acadver(&s).as_deref(), Some("AC1024"), "the drawing is now a 2010 drawing");
    assert!(!s.state().unwrap().is_dirty());
    // QSAVE writes the drawing's own version (the plain writer reads it from the drawing).
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(read("a.dxf"), "plain");

    // Binary, version as a number; QSAVE stays binary.
    s.execute("saveas", &json!({ "path": path("b.dxf"), "version": 2004, "binary": true })).unwrap();
    assert_eq!(read("b.dxf"), "2004|true");
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(read("b.dxf"), "-|true");
    // Saving as DXF without `binary` goes back to ASCII.
    s.execute("saveas", &json!({ "path": path("c.dxf") })).unwrap();
    assert_eq!(read("c.dxf"), "plain");
    assert!(!s.state().unwrap().binary_dxf);

    // document.bytes takes the same options.
    let r = s.execute("document.bytes", &json!({ "version": "R2013" })).unwrap();
    assert_eq!(base64_decode(r["data"].as_str().unwrap()).unwrap(), b"2013|false");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_save_options_are_errors() {
    install();
    let dir = std::env::temp_dir().join(format!("cadcraft-save-version-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().to_string();
    let mut s = Session::new();
    let err = |s: &mut Session, p: serde_json::Value| s.execute("saveas", &p).unwrap_err().to_string();
    assert!(err(&mut s, json!({ "path": path("x.dxf"), "version": "2005" })).contains("2000, 2004, 2007, 2010, 2013, 2018"));
    assert!(err(&mut s, json!({ "path": path("x.dxf"), "version": [2018] })).contains("unknown version"));
    assert!(err(&mut s, json!({ "path": path("x.dxf"), "binary": "yes" })).contains("binary"));
    assert!(err(&mut s, json!({ "path": path("x.dwg"), "binary": true })).contains("DXF"));
    assert!(!dir.join("x.dxf").exists() && !dir.join("x.dwg").exists(), "nothing is written");
    assert_eq!(acadver(&s).as_deref(), Some("AC1032"), "a failed save leaves the version alone");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A drawing opened from a binary DXF saves as binary DXF again.
#[test]
fn opening_binary_dxf_keeps_it_binary() {
    install();
    let dir = std::env::temp_dir().join(format!("cadcraft-save-version-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new();
    let binary = b"AutoCAD Binary DXF\r\n\x1a\0\0\0EOF\0";
    s.execute("open", &json!({ "data": base64_encode(binary), "name": "bin.dxf" })).unwrap();
    assert!(s.state().unwrap().binary_dxf);
    let out = dir.join("again.dxf");
    s.execute("qsave", &json!({ "path": out.to_string_lossy() })).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"-|true");
    // An ASCII file stays ASCII.
    s.execute("open", &json!({ "data": base64_encode(b"0\nEOF\n"), "name": "text.dxf" })).unwrap();
    assert!(!s.state().unwrap().binary_dxf);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #3213: EXPORT to DXF or DWG takes the same version and binary options; the drawing keeps its
/// own version and name, and other formats refuse a version.
#[test]
fn export_takes_the_version_and_binary_options() {
    install();
    let mut s = Session::new();
    s.cmdline("circle 0,0 5").unwrap();
    let data = |r: &serde_json::Value| String::from_utf8(base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();

    let r = s.execute("export", &json!({ "format": "dxf", "version": "2000" })).unwrap();
    assert_eq!(data(&r), "2000|false");
    assert_eq!(r["version"], json!("2000"));
    assert_eq!(acadver(&s).as_deref(), Some("AC1032"), "an export leaves the drawing's version alone");
    let r = s.execute("export", &json!({ "format": "dwg", "version": 2004 })).unwrap();
    assert_eq!(data(&r), "2004|false");
    let r = s.execute("export", &json!({ "format": "dxf", "binary": true })).unwrap();
    assert_eq!(data(&r), "-|true");
    // Without options: the plain writer, at the drawing's own version.
    let r = s.execute("export", &json!({ "format": "dxf" })).unwrap();
    assert_eq!(data(&r), "plain");
    assert!(r.get("version").is_none());

    let err = |s: &mut Session, p: serde_json::Value| s.execute("export", &p).unwrap_err().to_string();
    assert!(err(&mut s, json!({ "format": "svg", "version": "2000" })).contains("DXF and DWG"));
    assert!(err(&mut s, json!({ "format": "dwg", "binary": true })).contains("DXF"));
    assert!(err(&mut s, json!({ "format": "dxf", "version": "2005" })).contains("unknown version"));
    assert!(s.state().unwrap().path.is_none(), "the drawing keeps its name");
}
