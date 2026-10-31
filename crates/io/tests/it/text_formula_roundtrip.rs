//! A text's formula (`=...`) round-trips through DXF as CADCRAFTFORMULA extended data, a long
//! Hebrew one included (cut into 250-byte strings between letters).

use cadcraft_doc::{Common, Drawing, EntityKind, HAlign, MText, Space, Text, VAlign};
use cadcraft_geom::Vec3;

#[test]
fn text_and_mtext_formulas_roundtrip() {
    let mut d = Drawing::new_metric();
    let long = format!("=\"{}\"", "שלום ".repeat(120));
    let t = Text {
        backwards: false,
        upside_down: false,
        insert: Vec3::new(1.0, 2.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: "2".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
        formula: Some("=1+1".into()),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Text(t)).unwrap();
    let m = MText {
        frame: false,
        formula: Some(long.clone()),
        insert: Vec3::new(0.0, 10.0, 0.0),
        height: 2.5,
        width: 40.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "x".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(m)).unwrap();
    let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
    // The xdata application is registered (AutoCAD refuses xdata of an unknown application).
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("APPID") && text.matches("CADCRAFTFORMULA").count() >= 3, "registered once, used twice");
    let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
    let kinds: Vec<EntityKind> = back.model.iter().map(|e| e.kind.clone()).collect();
    assert!(kinds.iter().any(|k| matches!(k, EntityKind::Text(t) if t.formula.as_deref() == Some("=1+1"))));
    assert!(kinds.iter().any(|k| matches!(k, EntityKind::MText(m) if m.formula.as_deref() == Some(long.as_str()))));
}

fn text(value: &str, formula: Option<&str>) -> Text {
    Text {
        backwards: false,
        upside_down: false,
        insert: Vec3::new(0.0, 0.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
        formula: formula.map(str::to_string),
    }
}

fn mtext(contents: &str, formula: Option<&str>) -> MText {
    MText {
        frame: false,
        formula: formula.map(str::to_string),
        insert: Vec3::new(0.0, 10.0, 0.0),
        height: 2.5,
        width: 40.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: contents.into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    }
}

/// Attribute definitions, attributes and multileaders keep their formulas too (#2318 review).
#[test]
fn attribute_and_multileader_formulas_roundtrip() {
    let mut d = Drawing::new_metric();
    let def = cadcraft_doc::Attrib {
        tag: "T".into(),
        text: text("2", Some("=1+1")),
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::AttDef(def.clone())).unwrap();
    d.blocks.insert("B".into(), cadcraft_doc::Block::new("B").into());
    let ins = cadcraft_doc::Insert {
        block: "B".into(),
        insert: Vec3::new(5.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![cadcraft_doc::Attrib { text: text("6", Some("=2*3")), ..def }],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    let ml = cadcraft_doc::MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 0.0)]],
        landing: Vec3::new(10.0, 10.0, 0.0),
        dogleg: 2.0,
        text: Some(mtext("9", Some("=3*3"))),
        style: "Standard".into(),
        arrow_size: 1.0,
        spline: false,
        no_leader: false,
        arrow: String::new(),
        block: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(ml)).unwrap();
    let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
    let mut found = Vec::new();
    for e in back.model.iter() {
        match &e.kind {
            EntityKind::AttDef(a) => found.push(a.text.formula.clone()),
            EntityKind::Insert(i) => found.extend(i.attribs.iter().map(|a| a.text.formula.clone())),
            EntityKind::MLeader(m) => found.push(m.text.as_ref().and_then(|t| t.formula.clone())),
            _ => {}
        }
    }
    found.sort();
    assert_eq!(found, [Some("=1+1".to_string()), Some("=2*3".to_string()), Some("=3*3".to_string())]);
}

/// A formula replaced by plain text after reopening stays gone: its old extended data is not
/// kept as another program's data and written back (#2318 review).
#[test]
fn a_cleared_formula_is_not_restored_from_old_xdata() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Text(text("2", Some("=1+1")))).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::MText(mtext("4", Some("=2+2")))).unwrap();
    let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
    let mut back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
    let hs: Vec<_> = back.model.iter().map(|e| e.handle).collect();
    for h in hs {
        back.modify_entity(h, |e| match &mut e.kind {
            EntityKind::Text(t) => {
                t.value = "plain".into();
                t.formula = None;
            }
            EntityKind::MText(m) => {
                m.contents = "plain".into();
                m.formula = None;
            }
            _ => {}
        })
        .unwrap();
    }
    let again = cadcraft_io::read(&cadcraft_io::write(&back, "a.dxf").unwrap(), "a.dxf").unwrap();
    for e in again.model.iter() {
        match &e.kind {
            EntityKind::Text(t) => assert_eq!((t.value.as_str(), t.formula.as_deref()), ("plain", None)),
            EntityKind::MText(m) => assert_eq!((m.contents.as_str(), m.formula.as_deref()), ("plain", None)),
            _ => {}
        }
    }
}

/// A text with an AutoCAD field (its extension dictionary's ACAD_FIELD → TEXT → FIELD), as
/// another program writes it: `field` is the field code.
fn with_field(field: &str) -> String {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Text(text("old", None))).unwrap();
    // Group codes without their padding, one group a line.
    let dxf = String::from_utf8(cadcraft_io::write(&d, "a.dxf").unwrap()).unwrap().lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let at = dxf.find("\n0\nTEXT\n5\n").unwrap() + "\n0\nTEXT\n5\n".len();
    let end = at + dxf[at..].find('\n').unwrap();
    let h = dxf[at..end].trim().to_string();
    let mut out = format!("{}\n102\n{{ACAD_XDICTIONARY\n360\nFFF01\n102\n}}{}", &dxf[..end], &dxf[end..]);
    let objects = out.find("\n2\nOBJECTS\n").unwrap();
    let close = objects + out[objects..].find("\n0\nENDSEC").unwrap();
    let objs = format!(
        "\n0\nDICTIONARY\n5\nFFF01\n330\n{h}\n100\nAcDbDictionary\n280\n1\n3\nACAD_FIELD\n360\nFFF02\
         \n0\nDICTIONARY\n5\nFFF02\n330\nFFF01\n100\nAcDbDictionary\n280\n1\n3\nTEXT\n360\nFFF03\
         \n0\nFIELD\n5\nFFF03\n330\nFFF02\n100\nAcDbField\n1\nTEXT\n2\n{field}"
    );
    out.insert_str(close, &objs);
    out
}

/// A text whose AutoCAD field is replaced by plain text drops the field when saved; one that
/// keeps it writes it back (#2324 review).
#[test]
fn a_replaced_field_is_not_restored_on_reopen() {
    let src = with_field("Date: %<\\AcVar Date \\f \"yyyy\">%");
    let read = cadcraft_io::read(src.as_bytes(), "a.dxf").unwrap();
    let t = read.model.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some((e.handle, t.clone())) } else { None }).unwrap();
    assert!(t.1.formula.as_deref().is_some_and(|f| f.contains("%<\\AcVar")), "{:?}", t.1.formula);
    // Kept: written back.
    let again = cadcraft_io::read(&cadcraft_io::write(&read, "a.dxf").unwrap(), "a.dxf").unwrap();
    assert!(again.model.iter().any(|e| matches!(&e.kind, EntityKind::Text(x) if x.formula.is_some())));
    // Replaced: gone for good.
    let mut edited = read;
    edited
        .modify_entity(t.0, |e| {
            if let EntityKind::Text(x) = &mut e.kind {
                x.value = "plain".into();
                x.formula = None;
            }
        })
        .unwrap();
    let bytes = cadcraft_io::write(&edited, "a.dxf").unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("AcDbField"), "the field objects are not written");
    let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
    let t = back.model.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert_eq!((t.value.as_str(), t.formula.as_deref()), ("plain", None));
}
