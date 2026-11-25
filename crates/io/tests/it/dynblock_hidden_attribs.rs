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
    let src = include_str!("../data/dynblock_door.txt")
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
        clip: None,
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

/// DOOR with two attribute definitions tagged SWING: the first shown in the Open state, the second
/// in no state. The reference has an attribute for each.
fn door_with_two_swings() -> (Drawing, Handle) {
    let attdef = |handle: &str| {
        format!(
            "0 ATTDEF\n5 {handle}\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbText\n10 2\n20 2\n30 0\n40 1\n1 W\n100 AcDbAttributeDefinition\n3 Swing\n2 SWING\n70 0\n"
        )
    };
    let src = include_str!("../data/dynblock_door.txt")
        .replace("\r\n", "\n")
        .replace("93 2\n331 31\n331 32\n", "93 4\n331 31\n331 32\n331 33\n331 34\n")
        .replace("94 2\n332 31\n332 32\n", "94 3\n332 31\n332 32\n332 33\n")
        .replace("31 0\n0 ENDBLK\n5 25\n", &format!("31 0\n{}{}0 ENDBLK\n5 25\n", attdef("33"), attdef("34")));
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let defs: Vec<_> =
        d.block("Door").unwrap().entities.iter().filter_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None }).collect();
    assert_eq!(defs.len(), 2);
    let attribs = defs
        .into_iter()
        .map(|mut a| {
            a.text.insert = a.text.insert + Vec3::new(100.0, 0.0, 0.0);
            a
        })
        .collect();
    let ins = cadcraft_doc::Insert {
        block: "Door".into(),
        insert: Vec3::new(100.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs,
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    let r = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), EntityKind::Insert(ins)).unwrap();
    (d, r)
}

/// Whether each of the reference's SWING attributes is hidden, in order.
fn swings_hidden(d: &Drawing, r: Handle) -> Vec<bool> {
    let EntityKind::Insert(i) = &d.entity(r).unwrap().kind else { panic!("not a reference") };
    i.attribs.iter().filter(|a| a.tag == "SWING").map(|a| a.modes.hidden).collect()
}

#[test]
fn definitions_sharing_a_tag_each_hide_their_own_attribute() {
    let (mut d, r) = door_with_two_swings();
    set_property(&mut d, r, "Visibility1", "Closed").unwrap();
    assert_eq!(swings_hidden(&d, r), [true, true], "Closed shows neither definition");
    // Open shows the first definition again, not the second: the first attribute shows, the
    // second stays hidden (by tag alone, the second's unchanged state hid the first's change).
    set_property(&mut d, r, "Visibility1", "Open").unwrap();
    assert_eq!(swings_hidden(&d, r), [false, true]);
    set_property(&mut d, r, "Visibility1", "Closed").unwrap();
    assert_eq!(swings_hidden(&d, r), [true, true]);
}

#[test]
fn an_attribute_hidden_on_its_own_stays_hidden_when_another_property_changes() {
    // SWING hidden on its own (group 60 as read) while the Open state shows its definition: a
    // change of Width, which no visibility state follows, leaves it hidden.
    let (mut d, r) = door();
    d.modify_entity(r, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            for a in &mut i.attribs {
                a.modes.hidden = true;
            }
        }
    })
    .unwrap();
    let width = cadcraft_doc::dynblock::custom_properties(&d, r).unwrap().1.into_iter().find(|p| p.name == "Width").unwrap().value;
    let wider = format!("{}", width.parse::<f64>().unwrap() + 2.0);
    set_property(&mut d, r, "Width", &wider).unwrap();
    assert!(swing(&d, r).0, "still hidden after Width {width} → {wider}");
    assert!(attrib_groups(&write_dxf(&d), "SWING").contains(&("60".to_string(), "1".to_string())));
}
