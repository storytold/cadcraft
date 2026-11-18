//! #1645: a diameter dimension's text in its `*D` block, and in the MTEXT EXPLODE leaves, holds
//! the diameter sign as `%%c`, as CADCraft's own TEXT and MTEXT keep it. The decoded ⌀ (U+2300)
//! is in no SHX font, so other programs plotted the dimension in an SHX style ("romans.shx")
//! without its sign. CADCraft still draws and reads the sign.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

fn h(v: &Value) -> Handle {
    v["handle"].as_str().and_then(Handle::parse_hex).unwrap()
}

/// The issue's drawing: an SHX text style, a dimension style using it, a diameter dimension
/// of a circle of radius 30.
fn session() -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("units", &json!({"measurement": "metric"})).ok();
    s.execute("style", &json!({"name": "R", "font": "romans.shx", "current": true})).unwrap();
    s.execute("dimstyle", &json!({"name": "D", "current": true, "DIMTXSTY": "R", "DIMTXT": 5, "DIMASZ": 5, "DIMDEC": 0})).unwrap();
    let c = h(&s.execute("circle", &json!({"center": [0, 0], "radius": 30})).unwrap());
    let dim = h(&s.execute("dimdiameter", &json!({"handle": c.hex(), "at": [40, 40]})).unwrap());
    (s, dim)
}

/// The MTEXT contents (group 1) inside each `*D…` block of a DXF.
fn dblock_texts(dxf: &str) -> Vec<String> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let (mut in_d, mut in_mtext) = (false, false);
    for pair in lines.chunks(2) {
        let [code, value] = pair else { continue };
        match (*code, *value) {
            ("0", "BLOCK") => in_d = false,
            ("2", name) if !in_mtext && name.starts_with("*D") => in_d = true,
            ("0", "MTEXT") => in_mtext = in_d,
            ("0", _) => in_mtext = false,
            ("1", text) if in_mtext => out.push(text.to_string()),
            _ => {}
        }
    }
    out
}

#[test]
fn the_dimension_block_writes_the_sign_as_a_code() {
    let (s, dim) = session();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.entity(dim).unwrap().kind else { panic!("not a dimension") };
    assert!(matches!(dm.kind, cadcraft_engine::doc::DimKind::Diameter));
    let texts = dblock_texts(&cadcraft_io::write_dxf(d));
    assert_eq!(texts, vec!["%%c60".to_string()], "the *D block's MTEXT");
    // Read back, CADCraft still shows the sign.
    let read = cadcraft_io::read_dxf(cadcraft_io::write_dxf(d).as_bytes()).unwrap();
    let dm = read.model.iter().find_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm.clone()) } else { None }).unwrap();
    let st = read.dim_styles.iter().find(|x| x.name.eq_ignore_ascii_case(&dm.style)).cloned().unwrap_or_default();
    assert_eq!(cadcraft_render::dimension_geometry(&dm, &st, 1.0).value, "⌀60");
}

#[test]
fn exploding_keeps_the_sign_as_a_code() {
    let (mut s, dim) = session();
    s.execute("explode", &json!({"handles": [dim.hex()]})).unwrap();
    let d = s.doc().unwrap();
    let texts: Vec<String> =
        d.model.iter().filter_map(|e| if let EntityKind::MText(m) = &e.kind { Some(m.contents.clone()) } else { None }).collect();
    assert_eq!(texts, vec!["%%c60".to_string()]);
    let saved = cadcraft_io::write_dxf(d);
    assert!(!saved.contains('⌀'), "no U+2300 is saved");
}
