//! Opaque application records use the source file's class table, never decoder defaults.

use std::collections::{HashMap, HashSet};

use acadrust::CadDocument;
use acadrust::classes::{DxfClass, ProxyFlags};
use acadrust::entities::{EntityType, ExtendedEntityData, ProxyEntityData};
use acadrust::objects::{ObjectType, ProxyPayload};
use cadcraft_dxf::Tag;

fn class_number(index: usize) -> Result<i16, String> {
    index.checked_add(500).and_then(|n| i16::try_from(n).ok()).ok_or_else(|| "DXF: too many application classes".into())
}

/// Replace the dependency's private registered-class envelope with a standard proxy.
pub(super) fn prepare(doc: &mut CadDocument) -> Result<(), String> {
    let classes: Vec<_> = doc.classes.iter().cloned().collect();
    let mut numbers = HashMap::new();
    let mut identities = HashMap::new();
    for (index, class) in classes.iter().enumerate() {
        let number = class_number(index)?;
        numbers.entry(i32::from(class.class_number)).and_modify(|v| *v = None).or_insert(Some(i32::from(number)));
        let identity = (class.dxf_name.as_str(), class.cpp_class_name.as_str(), class.is_an_entity);
        identities.entry(identity).and_modify(|v| *v = None).or_insert(Some(number));
    }
    let identity = |dxf: &str, cpp: &str, entity: bool| {
        identities
            .get(&(dxf, cpp, entity))
            .copied()
            .flatten()
            .map(i32::from)
            .ok_or_else(|| format!("DWG: missing or ambiguous application CLASS for {dxf} ({cpp})"))
    };
    let source = doc.dwg_source_version.unwrap_or(doc.version).version_code();
    let dwg_version = match source {
        1012 => 19,
        1014 => 21,
        1015 => 23,
        1018 => 25,
        1021 => 27,
        1024 => 29,
        1027 => 31,
        1032 => 33,
        _ => return Ok(()),
    };
    let maintenance = i32::from(doc.maintenance_version);
    let remap = |id: i32| numbers.get(&id).copied().flatten().ok_or_else(|| format!("DWG: proxy names missing application class {id}"));
    // Only mutate proxies: entities_mut invalidates the decoder's verbatim records for every entity.
    let handles: Vec<_> = doc
        .entities()
        .filter_map(|e| match e {
            EntityType::Extended(e) => match &e.data {
                ExtendedEntityData::Proxy(_) => Some(e.common.handle),
                ExtendedEntityData::RegisteredClass(data) if data.properties.is_empty() || data.payload.bit_count > 0 => Some(e.common.handle),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for handle in handles {
        let text_payload = match doc.get_entity(handle) {
            Some(EntityType::Extended(e)) if matches!(e.data, ExtendedEntityData::RegisteredClass(_)) => {
                if let ExtendedEntityData::RegisteredClass(data) = &e.data
                    && !data.properties.is_empty()
                {
                    return Err(format!("DWG: cannot transport semantic properties of {} as an opaque proxy", data.dxf_name));
                }
                e.common.raw_record.as_deref().map(registered_text).transpose()?.unwrap_or_default()
            }
            _ => ProxyPayload::default(),
        };
        let Some(EntityType::Extended(entity)) = doc.get_entity_mut(handle) else { continue };
        match &mut entity.data {
            ExtendedEntityData::Proxy(data) => data.class_id = remap(data.class_id)?,
            ExtendedEntityData::RegisteredClass(data) => {
                let class_id = identity(&data.dxf_name, &data.cpp_class_name, true)?;
                entity.data = ExtendedEntityData::Proxy(ProxyEntityData {
                    proxy_id: 498,
                    class_id,
                    dxf_subclass: String::new(),
                    version: (maintenance << 16) | dwg_version,
                    dwg_version,
                    maintenance_version: maintenance,
                    from_dxf: false,
                    graphics: ProxyPayload::from_bytes(entity.common.graphic_data.as_deref().unwrap_or_default()),
                    payload: data.payload.clone(),
                    text_payload,
                    object_ids: data.object_ids.clone(),
                });
            }
            _ => {}
        }
    }
    for object in doc.objects.values_mut() {
        if let ObjectType::ProxyObject(proxy) = object {
            proxy.class_id = remap(proxy.class_id)?;
        }
    }
    Ok(())
}

/// The decoder's registered-class representation omits the separate R2007+ string stream.
fn registered_text(raw: &acadrust::entities::RawRecord) -> Result<ProxyPayload, String> {
    use acadrust::io::dwg::DwgVersion;
    use acadrust::io::dwg::dwg_stream_readers::object_reader::DwgObjectReader;
    use acadrust::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;
    use acadrust::types::DxfVersion;

    if raw.version < DxfVersion::AC1021 {
        return Ok(ProxyPayload::default());
    }
    let version = DwgVersion::from_dxf_version(raw.version).map_err(|e| format!("DWG proxy text: {e}"))?;
    let handle_bits = usize::try_from(raw.handle_bits).map_err(|_| "DWG proxy text: negative handle stream size")?;
    let total_bits = raw.data.len().checked_mul(8).ok_or("DWG proxy text: record too large")?;
    if handle_bits > total_bits {
        return Err("DWG proxy text: handle stream exceeds record".into());
    }
    // Frame the verbatim record for the dependency's checked object reader.
    let mut frame = DwgBitWriter::new(version, raw.version);
    frame.write_modular_short(raw.data.len());
    if raw.version >= DxfVersion::AC1024 {
        frame.write_modular_char(handle_bits);
    }
    frame.write_bytes(&raw.data);
    let reader = DwgObjectReader::new(frame.into_bytes(), raw.version, HashMap::new()).map_err(|e| format!("DWG proxy text: {e}"))?;
    let (kind, mut streams) = reader.read_record_at(0).map_err(|e| format!("DWG proxy text: {e}"))?;
    reader.read_common_entity_data(&mut streams, kind);
    if kind == 498 {
        // An existing registered-class envelope is a proxy and carries its class name first.
        streams.read_variable_text();
    } else if kind < 500 {
        return Err("DWG proxy text: registered entity has no application class".into());
    }
    streams.check_error().map_err(|e| format!("DWG proxy text: {e}"))?;
    let bits = usize::try_from(streams.text_remaining_bits()).map_err(|_| "DWG proxy text: negative string stream size")?;
    if bits > total_bits {
        return Err("DWG proxy text: string stream exceeds record".into());
    }
    let count = u32::try_from(bits).map_err(|_| "DWG proxy text: string stream too large")?;
    let mut bytes = vec![0; bits.div_ceil(8)];
    for (index, byte) in bytes.iter_mut().enumerate() {
        for bit in 0..8.min(bits.saturating_sub(index * 8)) {
            if streams.read_text_bit() {
                *byte |= 0x80 >> bit;
            }
        }
    }
    streams.check_error().map_err(|e| format!("DWG proxy text: {e}"))?;
    Ok(ProxyPayload::from_bits(&bytes, count))
}

/// Parse ASCII or binary DXF with the bounded, code-page-aware DXF parser.
pub(super) fn read_classes(dxf: &[u8]) -> Result<Vec<DxfClass>, String> {
    let tags = cadcraft_dxf::parse(dxf).map_err(|e| format!("DXF classes: {e}"))?;
    let mut classes = Vec::new();
    let mut in_classes = false;
    let mut at = 0;
    while let Some(tag) = tags.get(at) {
        if tag.code != 0 {
            at += 1;
            continue;
        }
        let end = tags.get(at + 1..).unwrap_or_default().iter().position(|t| t.code == 0).map_or(tags.len(), |n| at + 1 + n);
        let body = tags.get(at + 1..end).unwrap_or_default();
        match tag.str().as_str() {
            "SECTION" => in_classes = body.first().is_some_and(|t| t.code == 2 && t.str() == "CLASSES"),
            "ENDSEC" => in_classes = false,
            "CLASS" if in_classes => classes.push(read_class(body, class_number(classes.len())?)?),
            _ => {}
        }
        at = end;
    }
    Ok(classes)
}

fn read_class(tags: &[Tag], number: i16) -> Result<DxfClass, String> {
    let text = |code| tags.iter().find(|t| t.code == code).map(Tag::str).unwrap_or_default();
    let integer = |code| tags.iter().find(|t| t.code == code).map_or(0, Tag::i64);
    let mut class = DxfClass::new(text(1), text(2));
    if class.dxf_name.is_empty() || class.cpp_class_name.is_empty() {
        return Err("DXF: application CLASS has no name".into());
    }
    class.application_name = text(3);
    class.proxy_flags = ProxyFlags(u16::try_from(integer(90)).map_err(|_| "DXF: invalid class proxy flags")?);
    class.instance_count = i32::try_from(integer(91)).map_err(|_| "DXF: invalid class instance count")?;
    class.was_zombie = integer(280) != 0;
    class.is_an_entity = integer(281) != 0;
    class.item_class_id = if class.is_an_entity { 498 } else { 499 };
    class.class_number = number;
    Ok(class)
}

/// Keep source class positions and metadata, adding missing decoder defaults only afterwards.
pub(super) fn restore_classes(doc: &mut CadDocument, classes: Vec<DxfClass>) -> Result<(), String> {
    let mut proxies = Vec::new();
    for entity in doc.entities() {
        if let EntityType::Extended(e) = entity
            && let ExtendedEntityData::Proxy(p) = &e.data
        {
            proxies.push((p.class_id, true));
        }
    }
    for object in doc.objects.values() {
        if let ObjectType::ProxyObject(p) = object {
            proxies.push((p.class_id, false));
        }
    }
    if proxies.is_empty() {
        return Ok(());
    }
    for (number, entity) in proxies {
        let index = number.checked_sub(500).and_then(|n| usize::try_from(n).ok());
        if !index.and_then(|i| classes.get(i)).is_some_and(|c| c.is_an_entity == entity) {
            return Err(format!("DXF: proxy names missing or incompatible application class {number}"));
        }
    }
    if doc.version < acadrust::types::DxfVersion::AC1027 {
        return Err("DWG: preserving application proxies requires AutoCAD 2013 or later; choose 2013/2018 DWG or DXF".into());
    }
    let mut names: HashSet<_> = classes.iter().map(|c| c.dxf_name.to_uppercase()).collect();
    let defaults: Vec<_> = doc.classes.iter().filter(|c| names.insert(c.dxf_name.to_uppercase())).cloned().collect();
    doc.classes.clear();
    for mut class in classes.into_iter().chain(defaults) {
        class.class_number = class_number(doc.classes.len())?;
        doc.classes.push_preserving(class);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{EntityCommon, ExtendedEntity, RegisteredClassEntityData};
    use acadrust::types::DxfVersion;

    fn proxy_doc() -> CadDocument {
        let mut doc = CadDocument::new();
        doc.version = DxfVersion::AC1032;
        doc.add_entity(EntityType::Extended(Box::new(ExtendedEntity {
            common: EntityCommon::default(),
            data: ExtendedEntityData::Proxy(ProxyEntityData {
                proxy_id: 498,
                class_id: 501,
                dxf_subclass: String::new(),
                version: 33,
                dwg_version: 33,
                maintenance_version: 0,
                from_dxf: false,
                graphics: ProxyPayload::default(),
                payload: ProxyPayload::from_bits(&[0x80], 1),
                text_payload: ProxyPayload::default(),
                object_ids: vec![],
            }),
        })))
        .unwrap();
        doc
    }

    #[test]
    fn source_classes_keep_duplicates_and_metadata_before_defaults() {
        let tags = vec![
            Tag::s(0, "SECTION"),
            Tag::s(2, "CLASSES"),
            Tag::s(0, "CLASS"),
            Tag::s(1, "Объект"),
            Tag::s(2, "First"),
            Tag::s(3, "Первый"),
            Tag::i(90, 1025),
            Tag::i(91, 7),
            Tag::i(280, 1),
            Tag::i(281, 1),
            Tag::s(0, "CLASS"),
            Tag::s(1, "Объект"),
            Tag::s(2, "Second"),
            Tag::s(3, "Второй"),
            Tag::i(90, 128),
            Tag::i(281, 1),
            Tag::s(0, "ENDSEC"),
            Tag::s(0, "EOF"),
        ];
        for bytes in [cadcraft_dxf::write_ascii(&tags).into_bytes(), cadcraft_dxf::write_binary(&tags, true)] {
            let classes = read_classes(&bytes).unwrap();
            assert_eq!(classes.len(), 2);
            assert_eq!(classes[0].application_name, "Первый");
            assert_eq!(classes[0].proxy_flags.0, 1025);
            assert_eq!(classes[0].instance_count, 7);
            assert!(classes[0].was_zombie);
            let mut doc = proxy_doc();
            restore_classes(&mut doc, classes.clone()).unwrap();
            assert_eq!(doc.classes.iter().take(2).cloned().collect::<Vec<_>>(), classes);
        }
    }

    #[test]
    fn missing_negative_and_object_class_ids_are_rejected() {
        for id in [-1, 499, 501, i32::MAX] {
            let mut doc = proxy_doc();
            if let Some(EntityType::Extended(e)) = doc.entities_mut().next()
                && let ExtendedEntityData::Proxy(p) = &mut e.data
            {
                p.class_id = id;
            }
            let mut class = DxfClass::new_entity("ENTITY", "Entity");
            class.class_number = 500;
            assert!(restore_classes(&mut doc, vec![class]).is_err());
        }
        assert!(restore_classes(&mut proxy_doc(), vec![DxfClass::new("A", "A"), DxfClass::new("B", "B")]).is_err());
    }

    #[test]
    fn opaque_registered_class_ambiguity_is_not_guessed() {
        let mut doc = CadDocument::new();
        doc.classes.push_preserving(DxfClass::new_entity("CUSTOM", "Custom"));
        doc.classes.push_preserving(DxfClass::new_entity("CUSTOM", "Custom"));
        doc.add_entity(EntityType::Extended(Box::new(ExtendedEntity {
            common: EntityCommon::default(),
            data: ExtendedEntityData::RegisteredClass(RegisteredClassEntityData {
                dxf_name: "CUSTOM".into(),
                cpp_class_name: "Custom".into(),
                properties: vec![],
                payload: ProxyPayload::from_bits(&[0x80], 1),
                object_ids: vec![],
            }),
        })))
        .unwrap();
        assert!(prepare(&mut doc).unwrap_err().contains("ambiguous"));
        assert!(class_number(usize::MAX).is_err());
    }
    #[test]
    fn semantic_records_keep_their_verbatim_data_and_mixed_payloads_are_rejected() {
        use acadrust::entities::RawRecord;
        use acadrust::objects::{SemanticProperty, SemanticPropertyValue};
        for payload in [ProxyPayload::default(), ProxyPayload::from_bits(&[0x80], 1)] {
            let mut doc = CadDocument::new();
            let raw = RawRecord { data: vec![0xAA], handle_bits: 0, version: DxfVersion::AC1032 };
            let handle = doc
                .add_entity(EntityType::Extended(Box::new(ExtendedEntity {
                    common: EntityCommon::default(),
                    data: ExtendedEntityData::RegisteredClass(RegisteredClassEntityData {
                        dxf_name: "SEMANTIC".into(),
                        cpp_class_name: "Semantic".into(),
                        properties: vec![SemanticProperty {
                            subclass: "Semantic".into(),
                            code: 1,
                            value: SemanticPropertyValue::Text("value".into()),
                        }],
                        payload: payload.clone(),
                        object_ids: vec![],
                    }),
                })))
                .unwrap();
            doc.get_entity_mut(handle).unwrap().common_mut().raw_record = Some(std::sync::Arc::new(raw.clone()));
            assert_eq!(doc.get_entity(handle).unwrap().common().raw_record.as_deref(), Some(&raw));
            if payload.bit_count == 0 {
                prepare(&mut doc).unwrap();
                assert_eq!(doc.get_entity(handle).unwrap().common().raw_record.as_deref(), Some(&raw));
            } else {
                assert!(prepare(&mut doc).unwrap_err().contains("semantic properties"));
            }
        }
    }
}

#[cfg(test)]
mod string_stream_tests {
    use super::*;
    use acadrust::entities::{EntityCommon, ExtendedEntity, RawRecord, RegisteredClassEntityData};
    use acadrust::types::DxfVersion;
    use std::io::Cursor;

    #[test]
    fn registered_transport_recovers_separate_non_byte_aligned_strings() {
        for version in [DxfVersion::AC1027, DxfVersion::AC1032] {
            let mut source = CadDocument::new();
            source.version = version;
            source.classes.add_or_update(DxfClass::new_entity("CUSTOM_TEXT", "CustomText"));
            let class_id = i32::from(source.classes.get_by_name("CUSTOM_TEXT").unwrap().class_number);
            let text = ProxyPayload::from_bits(&[0xA5, 0xC7, 0xE0], 19);
            let payload = ProxyPayload::from_bits(&[0x80], 1);
            source
                .add_entity(EntityType::Extended(Box::new(ExtendedEntity {
                    common: EntityCommon::default(),
                    data: ExtendedEntityData::Proxy(ProxyEntityData {
                        proxy_id: 498,
                        class_id,
                        dxf_subclass: "CustomText".into(),
                        version: 33,
                        dwg_version: 33,
                        maintenance_version: 0,
                        from_dxf: false,
                        graphics: ProxyPayload::default(),
                        payload: payload.clone(),
                        text_payload: text.clone(),
                        object_ids: vec![],
                    }),
                })))
                .unwrap();
            let bytes = acadrust::DwgWriter::write_to_vec(&source).unwrap();
            let mut decoded = acadrust::DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
            let entity = decoded.entities().next().unwrap();
            let handle = entity.common().handle;
            let raw = entity.common().raw_record.clone().unwrap();
            assert_eq!(registered_text(&raw).unwrap().data(), text.data());
            assert_eq!(registered_text(&raw).unwrap().bit_count, 19);
            let EntityType::Extended(entity) = decoded.get_entity_mut(handle).unwrap() else { panic!() };
            entity.data = ExtendedEntityData::RegisteredClass(RegisteredClassEntityData {
                dxf_name: "CUSTOM_TEXT".into(),
                cpp_class_name: "CustomText".into(),
                properties: vec![],
                payload: payload.clone(),
                object_ids: vec![],
            });
            entity.common.raw_record = Some(raw);
            prepare(&mut decoded).unwrap();
            for _ in 0..2 {
                let mut dxf = Vec::new();
                acadrust::DxfWriter::new(&decoded).write_to_writer(&mut dxf).unwrap();
                let bytes = crate::dxf_to_dwg_as(&dxf, "AC1032").unwrap();
                decoded = acadrust::DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
                let p = decoded
                    .entities()
                    .find_map(|e| match e {
                        EntityType::Extended(e) => match &e.data {
                            ExtendedEntityData::Proxy(p) => Some(p),
                            _ => None,
                        },
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(p.payload.bit_count, 1);
                assert_eq!(p.payload.data(), payload.data());
                assert_eq!(p.text_payload.bit_count, 19);
                assert_eq!(p.text_payload.data(), text.data());
            }
        }
    }

    #[test]
    fn invalid_string_stream_boundaries_return_errors() {
        for handle_bits in [-1, 1000] {
            let raw = RawRecord { data: vec![0; 8], handle_bits, version: DxfVersion::AC1032 };
            assert!(registered_text(&raw).is_err());
        }
    }
}
