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
