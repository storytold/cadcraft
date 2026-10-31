//! DXFIN and DXFOUT, AutoCAD's names for bringing a DXF in and writing one out (#2455): DXFOUT
//! always writes DXF, DXFIN imports a file's objects as IMPORT does, and typed (at the command
//! line, in a script or from LISP) both ask for the file name, as AutoCAD does with FILEDIA 0.

use std::path::{Path, PathBuf};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::json;

fn install_io() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

/// A fresh folder with a space in its name, as typed paths often have.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft dxf {name} {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A drawing with a circle and a line.
fn drawing() -> Session {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 5})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().model.iter().count()
}

fn is_dxf(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.contains("SECTION") && text.contains("ENTITIES") && text.trim_end().ends_with("EOF")
}

#[test]
fn dxfout_always_writes_dxf_and_dxfin_brings_its_objects_in() {
    install_io();
    let dir = folder("json");
    let mut s = drawing();
    let out = dir.join("plan.dxf");
    let r = s.execute("dxfout", &json!({"path": out.to_string_lossy()})).unwrap();
    assert_eq!((r["format"].as_str(), r["path"].as_str()), (Some("dxf"), Some(out.to_string_lossy().as_ref())));
    assert!(is_dxf(&std::fs::read(&out).unwrap()), "the file is DXF");
    // A name without .dxf gets it, as AutoCAD adds it: a DXF is never written as plan.svg
    // (EXPORT would write SVG there) or under no extension.
    for name in ["plan.svg", "plan"] {
        let r = s.execute("dxfout", &json!({"path": dir.join(name).to_string_lossy()})).unwrap();
        let written = dir.join(format!("{name}.dxf"));
        assert_eq!(r["path"].as_str(), Some(written.to_string_lossy().as_ref()));
        assert!(is_dxf(&std::fs::read(&written).unwrap()), "{name}");
        assert!(!dir.join(name).exists(), "{name}: nothing written under the name as typed");
    }
    // Without a path it returns the DXF, and a `format` can't make it something else.
    let r = s.execute("dxfout", &json!({"format": "svg"})).unwrap();
    assert_eq!(r["format"], "dxf");
    assert!(r["data"].as_str().is_some_and(|d| !d.is_empty()));
    assert!(s.execute("dxfout", &json!({"path": null})).unwrap()["data"].is_string(), "a null path is no path");
    // A `path` that isn't a string is refused, never answered with base64 and nothing written.
    for cmd in ["dxfout", "export", "dxfin", "import"] {
        let e = s.execute(cmd, &json!({"path": 42, "format": "dxf"})).unwrap_err().to_string();
        assert!(e.contains("`path` must be a string"), "{cmd}: {e}");
    }

    // DXFIN brings the objects into the current drawing, as IMPORT does.
    let mut t = drawing();
    let r = t.execute("dxfin", &json!({"path": out.to_string_lossy(), "at": [100, 0]})).unwrap();
    assert_eq!(r["imported"], 2);
    assert_eq!(count(&t), 4);
    assert!(t.execute("dxfin", &json!({})).is_err(), "a file is required");
    assert!(t.execute("dxfin", &json!({"path": dir.join("missing.dxf").to_string_lossy()})).is_err());
    assert_eq!(count(&t), 4, "a refused DXFIN adds nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn typed_dxfout_and_dxfin_ask_for_the_file_name() {
    install_io();
    let dir = folder("typed");
    let mut s = drawing();
    s.cmdline("DXFOUT").unwrap();
    let p = s.current_prompt().unwrap();
    assert_eq!(p.message, "Enter file name");
    assert_eq!(p.default.as_deref(), Some("Drawing1.dxf"), "the drawing's name as .dxf");
    // The whole line is the name, spaces and all; quotes around it are dropped.
    let out = dir.join("site plan.dxf");
    s.cmdline(&format!("\"{}\"", out.display())).unwrap();
    assert!(s.running.is_none());
    assert!(is_dxf(&std::fs::read(&out).unwrap()));
    assert!(s.log.iter().any(|l| l.contains("Exported to")), "{:?}", s.log);

    let mut t = drawing();
    t.cmdline("DXFIN").unwrap();
    assert_eq!(t.current_prompt().unwrap().message, "Enter name of file to import");
    t.cmdline(&out.display().to_string()).unwrap();
    assert!(t.running.is_none());
    assert_eq!(count(&t), 4);
    assert!(t.log.iter().any(|l| l.contains("2 objects imported from site plan.dxf")), "{:?}", t.log);

    // Enter at DXFIN's prompt brings nothing in.
    t.cmdline("DXFIN").unwrap();
    t.cmdline("").unwrap();
    assert!(t.running.is_none());
    assert_eq!(count(&t), 4);

    // A script line takes the rest of the line as the name.
    t.script(&format!("DXFIN {}\n", out.display())).unwrap();
    assert_eq!(count(&t), 6);
    // And so does LISP's `command`.
    let lisp = format!("(command \"DXFIN\" \"{}\")", out.display().to_string().replace('\\', "/"));
    t.cmdline(&lisp).unwrap();
    assert_eq!(count(&t), 8);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The default name keeps the drawing's folder, so Enter writes next to the drawing.
#[test]
fn dxfout_by_default_writes_next_to_the_drawing() {
    install_io();
    let dir = folder("default");
    let mut s = drawing();
    let dwg = dir.join("bracket.dwg");
    s.execute("saveas", &json!({"path": dwg.to_string_lossy()})).unwrap();
    s.cmdline("DXFOUT").unwrap();
    let expected = dir.join("bracket.dxf");
    assert_eq!(s.current_prompt().unwrap().default.map(PathBuf::from), Some(expected.clone()));
    s.cmdline("").unwrap();
    assert!(is_dxf(&std::fs::read(&expected).unwrap()));
    assert!(Path::new(&dwg).exists(), "the drawing's own file is untouched");
    let _ = std::fs::remove_dir_all(&dir);
}
