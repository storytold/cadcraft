//! Kept render settings are in the form of the file they were read from, whatever the drawing's
//! `$ACADVER` says after a SAVEAS to another version: saving a 2018 drawing down to 2000 and back
//! up to 2018 writes the class version the 2018 file had, not one higher (which BricsCAD refuses,
//! #907).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io, set_write_as};
use serde_json::json;

/// A 2018 DXF with one mental ray render settings object, stored as `90 2` (class version 1 in the
/// 2013+ form).
const DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n\
    0\nSECTION\n2\nCLASSES\n0\nCLASS\n1\nMENTALRAYRENDERSETTINGS\n2\nAcDbMentalRayRenderSettings\n3\nSCENEOE\n90\n1024\n91\n1\n280\n0\n281\n0\n0\nENDSEC\n\
    0\nSECTION\n2\nOBJECTS\n\
    0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n3\nACAD_RENDER_SETTINGS\n350\n20\n\
    0\nDICTIONARY\n5\n20\n330\nC\n100\nAcDbDictionary\n281\n1\n3\nMedium\n350\n21\n\
    0\nMENTALRAYRENDERSETTINGS\n5\n21\n330\n20\n100\nAcDbRenderSettings\n90\n2\n1\nMedium\n290\n1\n290\n1\n290\n1\n290\n1\n1\n\n1\n\n90\n3\n\
    100\nAcDbMentalRayRenderSettings\n90\n2\n0\nENDSEC\n0\nEOF\n";

/// The `AcDbRenderSettings` class version in a written DXF.
fn class_version(path: &std::path::Path) -> Option<i64> {
    let text = std::fs::read_to_string(path).unwrap();
    let l: Vec<&str> = text.lines().map(str::trim).collect();
    let at = l.windows(2).position(|w| w == ["0", "MENTALRAYRENDERSETTINGS"])?;
    let base = l[at..].windows(2).position(|w| w == ["100", "AcDbRenderSettings"])? + at;
    l[base..].windows(2).find(|w| w[0] == "90").map(|w| w[1].parse().unwrap())
}

#[test]
fn saveas_down_and_back_up_keeps_the_render_settings_class_version() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    set_write_as(|d, name, o| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    let dir = std::env::temp_dir().join(format!("cadcraft-render-settings-saveas-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::empty();
    s.open_drawing(cadcraft_io::read_dxf(DXF.as_bytes()).unwrap(), "render.dxf", None);
    let save = |s: &mut Session, name: &str, version: &str| {
        let path = dir.join(name);
        s.execute("saveas", &json!({ "path": path.to_string_lossy(), "version": version })).unwrap();
        class_version(&path)
    };

    assert_eq!(save(&mut s, "2018.dxf", "2018"), Some(2), "same version: as read");
    assert_eq!(save(&mut s, "2000.dxf", "2000"), Some(1), "the 2000 form is one lower");
    s.execute("qsave", &json!({})).unwrap();
    assert_eq!(class_version(&dir.join("2000.dxf")), Some(1), "QSAVE keeps the 2000 form");
    assert_eq!(save(&mut s, "back.dxf", "2018"), Some(2), "back to 2018: the 2018 file's value, not one higher");
    let _ = std::fs::remove_dir_all(&dir);
}
