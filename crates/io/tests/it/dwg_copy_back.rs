//! Objects CADCraft didn't change are saved as the DWG had them (`dwg_check::restore_unchanged`):
//! what the bridge (acadrust) can't carry survives open and save, and what CADCraft changed is
//! saved as CADCraft wrote it.

use cadcraft_doc::{AssocSnap, DimAssoc, DimKind, Dimension, Drawing, EntityKind, Line, Space};
use cadcraft_dwg::codec::{edit, read_container, type_name};
use cadcraft_geom::Vec3;

/// The handle of the first object of type `kind`.
fn first_of(bytes: &[u8], kind: &str) -> u64 {
    let c = read_container(bytes).unwrap();
    c.objects_iter().find_map(|(r, o)| (type_name(o.ok()?.type_code, &c.classes).0 == kind).then_some(r.handle)).unwrap()
}

/// The references of the object with handle `h`.
fn refs_of(bytes: &[u8], h: u64) -> Vec<u64> {
    let c = read_container(bytes).unwrap();
    let r = c.map.iter().find(|r| r.handle == h).unwrap();
    edit::handle_refs(&c.object_at(r.offset).unwrap(), c.version).unwrap()
}

/// A DWG whose dimension style names the UCS table as its text style, as some AutoCAD files do:
/// a drawing CADCraft saved, with that one reference rewritten.
fn odd_dimstyle_dwg(version: cadcraft_dxf::Version) -> (Vec<u8>, u64, u64) {
    let d = Drawing::new_imperial();
    let opts = cadcraft_io::SaveOptions { version: Some(version), binary: false };
    let bytes = cadcraft_io::write_with(&d, "x.dwg", &opts).unwrap();
    let (dimstyle, style, ucs) = (first_of(&bytes, "DIMSTYLE"), first_of(&bytes, "STYLE"), first_of(&bytes, "UCS_CONTROL"));
    let c = read_container(&bytes).unwrap();
    let r = c.map.iter().find(|r| r.handle == dimstyle).unwrap();
    let o = c.object_at(r.offset).unwrap();
    let at = edit::handle_refs(&o, c.version).unwrap().iter().position(|h| *h == style).unwrap();
    let stored = edit::with_reference(&o, c.version, at, ucs).unwrap();
    let file = edit::replace_objects(&bytes, &[(dimstyle, stored)].into_iter().collect()).unwrap();
    (file, dimstyle, ucs)
}

/// The bridge reads the reference and writes the standard text style instead; the object comes
/// back as the file had it, in every version the container writer rewrites.
#[test]
fn an_unchanged_dimension_style_keeps_a_reference_the_bridge_changes() {
    for version in [cadcraft_dxf::Version::R2004, cadcraft_dxf::Version::R2010, cadcraft_dxf::Version::R2013, cadcraft_dxf::Version::R2018] {
        let (file, dimstyle, ucs) = odd_dimstyle_dwg(version);
        assert!(refs_of(&file, dimstyle).contains(&ucs), "{version:?}: the test file");
        let d = cadcraft_io::read(&file, "x.dwg").unwrap();
        let saved = cadcraft_io::write(&d, "x.dwg").unwrap();
        assert_eq!(refs_of(&saved, dimstyle), refs_of(&file, dimstyle), "{version:?}: saved");
        // And the drawing it reads back is the one that was opened.
        let back = cadcraft_io::read(&saved, "x.dwg").unwrap();
        assert_eq!(back.dim_styles, d.dim_styles, "{version:?}");
    }
}

/// A dimension style CADCraft changed is saved as CADCraft writes it, not as the file had it.
#[test]
fn a_changed_dimension_style_is_saved_as_changed() {
    let (file, dimstyle, ucs) = odd_dimstyle_dwg(cadcraft_dxf::Version::R2018);
    let mut d = cadcraft_io::read(&file, "x.dwg").unwrap();
    let st = d.dim_styles.first_mut().unwrap();
    st.text_height = 0.5;
    let saved = cadcraft_io::write(&d, "x.dwg").unwrap();
    assert!(!refs_of(&saved, dimstyle).contains(&ucs), "the bridge's object");
    let back = cadcraft_io::read(&saved, "x.dwg").unwrap();
    assert_eq!(back.dim_styles.first().map(|s| s.text_height), Some(0.5));
}

/// A drawing that wasn't read from a DWG, or was read from one of another version, is saved as
/// the bridge writes it.
#[test]
fn a_dwg_of_another_version_is_not_copied_from() {
    let (file, dimstyle, ucs) = odd_dimstyle_dwg(cadcraft_dxf::Version::R2018);
    let d = cadcraft_io::read(&file, "x.dwg").unwrap();
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_dxf::Version::R2013), binary: false };
    let saved = cadcraft_io::write_with(&d, "x.dwg", &opts).unwrap();
    assert!(!refs_of(&saved, dimstyle).contains(&ucs));
}

/// A DWG with a line and a dimension associated with it, opened again: deleting the line, the
/// dimension or both and saving leaves no reference to what was deleted, though the model space
/// block's entity list and the associativity object named them when the file was opened (an
/// object is copied back as the file had it only when the new file has everything it names).
#[test]
fn deleted_entities_leave_no_references_behind() {
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
    let file = cadcraft_io::write(&d, "x.dwg").unwrap();
    assert!(super::dwg_corpus_content::dangling(&file).is_empty(), "the file itself");
    let c = read_container(&file).unwrap();
    let naming = |h: u64| -> Vec<String> {
        c.objects_iter()
            .filter_map(|(r, o)| {
                let o = o.ok()?;
                (r.handle != h && edit::handle_refs(&o, c.version).ok()?.contains(&h)).then(|| type_name(o.type_code, &c.classes).0)
            })
            .collect()
    };
    // The block's entity list (DWG only: CADCraft's DXF record of the block doesn't change) and
    // the associativity object name both.
    for h in [line.0, dh.0] {
        assert!(naming(h).iter().any(|n| n == "BLOCK_HEADER") && naming(h).iter().any(|n| n == "DIMASSOC"), "{h:X}: {:?}", naming(h));
    }
    let opened = cadcraft_io::read(&file, "x.dwg").unwrap();
    for gone in [vec![line], vec![dh], vec![line, dh]] {
        let mut d = opened.clone();
        for h in &gone {
            d.remove_entity(*h);
        }
        let saved = cadcraft_io::write(&d, "x.dwg").unwrap();
        let left = super::dwg_corpus_content::dangling(&saved);
        assert!(left.is_empty(), "deleting {gone:X?}: {left:X?}");
        assert_eq!(cadcraft_io::read(&saved, "x.dwg").unwrap().entity_count(), d.entity_count());
    }
}
