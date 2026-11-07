//! A drawing that was never read from a file has no kept DXF version, so the DWG bridge used to
//! read its FIELD groups as R2000 and acadrust dropped their flags, cached values and formats
//! (#551). With field texts the bridge is now read as the version `dxf_field` writes them in.
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
    let mut d = Drawing::new_metric();
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
