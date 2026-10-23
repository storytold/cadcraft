//! A dynamic block's visibility state hides a reference's attribute with its definition: the
//! attribute is not drawn, has no grip, is saved hidden (group 60) and read back hidden, and
//! shows again in a state that shows its definition.
//!
//! The fixture is the synthetic DOOR block of `dynblock_read`, with an attribute definition SWING
//! added to it and to its Open state (so the Closed state hides it).

use cadcraft_doc::dynblock::set_property;
use cadcraft_doc::geom::Vec3;
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_io::{read_dxf, write_dxf};

fn dxf(src: &str) -> String {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (code, value) = l.split_once(' ').unwrap_or((l, ""));
            format!("{code}\n{value}\n")
        })
        .collect()
}

/// DOOR with SWING shown in its Open state, and a reference to it at (100, 0) with SWING.
fn door() -> (Drawing, Handle) {
    let attdef = "0 ATTDEF\n5 33\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbText\n10 2\n20 2\n30 0\n40 1\n1 W\n100 AcDbAttributeDefinition\n3 Swing\n2 SWING\n70 0\n";
    let src = include_str!("data/dynblock_door.txt")
        .replace("\r\n", "\n")
        .replace("93 2\n331 31\n331 32\n", "93 3\n331 31\n331 32\n331 33\n")
        .replace("94 2\n332 31\n332 32\n", "94 3\n332 31\n332 32\n332 33\n")
        .replace("31 0\n0 ENDBLK\n5 25\n", &format!("31 0\n{attdef}0 ENDBLK\n5 25\n"));
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let def =
        d.block("Door").unwrap().entities.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None }).unwrap();
    let mut a = def;
    a.text.insert = a.text.insert + Vec3::new(100.0, 0.0, 0.0);
    let ins = cadcraft_doc::Insert {
        block: "Door".into(),
        insert: Vec3::new(100.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![a],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let r = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), EntityKind::Insert(ins)).unwrap();
    (d, r)
}

/// The reference's SWING attribute: whether it is hidden, and how many grips the reference has.
fn swing(d: &Drawing, r: Handle) -> (bool, usize) {
    let e = d.entity(r).unwrap();
    let EntityKind::Insert(i) = &e.kind else { panic!("not a reference") };
    let a = i.attribs.iter().find(|a| a.tag == "SWING").unwrap();
    (a.modes.hidden, e.kind.grips().len())
}

/// The groups of the first ATTRIB tagged `tag` in DXF text `text`.
fn attrib_groups(text: &str, tag: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(String, String)> = lines.chunks(2).filter_map(|c| Some((c.first()?.to_string(), c.get(1)?.to_string()))).collect();
    let mut out = Vec::new();
    let mut inside = false;
    for (code, value) in pairs {
        if code == "0" {
            if inside && out.iter().any(|(c, v): &(String, String)| c == "2" && v == tag) {
                return out;
            }
            inside = value == "ATTRIB";
            out.clear();
        } else if inside {
            out.push((code, value));
        }
    }
    Vec::new()
}

#[test]
fn a_state_that_hides_the_definition_hides_the_attribute() {
    let (mut d, r) = door();
    // Open shows SWING: the insertion grip and the attribute's.
    assert_eq!(swing(&d, r), (false, 2));

    set_property(&mut d, r, "Visibility1", "Closed").unwrap();
    assert_eq!(swing(&d, r), (true, 1), "hidden, and no grip");
    // Saved hidden, read back hidden.
    let text = write_dxf(&d);
    assert!(attrib_groups(&text, "SWING").contains(&("60".to_string(), "1".to_string())));
    let again = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(swing(&again, r), (true, 1));

    // Open again: shown, with its grip, and saved without group 60.
    set_property(&mut d, r, "Visibility1", "Open").unwrap();
    assert_eq!(swing(&d, r), (false, 2));
    let groups = attrib_groups(&write_dxf(&d), "SWING");
    assert!(!groups.is_empty() && !groups.iter().any(|(c, _)| c == "60"), "{groups:?}");
}
