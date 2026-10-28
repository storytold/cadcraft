//! Persistent reactors survive DWG. The bridge (acadrust) drops two kinds: on save, the reactor
//! that ties a dictionary's entries to their dictionary (scales, multileader and table styles…);
//! on open, a table record's reactors (a dimension style's reactors to its dimensions). CADCraft's
//! own DWG reader puts both back (`dwg_check`).

use cadcraft_doc::foreign::ForeignExtra;
use cadcraft_doc::{AssocSnap, DimAssoc, DimKind, Dimension, Drawing, EntityKind, Handle, Line, Space};
use cadcraft_dwg::codec::{DwgContainer, edit, read_container, type_name};
use cadcraft_geom::Vec3;

/// The reactors of every object of type `kind`, by handle.
fn reactors_of(c: &DwgContainer, kind: &str) -> Vec<(u64, Vec<u64>)> {
    c.objects_iter()
        .filter_map(|(r, o)| {
            let o = o.ok()?;
            let (name, entity) = type_name(o.type_code, &c.classes);
            (name == kind).then(|| (r.handle, edit::reactors(&o, c.version, entity).unwrap_or_default()))
        })
        .collect()
}

/// The handle of the dictionary that lists `entry` as an entry, from its owner field.
fn owner_of(c: &DwgContainer, entry: u64) -> Option<u64> {
    let r = c.map.iter().find(|r| r.handle == entry)?;
    let o = c.object_at(r.offset).ok()?;
    let (_, entity) = type_name(o.type_code, &c.classes);
    o.common(c.version, entity).ok()?.owner
}

/// A drawing saved as DWG keeps the reactor from each style and scale object to the dictionary
/// that lists it, in every version the container writer rewrites.
#[test]
fn dictionary_entries_keep_their_reactor_to_the_dictionary() {
    let d = Drawing::new_imperial();
    for version in [cadcraft_dxf::Version::R2004, cadcraft_dxf::Version::R2010, cadcraft_dxf::Version::R2013, cadcraft_dxf::Version::R2018] {
        let opts = cadcraft_io::SaveOptions { version: Some(version), binary: false };
        let bytes = cadcraft_io::write_with(&d, "x.dwg", &opts).unwrap();
        let c = read_container(&bytes).unwrap();
        for kind in ["MLEADERSTYLE", "TABLESTYLE", "SCALE"] {
            let objects = reactors_of(&c, kind);
            assert!(!objects.is_empty(), "{version:?}: no {kind}");
            for (h, reactors) in objects {
                let dict = owner_of(&c, h).unwrap();
                assert_eq!(reactors, vec![dict], "{version:?}: {kind} {h:X} reactors");
            }
        }
        // The rewritten file is one the bridge reads too.
        cadcraft_dwg::dwg_to_dxf(&bytes).unwrap();
    }
}

/// A dimension style's reactors to its dimensions go out in the DWG and come back when it is
/// opened, though the bridge leaves them out of what it reads.
#[test]
fn a_dimension_styles_reactors_survive_a_dwg_round_trip() {
    let mut d = Drawing::new_imperial();
    let line = d.add(&Space::Model, Default::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) })).unwrap();
    let dim = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        defpt: Vec3::new(10.0, 2.0, 0.0),
        style: "Standard".into(),
        assoc: vec![DimAssoc { read: Default::default(), point: "p13".into(), handle: line, snap: AssocSnap::Start }],
        ..serde_json::from_value(serde_json::json!({"kind": {"linear": {"rotation": 0.0}}, "defpt": [0, 0, 0], "textMid": [0, 0, 0]})).unwrap()
    };
    let dh = d.add(&Space::Model, Default::default(), EntityKind::Dimension(dim)).unwrap();
    let mut f = (*d.foreign).clone();
    f.records.insert(("DIMSTYLE".into(), "STANDARD".into()), ForeignExtra { reactors: vec![dh], ..ForeignExtra::default() });
    d.foreign = std::sync::Arc::new(f);
    let bytes = cadcraft_io::write(&d, "x.dwg").unwrap();
    let c = read_container(&bytes).unwrap();
    assert_eq!(reactors_of(&c, "DIMSTYLE").first().map(|(_, r)| r.clone()), Some(vec![dh.0]), "written");
    let back = cadcraft_io::read(&bytes, "x.dwg").unwrap();
    let kept = back.foreign.records.get(&("DIMSTYLE".to_string(), "STANDARD".to_string())).map(|x| x.reactors.clone());
    assert_eq!(kept, Some(vec![Handle(dh.0)]), "read back");
}
