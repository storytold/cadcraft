//! A drawing that was never read from a file has no kept DXF version, so the DWG bridge used to
//! read its FIELD groups as R2000 and acadrust dropped their flags, cached values and formats
//! (#551). With field texts such a drawing is now read as AutoCAD 2010. A drawing with a kept
//! version is read as that version, and its new FIELD objects are written in that version's
//! groups (before 2007: the format in group 4, values without flags and format).
//!
//! Non-ASCII field text is not covered: the bridge writes it as `\U+XXXX` escapes, which the
//! acadrust used here decodes for every version; acadrust#213 / CADCraft #2851 track a newer one
//! that does not.
#![cfg(not(target_arch = "wasm32"))]

use std::io::Cursor;

use acadrust::objects::{Field, ObjectType};
use cadcraft_doc::{Common, Drawing, EntityKind, MText, Space};
use cadcraft_io::{SaveOptions, Version, read, write_with};

const DATE: &str = "Date : %<\\AcVar Date \\f \"dd/MM/yyyy\" \\CcValue \"03/11/2026\">%";
const TWO: &str = "File %<\\AcVar DWGNAME \\CcValue \"plan.dxf\">% on %<\\AcVar Date \\f \"dd/MM/yyyy\" \\CcValue \"03/11/2026\">%";
const EXPR: &str = "Twice: %<\\AcExpr (%<\\AcVar LTSCALE \\CcValue \"1.000000\">%*2) \\f \"%lu2\" \\CcValue \"2.0000\">%";

fn new_drawing() -> Drawing {
    with_fields(Drawing::new_metric())
}

fn with_fields(mut d: Drawing) -> Drawing {
    for (i, c) in [DATE, TWO, EXPR].into_iter().enumerate() {
        let t = MText {
            insert: cadcraft_geom::Vec3::new(0.0, -10.0 * i as f64, 0.0),
            height: 2.5,
            width: 120.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: c.into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    }
    d
}

fn fields_of(dwg: Vec<u8>) -> Vec<Field> {
    let doc = acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().unwrap();
    let mut v: Vec<Field> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            ObjectType::Field(f) => Some(f.clone()),
            _ => None,
        })
        .collect();
    v.sort_by_key(|f| f.handle.value());
    v
}

fn contents(d: &Drawing) -> Vec<String> {
    let mut v: Vec<(f64, String)> =
        d.model.iter().filter_map(|e| if let EntityKind::MText(t) = &e.kind { Some((-t.insert.y, t.contents.clone())) } else { None }).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().map(|(_, c)| c).collect()
}

/// The new drawing with 5 fields (and the 3 `_text` fields above them) saved as DWG 2018 keeps
/// every field's value and format, and reopens with the same field codes.
#[test]
fn a_new_drawing_keeps_its_fields_in_a_dwg() {
    let d = new_drawing();
    assert_eq!(d.foreign.version, None, "a new drawing has no kept version");
    let dwg = write_with(&d, "new.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    let fields = fields_of(dwg.clone());
    assert_eq!(fields.len(), 8, "3 `_text` fields and 5 field codes");
    let by_code = |code: &str| fields.iter().filter(|f| f.code == code).collect::<Vec<_>>();
    let dates = by_code("\\AcVar Date \\f \"dd/MM/yyyy\"");
    assert_eq!(dates.len(), 2);
    for f in dates {
        assert_eq!((f.value.format.as_str(), f.value_string.as_str()), ("dd/MM/yyyy", "03/11/2026"));
    }
    let name = by_code("\\AcVar DWGNAME");
    assert_eq!(name.len(), 1);
    assert_eq!(name[0].value_string, "plan.dxf");
    let expr = by_code("\\AcExpr (%<\\_FldIdx 0>%*2) \\f \"%lu2\"");
    assert_eq!(expr.len(), 1);
    assert_eq!((expr[0].value.format.as_str(), expr[0].value_string.as_str()), ("%lu2", "2.0000"));
    let var = by_code("\\AcVar LTSCALE");
    assert_eq!(var.len(), 1);
    assert_eq!(var[0].value_string, "1.000000");

    // And CADCraft reads the DWG back with all the field codes and values.
    let back = read(&dwg, "new.dwg").unwrap();
    assert_eq!(contents(&back), [DATE, TWO, EXPR]);
}

/// The same drawing saved through a DXF first (it then has a kept version) gives the same fields.
#[test]
fn a_new_drawing_and_its_dxf_save_alike() {
    let d = new_drawing();
    let via_dxf = read(&cadcraft_io::write(&d, "x.dxf").unwrap(), "x.dxf").unwrap();
    let opts = SaveOptions { version: Some(Version::R2018), binary: false };
    let key = |f: &Field| (f.evaluator_id.clone(), f.code.clone(), f.value.format.clone(), f.value_string.clone());
    let a: Vec<_> = fields_of(write_with(&d, "a.dwg", &opts).unwrap()).iter().map(key).collect();
    let b: Vec<_> = fields_of(write_with(&via_dxf, "b.dwg", &opts).unwrap()).iter().map(key).collect();
    assert_eq!(a, b);
}

/// Without field texts nothing changes: no kept version is sent.
#[test]
fn a_drawing_without_fields_saves_as_before() {
    let d = Drawing::new_metric();
    let dwg = write_with(&d, "plain.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    assert!(fields_of(dwg).is_empty());
}

/// An R2000 file (`$ACADVER` AC1015) whose named object dictionary holds an XRECORD, which
/// CADCraft keeps without modelling it.
const R2000_WITH_XRECORD: &str = "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1015\n  9\n$HANDSEED\n  5\nFFF\n  0\nENDSEC\n\
  0\nSECTION\n  2\nOBJECTS\n  0\nDICTIONARY\n  5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n  3\nRETAINED_NOTE\n350\n300\n\
  0\nXRECORD\n  5\n300\n330\nC\n100\nAcDbXrecord\n280\n1\n  1\nretained note\n  0\nENDSEC\n  0\nEOF\n";

/// A drawing imported from an R2000 file with a retained object, then given fields, is saved like
/// a new one: the fields keep their values and formats, and the retained object is still there.
#[test]
fn fields_added_to_an_r2000_drawing_with_retained_objects_survive() {
    let imported = read(R2000_WITH_XRECORD.as_bytes(), "old.dxf").unwrap();
    assert_eq!(imported.foreign.version.as_deref(), Some("AC1015"));
    assert!(imported.foreign.objects.iter().any(|o| o.kind == "XRECORD"), "the XRECORD is kept");
    let d = with_fields(imported);
    let dwg = write_with(&d, "old.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();
    let fields = fields_of(dwg.clone());
    assert_eq!(fields.len(), 8, "3 `_text` fields and 5 field codes");
    let dates: Vec<_> = fields.iter().filter(|f| f.code == "\\AcVar Date \\f \"dd/MM/yyyy\"").collect();
    assert_eq!(dates.len(), 2);
    for f in dates {
        assert_eq!((f.value.format.as_str(), f.value_string.as_str()), ("dd/MM/yyyy", "03/11/2026"));
    }
    let expr: Vec<_> = fields.iter().filter(|f| f.code.starts_with("\\AcExpr")).collect();
    assert_eq!((expr[0].value.format.as_str(), expr[0].value_string.as_str()), ("%lu2", "2.0000"));
    let back = read(&dwg, "old.dwg").unwrap();
    assert_eq!(contents(&back), [DATE, TWO, EXPR]);
    assert!(note(&back), "the retained XRECORD survives the save");
}

// Retained records that acadrust 0.6.6 reads by the `$ACADVER` the bridge declares. Declaring an
// older drawing as AutoCAD 2010 when it gets field texts changed how they read; each test below
// saves the same import with and without an added field text and checks the retained record
// reads the same.

fn r2018(d: &Drawing, name: &str) -> Vec<u8> {
    write_with(d, name, &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap()
}

fn acad(dwg: Vec<u8>) -> acadrust::CadDocument {
    acadrust::DwgReader::from_stream(Cursor::new(dwg)).read().unwrap()
}

/// A field's evaluator, code, format (as either layout holds it), cached value and its type.
type FieldKey = (String, String, String, String, i32);

fn field_keys(dwg: Vec<u8>) -> Vec<FieldKey> {
    let mut v: Vec<FieldKey> = fields_of(dwg)
        .iter()
        .map(|f| (f.evaluator_id.clone(), f.code.clone(), f.pre2007_format().to_string(), f.value_string.clone(), f.value.raw_type_code))
        .collect();
    v.sort();
    v
}

/// FIELD objects of an AutoCAD 2004 (AC1018) drawing are kept in that version's layout (the
/// format in group 4, values without the 2007 flags, format and text groups). acadrust reads
/// those groups by version, so saved again as 2004 they must keep their formats and values,
/// also when a new field text is added.
#[test]
fn kept_r2004_field_objects_keep_formats_and_values() {
    let opts = SaveOptions { version: Some(Version::R2004), binary: false };
    let r2004 = write_with(&new_drawing(), "old.dwg", &opts).unwrap();
    let before = field_keys(r2004.clone());
    assert_eq!(before.len(), 8);
    assert!(before.iter().any(|k| k.2 == "dd/MM/yyyy") && before.iter().any(|k| k.2 == "%lu2"), "{before:#?}");
    let imported = read(&r2004, "old.dwg").unwrap();
    assert_eq!(imported.foreign.version.as_deref(), Some("AC1018"));
    let kept: Vec<_> = imported.foreign.objects.iter().filter(|o| o.kind == "FIELD").collect();
    assert_eq!(kept.len(), 8, "the FIELD objects are kept");
    assert!(kept.iter().all(|o| o.tags.iter().any(|t| t.code == 4) && !o.tags.iter().any(|t| t.code == 304)), "pre-2007 layout");

    let same = field_keys(write_with(&imported, "same.dwg", &opts).unwrap());
    assert_eq!(same, before, "re-saved as DWG 2004");
    let mut d = imported;
    let t = MText {
        insert: cadcraft_geom::Vec3::new(0.0, -40.0, 0.0),
        height: 2.5,
        width: 120.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "Name %<\\AcVar DWGNAME \\CcValue \"x.dwg\">%".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    let more = field_keys(write_with(&d, "more.dwg", &opts).unwrap());
    for k in &before {
        assert!(more.contains(k), "kept field {k:?} changed: {more:#?}");
    }
}

/// The R2000 header of the hand-written files below, with an ANSI_1252 code page.
const R2000_HEADER: &str = "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1015\n  9\n$DWGCODEPAGE\n  3\nANSI_1252\n\
  9\n$HANDSEED\n  5\nFFF\n  0\nENDSEC\n";

/// An R2000 MTEXT with the background fill an AutoCAD 2004 drawing saved as R2000 keeps in
/// `ACAD_MTEXT_BBRT` round-trip xdata (flags 1, scale 1.5, colour 1). acadrust turns it into the
/// native fill only for a file older than 2004. The XRECORD gives the drawing a kept version.
fn r2000_mtext_background() -> String {
    format!(
        "{R2000_HEADER}  0\nSECTION\n  2\nENTITIES\n  0\nMTEXT\n  5\n200\n100\nAcDbEntity\n  8\n0\n100\nAcDbMText\n\
 10\n0.0\n 20\n0.0\n 30\n0.0\n 40\n2.5\n 41\n50.0\n 71\n1\n  1\nBackground\n1001\nACAD\n1000\nACAD_MTEXT_BBRT\n\
1070\n91\n1071\n1\n1070\n46\n1040\n1.5\n1070\n64\n1071\n-1023410175\n1070\n442\n1071\n0\n1000\nACAD_MTEXT_BERT\n\
  0\nENDSEC\n  0\nSECTION\n  2\nOBJECTS\n  0\nDICTIONARY\n  5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n\
  3\nRETAINED_NOTE\n350\n300\n  0\nXRECORD\n  5\n300\n330\nC\n100\nAcDbXrecord\n280\n1\n  1\nretained note\n\
  0\nENDSEC\n  0\nEOF\n"
    )
}

fn background_flags(dwg: Vec<u8>) -> Vec<i32> {
    acad(dwg)
        .entities()
        .filter_map(|e| match e {
            acadrust::EntityType::MText(m) if m.value == "Background" => Some(m.background_fill_flags),
            _ => None,
        })
        .collect()
}

#[test]
fn r2000_mtext_background_roundtrip_xdata_reads_the_same_with_fields() {
    let imported = read(r2000_mtext_background().as_bytes(), "bg.dxf").unwrap();
    assert_eq!(imported.foreign.version.as_deref(), Some("AC1015"));
    let plain = background_flags(r2018(&imported, "plain.dwg"));
    let with = background_flags(r2018(&with_fields(imported), "fields.dwg"));
    assert_eq!(plain, [1], "without fields the bridge reads the round-trip fill");
    assert_eq!(with, plain, "the same with field texts");
}

/// An R2000 solid hatch with the gradient an AutoCAD 2004 drawing saved as R2000 keeps in
/// `GradientColor1ACI`/`GradientColor2ACI` xdata and an `ACAD_XREC_ROUNDTRIP` XRECORD. acadrust
/// turns it into the native gradient only for a file older than 2004.
fn r2000_hatch_gradient() -> String {
    format!(
        "{R2000_HEADER}  0\nSECTION\n  2\nENTITIES\n  0\nHATCH\n  5\n210\n102\n{{ACAD_XDICTIONARY\n360\n220\n102\n}}\n\
100\nAcDbEntity\n  8\n0\n100\nAcDbHatch\n 10\n0.0\n 20\n0.0\n 30\n0.0\n210\n0.0\n220\n0.0\n230\n1.0\n  2\nSOLID\n\
 70\n1\n 71\n0\n 91\n1\n 92\n3\n 72\n0\n 73\n1\n 93\n4\n 10\n0.0\n 20\n0.0\n 10\n10.0\n 20\n0.0\n 10\n10.0\n 20\n10.0\n\
 10\n0.0\n 20\n10.0\n 97\n0\n 75\n0\n 76\n1\n 98\n0\n1001\nGradientColor1ACI\n1070\n5\n1001\nGradientColor2ACI\n1070\n2\n\
  0\nENDSEC\n  0\nSECTION\n  2\nOBJECTS\n  0\nDICTIONARY\n  5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n\
  0\nDICTIONARY\n  5\n220\n330\n210\n100\nAcDbDictionary\n281\n1\n  3\nACAD_XREC_ROUNDTRIP\n350\n221\n\
  0\nXRECORD\n  5\n221\n330\n220\n100\nAcDbXrecord\n280\n1\n  1\nLINEAR\n  0\nENDSEC\n  0\nEOF\n"
    )
}

fn gradients(dwg: Vec<u8>) -> Vec<bool> {
    acad(dwg)
        .entities()
        .filter_map(|e| match e {
            acadrust::EntityType::Hatch(h) => Some(h.gradient_color.enabled),
            _ => None,
        })
        .collect()
}

#[test]
fn r2000_hatch_gradient_roundtrip_data_reads_the_same_with_fields() {
    let imported = read(r2000_hatch_gradient().as_bytes(), "grad.dxf").unwrap();
    assert_eq!(imported.foreign.version.as_deref(), Some("AC1015"));
    assert!(imported.foreign.objects.iter().any(|o| o.kind == "XRECORD"), "the round-trip XRECORD is kept");
    let plain = gradients(r2018(&imported, "plain.dwg"));
    let with = gradients(r2018(&with_fields(imported), "fields.dwg"));
    assert_eq!(plain, [true], "without fields the bridge reads the round-trip gradient");
    assert_eq!(with, plain, "the same with field texts");
}

/// Kept text in a code page: before 2007 acadrust decodes a DXF by `$DWGCODEPAGE`, from 2007 as
/// UTF-8.
#[test]
fn r2000_code_page_text_in_a_kept_object_reads_the_same_with_fields() {
    let mut dxf = R2000_HEADER.as_bytes().to_vec();
    dxf.extend_from_slice(
        b"  0\nSECTION\n  2\nOBJECTS\n  0\nDICTIONARY\n  5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n  3\nRETAINED_NOTE\n350\n300\n\
  0\nXRECORD\n  5\n300\n330\nC\n100\nAcDbXrecord\n280\n1\n  1\nCaf\xe9 \xc0 la carte\n  0\nENDSEC\n  0\nEOF\n",
    );
    let imported = read(&dxf, "cp.dxf").unwrap();
    assert_eq!(imported.foreign.version.as_deref(), Some("AC1015"));
    let note = |d: &Drawing| -> Vec<String> {
        d.foreign
            .objects
            .iter()
            .filter(|o| o.kind == "XRECORD")
            .flat_map(|o| o.tags.iter().filter(|t| t.code == 1).map(|t| t.value.clone()))
            .collect()
    };
    assert_eq!(note(&imported), ["Café À la carte"]);
    let plain = read(&r2018(&imported, "plain.dwg"), "plain.dwg").unwrap();
    let with = read(&r2018(&with_fields(imported), "fields.dwg"), "fields.dwg").unwrap();
    assert_eq!(note(&plain), ["Café À la carte"]);
    assert_eq!(note(&with), note(&plain));
}

fn note(d: &Drawing) -> bool {
    d.foreign.objects.iter().any(|o| o.kind == "XRECORD" && o.tags.iter().any(|t| t.value == "retained note"))
}

/// The format and cached value of each field with `code`.
fn formats(keys: &[FieldKey], code: &str) -> Vec<(String, String)> {
    keys.iter().filter(|k| k.1 == code).map(|k| (k.2.clone(), k.3.clone())).collect()
}

fn assert_field_formats(keys: &[FieldKey], what: &str) {
    assert_eq!(keys.len(), 8, "{what}: 3 `_text` fields and 5 field codes");
    let date = ("dd/MM/yyyy".to_string(), "03/11/2026".to_string());
    assert_eq!(formats(keys, "\\AcVar Date \\f \"dd/MM/yyyy\""), [date.clone(), date], "{what}");
    assert_eq!(formats(keys, "\\AcExpr (%<\\_FldIdx 0>%*2) \\f \"%lu2\""), [("%lu2".to_string(), "2.0000".to_string())], "{what}");
    assert_eq!(formats(keys, "\\AcVar LTSCALE"), [(String::new(), "1.000000".to_string())], "{what}");
}

/// An R2000 import keeps its version (AC1015) for the bridge also with field texts, so the new
/// FIELD objects are written in the pre-2007 groups. Saved as DWG 2004 and as DWG 2018 the fields
/// keep their formats and values, CADCraft reads the codes back and the retained XRECORD stays.
#[test]
fn fields_added_to_an_r2000_drawing_keep_formats_in_dwg_2004_and_2018() {
    let d = with_fields(read(R2000_WITH_XRECORD.as_bytes(), "old.dxf").unwrap());
    assert_eq!(d.foreign.version.as_deref(), Some("AC1015"));
    for v in [Version::R2004, Version::R2018] {
        let dwg = write_with(&d, "old.dwg", &SaveOptions { version: Some(v), binary: false }).unwrap();
        assert_field_formats(&field_keys(dwg.clone()), &format!("{v:?}"));
        let back = read(&dwg, "old.dwg").unwrap();
        assert_eq!(contents(&back), [DATE, TWO, EXPR], "{v:?}");
        assert!(note(&back), "{v:?}: the retained XRECORD survives the save");
    }
    assert_eq!(d.foreign.version.as_deref(), Some("AC1015"), "saving keeps the kept version");
}

/// The DWG 2004 of a drawing with fields reopens with its fields (their FIELD objects are in the
/// pre-2007 groups, the format in group 4 and the values without flags) and saves the same again.
#[test]
fn a_dwg_2004_with_fields_reopens_with_them_and_saves_the_same() {
    let opts = SaveOptions { version: Some(Version::R2004), binary: false };
    let first = write_with(&with_fields(read(R2000_WITH_XRECORD.as_bytes(), "old.dxf").unwrap()), "a.dwg", &opts).unwrap();
    let back = read(&first, "a.dwg").unwrap();
    assert_eq!(back.foreign.version.as_deref(), Some("AC1018"));
    assert_eq!(contents(&back), [DATE, TWO, EXPR], "the field codes and values are read back");
    let second = write_with(&back, "b.dwg", &opts).unwrap();
    assert_eq!(field_keys(second.clone()), field_keys(first));
    assert_eq!(contents(&read(&second, "b.dwg").unwrap()), [DATE, TWO, EXPR]);
}

/// A drawing kept as AutoCAD 2007 (AC1021) is read as that, no longer as 2010: its FIELD groups
/// are the 2007 ones, so a DWG 2018 keeps the formats.
#[test]
fn fields_added_to_a_2007_drawing_keep_formats_in_dwg_2018() {
    let r2007 = write_with(&Drawing::new_metric(), "old.dwg", &SaveOptions { version: Some(Version::R2007), binary: false }).unwrap();
    let d = with_fields(read(&r2007, "old.dwg").unwrap());
    assert_eq!(d.foreign.version.as_deref(), Some("AC1021"));
    let dwg = r2018(&d, "new.dwg");
    assert_field_formats(&field_keys(dwg.clone()), "R2018");
    assert_eq!(contents(&read(&dwg, "new.dwg").unwrap()), [DATE, TWO, EXPR]);
}

fn dxf_2004(d: &Drawing) -> String {
    String::from_utf8(write_with(d, "f.dxf", &SaveOptions { version: Some(Version::R2004), binary: false }).unwrap()).unwrap()
}

/// A DXF 2004 has its FIELD objects in the 2004 groups: a reader of that version finds the
/// formats (group 4) and values, and CADCraft reads the field codes back.
#[test]
fn a_dxf_2004_has_its_fields_in_the_2004_groups() {
    let dxf = dxf_2004(&new_drawing());
    let doc = acadrust::DxfReader::from_reader(Cursor::new(dxf.clone().into_bytes())).unwrap().read().unwrap();
    let mut keys: Vec<FieldKey> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            ObjectType::Field(f) => {
                Some((f.evaluator_id.clone(), f.code.clone(), f.pre2007_format().to_string(), f.value_string.clone(), f.value.raw_type_code))
            }
            _ => None,
        })
        .collect();
    keys.sort();
    assert_field_formats(&keys, "DXF 2004");
    assert_eq!(contents(&read(dxf.as_bytes(), "f.dxf").unwrap()), [DATE, TWO, EXPR]);
}

/// The DXF text with each FIELD's format (group 4) moved from after the object list to just after
/// the code (group 2), where some pre-2007 files have it.
fn format_after_code(dxf: &str) -> String {
    let lines: Vec<&str> = dxf.lines().collect();
    let mut pairs: Vec<(&str, &str)> = lines.chunks(2).map(|c| (c[0], c.get(1).copied().unwrap_or(""))).collect();
    let mut moved = 0;
    let mut i = 0;
    while i < pairs.len() {
        if pairs[i].0.trim() != "0" || pairs[i].1.trim() != "FIELD" {
            i += 1;
            continue;
        }
        let end = pairs[i + 1..].iter().position(|p| p.0.trim() == "0").map_or(pairs.len(), |p| i + 1 + p);
        let code = (i..end).find(|&j| pairs[j].0.trim() == "2");
        let format = (i..end).find(|&j| pairs[j].0.trim() == "4");
        if let (Some(c), Some(f)) = (code, format) {
            let p = pairs.remove(f);
            pairs.insert(c + 1, p);
            moved += 1;
        }
        i = end;
    }
    assert_eq!(moved, 8, "every FIELD has a format group");
    pairs.iter().flat_map(|(a, b)| [*a, *b]).collect::<Vec<_>>().join("\n") + "\n"
}

/// A pre-2007 FIELD with its format right after the code is read too.
#[test]
fn a_pre_2007_field_with_the_format_after_the_code_is_read() {
    let dxf = format_after_code(&dxf_2004(&new_drawing()));
    assert_eq!(contents(&read(dxf.as_bytes(), "f.dxf").unwrap()), [DATE, TWO, EXPR]);
}
