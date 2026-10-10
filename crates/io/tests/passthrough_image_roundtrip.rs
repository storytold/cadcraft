//! Raster images survive DXF save and reopen: the IMAGE entity's placement, clip boundary and
//! display settings, and its IMAGEDEF (file path and name), written with the IMAGEDEF_REACTOR,
//! RASTERVARIABLES and dictionaries the DXF Reference describes.

use cadcraft_doc::{Drawing, EntityKind, Image};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::Vec2;

/// A small DXF as another program writes it: a line, an image on layer `Pics` pointing at an
/// IMAGEDEF named `logo`, and an MLINE (a type CADCraft doesn't model).
fn foreign_dxf() -> String {
    let groups: &[(i32, &str)] = &[
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "LINE"),
        (5, "30"),
        (8, "0"),
        (10, "0"),
        (20, "0"),
        (11, "10"),
        (21, "0"),
        (0, "IMAGE"),
        (5, "31"),
        (100, "AcDbEntity"),
        (8, "Pics"),
        (100, "AcDbRasterImage"),
        (90, "0"),
        (10, "5"),
        (20, "5"),
        (30, "0"),
        (11, "0.01"),
        (21, "0"),
        (31, "0"),
        (12, "0"),
        (22, "0.01"),
        (32, "0"),
        (13, "640"),
        (23, "480"),
        (340, "40"),
        (70, "15"),
        (280, "1"),
        (281, "60"),
        (282, "40"),
        (283, "10"),
        (360, "42"),
        (71, "2"),
        (91, "4"),
        (14, "-0.5"),
        (24, "-0.5"),
        (14, "639.5"),
        (24, "-0.5"),
        (14, "320"),
        (24, "479.5"),
        (14, "-0.5"),
        (24, "-0.5"),
        (0, "MLINE"),
        (5, "32"),
        (100, "AcDbEntity"),
        (8, "0"),
        (100, "AcDbMline"),
        (2, "STANDARD"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "OBJECTS"),
        (0, "DICTIONARY"),
        (5, "C"),
        (330, "0"),
        (100, "AcDbDictionary"),
        (3, "ACAD_IMAGE_DICT"),
        (350, "41"),
        (0, "DICTIONARY"),
        (5, "41"),
        (330, "C"),
        (100, "AcDbDictionary"),
        (3, "logo"),
        (350, "40"),
        (0, "IMAGEDEF"),
        (5, "40"),
        (102, "{ACAD_REACTORS"),
        (330, "41"),
        (330, "42"),
        (102, "}"),
        (330, "41"),
        (100, "AcDbRasterImageDef"),
        (90, "0"),
        (1, "images/logo.png"),
        (10, "640"),
        (20, "480"),
        (11, "0.25"),
        (21, "0.25"),
        (280, "1"),
        (281, "5"),
        (0, "IMAGEDEF_REACTOR"),
        (5, "42"),
        (330, "31"),
        (100, "AcDbRasterImageDefReactor"),
        (90, "2"),
        (330, "31"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    groups.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

fn images(d: &Drawing) -> Vec<Image> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Image(i) => Some(i.clone()),
            _ => None,
        })
        .collect()
}

/// The records of a section of a DXF text.
fn section(dxf: &str, name: &str) -> Vec<(String, Vec<Tag>)> {
    let tags = cadcraft_dxf::parse(dxf.as_bytes()).unwrap();
    let s = sections(&tags).into_iter().find(|s| s.name == name).unwrap();
    records(&s.tags)
}

fn val(tags: &[Tag], code: i32) -> String {
    tags.iter().find(|t| t.code == code).map(Tag::str).unwrap_or_default()
}

#[test]
fn image_and_its_definition_survive_save_and_reopen() {
    let d = cadcraft_io::read_dxf(foreign_dxf().as_bytes()).unwrap();
    let [img] = images(&d).try_into().unwrap();
    assert_eq!((img.path.as_str(), img.name.as_str()), ("images/logo.png", "logo"));
    assert_eq!(img.size, Vec2::new(640.0, 480.0));
    assert_eq!(img.clip.len(), 4);
    assert!(img.clipping);
    assert_eq!((img.display, img.brightness, img.contrast, img.fade), (15, 60, 40, 10));
    assert_eq!((img.pixel_size, img.resolution_units), (Vec2::new(0.25, 0.25), 5));

    let saved = cadcraft_io::write_dxf(&d);
    let back = cadcraft_io::read_dxf(saved.as_bytes()).unwrap();
    assert_eq!(images(&back), vec![img.clone()]);
    let e = back.model.iter().find(|e| matches!(e.kind, EntityKind::Image(_))).unwrap();
    assert_eq!(e.common.layer, "Pics");

    // The saved objects link up as the DXF Reference describes.
    let ents = section(&saved, "ENTITIES");
    let (_, it) = ents.iter().find(|(k, _)| k == "IMAGE").unwrap();
    let objs = section(&saved, "OBJECTS");
    let find = |kind: &str| objs.iter().filter(|(k, _)| k == kind).map(|(_, t)| t.clone()).collect::<Vec<_>>();
    let [def] = find("IMAGEDEF").try_into().unwrap();
    let [reactor] = find("IMAGEDEF_REACTOR").try_into().unwrap();
    assert_eq!(find("RASTERVARIABLES").len(), 1);
    assert_eq!(val(it, 340), val(&def, 5));
    assert_eq!(val(it, 360), val(&reactor, 5));
    assert_eq!(val(&reactor, 330), val(it, 5));
    assert!(def.iter().any(|t| t.code == 330 && t.str() == val(&reactor, 5)), "the IMAGEDEF lists its reactor");
    assert_eq!(val(&def, 1), "images/logo.png");
    let dicts = find("DICTIONARY");
    assert!(dicts.iter().any(|t| t.iter().any(|g| g.code == 3 && g.str() == "ACAD_IMAGE_DICT")));
    assert!(dicts.iter().any(|t| t.iter().any(|g| g.code == 3 && g.str() == "logo") && t.iter().any(|g| g.code == 350 && g.str() == val(&def, 5))));
    let classes = section(&saved, "CLASSES");
    for c in ["IMAGE", "IMAGEDEF", "IMAGEDEF_REACTOR", "RASTERVARIABLES"] {
        assert!(classes.iter().any(|(_, t)| val(t, 1) == c), "CLASS {c}");
    }
    // Saving again is stable.
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&back).as_bytes()).unwrap();
    assert_eq!(images(&again), vec![img]);
}

#[test]
fn images_of_one_file_share_a_definition() {
    let mut d = cadcraft_io::read_dxf(foreign_dxf().as_bytes()).unwrap();
    let e = d.model.iter().find(|e| matches!(e.kind, EntityKind::Image(_))).unwrap().as_ref().clone();
    d.add(&cadcraft_doc::Space::Model, e.common.clone(), e.kind.clone()).unwrap();
    let saved = cadcraft_io::write_dxf(&d);
    let objs = section(&saved, "OBJECTS");
    assert_eq!(objs.iter().filter(|(k, _)| k == "IMAGEDEF").count(), 1);
    assert_eq!(objs.iter().filter(|(k, _)| k == "IMAGEDEF_REACTOR").count(), 2);
    let back = cadcraft_io::read_dxf(saved.as_bytes()).unwrap();
    assert!(images(&back).iter().all(|i| i.path == "images/logo.png" && i.name == "logo"));
    assert_eq!(images(&back).len(), 2);
}

#[test]
fn image_without_definition_is_still_written() {
    // An image whose IMAGEDEF is missing (or an empty path) keeps its placement.
    let text = foreign_dxf().replace("340\n40\n", "340\n99\n");
    let d = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let [img] = images(&d).try_into().unwrap();
    assert_eq!(img.path, "");
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let [b] = images(&back).try_into().unwrap();
    assert_eq!((b.insert, b.size, b.clip.len()), (img.insert, img.size, 4));
    assert_eq!(b.name, "Image");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn image_survives_dwg_save_and_reopen() {
    let d = cadcraft_io::read_dxf(foreign_dxf().as_bytes()).unwrap();
    let [img] = images(&d).try_into().unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let [b] = images(&back).try_into().unwrap();
    // The DWG bridge keeps everything but the definition's resolution unit.
    assert_eq!(Image { resolution_units: img.resolution_units, ..b }, img);
}
