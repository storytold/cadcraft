//! Synthetic application data must keep its identity as well as its bytes.
#![cfg(not(target_arch = "wasm32"))]
use acadrust::classes::DxfClass;
use acadrust::entities::{EntityCommon, EntityType, ExtendedEntity, ExtendedEntityData, Line, RegisteredClassEntityData};
use acadrust::objects::{ProxyObjectReference, ProxyPayload, ProxyReferenceKind};
use acadrust::types::{DxfVersion, Vector3};
use cadcraft_doc::EntityKind;
use cadcraft_io::{SaveOptions, Version};
use std::io::Cursor;

fn source(version: DxfVersion) -> Vec<u8> {
    source_payload(version, ProxyPayload::from_bits(&[0xA5, 0x80], 9))
}

fn source_payload(version: DxfVersion, payload: ProxyPayload) -> Vec<u8> {
    let mut document = acadrust::CadDocument::new();
    document.version = version;
    document.classes.add_or_update(DxfClass::new_entity("SPDSNOTEPOSITION", "mcsDbObjectNotePosition"));
    let target = document.add_entity(EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0)))).unwrap();
    document
        .add_entity(EntityType::Extended(Box::new(ExtendedEntity {
            common: EntityCommon { graphic_data: Some(vec![0x12, 0x34, 0x56]), ..Default::default() },
            data: ExtendedEntityData::RegisteredClass(RegisteredClassEntityData {
                dxf_name: "SPDSNOTEPOSITION".into(),
                cpp_class_name: "mcsDbObjectNotePosition".into(),
                properties: vec![],
                payload,
                object_ids: vec![ProxyObjectReference { kind: ProxyReferenceKind::HardPointer, handle: target }],
            }),
        })))
        .unwrap();
    acadrust::DwgWriter::write_to_vec(&document).unwrap()
}

fn check(bytes: &[u8], name: &str) -> cadcraft_doc::Drawing {
    let drawing = cadcraft_io::read(bytes, name).unwrap();
    assert_eq!(drawing.entity_count(), 2);
    let line = drawing.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
    let unknown = drawing
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Unknown(u) => Some(u),
            _ => None,
        })
        .unwrap();
    assert!(unknown.tags.iter().any(|t| t.code == 340 && t.value == line.hex()));
    let dxf = if name.ends_with(".dwg") { cadcraft_dwg::dwg_to_dxf(bytes).unwrap() } else { bytes.to_vec() };
    let tags = cadcraft_dxf::parse(&dxf).unwrap();
    let sections = cadcraft_dxf::sections(&tags);
    let classes = sections.iter().find(|s| s.name == "CLASSES").unwrap();
    let classes = cadcraft_dxf::records(&classes.tags);
    let decoded = acadrust::DxfReader::from_reader(Cursor::new(dxf)).unwrap().read().unwrap();
    let proxy = decoded
        .entities()
        .find_map(|e| match e {
            EntityType::Extended(e) => match &e.data {
                ExtendedEntityData::Proxy(p) => Some(p),
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    assert_eq!(proxy.payload.bit_count, 9);
    assert_eq!(proxy.payload.data(), [0xA5, 0x80]);
    assert_eq!(proxy.graphics.data(), [0x12, 0x34, 0x56]);
    let class = classes.get(usize::try_from(proxy.class_id - 500).unwrap()).unwrap();
    assert!(class.1.iter().any(|t| t.code == 1 && t.str() == "SPDSNOTEPOSITION"));
    assert!(class.1.iter().any(|t| t.code == 2 && t.str() == "mcsDbObjectNotePosition"));
    drawing
}

#[test]
fn opaque_registered_entities_survive_repeated_saves_with_their_original_class() {
    for source_version in [DxfVersion::AC1015, DxfVersion::AC1027, DxfVersion::AC1032] {
        let input = source(source_version);
        let initial = check(&input, "source.dwg");
        for target in [Version::R2013, Version::R2018] {
            for name in ["copy.dxf", "copy.dwg"] {
                let mut drawing = initial.clone();
                for _ in 0..3 {
                    let bytes = cadcraft_io::write_with(&drawing, name, &SaveOptions { version: Some(target), ..Default::default() }).unwrap();
                    drawing = check(&bytes, name);
                }
            }
        }
    }
}

#[test]
fn legacy_dwg_rejects_proxy_loss_while_legacy_dxf_retains_the_data() {
    let drawing = check(&source(DxfVersion::AC1032), "source.dwg");
    for target in [Version::R2000, Version::R2004, Version::R2007, Version::R2010] {
        let options = SaveOptions { version: Some(target), ..Default::default() };
        let error = cadcraft_io::write_with(&drawing, "old.dwg", &options).unwrap_err().to_string();
        assert!(error.contains("2013"), "{error}");
        let bytes = cadcraft_io::write_with(&drawing, "old.dxf", &options).unwrap();
        check(&bytes, "old.dxf");
    }
}

fn duplicate_class_dxf() -> Vec<u8> {
    let mut text = String::from("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nCLASSES\n");
    for (cpp, entity, flags) in [("First", 1, 1025), ("Second", 1, 128), ("Object", 0, 17)] {
        text.push_str(&format!("0\nCLASS\n1\nCUSTOM\n2\n{cpp}\n3\nSynthetic\n90\n{flags}\n91\n1\n280\n1\n281\n{entity}\n"));
    }
    text.push_str("0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n10\n100\nAcDbEntity\n8\n0\n100\nAcDbLine\n10\n0\n20\n0\n11\n10\n21\n0\n");
    for (handle, class) in [("20", 500), ("21", 501)] {
        text.push_str(&format!("0\nACAD_PROXY_ENTITY\n5\n{handle}\n100\nAcDbEntity\n8\n0\n100\nAcDbProxyEntity\n90\n498\n91\n{class}\n92\n0\n93\n9\n310\nA580\n330\n10\n340\n10\n350\n10\n360\n10\n94\n0\n95\n33\n70\n0\n"));
    }
    text.push_str("0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n3\nCUSTOM_OBJECT\n350\n30\n0\nACAD_PROXY_OBJECT\n5\n30\n330\nC\n100\nAcDbProxyObject\n90\n499\n91\n502\n93\n9\n310\nA580\n340\n10\n94\n0\n95\n33\n70\n0\n0\nENDSEC\n0\nEOF\n");
    text.into_bytes()
}

#[test]
fn duplicate_names_keep_distinct_classes_for_entities_and_dictionary_objects() {
    let source = duplicate_class_dxf();
    for binary in [false, true] {
        let input = if binary { cadcraft_dxf::write_binary(&cadcraft_dxf::parse(&source).unwrap(), true) } else { source.clone() };
        let mut drawing = cadcraft_io::read(&input, "source.dxf").unwrap();
        for _ in 0..3 {
            for name in ["copy.dxf", "copy.dwg"] {
                let bytes = cadcraft_io::write_with(&drawing, name, &SaveOptions { version: Some(Version::R2018), ..Default::default() }).unwrap();
                let dxf = if name.ends_with(".dwg") { cadcraft_dwg::dwg_to_dxf(&bytes).unwrap() } else { bytes.clone() };
                let tags = cadcraft_dxf::parse(&dxf).unwrap();
                let sections = cadcraft_dxf::sections(&tags);
                let classes = cadcraft_dxf::records(&sections.iter().find(|s| s.name == "CLASSES").unwrap().tags);
                let custom: Vec<_> = classes.iter().filter(|(_, t)| t.iter().any(|t| t.code == 1 && t.str() == "CUSTOM")).collect();
                assert_eq!(custom.len(), 3);
                for (class, (cpp, flags)) in custom.iter().zip([("First", 1025), ("Second", 128), ("Object", 17)]) {
                    assert!(class.1.iter().any(|t| t.code == 2 && t.str() == cpp));
                    assert!(class.1.iter().any(|t| t.code == 90 && t.i64() == flags));
                    assert!(class.1.iter().any(|t| t.code == 3 && t.str() == "Synthetic"));
                    assert!(class.1.iter().any(|t| t.code == 280 && t.i64() == 1));
                }
                let records: Vec<_> =
                    sections.iter().flat_map(|s| cadcraft_dxf::records(&s.tags)).filter(|(kind, _)| kind.starts_with("ACAD_PROXY_")).collect();
                assert_eq!(records.len(), 3);
                let mut identities = Vec::new();
                for (_, tags) in &records {
                    let id = tags.iter().find(|t| t.code == 91).unwrap().i64();
                    identities.push(classes[usize::try_from(id - 500).unwrap()].1.iter().find(|t| t.code == 2).unwrap().str());
                    assert!(tags.iter().any(|t| matches!(t.code, 93 | 161) && t.i64() == 9), "{name}: {tags:?}");
                    assert!(tags.iter().any(|t| t.code == 310 && t.str() == "A580"));
                }
                identities.sort();
                assert_eq!(identities, ["First", "Object", "Second"]);
                drawing = cadcraft_io::read(&bytes, name).unwrap();
                assert_eq!(drawing.entity_count(), 3);
                let line = drawing.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle.hex();
                for entity in drawing.model.iter().filter_map(|e| if let EntityKind::Unknown(u) = &e.kind { Some(u) } else { None }) {
                    for code in [330, 340, 350, 360] {
                        assert!(entity.tags.iter().any(|t| t.code == code && t.value == line), "missing reference {code}");
                    }
                }
            }
        }
    }
}

#[test]
fn zero_bit_opaque_entities_are_still_retained() {
    let source = source_payload(DxfVersion::AC1032, ProxyPayload::default());
    let mut drawing = cadcraft_io::read(&source, "source.dwg").unwrap();
    for name in ["copy.dxf", "copy.dwg", "again.dwg"] {
        let bytes = cadcraft_io::write_with(&drawing, name, &SaveOptions { version: Some(Version::R2018), ..Default::default() }).unwrap();
        drawing = cadcraft_io::read(&bytes, name).unwrap();
        assert_eq!(drawing.entity_count(), 2);
        assert!(drawing.model.iter().any(|e| matches!(e.kind, EntityKind::Unknown(_))));
    }
}

#[test]
fn proxies_inside_block_definitions_survive_repeated_saves() {
    use cadcraft_doc::{Block, Common, Entity};
    let mut drawing = check(&source(DxfVersion::AC1032), "source.dwg");
    let kind = drawing.model.iter().find(|e| matches!(e.kind, EntityKind::Unknown(_))).unwrap().kind.clone();
    let handle = drawing.new_handle();
    let mut block = Block::new("CUSTOM_BLOCK");
    block.entities.push(Entity { handle, common: Common::default(), kind });
    drawing.blocks.insert("CUSTOM_BLOCK".into(), std::sync::Arc::new(block));
    for name in ["copy.dxf", "copy.dwg", "again.dwg"] {
        let bytes = cadcraft_io::write_with(&drawing, name, &SaveOptions { version: Some(Version::R2018), ..Default::default() }).unwrap();
        drawing = check(&bytes, name);
        let block = drawing.blocks.get("CUSTOM_BLOCK").unwrap();
        assert_eq!(block.entities.len(), 1);
        assert!(block.entities.iter().any(|e| matches!(e.kind, EntityKind::Unknown(_))));
    }
}

#[test]
fn unused_malformed_classes_do_not_block_plain_geometry() {
    for (name, cpp, flags, count) in [("", "Unused", 0, 0), ("UNUSED", "", 0, 0), ("UNUSED", "Unused", 65536, 0), ("UNUSED", "Unused", 0, i64::MAX)] {
        let dxf = format!(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nCLASSES\n0\nCLASS\n1\n{name}\n2\n{cpp}\n3\nSynthetic\n90\n{flags}\n91\n{count}\n281\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n10\n100\nAcDbEntity\n8\n0\n100\nAcDbLine\n10\n2\n20\n3\n11\n12\n21\n13\n0\nENDSEC\n0\nEOF\n"
        );
        let bytes = cadcraft_dwg::dxf_to_dwg_as(dxf.as_bytes(), "AC1032").unwrap();
        let drawing = cadcraft_io::read(&bytes, "plain.dwg").unwrap();
        let lines: Vec<_> = drawing.model.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l) } else { None }).collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].a.x, 2.0);
        assert_eq!(lines[0].a.y, 3.0);
        assert_eq!(lines[0].b.x, 12.0);
        assert_eq!(lines[0].b.y, 13.0);
    }
}

#[test]
fn proxy_spelling_is_preserved_in_dxf_and_normalized_only_for_dwg() {
    let source = String::from_utf8(duplicate_class_dxf()).unwrap();
    for (entity, object) in [("acad_proxy_entity", "acad_proxy_object"), ("AcAd_PrOxY_EnTiTy", "AcAd_PrOxY_ObJeCt")] {
        let dxf = source.replace("ACAD_PROXY_ENTITY", entity).replace("ACAD_PROXY_OBJECT", object);
        let drawing = cadcraft_io::read(dxf.as_bytes(), "source.dxf").unwrap();
        assert_eq!(drawing.entity_count(), 3);
        let out = cadcraft_io::write_with(&drawing, "copy.dxf", &SaveOptions { version: Some(Version::R2018), ..Default::default() }).unwrap();
        let tags = cadcraft_dxf::parse(&out).unwrap();
        assert!(tags.iter().any(|t| t.code == 0 && t.str() == entity));
        let bytes = cadcraft_io::write_with(&drawing, "copy.dwg", &SaveOptions { version: Some(Version::R2018), ..Default::default() }).unwrap();
        let decoded = cadcraft_dwg::dwg_to_dxf(&bytes).unwrap();
        let tags = cadcraft_dxf::parse(&decoded).unwrap();
        let proxies: Vec<_> = cadcraft_dxf::sections(&tags)
            .iter()
            .flat_map(|s| cadcraft_dxf::records(&s.tags))
            .filter(|(kind, _)| kind.starts_with("ACAD_PROXY_"))
            .collect();
        assert_eq!(proxies.len(), 3);
        for (_, tags) in proxies {
            assert!(tags.iter().any(|t| t.code == 310 && t.str() == "A580"));
            assert!(tags.iter().any(|t| matches!(t.code, 93 | 161) && t.i64() == 9));
        }
        let recovered = cadcraft_io::read(&bytes, "copy.dwg").unwrap();
        assert_eq!(recovered.entity_count(), 3);
    }
}
