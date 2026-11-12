//! Check multiline ATTRIB values, embedded geometry and legacy placement independently.
//! Unchanged MTEXT rendering does not excuse corrupted attribute values or fallback TEXT
//! justification (#2255). Full field preservation remains in `extended::multiline_attributes`;
//! the legacy-placement test must reach its geometry assertion even while that check fails.
//! ODA's DWG specification §20.4.4 maps the R2018+ embedded object to DXF group 101.

use super::super::cases::blocks;
use cadcraft_doc::{Attrib, Drawing, EntityKind, Space};
use cadcraft_io::{SaveOptions, Version};
use cadcraft_render::{DisplayList, Options, build};

const EDITED_VALUE: &str = "edited {value}\nthird line";
const EDITED_CONTENTS: &str = "edited \\{value\\}\\Pthird line";

/// The fixture's primary attribute, not its unrelated single-line control or its ATTDEF.
fn primary(drawing: &Drawing) -> &Attrib {
    let mut attributes = drawing
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some(i.attribs.iter()),
            _ => None,
        })
        .flatten()
        .filter(|a| a.tag == "ID");
    let attribute = attributes.next().expect("primary ATTRIB must survive");
    assert!(attributes.next().is_none(), "primary ATTRIB must not be duplicated");
    attribute
}

fn edit_primary(drawing: &mut Drawing, edit: impl Fn(&mut Attrib)) {
    let handles: Vec<_> = drawing.model.iter().map(|e| e.handle).collect();
    let mut changed = 0;
    for handle in handles {
        drawing.model.modify(handle, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                for a in i.attribs.iter_mut().filter(|a| a.tag == "ID") {
                    edit(a);
                    changed += 1;
                }
            }
        });
    }
    assert_eq!(changed, 1, "edit exactly the primary ATTRIB");
}

fn reopen(drawing: &Drawing, version: Version, binary: bool, context: &str) -> Drawing {
    let options = SaveOptions { version: Some(version), binary };
    let bytes = cadcraft_io::write_with(drawing, "attributes.dxf", &options).unwrap_or_else(|e| panic!("{context}: write: {e}"));
    assert_eq!(cadcraft_dxf::is_binary(&bytes), binary, "{context}: encoding");
    let back = cadcraft_io::read(&bytes, "attributes.dxf").unwrap_or_else(|e| panic!("{context}: read: {e}"));
    assert_eq!(cadcraft_io::save_version(&back), version, "{context}: version");
    assert!(back.read_notes.is_empty(), "{context}: {:?}", back.read_notes);
    assert!(back.skipped_records.is_empty(), "{context}: {:?}", back.skipped_records);
    back
}

fn geometry(drawing: &Drawing) -> DisplayList {
    build(drawing, &Space::Model, &Options::default())
}

fn same_geometry(expected: &DisplayList, actual: &DisplayList, context: &str) {
    assert!(!expected.prims.is_empty(), "{context}: nonempty fixture");
    assert_eq!(expected.prims.len(), actual.prims.len(), "{context}: primitive count");
    for (a, b) in expected.prims.iter().zip(&actual.prims) {
        assert_eq!(a.kind, b.kind, "{context}: primitive kind");
        assert_eq!(expected.points(a), actual.points(b), "{context}: world-space geometry");
    }
}

/// Check saved data, not only the effective text drawn by `multiline_text()`. That method can
/// hide a corrupt value/read_as pair or replace stale embedded contents from the plain value.
fn saved_text(drawing: &Drawing, value: &str, contents: &str, context: &str) {
    let a = primary(drawing);
    assert_eq!(a.text.value, value, "{context}: attribute value");
    let m = a.multiline.as_ref().expect("embedded MTEXT must survive");
    assert_eq!(m.read_as, value, "{context}: read_as");
    assert_eq!(m.mtext.contents, contents, "{context}: raw embedded contents");
}

#[test]
fn multiline_geometry_and_value_edits_survive_two_saves() {
    for case in blocks::multiline_attributes() {
        let expected = geometry(&case.drawing);
        let mut edited = case.drawing.clone();
        edit_primary(&mut edited, |a| a.text.value = EDITED_VALUE.into());
        let edited_expected = geometry(&edited);
        assert_ne!(expected.verts, edited_expected.verts, "value edit must actually change the drawn text");
        for binary in [false, true] {
            let mut current = case.drawing.clone();
            for pass in 1..=2 {
                let context = format!("{} · R2018 · binary={binary} · pass={pass}", case.name);
                current = reopen(&current, Version::R2018, binary, &context);
                saved_text(&current, "first\nsecond", "first\\Psecond", &context);
                same_geometry(&expected, &geometry(&current), &context);
                let mut edited = current.clone();
                edit_primary(&mut edited, |a| a.text.value = EDITED_VALUE.into());
                same_geometry(&edited_expected, &geometry(&edited), &format!("{context} · edit after reopening"));
                let context = format!("{context} · save after editing");
                let back = reopen(&edited, Version::R2018, binary, &context);
                saved_text(&back, EDITED_VALUE, EDITED_CONTENTS, &context);
                same_geometry(&edited_expected, &geometry(&back), &context);
            }
        }
    }
}

#[test]
fn saved_text_checks_reject_corruption_hidden_by_rendering() {
    let mut source = blocks::multiline_attributes().remove(0).drawing;
    edit_primary(&mut source, |a| a.text.value = EDITED_VALUE.into());
    let back = reopen(&source, Version::R2018, true, "mutation control");
    saved_text(&back, EDITED_VALUE, EDITED_CONTENTS, "mutation control");
    for field in ["value", "read_as", "contents", "value-and-read_as"] {
        let mut corrupt = back.clone();
        edit_primary(&mut corrupt, |a| {
            let m = a.multiline.as_mut().unwrap();
            match field {
                "value" => a.text.value = "CORRUPTED VALUE".into(),
                "read_as" => m.read_as = "CORRUPTED VALUE".into(),
                "contents" => m.mtext.contents = "CORRUPTED CONTENTS".into(),
                "value-and-read_as" => {
                    a.text.value = "CORRUPTED VALUE".into();
                    m.read_as.clone_from(&a.text.value);
                }
                _ => unreachable!(),
            }
        });
        if field == "value-and-read_as" {
            same_geometry(&geometry(&back), &geometry(&corrupt), "corruption invisible to geometry");
        }
        assert!(std::panic::catch_unwind(|| saved_text(&corrupt, EDITED_VALUE, EDITED_CONTENTS, field)).is_err(), "must reject {field}");
    }
}

#[test]
#[ignore = "Confirmed DXF placement loss (#2255): R2018 save changes subsequent legacy ATTRIB placement; existing reader fix #1942"]
fn legacy_placement_is_independent_of_an_intermediate_modern_save() {
    for case in blocks::multiline_attributes() {
        for version in [Version::R2000, Version::R2004] {
            for binary in [false, true] {
                let legacy_context = format!("{} · legacy={version} · binary={binary}", case.name);
                let direct = reopen(&case.drawing, version, binary, &legacy_context);
                let a = primary(&direct);
                assert!(a.multiline.is_none(), "{legacy_context}: single-line fallback required");
                assert_eq!(a.text.value, "first second", "{legacy_context}: fallback value");
                let expected = geometry(&direct);
                for modern_binary in [false, true] {
                    let mut current = case.drawing.clone();
                    for pass in 1..=2 {
                        let context = format!("{legacy_context} · intermediate=R2018 · binary={modern_binary} · pass={pass}");
                        current = reopen(&current, Version::R2018, modern_binary, &context);
                        let indirect = reopen(&current, version, binary, &context);
                        let a = primary(&indirect);
                        assert!(a.multiline.is_none(), "{context}: single-line fallback required");
                        assert_eq!(a.text.value, "first second", "{context}: fallback value");
                        same_geometry(&expected, &geometry(&indirect), &context);
                    }
                }
            }
        }
    }
}
