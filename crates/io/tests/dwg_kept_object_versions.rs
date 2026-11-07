//! Objects CADCraft keeps without modelling them reach the DWG as the file they came from holds
//! them. The DWG bridge is an R2000 DXF, and acadrust reads some objects differently by version:
//! read as R2000, a 2013 or later file's mental ray render settings came out a class version too
//! high (the ODA File Converter then refuses the whole DWG: "Object improperly read:
//! AcDbMentalRayRenderSettings"), RapidRT render settings from a 2013 file a version too low, and
//! a field lost its flags and cached value. The bridge now reads the kept groups as the version
//! they were read from (`$ACADVER`).
//!
//! The drawing is synthetic: render settings and a field in the named object dictionary, with
//! values chosen to be told apart. The reference is acadrust's own conversion of the same DXF to
//! the same DWG version (an older DWG can't hold everything: a 2004 field has no value flags or
//! format), read back; CADCraft's DWG must hold the same objects. When the DWG is the DXF's own
//! version, that reference is also checked against the DXF as read.
#![cfg(not(target_arch = "wasm32"))]

use std::fmt::Write as _;
use std::io::Cursor;

use acadrust::objects::{ClassObjectData, Field, ObjectType};
use cadcraft_io::{SaveOptions, Version, read, write_with};

/// A minimal DXF of `version` ("AC1032"): a model space MTEXT, and in the named object dictionary
/// mental ray and RapidRT render settings and a field with a non-ASCII cached value.
fn synthetic(version: &str) -> String {
    let mut g: Vec<(i32, String)> = Vec::new();
    let mut put = |code: i32, value: &str| g.push((code, value.to_string()));
    for (c, v) in [(0, "SECTION"), (2, "HEADER"), (9, "$ACADVER"), (1, version), (9, "$HANDSEED"), (5, "FFF"), (0, "ENDSEC")] {
        put(c, v);
    }
    put(0, "SECTION");
    put(2, "CLASSES");
    for (name, cpp, app) in [
        ("MENTALRAYRENDERSETTINGS", "AcDbMentalRayRenderSettings", "SCENEOE"),
        ("RAPIDRTRENDERSETTINGS", "AcDbRapidRTRenderSettings", "ObjectDBX Classes"),
        ("FIELD", "AcDbField", "ObjectDBX Classes"),
    ] {
        for (c, v) in [(0, "CLASS"), (1, name), (2, cpp), (3, app), (90, "1024"), (91, "1"), (280, "0"), (281, "0")] {
            put(c, v);
        }
    }
    put(0, "ENDSEC");
    let mtext = [(0, "MTEXT"), (5, "200"), (100, "AcDbEntity"), (8, "0"), (100, "AcDbMText"), (10, "0"), (20, "0"), (30, "0")];
    for (c, v) in [(0, "SECTION"), (2, "ENTITIES")].into_iter().chain(mtext) {
        put(c, v);
    }
    for (c, v) in [(40, "2.5"), (41, "0"), (71, "1"), (72, "5"), (1, "synthetic"), (0, "ENDSEC")] {
        put(c, v);
    }
    for (c, v) in [(0, "SECTION"), (2, "OBJECTS"), (0, "DICTIONARY"), (5, "C"), (330, "0"), (100, "AcDbDictionary"), (281, "1")] {
        put(c, v);
    }
    for (c, v) in [(3, "ACAD_RENDER_ACTIVE_SETTINGS"), (350, "300"), (3, "ACAD_RENDER_ACTIVE_RAPIDRT_SETTINGS"), (350, "301")] {
        put(c, v);
    }
    put(3, "SYNTHETIC_FIELD");
    put(350, "302");
    // The common AcDbRenderSettings groups; 90 is the class version as a 2013+ DXF stores it.
    let base = |name: &str| {
        let base: Vec<(i32, String)> = [
            (100, "AcDbRenderSettings"),
            (90, "2"),
            (1, name),
            (290, "1"),
            (290, "0"),
            (290, "1"),
            (290, "0"),
            (1, ""),
            (1, "synthetic preset"),
            (90, "7"),
        ]
        .into_iter()
        .map(|(c, v)| (c, v.to_string()))
        .collect();
        base
    };
    g.extend([(0, "MENTALRAYRENDERSETTINGS"), (5, "300"), (330, "C")].map(|(c, v)| (c, v.to_string())));
    g.extend(base("SyntheticMR"));
    g.push((100, "AcDbMentalRayRenderSettings".into()));
    let mental_ray = [
        (90, "2"),
        (90, "-1"),
        (90, "2"),
        (70, "1"),
        (40, "2.0"),
        (40, "2.0"),
        (40, "0.125"),
        (40, "0.25"),
        (40, "0.375"),
        (40, "0.5"),
        (70, "1"),
        (290, "1"),
        (290, "1"),
        (90, "3"),
        (90, "4"),
        (90, "6"),
        (290, "1"),
        (90, "250"),
        (290, "1"),
        (40, "2.5"),
        (90, "5000"),
        (90, "3"),
        (90, "4"),
        (90, "6"),
        (290, "1"),
        (90, "100"),
        (290, "1"),
        (290, "0"),
        (290, "1"),
        (40, "1.5"),
        (40, "0.5"),
        (40, "1200.0"),
        (70, "1"),
        (70, "2"),
        (40, "5.0"),
        (70, "1"),
        (70, "1"),
        (290, "0"),
        (1, "mr note"),
        (90, "16"),
        (70, "2"),
        (90, "2048"),
        (290, "1"),
        (40, "1.25"),
    ];
    g.extend(mental_ray.map(|(c, v)| (c, v.to_string())));
    g.extend([(0, "RAPIDRTRENDERSETTINGS"), (5, "301"), (330, "C")].map(|(c, v)| (c, v.to_string())));
    g.extend(base("SyntheticRT"));
    g.push((100, "AcDbRapidRTRenderSettings".into()));
    let rapid_rt = [(90, "3"), (70, "1"), (90, "4"), (90, "5"), (70, "1"), (70, "2"), (40, "2.5"), (40, "1.5"), (290, "1")];
    g.extend(rapid_rt.map(|(c, v)| (c, v.to_string())));
    let field = [
        (0, "FIELD"),
        (5, "302"),
        (330, "C"),
        (100, "AcDbField"),
        (1, "AcVar"),
        (2, "\\AcVar Filename \\f \"%fn2\""),
        (90, "0"),
        (97, "0"),
        (91, "63"),
        (92, "0"),
        (94, "59"),
        (95, "2"),
        (96, "0"),
        (300, ""),
        (93, "1"),
        (6, "Variable"),
        (93, "0"),
        (90, "4"),
        (1, "Filename"),
        (94, "0"),
        (300, ""),
        (302, ""),
        (304, "ACVALUE_END"),
        (7, "ACFD_FIELD_VALUE"),
        (93, "4"),
        (90, "4"),
        (1, "C:\\synthetic\\Ação-01.dwg"),
        (94, "0"),
        (300, "%fn2"),
        (302, "Ação-01"),
        (304, "ACVALUE_END"),
        (301, "Ação-01"),
        (98, "7"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    g.extend(field.map(|(c, v)| (c, v.to_string())));
    g.iter().fold(String::new(), |mut s, (c, v)| {
        let _ = write!(s, "{c:>3}\r\n{v}\r\n");
        s
    })
}

/// The render settings and the field a document holds (the drawing has one of each).
struct Kept {
    mental_ray: Option<ClassObjectData>,
    rapid_rt: Option<ClassObjectData>,
    field: Option<Field>,
}

fn kept(doc: &acadrust::CadDocument) -> Kept {
    let mut k = Kept { mental_ray: None, rapid_rt: None, field: None };
    for o in doc.objects.values() {
        match o {
            ObjectType::ClassObject(c) if matches!(c.data, ClassObjectData::MentalRayRenderSettings(_)) => k.mental_ray = Some(c.data.clone()),
            ObjectType::ClassObject(c) if matches!(c.data, ClassObjectData::RapidRtRenderSettings(_)) => k.rapid_rt = Some(c.data.clone()),
            ObjectType::Field(f) => k.field = Some(f.clone()),
            _ => {}
        }
    }
    k
}

/// acadrust's version for a CADCraft one.
fn acad(v: Version) -> acadrust::DxfVersion {
    acadrust::DxfVersion::from_version_string(v.code())
}

/// The DXF of `version` saved as a `target` DWG holds the render settings and the field that
/// acadrust's own DXF → DWG conversion keeps.
fn kept_objects_survive(version: &str, target: Version) {
    let dxf = synthetic(version);
    let mut source = acadrust::DxfReader::from_reader(Cursor::new(dxf.clone().into_bytes())).unwrap().read().unwrap();
    let as_read = kept(&source);
    assert!(as_read.mental_ray.is_some() && as_read.rapid_rt.is_some() && as_read.field.is_some(), "the synthetic DXF reads");
    source.version = acad(target);
    let direct = acadrust::DwgWriter::write_to_vec(&source).unwrap();
    let reference = kept(&acadrust::DwgReader::from_stream(Cursor::new(direct)).read().unwrap());
    if target.code() == version {
        assert_eq!(reference.mental_ray, as_read.mental_ray, "the reference DWG keeps the mental ray settings");
        assert_eq!(reference.rapid_rt, as_read.rapid_rt, "the reference DWG keeps the RapidRT settings");
        assert_eq!(reference.field.as_ref().map(|f| &f.value), as_read.field.as_ref().map(|f| &f.value), "the reference DWG keeps the field");
    }

    let d = read(dxf.as_bytes(), "kept.dxf").unwrap();
    let dwg = write_with(&d, "kept.dwg", &SaveOptions { version: Some(target), binary: false }).unwrap();
    let written = acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().unwrap();
    let written = kept(&written);

    // Every difference is listed, so a failure names all the objects that came out wrong.
    let mut wrong = Vec::new();
    if written.mental_ray != reference.mental_ray {
        wrong.push(format!("mental ray render settings: {:?}\n  expected {:?}", written.mental_ray, reference.mental_ray));
    }
    if written.rapid_rt != reference.rapid_rt {
        wrong.push(format!("RapidRT render settings: {:?}\n  expected {:?}", written.rapid_rt, reference.rapid_rt));
    }
    let field = |f: &Option<Field>| {
        f.as_ref().map(|f| (f.evaluator_id.clone(), f.code.clone(), f.value.clone(), f.value_string.clone(), f.child_values.clone()))
    };
    if field(&written.field) != field(&reference.field) {
        wrong.push(format!("field: {:?}\n  expected {:?}", field(&written.field), field(&reference.field)));
    }
    assert!(wrong.is_empty(), "{version} → {target:?}:\n{}", wrong.join("\n"));
}

/// A drawing saved once as an older DWG has that version as its `$ACADVER` (SAVEAS sets it, so a
/// later QSAVE writes it again), but its kept objects are still in the 2018 file's groups. The
/// next save must still read them as 2018: read as the saved version, the mental ray settings
/// came out a class version too high (the ODA refuses the DWG) and a 2004 field lost its cached
/// value.
#[test]
fn kept_groups_keep_their_version_across_saves() {
    let dxf = synthetic("AC1032");
    for (first, second) in [(Version::R2004, Version::R2004), (Version::R2010, Version::R2018), (Version::R2018, Version::R2018)] {
        let mut source = acadrust::DxfReader::from_reader(Cursor::new(dxf.clone().into_bytes())).unwrap().read().unwrap();
        source.version = acad(second);
        let reference = kept(&acadrust::DwgReader::from_stream(Cursor::new(acadrust::DwgWriter::write_to_vec(&source).unwrap())).read().unwrap());

        let mut d = read(dxf.as_bytes(), "kept.dxf").unwrap();
        write_with(&d, "kept.dwg", &SaveOptions { version: Some(first), binary: false }).unwrap();
        // What SAVEAS does to the drawing after writing it.
        d.header.set_str("ACADVER", first.code());
        let dwg = write_with(&d, "kept.dwg", &SaveOptions { version: Some(second), binary: false }).unwrap();
        let written = kept(&acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().unwrap());
        let field = |k: &Kept| k.field.as_ref().map(|f| (f.value.clone(), f.value_string.clone()));
        assert_eq!(written.mental_ray, reference.mental_ray, "{first:?} then {second:?}: mental ray render settings");
        assert_eq!(field(&written), field(&reference), "{first:?} then {second:?}: field");
    }
}

/// A DXF without `$ACADVER` has no version for its kept groups: the drawing's own `$ACADVER`
/// is then the default a new drawing starts with (AC1032), which says nothing about the file, so
/// the kept groups are read as the bridge's R2000, as before.
#[test]
fn a_file_without_acadver_records_no_version() {
    let dxf = synthetic("AC1032").replacen("  9\r\n$ACADVER\r\n  1\r\nAC1032\r\n", "", 1);
    assert!(!dxf.contains("$ACADVER"));
    let d = read(dxf.as_bytes(), "kept.dxf").unwrap();
    assert!(!d.foreign.objects.is_empty(), "the render settings and the field are kept");
    assert_eq!(d.foreign.version, None);
    let with = read(synthetic("AC1032").as_bytes(), "kept.dxf").unwrap();
    assert_eq!(with.foreign.version.as_deref(), Some("AC1032"));
}

#[test]
fn a_2018_files_kept_objects_reach_a_2018_dwg() {
    kept_objects_survive("AC1032", Version::R2018);
}

/// RapidRT render settings are the case whose class version acadrust adjusts only in 2013 files.
#[test]
fn a_2013_files_kept_objects_reach_a_2013_dwg() {
    kept_objects_survive("AC1027", Version::R2013);
}

#[test]
fn a_2018_files_kept_objects_reach_a_2004_dwg() {
    kept_objects_survive("AC1032", Version::R2004);
}

/// A drawing from an R2000 DXF (no newer groups to read differently) saves as before.
#[test]
fn an_r2000_files_drawing_still_saves_as_dwg() {
    let dxf = "  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$ACADVER\r\n  1\r\nAC1015\r\n  0\r\nENDSEC\r\n  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\nLINE\r\n  8\r\n0\r\n 10\r\n0\r\n 20\r\n0\r\n 30\r\n0\r\n 11\r\n10\r\n 21\r\n5\r\n 31\r\n0\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
    let d = read(dxf.as_bytes(), "line.dxf").unwrap();
    let dwg = write_with(&d, "line.dwg", &SaveOptions::default()).unwrap();
    let doc = acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().unwrap();
    assert_eq!(doc.entities().count(), 1);
}
