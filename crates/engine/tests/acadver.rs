//! ACADVER is the program's release as text, as AutoCAD documents it, so `(atof (getvar
//! "ACADVER"))` reads a number from it. It is read-only, and the drawing's `$ACADVER` stays the
//! file version a save writes, so a drawing read from a 2000 DXF saves as one.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::lisp::value::Value;
use serde_json::json;

/// The release text `getvar` and `(getvar "ACADVER")` report.
const RELEASE: &str = "24.3s (CADCraft)";

/// A 2000 DXF (`$ACADVER` `AC1015`) holding one line.
const DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n\
    0\nSECTION\n2\nENTITIES\n\
    0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n10.0\n21\n0.0\n31\n0.0\n\
    0\nENDSEC\n0\nEOF\n";

/// The file formats the save path writes through.
fn install_io() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

/// A LISP expression's value, or its error message.
fn lisp(s: &mut Session, code: &str) -> Result<Value, String> {
    match s.lisp_eval(code, false, true, Vec::new()) {
        Ok(Some(Ok(v))) => Ok(v),
        Ok(Some(Err(e))) => Err(e),
        Ok(None) => Err("did not finish".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// GETVAR's value.
fn getvar(s: &mut Session, name: &str) -> serde_json::Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

/// The `$ACADVER` a DXF text declares.
fn file_version(text: &str) -> Option<String> {
    let l: Vec<&str> = text.lines().map(str::trim).collect();
    let at = l.windows(2).position(|w| w == ["9", "$ACADVER"])?;
    l.get(at + 2).filter(|c| **c == "1").and_then(|_| l.get(at + 3)).map(|v| (*v).to_string())
}

#[test]
fn getvar_gives_the_release_text_and_atof_reads_the_number() {
    let mut s = Session::new();
    assert_eq!(getvar(&mut s, "ACADVER"), json!(RELEASE));
    assert_eq!(getvar(&mut s, "acadver"), json!(RELEASE), "lower-case names work");
    assert!(matches!(lisp(&mut s, "(getvar \"ACADVER\")"), Ok(Value::Str(t)) if &*t == RELEASE), "a LISP string");
    assert!(matches!(lisp(&mut s, "(atof (getvar \"ACADVER\"))"), Ok(Value::Real(r)) if (r - 24.3).abs() < 1e-9), "(atof) reads the release number");
}

#[test]
fn setvar_refuses_acadver_and_changes_nothing() {
    let mut s = Session::new();
    let before = s.state().unwrap().doc.header.str("ACADVER", "");
    let e = s.execute("setvar", &json!({ "name": "ACADVER", "value": "AC1027" })).unwrap_err().to_string();
    assert!(e.contains("read-only"), "{e}");
    assert!(lisp(&mut s, "(setvar \"ACADVER\" \"AC1027\")").is_err(), "AutoLISP's setvar too");
    // Typed at the command line, SETVAR reports the variable as read-only at its name prompt.
    s.cmdline("setvar acadver").unwrap();
    assert!(s.log.iter().any(|l| l == &format!("ACADVER = {RELEASE} (read only)")), "{:?}", s.log);
    // Neither the release text nor the drawing's file version moved.
    assert_eq!(getvar(&mut s, "ACADVER"), json!(RELEASE));
    assert_eq!(s.state().unwrap().doc.header.str("ACADVER", ""), before);
    assert_eq!(before, "AC1032");
}

#[test]
fn a_2000_drawing_saves_as_2000() {
    install_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-acadver-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("r2000.dxf");
    let name = path.to_string_lossy().into_owned();
    let mut s = Session::empty();
    s.open_drawing(cadcraft_io::read_dxf(DXF.as_bytes()).unwrap(), "r2000.dxf", Some(name.clone()));
    assert_eq!(s.state().unwrap().doc.header.str("ACADVER", ""), "AC1015", "read as 2000");
    assert_eq!(cadcraft_io::save_version(&s.state().unwrap().doc), cadcraft_io::Version::R2000);
    assert_eq!(getvar(&mut s, "ACADVER"), json!(RELEASE), "still the release text");

    s.execute("qsave", &json!({})).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(file_version(&saved).as_deref(), Some("AC1015"), "the file keeps its 2000 version");
    assert_eq!(s.state().unwrap().doc.header.str("ACADVER", ""), "AC1015");
    assert_eq!(getvar(&mut s, "ACADVER"), json!(RELEASE));
    assert_eq!(cadcraft_io::read_dxf(saved.as_bytes()).unwrap().header.str("ACADVER", ""), "AC1015", "and reopens as 2000");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dollar_name_cannot_change_the_file_version() {
    let mut s = Session::new();
    let e = s.execute("setvar", &json!({ "name": "$ACADVER", "value": "AC1027" })).unwrap_err().to_string();
    assert!(e.contains("read-only"), "{e}");
    assert!(lisp(&mut s, "(setvar \"$ACADVER\" \"AC1015\")").is_err(), "AutoLISP's setvar too");
    assert_eq!(s.state().unwrap().doc.header.str("ACADVER", ""), "AC1032", "the save version did not move");
    assert!(cadcraft_engine::sysvars::is_read_only("$acadver"), "SETVAR's prompt sees it as read-only too");
    // Other `$` names still reach the header as before, in LISP's units.
    assert!(lisp(&mut s, "(setvar \"$ANGBASE\" (/ pi 2))").is_ok());
    assert!(matches!(lisp(&mut s, "(getvar \"$ANGBASE\")"), Ok(Value::Real(r)) if (r - std::f64::consts::FRAC_PI_2).abs() < 1e-9));
    let r = s.execute("setvar", &json!({ "name": "$OSMODE", "value": 9 })).unwrap();
    assert_eq!(r["value"], json!(9), "{r}");
}

#[test]
fn listings_show_the_release_text_without_a_drawing() {
    let mut s = Session::empty();
    let all = s.execute("sysvars", &json!({})).unwrap();
    let text = all.to_string();
    assert!(text.contains(RELEASE), "{text}");
    let mut s = Session::new();
    let all = s.execute("sysvars", &json!({})).unwrap().to_string();
    assert!(all.contains(RELEASE) && !all.contains("\"AC1032\""), "{all}");
}
