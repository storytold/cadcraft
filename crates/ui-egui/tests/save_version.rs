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

/// A writer that records the version and binary flag it was asked for instead of a drawing.
fn with_options(_: &Drawing, _: &str, o: &SaveAs) -> Result<Vec<u8>, String> {
    Ok(format!("{}|{}", o.version.map(|v| v.year()).unwrap_or("-"), o.binary).into_bytes())
}

/// The default save version is offered to untitled drawings by the Save As dialog, written by its
/// Save… and by an untitled QSAVE (in the default format), and kept in the preferences.
#[test]
fn default_save_version_applies_to_new_drawings_and_persists() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"plain".to_vec()), plot: None });
    set_write_as(with_options);
    let services = Services { pick_save: Some(Box::new(|_: &str| Some(picked().to_string_lossy().to_string()))), ..Services::default() };
    let mut app = CadApp::new(Session::new(), services);
    assert_eq!(app.run("ui.saveversion", json!({})).ok(), Some(json!({ "saveVersion": "2018" })), "2018 by default");
    assert!(app.run("ui.saveversion", json!({ "version": "2005" })).is_err());
    assert_eq!(app.run("ui.saveversion", json!({ "version": 2010 })).ok(), Some(json!({ "saveVersion": "2010" })));

    // The Save As dialog offers an untitled drawing's default version, and its Save... runs the
    // picker and saves in it …
    app.run("ui.saveas", Value::Null).unwrap();
    assert_eq!(app.ui.dialog.as_deref(), Some("saveas"));
    assert_eq!(cadcraft_ui_egui::saveas::save(&mut app), Some(true));
    assert_eq!(std::fs::read(picked()).unwrap(), b"2010|false");
    // … and a drawing that has a file starts from its own version.
    app.run("ui.saveas", Value::Null).unwrap();
    assert_eq!(cadcraft_ui_egui::saveas::save(&mut app), Some(true));
    assert_eq!(std::fs::read(picked()).unwrap(), b"2010|false");
    let _ = std::fs::remove_file(picked());

    // QSAVE of an untitled drawing (the picker alone): a name picked with another extension
    // gets the default format's, so DXF is what is written.
    let dwg = std::env::temp_dir().join(format!("cadcraft-ui-save-version-{}.dwg", std::process::id()));
    let services = Services { pick_save: Some(Box::new(move |_: &str| Some(dwg.to_string_lossy().to_string()))), ..Services::default() };
    let mut untitled = CadApp::new(Session::new(), services);
    untitled.start("qsave");
    let written = std::env::temp_dir().join(format!("cadcraft-ui-save-version-{}.dwg.dxf", std::process::id()));
    assert!(written.exists(), "saved as {written:?}");
    let _ = std::fs::remove_file(&written);

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
        // Save As opens its dialog on the default format; Save there opens the picker.
        app.run("ui.saveas", Value::Null).unwrap();
        assert_eq!(cadcraft_ui_egui::saveas::save(&mut app), Some(true), "{opened} with {format}");
        assert_eq!(suggested.lock().unwrap().last().map(String::as_str), Some(expected), "{opened} with {format}");
        let path = app.session.state().unwrap().path.clone().unwrap_or_default();
        assert!(path.ends_with(expected), "{opened}: saved as {path}");
        assert!(dir.join(expected).exists());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
