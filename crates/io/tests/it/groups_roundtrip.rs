//! Groups (#417) survive a DWG round trip through the DXF bridge (the DXF round trip is in
//! `tables_objects_roundtrip`).

use cadcraft_doc::{Common, Drawing, EntityKind, Group, Line, Space};
use cadcraft_geom::Vec3;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn groups_survive_dwg_round_trip() {
    let mut d = Drawing::new_imperial();
    let mut line = |x: f64| {
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x, 10.0, 0.0) })).expect("line")
    };
    let (a, b) = (line(0.0), line(5.0));
    d.groups.push(Group { name: "Doors".into(), description: "front".into(), selectable: false, members: vec![b, a] });
    d.groups.push(Group { name: "*A1".into(), description: String::new(), selectable: true, members: vec![a] });
    let bytes = cadcraft_io::write(&d, "groups.dwg").expect("write dwg");
    let r = cadcraft_io::read(&bytes, "groups.dwg").expect("read dwg");
    let got: Vec<(&str, &str, bool, Vec<f64>)> = r
        .groups
        .iter()
        .map(|g| {
            let xs = g.members.iter().filter_map(|h| match &r.entity(*h)?.kind {
                EntityKind::Line(l) => Some(l.a.x),
                _ => None,
            });
            (g.name.as_str(), g.description.as_str(), g.selectable, xs.collect())
        })
        .collect();
    assert_eq!(got, vec![("Doors", "front", false, vec![5.0, 0.0]), ("*A1", "", true, vec![0.0])]);
}

/// Drawings can carry hundreds of thousands of empty anonymous groups (#862). Reading them stays
/// linear: every group arrives, and a name repeated in another case still keeps one group, the
/// later definition.
#[test]
fn many_anonymous_groups_read_and_names_merge_case_insensitively() {
    const N: usize = 20_000;
    let mut dict = String::from("0\nDICTIONARY\n5\nC\n3\nACAD_GROUP\n350\nD\n0\nDICTIONARY\n5\nD\n330\nC\n");
    let mut groups = String::new();
    for i in 1..=N {
        let h = format!("{:X}", 0x1000 + i);
        dict.push_str(&format!("3\n*A{i}\n350\n{h}\n"));
        groups.push_str(&format!("0\nGROUP\n5\n{h}\n330\nD\n300\n\n70\n1\n71\n1\n"));
    }
    for (name, h, text) in [("Doors", "100", "first"), ("DOORS", "101", "second")] {
        dict.push_str(&format!("3\n{name}\n350\n{h}\n"));
        groups.push_str(&format!("0\nGROUP\n5\n{h}\n330\nD\n300\n{text}\n70\n0\n71\n1\n340\n2A\n"));
    }
    let dxf = format!(
        "0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n2A\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n\
         0\nSECTION\n2\nOBJECTS\n{dict}{groups}0\nENDSEC\n0\nEOF\n"
    );
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).expect("read");
    assert_eq!(d.groups.len(), N + 1);
    assert!(d.groups.iter().take(N).enumerate().all(|(i, g)| g.name == format!("*A{}", i + 1) && g.members.is_empty()));
    let doors: Vec<(&str, &str, usize)> = d
        .groups
        .iter()
        .filter(|g| g.name.eq_ignore_ascii_case("doors"))
        .map(|g| (g.name.as_str(), g.description.as_str(), g.members.len()))
        .collect();
    assert_eq!(doors, [("DOORS", "second", 1)]);
}
