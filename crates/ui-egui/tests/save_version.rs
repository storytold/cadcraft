//! The default save version (#421): File > Default Save Format > Version …, kept in the
//! preferences, applied by the Save dialog to drawings that have no file yet.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, SaveAs, set_io, set_write_as};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

fn picked() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("cadcraft-ui-save-version-{}.dxf", std::process::id()))
}

fn with_options(_: &Drawing, _: &str, o: &SaveAs) -> Result<Vec<u8>, String> {
    Ok(format!("{}|{}", o.version.map(|v| v.year()).unwrap_or("-"), o.binary).into_bytes())
}

#[test]
fn default_save_version_applies_to_new_drawings_and_persists() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"plain".to_vec()), plot: None });
    set_write_as(with_options);
    let services = Services { pick_save: Some(Box::new(|_: &str| Some(picked().to_string_lossy().to_string()))), ..Services::default() };
    let mut app = CadApp::new(Session::new(), services);
    assert_eq!(app.run("ui.saveversion", json!({})).ok(), Some(json!({ "saveVersion": "2018" })), "2018 by default");
    assert!(app.run("ui.saveversion", json!({ "version": "2005" })).is_err());
    assert_eq!(app.run("ui.saveversion", json!({ "version": 2010 })).ok(), Some(json!({ "saveVersion": "2010" })));

    // The Save dialog saves an untitled drawing in the default version …
    app.run("ui.saveas", Value::Null).unwrap();
    assert_eq!(std::fs::read(picked()).unwrap(), b"2010|false");
    // … and a drawing that has a file in its own.
    app.run("ui.saveas", Value::Null).unwrap();
    assert_eq!(std::fs::read(picked()).unwrap(), b"plain");
    let _ = std::fs::remove_file(picked());

    // Menu items, preferences (bad values ignored) and resetting the palettes.
    app.start("ui.saveversion.2004");
    assert_eq!(app.ui.save_version, "2004");
    let mut b = CadApp::new(Session::empty(), Services::default());
    b.load_prefs(&app.prefs_json());
    assert_eq!(b.ui.save_version, "2004");
    b.load_prefs(r#"{"saveVersion": "R14"}"#);
    assert_eq!(b.ui.save_version, "2004");
    b.start("ui.resetpalettes");
    assert_eq!(b.ui.save_version, "2004");
}

/// The default save format (#2199) is what Save As suggests for an opened DWG or DXF too, as
/// AutoCAD's Options > Open and Save > "Save as" does; Save keeps the opened file.
#[test]
fn default_save_format_applies_to_opened_drawings() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"plain".to_vec()), plot: None });
    set_write_as(with_options);
    let dir = std::env::temp_dir().join(format!("cadcraft-ui-save-format-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let suggested = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (seen, to) = (suggested.clone(), dir.clone());
    let pick = move |name: &str| {
        seen.lock().unwrap().push(name.to_string());
        Some(to.join(name).to_string_lossy().to_string())
    };
    let mut app = CadApp::new(Session::empty(), Services { pick_save: Some(Box::new(pick)), ..Services::default() });
    for (opened, format, expected) in
        [("Plan.dwg", "dxf", "Plan.dxf"), ("Plan.DXF", "dwg", "Plan.dwg"), ("Plan.dxf", "dxf", "Plan.dxf"), ("Sheet.dwt", "dwg", "Sheet.dwg")]
    {
        let file = dir.join(opened);
        std::fs::write(&file, b"0\nEOF\n").unwrap();
        app.run("open", json!({ "path": file.to_string_lossy() })).unwrap();
        app.run("ui.saveformat", json!({ "format": format })).unwrap();
        // the picker's path decides the format, so the echo promises only the suggestion
        let echo = format!("Save As will suggest {}", format.to_ascii_uppercase());
        assert_eq!(app.session.log.last(), Some(&echo), "{opened} with {format}");
        app.run("ui.saveas", Value::Null).unwrap();
        assert_eq!(suggested.lock().unwrap().last().map(String::as_str), Some(expected), "{opened} with {format}");
        let path = app.session.state().unwrap().path.clone().unwrap_or_default();
        assert!(path.ends_with(expected), "{opened}: saved as {path}");
        assert!(dir.join(expected).exists());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
