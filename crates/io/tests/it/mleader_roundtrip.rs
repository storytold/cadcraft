//! Multileaders are saved as native MULTILEADER entities (with their MLEADERSTYLE) and read
//! back as multileaders (#459), instead of being broken into LEADER + MTEXT / INSERT.

use cadcraft_doc::*;
use cadcraft_geom::Vec3;

/// Saved as 2007 DXF: older versions have no MULTILEADER (they get LEADER + MTEXT / INSERT).
fn roundtrip(d: &Drawing) -> (String, Drawing) {
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2007), binary: false };
    let text = String::from_utf8(cadcraft_io::write_with(d, "x.dxf", &opts).unwrap()).unwrap();
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    (text, back)
}

fn mleaders(d: &Drawing) -> Vec<&MLeader> {
    d.model.iter().filter_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m) } else { None }).collect()
}

fn text_mleader() -> MLeader {
    MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 3.0, 0.0)], vec![Vec3::new(1.0, -2.0, 0.0)]],
        landing: Vec3::new(5.0, 4.0, 0.0),
        dogleg: 0.75,
        text: Some(MText {
            insert: Vec3::new(5.9, 4.0, 0.0),
            height: 0.4,
            width: 6.0,
            attach: 4,
            rotation: 0.0,
            style: "Notes".into(),
            contents: "FIRST\\PSECOND".into(),
            line_spacing: 1.25,
            line_spacing_exact: true,
            columns: None,
        }),
        style: "Callout".into(),
        arrow_size: 0.3,
        spline: false,
        block: None,
    }
}

fn insert(block: &str, at: Vec3) -> Insert {
    Insert {
        block: block.into(),
        insert: at,
        scale: Vec3::new(2.0, 2.0, 1.0),
        rotation: 0.5,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    }
}

#[test]
fn text_multileader_survives_as_multileader() {
    let mut d = Drawing::new_metric();
    d.text_styles.push(TextStyle { name: "Notes".into(), ..TextStyle::default() });
    d.mleader_styles.push(MLeaderStyle { name: "Callout".into(), arrow_size: 0.3, ..MLeaderStyle::default() });
    let m = text_mleader();
    d.add(&Space::Model, Common { layer: "0".into(), ..Common::default() }, EntityKind::MLeader(m.clone())).unwrap();
    let (text, back) = roundtrip(&d);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(lines.chunks(2).any(|p| p == ["0", "MULTILEADER"]), "a MULTILEADER entity is written");
    assert!(text.contains("AcDbMLeader"), "with its CLASS and subclass");
    assert!(!back.model.iter().any(|e| matches!(e.kind, EntityKind::Leader(_) | EntityKind::MText(_))), "no LEADER/MTEXT pieces");
    let got = mleaders(&back);
    assert_eq!(got.len(), 1);
    assert_eq!(*got[0], m, "same multileader after save and reopen");
    assert!(back.mleader_styles.iter().any(|s| s.name == "Callout" && s.arrow_size == 0.3));
}

#[test]
fn spline_leader_with_block_content_and_attributes() {
    let mut d = Drawing::new_metric();
    let mut blk = Block::new("BUBBLE");
    blk.entities.push(Entity::new(Handle(0x200), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 1.0 })));
    let tag_text = Text {
        insert: Vec3::new(-0.2, -0.1, 0.0),
        align_pt: None,
        height: 0.25,
        value: "?".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::default(),
        valign: VAlign::default(),
    };
    blk.entities.push(Entity::new(
        Handle(0x201),
        EntityKind::AttDef(Attrib {
            tag: "NUM".into(),
            text: tag_text,
            invisible: false,
            constant: false,
            prompt: "Number".into(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        }),
    ));
    d.blocks.insert(blk.name.clone(), std::sync::Arc::new(blk));
    let mut ins = insert("BUBBLE", Vec3::new(12.0, 3.0, 0.0));
    // The attribute placed like its definition under the block reference.
    let blk = d.block("BUBBLE").unwrap().clone();
    let xf = ins.transform(blk.base.xy());
    let Some(EntityKind::AttDef(ad)) = blk.entities.iter().nth(1).map(|e| &e.kind) else { unreachable!() };
    let mut k = EntityKind::Text(ad.text.clone());
    k.transform(&xf);
    let EntityKind::Text(mut placed) = k else { unreachable!() };
    placed.value = "42".into();
    ins.attribs.push(Attrib {
        tag: "NUM".into(),
        text: placed,
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    });
    let m = MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 5.0, 0.0), Vec3::new(8.0, 4.0, 0.0)]],
        landing: Vec3::new(10.0, 3.0, 0.0),
        dogleg: 1.0,
        text: None,
        style: "Standard".into(),
        arrow_size: 0.18,
        spline: true,
        block: Some(ins),
    };
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m.clone())).unwrap();
    let (_, back) = roundtrip(&d);
    assert!(!back.model.iter().any(|e| matches!(e.kind, EntityKind::Insert(_) | EntityKind::Leader(_))));
    let got = mleaders(&back);
    assert_eq!(got.len(), 1);
    let g = got[0];
    assert!(g.spline);
    assert_eq!(g.leaders, m.leaders);
    let b = g.block.as_ref().expect("block content");
    assert_eq!(b.block, "BUBBLE");
    assert_eq!(b.insert, Vec3::new(12.0, 3.0, 0.0));
    assert_eq!(b.scale, Vec3::new(2.0, 2.0, 1.0));
    assert!((b.rotation - 0.5).abs() < 1e-12);
    assert_eq!(b.attribs.len(), 1);
    assert_eq!(b.attribs[0].tag, "NUM");
    assert_eq!(b.attribs[0].text.value, "42");
    let want = &m.block.as_ref().unwrap().attribs[0].text;
    assert!((b.attribs[0].text.insert - want.insert).len() < 1e-9);
}

#[test]
fn multileader_without_leader_lines() {
    let mut d = Drawing::new_metric();
    let mut m = text_mleader();
    m.leaders.clear();
    m.dogleg = 0.0;
    m.style = "Standard".into();
    if let Some(t) = &mut m.text {
        t.insert = m.landing;
        t.attach = 1;
        t.style = "Standard".into();
    }
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m.clone())).unwrap();
    let (_, back) = roundtrip(&d);
    let got = mleaders(&back);
    assert_eq!(got.len(), 1);
    assert_eq!(*got[0], m);
}

#[test]
fn multileader_in_a_block_and_in_paper_space() {
    let mut d = Drawing::new_metric();
    d.text_styles.push(TextStyle { name: "Notes".into(), ..TextStyle::default() });
    d.mleader_styles.push(MLeaderStyle { name: "Callout".into(), ..MLeaderStyle::default() });
    let m = text_mleader();
    let mut blk = Block::new("NOTE");
    blk.entities.push(Entity::new(Handle(0x300), EntityKind::MLeader(m.clone())));
    d.blocks.insert(blk.name.clone(), std::sync::Arc::new(blk));
    let layout = d.layouts[0].name.clone();
    d.add(&Space::Paper(layout.clone()), Common::default(), EntityKind::MLeader(m.clone())).unwrap();
    let (_, back) = roundtrip(&d);
    let in_block = back.block("NOTE").unwrap().entities.iter().filter(|e| matches!(&e.kind, EntityKind::MLeader(x) if *x == m)).count();
    assert_eq!(in_block, 1);
    let l = back.layouts.iter().find(|l| l.name == layout).unwrap();
    assert_eq!(l.entities.iter().filter(|e| matches!(&e.kind, EntityKind::MLeader(x) if *x == m)).count(), 1);
}

/// A DXF file with one entity record (`body`: group/value lines) in its ENTITIES section.
fn dxf_with(body: &str) -> String {
    format!("0\nSECTION\n2\nENTITIES\n0\nMULTILEADER\n5\nA0\n100\nAcDbEntity\n8\n0\n100\nAcDbMLeader\n{body}0\nENDSEC\n0\nEOF\n")
}

#[test]
fn foreign_multileader_with_two_landings() {
    // Two leader roots (left and right of the text), each with a line; MText content.
    let body = "\
270\n2\n300\nCONTEXT_DATA{\n40\n1.0\n10\n5.0\n20\n5.0\n30\n0.0\n41\n2.5\n140\n1.5\n290\n1\n304\nHELLO\n\
12\n6.0\n22\n5.0\n32\n0.0\n13\n1.0\n23\n0.0\n33\n0.0\n171\n1\n174\n2\n296\n0\n\
302\nLEADER{\n10\n5.0\n20\n5.0\n30\n0.0\n11\n1.0\n21\n0.0\n31\n0.0\n40\n0.5\n\
304\nLEADER_LINE{\n10\n0.0\n20\n0.0\n30\n0.0\n91\n0\n305\n}\n303\n}\n\
302\nLEADER{\n10\n20.0\n20\n5.0\n30\n0.0\n40\n0.5\n304\nLEADER_LINE{\n10\n25.0\n20\n0.0\n30\n0.0\n305\n}\n303\n}\n\
301\n}\n170\n1\n291\n1\n172\n2\n";
    let d = cadcraft_io::read_dxf(dxf_with(body).as_bytes()).unwrap();
    let got = mleaders(&d);
    assert_eq!(got.len(), 1);
    let m = got[0];
    assert_eq!(m.landing, Vec3::new(5.0, 5.0, 0.0));
    assert_eq!(m.dogleg, 0.5);
    assert_eq!(m.arrow_size, 1.5);
    assert_eq!(m.leaders, vec![vec![Vec3::ZERO], vec![Vec3::new(25.0, 0.0, 0.0), Vec3::new(20.0, 5.0, 0.0)]]);
    let t = m.text.as_ref().unwrap();
    assert_eq!(t.contents, "HELLO");
    assert_eq!(t.insert, Vec3::new(6.0, 5.0, 0.0));
    assert_eq!(t.height, 2.5);
    assert_eq!(t.attach, 4);
}

#[test]
fn hostile_multileaders_do_not_panic() {
    let many_lines: String = (0..10_000).map(|i| format!("304\nLEADER_LINE{{\n10\n{i}\n20\nnan\n30\n1e999\n")).collect();
    let many_roots: String = (0..2_000).map(|_| "302\nLEADER{\n10\n1e999\n").collect();
    let cases = [
        // Unterminated sections.
        "300\nCONTEXT_DATA{\n302\nLEADER{\n304\nLEADER_LINE{\n10\n1\n".to_string(),
        "300\nCONTEXT_DATA{\n".to_string(),
        // Closers without openers, attribute values without definitions, bad handles.
        "305\n}\n303\n}\n301\n}\n302\nlonely\n330\nZZZ\n302\nvalue\n340\n-1\n344\nFFFFFFFFFFFFFFFFFFFF\n172\n1\n".to_string(),
        // Garbage numbers and switches.
        "300\nCONTEXT_DATA{\n41\n-5\n140\nabc\n171\n99999999999\n290\n1\n296\n1\n16\n0\n26\n0\n301\n}\n170\n-7\n41\n-3\n".to_string(),
        format!("300\nCONTEXT_DATA{{\n302\nLEADER{{\n{many_lines}"),
        format!("300\nCONTEXT_DATA{{\n{many_roots}"),
    ];
    for body in &cases {
        let d = cadcraft_io::read_dxf(dxf_with(body).as_bytes()).unwrap();
        for m in mleaders(&d) {
            assert!(m.leaders.len() <= 4096);
            assert!(m.landing.x.is_finite() && m.landing.y.is_finite());
            assert!(m.leaders.iter().flatten().all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite()));
            assert!(m.dogleg.is_finite() && m.dogleg >= 0.0);
            if let Some(t) = &m.text {
                assert!((1..=9).contains(&t.attach) && t.height > 0.0);
            }
        }
        // And the result saves.
        let _ = cadcraft_io::write_dxf(&d);
    }
}

/// Through the DWG bridge (acadrust converts our DXF to DWG and back).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn multileader_through_dwg() {
    let mut d = Drawing::new_metric();
    d.text_styles.push(TextStyle { name: "Notes".into(), ..TextStyle::default() });
    d.mleader_styles.push(MLeaderStyle { name: "Callout".into(), arrow_size: 0.3, ..MLeaderStyle::default() });
    let m = text_mleader();
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m.clone())).unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let got = mleaders(&back);
    assert_eq!(got.len(), 1, "{:?}", back.model.iter().map(|e| e.kind.dxf_name()).collect::<Vec<_>>());
    assert_eq!(*got[0], m);
}
