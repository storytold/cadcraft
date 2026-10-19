//! Render settings store their `AcDbRenderSettings` class version in the form of the file's
//! version: a 2013+ file stores it one higher (a 2013 file stores a rapid RT one one lower), as
//! acadrust reads and writes them. A kept object carries the form of the file it came from; written
//! into a file declaring another version, it takes that version's form. The DWG bridge is an R2000
//! DXF for a 2018 DWG: without this, a 2018 drawing's MENTALRAYRENDERSETTINGS went in with the 2018
//! form, the DWG writer raised it once more, and BricsCAD refused the DWG ("Object improperly
//! read", #907).

use cadcraft_io::{SaveOptions, Version};

/// A DXF of `version` with a named object dictionary, its render settings dictionary and one
/// mental ray render settings object whose `AcDbRenderSettings` class version is `class_version`.
fn dxf(version: &str, kind: &str, class_version: i64) -> String {
    let body = match kind {
        "MENTALRAYRENDERSETTINGS" => {
            "100\nAcDbMentalRayRenderSettings\n90\n2\n90\n0\n90\n1\n70\n2\n40\n3.0\n40\n3.0\n40\n0.05\n40\n0.05\n40\n0.05\n40\n0.05\n\
             70\n0\n290\n0\n290\n1\n90\n5\n90\n5\n90\n5\n290\n0\n90\n500\n290\n0\n40\n1.0\n90\n10000\n90\n5\n90\n5\n90\n5\n290\n0\n\
             90\n200\n290\n0\n290\n0\n290\n0\n40\n0.1\n40\n1.0\n40\n1500.0\n70\n0\n70\n0\n40\n10.0\n70\n0\n70\n0\n290\n0\n1\n\n\
             90\n128\n70\n0\n90\n1048\n290\n0\n40\n1.0\n"
        }
        _ => "",
    };
    let cpp = match kind {
        "MENTALRAYRENDERSETTINGS" => "AcDbMentalRayRenderSettings",
        "RAPIDRTRENDERSETTINGS" => "AcDbRapidRTRenderSettings",
        _ => "AcDbRenderSettings",
    };
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{version}\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n\
         0\nSECTION\n2\nCLASSES\n0\nCLASS\n1\n{kind}\n2\n{cpp}\n3\nSCENEOE\n90\n1024\n91\n1\n280\n0\n281\n0\n0\nENDSEC\n\
         0\nSECTION\n2\nOBJECTS\n\
         0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n3\nACAD_RENDER_SETTINGS\n350\n20\n\
         0\nDICTIONARY\n5\n20\n330\nC\n100\nAcDbDictionary\n281\n1\n3\nMedium\n350\n21\n\
         0\n{kind}\n5\n21\n330\n20\n100\nAcDbRenderSettings\n90\n{class_version}\n1\nMedium\n290\n1\n290\n1\n290\n1\n290\n1\n1\n\n1\n\n90\n3\n{body}\
         0\nENDSEC\n0\nEOF\n"
    )
}

/// The `AcDbRenderSettings` class version of the `kind` object in DXF text.
fn class_version(text: &str, kind: &str) -> Option<i64> {
    let l: Vec<&str> = text.lines().map(str::trim).collect();
    let at = l.windows(2).position(|w| w == ["0", kind])?;
    let base = l[at..].windows(2).position(|w| w == ["100", "AcDbRenderSettings"])? + at;
    l[base..].windows(2).find(|w| w[0] == "90").map(|w| w[1].parse().unwrap())
}

fn written(d: &cadcraft_doc::Drawing, version: Version) -> String {
    String::from_utf8(cadcraft_io::write_with(d, "x.dxf", &SaveOptions { version: Some(version), binary: false }).unwrap()).unwrap()
}

#[test]
fn render_settings_class_version_takes_the_declared_versions_form() {
    let kind = "MENTALRAYRENDERSETTINGS";
    // From a 2018 file (stored 2 = class version 1 in the 2013+ form).
    let d = cadcraft_io::read_dxf(dxf("AC1032", kind, 2).as_bytes()).unwrap();
    assert_eq!(class_version(&written(&d, Version::R2018), kind), Some(2), "same version: as read");
    assert_eq!(class_version(&written(&d, Version::R2013), kind), Some(2));
    assert_eq!(class_version(&written(&d, Version::R2000), kind), Some(1), "2000 form: one lower");
    assert_eq!(class_version(&written(&d, Version::R2010), kind), Some(1));
    // From a 2000 file (stored 1), written as 2018: one higher.
    let d = cadcraft_io::read_dxf(dxf("AC1015", kind, 1).as_bytes()).unwrap();
    assert_eq!(class_version(&written(&d, Version::R2000), kind), Some(1));
    assert_eq!(class_version(&written(&d, Version::R2018), kind), Some(2));
    // Rapid RT settings: the 2013 file alone stores one lower.
    let kind = "RAPIDRTRENDERSETTINGS";
    let d = cadcraft_io::read_dxf(dxf("AC1027", kind, 1).as_bytes()).unwrap();
    assert_eq!(class_version(&written(&d, Version::R2013), kind), Some(1));
    assert_eq!(class_version(&written(&d, Version::R2018), kind), Some(2));
    assert_eq!(class_version(&written(&d, Version::R2000), kind), Some(2));
}

/// A file that declares no `$ACADVER` gives no source form: its kept objects are written as read,
/// not as if the new drawing's 2018 were their form.
#[test]
fn render_settings_without_a_declared_version_are_written_as_read() {
    let kind = "MENTALRAYRENDERSETTINGS";
    let text = dxf("AC1032", kind, 1).replace("9\n$ACADVER\n1\nAC1032\n", "");
    assert!(!text.contains("$ACADVER"));
    let d = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(d.foreign.acadver, None);
    assert_eq!(class_version(&written(&d, Version::R2000), kind), Some(1));
    assert_eq!(class_version(&written(&d, Version::R2018), kind), Some(1));
}

/// Through the DWG bridge (an R2000 DXF for a 2018 DWG) and back, the class version is what the
/// 2018 file had: before, it came back one higher each time, and BricsCAD refused the DWG.
#[test]
fn render_settings_class_version_survives_the_dwg_bridge() {
    let kind = "MENTALRAYRENDERSETTINGS";
    let d = cadcraft_io::read_dxf(dxf("AC1032", kind, 2).as_bytes()).unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert_eq!(class_version(&written(&back, Version::R2018), kind), Some(2), "{}", written(&back, Version::R2018));
    let dwg = cadcraft_io::write(&back, "x.dwg").unwrap();
    let again = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert_eq!(class_version(&written(&again, Version::R2018), kind), Some(2));
}

/// The conversion mirrors how acadrust stores the value, so it is checked against acadrust itself:
/// acadrust reads the fixture and writes it in each version, and CADCraft, reading any of those
/// files and saving it in any version, stores what acadrust stores for that version. If acadrust
/// changes the encoding, this fails before the DWG bridge drifts.
#[test]
fn render_settings_class_version_matches_what_acadrust_writes() {
    use acadrust::DxfVersion;
    let versions = [
        (DxfVersion::AC1015, Version::R2000),
        (DxfVersion::AC1024, Version::R2010),
        (DxfVersion::AC1027, Version::R2013),
        (DxfVersion::AC1032, Version::R2018),
    ];
    // As a 2018 file stores them: mental ray one higher than its class version, rapid RT as is.
    for (kind, stored) in [("MENTALRAYRENDERSETTINGS", 2), ("RAPIDRTRENDERSETTINGS", 2)] {
        let source = std::io::Cursor::new(dxf("AC1032", kind, stored).into_bytes());
        let mut doc = acadrust::DxfReader::from_reader(source).unwrap().read().unwrap();
        let by_acadrust: Vec<(Version, String)> = versions
            .iter()
            .map(|(theirs, ours)| {
                doc.version = *theirs;
                (*ours, String::from_utf8(acadrust::DxfWriter::new(&doc).write_to_vec().unwrap()).unwrap())
            })
            .collect();
        let forms: Vec<Option<i64>> = by_acadrust.iter().map(|(_, text)| class_version(text, kind)).collect();
        let expected = if kind == "RAPIDRTRENDERSETTINGS" { [2, 2, 1, 2] } else { [1, 1, 2, 2] };
        assert_eq!(forms, expected.map(Some), "{kind}: acadrust's own forms for 2000, 2010, 2013, 2018");
        for (read_from, text) in &by_acadrust {
            let d = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
            for ((declared, _), form) in by_acadrust.iter().zip(&forms) {
                assert_eq!(class_version(&written(&d, *declared), kind), *form, "{kind}: read from {read_from:?}, saved as {declared:?}");
            }
        }
    }
}
