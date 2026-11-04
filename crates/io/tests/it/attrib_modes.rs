//! Attribute modes beyond invisible and constant survive a save and reopen: Verify and Preset
//! (flags 4 and 8) and Lock position (group 280 after the tag, AutoCAD 2007 and later).

use std::sync::Arc;

use cadcraft_doc::*;
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read_dxf, write_dxf, write_with};

fn attrib(tag: &str, modes: AttribModes) -> Attrib {
    let text = Text {
        insert: Vec3::ZERO,
        align_pt: None,
        height: 1.0,
        value: tag.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    };
    Attrib { tag: tag.into(), text, invisible: false, constant: false, prompt: String::new(), props: Default::default(), multiline: None, modes }
}

const ALL: AttribModes = AttribModes { verify: true, preset: true, lock_position: true };
const LOCKED: AttribModes = AttribModes { verify: false, preset: false, lock_position: true };

/// Block TAG with definitions A (every mode) and B (none), and a reference whose A is locked.
fn drawing() -> Drawing {
    drawing_c(false)
}

fn drawing_c(constant: bool) -> Drawing {
    let mut d = Drawing::new_metric();
    let mut b = Block::new("TAG");
    for a in [attrib("A", ALL), attrib("B", AttribModes::default())] {
        let h = d.new_handle();
        b.entities.push(Entity::new(h, EntityKind::AttDef(a)));
    }
    d.blocks.insert("TAG".into(), Arc::new(b));
    let ins = Insert {
        block: "TAG".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![Attrib { constant, ..attrib("A", LOCKED) }, attrib("B", AttribModes::default())],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).expect("add");
    d
}

/// The modes of the definitions and of the reference's attributes.
fn modes(d: &Drawing) -> (Vec<AttribModes>, Vec<AttribModes>) {
    let defs = d
        .block("TAG")
        .expect("block")
        .entities
        .iter()
        .filter_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.modes) } else { None })
        .collect();
    let refs =
        d.model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.attribs.iter().map(|a| a.modes).collect()) } else { None });
    (defs, refs.unwrap_or_default())
}

fn pairs(bytes: &[u8]) -> Vec<(i32, String)> {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    lines.chunks(2).filter_map(|c| Some((c.first()?.trim().parse().ok()?, c.get(1)?.trim().to_string()))).collect()
}

#[test]
fn modes_survive_dxf_and_dwg() {
    let d = drawing();
    let want = (vec![ALL, AttribModes::default()], vec![LOCKED, AttribModes::default()]);
    assert_eq!(modes(&d), want);
    // DXF 2018 (the default): AutoCAD's layout, subclass version first and Lock position after
    // the flags.
    let dxf = cadcraft_io::write(&d, "x.dxf").expect("dxf");
    let p = pairs(&dxf);
    let at = p.iter().position(|(c, v)| *c == 100 && v == "AcDbAttributeDefinition").expect("ATTDEF");
    let tail: Vec<(i32, String)> = p[at + 1..].iter().take_while(|(c, _)| *c != 0 && *c != 1001).cloned().collect();
    let s = |v: &[(i32, &str)]| v.iter().map(|(c, x)| (*c, x.to_string())).collect::<Vec<_>>();
    assert_eq!(tail, s(&[(280, "0"), (3, ""), (2, "A"), (70, "12"), (280, "1")]));
    assert_eq!(modes(&read_dxf(&String::from_utf8_lossy(&dxf).into_owned().into_bytes()).expect("read")), want);
    let dwg = cadcraft_io::write(&d, "x.dwg").expect("dwg");
    assert_eq!(modes(&cadcraft_io::read(&dwg, "x.dwg").expect("read dwg")), want, "dwg");
    // DXF 2000 has no Lock position; the flags stay.
    let old = modes(&read_dxf(write_dxf(&d).as_bytes()).expect("read"));
    let unlocked = AttribModes { lock_position: false, ..ALL };
    assert_eq!(old, (vec![unlocked, AttribModes::default()], vec![AttribModes::default(), AttribModes::default()]));
}

#[test]
fn a_280_before_the_tag_is_the_version_not_lock_position() {
    let mut p = pairs(&cadcraft_io::write(&drawing(), "x.dxf").expect("dxf"));
    // A's subclass version made 1 and its Lock position cleared: nothing is locked.
    let at = p.iter().position(|(c, v)| *c == 100 && v == "AcDbAttributeDefinition").expect("ATTDEF");
    let lock = at + p[at..].iter().position(|(c, v)| *c == 70 && v == "12").expect("flags") + 1;
    assert_eq!((p[at + 1].clone(), p[lock].clone()), ((280, "0".to_string()), (280, "1".to_string())));
    p[at + 1].1 = "1".into();
    p[lock].1 = "0".into();
    let text: String = p
        .iter()
        .map(|(c, v)| {
            format!(
                "{c}
{v}
"
            )
        })
        .collect();
    assert!(!modes(&read_dxf(text.as_bytes()).expect("read")).0[0].lock_position);
}

/// The Constant mode (flag 2) of an ATTRIB attached to an INSERT, not only of an ATTDEF (#2246).
#[test]
fn constant_attached_attrib_survives_dxf_and_dwg() {
    let d = drawing_c(true);
    let constants = |d: &Drawing| -> Vec<bool> {
        d.model
            .iter()
            .find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.attribs.iter().map(|a| a.constant).collect()) } else { None })
            .unwrap_or_default()
    };
    assert_eq!(constants(&d), [true, false]);
    let dxf = cadcraft_io::write(&d, "x.dxf").expect("dxf");
    assert_eq!(constants(&cadcraft_io::read(&dxf, "x.dxf").expect("read dxf")), [true, false], "dxf");
    let dwg = cadcraft_io::write(&d, "x.dwg").expect("dwg");
    assert_eq!(constants(&cadcraft_io::read(&dwg, "x.dwg").expect("read dwg")), [true, false], "dwg");
    // Every save version, in both DXF encodings and in DWG.
    for version in Version::ALL {
        for (name, binary) in [("x.dxf", false), ("x.dxf", true), ("x.dwg", false)] {
            let bytes = write_with(&d, name, &SaveOptions { version: Some(version), binary }).expect("write");
            assert_eq!(constants(&cadcraft_io::read(&bytes, name).expect("read")), [true, false], "{version:?} {name} binary={binary}");
        }
    }
}
