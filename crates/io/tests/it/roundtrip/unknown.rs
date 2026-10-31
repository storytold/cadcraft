//! Unknown-entity preservation is payload- and reference-aware: the entity's handle, Common
//! fields, model-space membership and every application payload tag must survive. Transport
//! owner handles may be reallocated, but the DXF owner must still resolve to model space.
//! DWG may explicitly refuse an unregistered class; a successful save must not silently drop it.

use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Line, RawTag, Unknown};
use cadcraft_dxf::{records, sections};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_with};

fn drawing() -> Drawing {
    let mut d = super::cases::drawing();
    let handle = d.new_handle();
    let target = d.new_handle();
    let tags = vec![
        RawTag { code: 5, value: handle.hex() },
        // The store supplies the owner when saved, as it does for every modeled entity.
        RawTag { code: 330, value: "0".into() },
        RawTag { code: 100, value: "AcDbEntity".into() },
        RawTag { code: 8, value: "0".into() },
        RawTag { code: 100, value: "SyntheticWidget".into() },
        RawTag { code: 1, value: "Original Ø payload".into() },
        RawTag { code: 90, value: "-7".into() },
        RawTag { code: 310, value: "00FF80".into() },
        RawTag { code: 10, value: "1.25".into() },
        RawTag { code: 20, value: "-2.5".into() },
        RawTag { code: 30, value: "3.75".into() },
        RawTag { code: 330, value: target.hex() },
    ];
    d.model.push(Entity::new(handle, EntityKind::Unknown(Unknown { dxf_type: "SYNTHETIC_WIDGET".into(), tags })));
    d.model.push(Entity::new(target, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(5.0, 6.0, 7.0) })));
    d
}

fn payload(tags: &[RawTag]) -> &[RawTag] {
    let start = tags.iter().position(|t| t.code == 100 && t.value == "SyntheticWidget").expect("application subclass must survive");
    &tags[start..]
}

fn preserved(original: &Drawing, back: &Drawing, context: &str) {
    assert!(back.read_notes.is_empty(), "{context}: {:?}", back.read_notes);
    assert!(back.skipped_records.is_empty(), "{context}: {:?}", back.skipped_records);
    assert_eq!(back.model.len(), original.model.len(), "{context}: entity count");
    for (a, b) in original.model.iter().zip(back.model.iter()) {
        assert_eq!(b.handle, a.handle, "{context}: handle");
        assert_eq!(b.common, a.common, "{context}: Common");
        match (&a.kind, &b.kind) {
            (EntityKind::Unknown(a), EntityKind::Unknown(b)) => {
                assert_eq!(b.dxf_type, a.dxf_type, "{context}: application entity type");
                assert_eq!(payload(&b.tags), payload(&a.tags), "{context}: full payload including handle reference");
            }
            _ => assert_eq!(b.kind, a.kind, "{context}: geometry"),
        }
    }
}

fn dxf_owner(bytes: &[u8]) {
    let tags = cadcraft_dxf::parse(bytes).unwrap();
    let sections = sections(&tags);
    let tables = records(&sections.iter().find(|s| s.name == "TABLES").unwrap().tags);
    let (_, model) = tables
        .iter()
        .find(|(kind, tags)| kind == "BLOCK_RECORD" && tags.iter().any(|t| t.code == 2 && t.str().eq_ignore_ascii_case("*Model_Space")))
        .unwrap();
    let model_handle = model.iter().find(|t| t.code == 5).unwrap().str();
    let entities = records(&sections.iter().find(|s| s.name == "ENTITIES").unwrap().tags);
    let (_, widget) = entities.iter().find(|(kind, _)| kind == "SYNTHETIC_WIDGET").unwrap();
    assert_eq!(widget.iter().find(|t| t.code == 330).unwrap().str(), model_handle, "unknown entity belongs to the model-space block");
}

pub(super) fn check(path: &str, version: Version, binary: bool) {
    let original = drawing();
    let mut current = original.clone();
    for pass in 1..=2 {
        let context = format!("unknown entity {path} {version} binary={binary} pass={pass}");
        let bytes = match write_with(&current, path, &SaveOptions { version: Some(version), binary }) {
            Ok(bytes) => bytes,
            Err(_) if path.ends_with(".dwg") && pass == 1 => return,
            Err(error) => panic!("{context}: {error}"),
        };
        if path.ends_with(".dxf") {
            assert_eq!(cadcraft_dxf::is_binary(&bytes), binary, "{context}: encoding");
            dxf_owner(&bytes);
        } else {
            assert_eq!(cadcraft_dwg::version(&bytes).as_deref(), Some(version.code()), "{context}: wire version");
        }
        let back = read(&bytes, path).unwrap();
        preserved(&original, &back, &context);
        preserved(&current, &back, &context);
        current = back;
    }
}

#[test]
fn unknown_payload_assertion_rejects_lost_references() {
    let original = drawing();
    preserved(&original, &original, "unchanged unknown entity");
    let mut changed = original.clone();
    let handle = changed.model.iter().next().unwrap().handle;
    changed.model.modify(handle, |e| {
        let EntityKind::Unknown(u) = &mut e.kind else { panic!("expected unknown entity") };
        u.tags.pop();
    });
    assert_eq!(original.model.iter().next().unwrap().common, Common::default());
    assert!(std::panic::catch_unwind(|| preserved(&original, &changed, "missing application reference")).is_err());
}
