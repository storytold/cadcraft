//! An object of another application whose owner is null, but which another kept object holds by a
//! hard owner reference (360), is kept with it, as one application's proxy object held by another.
//! In DXF and in DWG, save after save.

use cadcraft_io::{read, read_dxf, write, write_dxf};

/// A named object dictionary entry `OTHER_APP` (a dictionary) with an XRECORD that holds a second
/// XRECORD by a 360 reference; the second one's owner is null. An object nothing keeps names the
/// second XRECORD by a 360 first.
fn source() -> String {
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
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "OBJECTS"),
        (0, "DICTIONARY"),
        (5, "C"),
        (330, "0"),
        (100, "AcDbDictionary"),
        (3, "OTHER_APP"),
        (350, "50"),
        // An object nothing keeps (null owner, no holder) that also names the held record by a
        // 360, before the holder does: it is dropped, and the held record stays with the holder.
        (0, "XRECORD"),
        (5, "60"),
        (330, "0"),
        (100, "AcDbXrecord"),
        (280, "1"),
        (1, "stray"),
        (360, "52"),
        (0, "DICTIONARY"),
        (5, "50"),
        (330, "C"),
        (100, "AcDbDictionary"),
        (281, "1"),
        (3, "HOLDER"),
        (360, "51"),
        (0, "XRECORD"),
        (5, "51"),
        (102, "{ACAD_REACTORS"),
        (330, "50"),
        (102, "}"),
        (330, "50"),
        (100, "AcDbXrecord"),
        (280, "1"),
        (1, "holder"),
        (360, "52"),
        (0, "XRECORD"),
        (5, "52"),
        (102, "{ACAD_REACTORS"),
        (330, "51"),
        (102, "}"),
        (330, "0"),
        (100, "AcDbXrecord"),
        (280, "1"),
        (1, "held"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    groups.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

fn check(text: &str, label: &str) {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|c| Some((*c.first()?, *c.get(1)?))).collect();
    // Each record from its `0` group on, as (code, value) pairs.
    let records: Vec<&[(&str, &str)]> = pairs
        .iter()
        .enumerate()
        .filter(|(_, (c, _))| *c == "0")
        .map(|(i, _)| {
            let n = pairs[i + 1..].iter().position(|(c, _)| *c == "0").map_or(pairs.len(), |n| i + 1 + n);
            &pairs[i..n]
        })
        .collect();
    let with = |text: &str| records.iter().find(|r| r.contains(&("1", text))).copied().unwrap_or_else(|| panic!("{label}: no record {text}"));
    let holder = with("holder");
    let Some((_, held_handle)) = holder.iter().find(|(c, _)| *c == "360") else { panic!("{label}: the holder holds nothing: {holder:?}") };
    let held = with("held");
    assert!(held.contains(&("5", *held_handle)), "{label}: the holder's 360 names the held record: {holder:?} {held:?}");
    assert!(!records.iter().any(|r| r.contains(&("1", "stray"))), "{label}: the object nothing keeps is dropped");
}

#[test]
fn an_object_held_by_a_kept_one_is_kept() {
    let mut d = read_dxf(source().as_bytes()).expect("read");
    for generation in 1..=2 {
        let text = write_dxf(&d);
        check(&text, &format!("DXF {generation}"));
        d = read_dxf(text.as_bytes()).expect("reread");
    }
    for generation in 1..=2 {
        let dwg = write(&d, "held.dwg").expect("DWG");
        d = read(&dwg, "held.dwg").expect("DWG read");
        check(&write_dxf(&d), &format!("DWG {generation}"));
    }
}
