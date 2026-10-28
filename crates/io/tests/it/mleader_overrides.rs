//! A multileader's own leader colour, linetype and lineweight, text colour and frame and block
//! colour (gap FF2) through DXF and DWG: written with their override flags (group 90) and read
//! back, from our files and from a foreign record.

use cadcraft_doc::color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_geom::Vec3;

fn mleaders(d: &Drawing) -> Vec<&MLeader> {
    d.model.iter().filter_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m) } else { None }).collect()
}

fn text_mleader() -> MLeader {
    MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)]],
        landing: Vec3::new(10.0, 5.0, 0.0),
        dogleg: 2.0,
        text: Some(MText {
            insert: Vec3::new(12.625, 5.0, 0.0),
            height: 2.5,
            width: 0.0,
            attach: 4,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "NOTE".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }),
        arrow_size: 2.5,
        ..MLeader::default()
    }
}

fn block_mleader() -> MLeader {
    MLeader {
        leaders: vec![vec![Vec3::new(0.0, 30.0, 0.0)]],
        landing: Vec3::new(10.0, 40.0, 0.0),
        block: Some(Insert {
            block: "PIN".into(),
            insert: Vec3::new(11.0, 40.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
        ..MLeader::default()
    }
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    if d.linetype("DASHED").is_none() {
        d.linetypes.extend(library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
    }
    let mut blk = Block::new("PIN");
    blk.entities.push(Entity::new(Handle(0x700), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 1.0 })));
    d.blocks.insert(blk.name.clone(), std::sync::Arc::new(blk));
    d
}

fn dxf_text(d: &Drawing) -> String {
    String::from_utf8(cadcraft_io::write(d, "x.dxf").unwrap()).unwrap()
}

/// The groups of the first MULTILEADER record of a DXF text, as (code, value) pairs.
fn record(dxf: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let pairs: Vec<(i32, String)> = lines.chunks(2).filter_map(|p| Some((p.first()?.parse().ok()?, p.get(1)?.to_string()))).collect();
    let start = pairs.iter().position(|(c, v)| *c == 0 && v == "MULTILEADER").unwrap();
    pairs[start + 1..].iter().take_while(|(c, _)| *c != 0).cloned().collect()
}

/// The override flags (group 90 after the context data) of the first MULTILEADER.
fn flags(dxf: &str) -> i64 {
    record(dxf).iter().rev().find(|(c, _)| *c == 90).and_then(|(_, v)| v.parse::<i64>().ok()).unwrap()
}

#[test]
fn overrides_survive_dxf_with_their_flags() {
    let mut d = drawing();
    let text = MLeader {
        leader_color: Color::Index(1),
        leader_linetype: "DASHED".into(),
        leader_lineweight: Lineweight::Mm100(50),
        text_color: Color::True(Rgb(10, 120, 250)),
        text_frame: true,
        ..text_mleader()
    };
    let block =
        MLeader { block_color: Color::Index(3), leader_lineweight: Lineweight::ByLayer, leader_linetype: "ByLayer".into(), ..block_mleader() };
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(text.clone())).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(block.clone())).unwrap();
    let dxf = dxf_text(&d);
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let got = mleaders(&back);
    assert_eq!(got.len(), 2);
    assert_eq!(*got[0], text);
    assert_eq!(*got[1], block);
    let f = flags(&dxf);
    for bit in [0x2, 0x4, 0x8, 0x8000, 0x2_0000] {
        assert_ne!(f & bit, 0, "flag {bit:#x} in {f:#x}");
    }
    assert_eq!(f & 0x10_0000, 0, "no block colour override on the text multileader");
    // Without its override flag the linetype handle is the style's: ByBlock.
    let mut lines: Vec<String> = dxf.lines().map(|l| l.trim().to_string()).collect();
    let at = lines.iter().position(|l| l == "AcDbMLeader").unwrap();
    let flag_at = (at..lines.len() - 1).rev().find(|i| lines[*i] == "90" && lines[i + 1] == f.to_string()).unwrap();
    lines[flag_at + 1] = (f & !0x4).to_string();
    let unflagged = lines.join("\n");
    let back = cadcraft_io::read_dxf(unflagged.as_bytes()).unwrap();
    assert_eq!(mleaders(&back)[0].leader_linetype, "ByBlock");
    // A plain multileader writes ByBlock and no override flags for them.
    let mut plain = drawing();
    plain.add(&Space::Model, Common::default(), EntityKind::MLeader(text_mleader())).unwrap();
    let dxf = dxf_text(&plain);
    let f = flags(&dxf);
    for bit in [0x2, 0x4, 0x8, 0x8000, 0x2_0000, 0x10_0000] {
        assert_eq!(f & bit, 0, "flag {bit:#x} in {f:#x}");
    }
    assert!(record(&dxf).iter().any(|(c, v)| *c == 91 && v == "-1056964608"), "ByBlock leader colour");
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    assert_eq!(*mleaders(&back)[0], text_mleader());
}

/// A DXF file with one MULTILEADER record (`body`: group/value lines).
fn dxf_with(body: &str) -> String {
    format!("0\nSECTION\n2\nENTITIES\n0\nMULTILEADER\n5\nA0\n100\nAcDbEntity\n8\n0\n100\nAcDbMLeader\n{body}0\nENDSEC\n0\nEOF\n")
}

#[test]
fn foreign_overrides_read() {
    // Magenta (index 6) leader, 0.50 mm, blue text (RGB) with a frame.
    let body = concat!(
        "270\n2\n300\nCONTEXT_DATA{\n40\n1.0\n10\n10.0\n20\n5.0\n30\n0.0\n41\n2.5\n290\n1\n304\nA\n",
        "12\n20.0\n22\n5.0\n32\n0.0\n171\n1\n296\n0\n",
        "302\nLEADER{\n10\n6.0\n20\n5.0\n30\n0.0\n40\n4.0\n304\nLEADER_LINE{\n10\n0.0\n20\n0.0\n30\n0.0\n305\n}\n303\n}\n",
        "301\n}\n170\n1\n91\n-1023410170\n171\n50\n292\n1\n92\n-1040187137\n172\n2\n",
    );
    let d = cadcraft_io::read_dxf(dxf_with(body).as_bytes()).unwrap();
    let m = mleaders(&d)[0];
    assert_eq!(m.leader_color, Color::Index(6));
    assert_eq!(m.leader_lineweight, Lineweight::Mm100(50));
    assert_eq!(m.text_color, Color::True(Rgb(0, 0, 255)));
    assert!(m.text_frame);
    assert_eq!(m.leader_linetype, "ByBlock", "no linetype handle: ByBlock");
}

#[test]
fn hostile_overrides_do_not_panic() {
    let cases = [
        "91\n99999999999999\n92\nabc\n93\n-1\n341\nZZ\n171\n70000\n292\n7\n",
        "91\n-9223372036854775808\n171\n-32768\n341\n0\n300\nCONTEXT_DATA{\n90\n1e300\n93\nnan\n301\n}\n172\n1\n",
        "91\n0\n92\n-939524096\n93\n2147483647\n171\n-4\n",
    ];
    for body in cases {
        let d = cadcraft_io::read_dxf(dxf_with(body).as_bytes()).unwrap();
        for m in mleaders(&d) {
            let again = cadcraft_io::read_dxf(dxf_text(&d).as_bytes()).unwrap();
            assert_eq!(mleaders(&again).first().map(|x| x.leader_color), Some(m.leader_color));
        }
        let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
    }
}

/// Through the DWG bridge (acadrust converts our DXF to DWG and back).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn overrides_survive_dwg() {
    let mut d = drawing();
    let m = MLeader {
        leader_color: Color::Index(1),
        leader_lineweight: Lineweight::Mm100(35),
        text_color: Color::True(Rgb(200, 10, 10)),
        text_frame: true,
        leader_linetype: "DASHED".into(),
        ..text_mleader()
    };
    let b = MLeader { block_color: Color::Index(3), ..block_mleader() };
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m.clone())).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(b.clone())).unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let got = mleaders(&back);
    assert_eq!(got.len(), 2);
    assert_eq!(*got[0], m);
    assert_eq!(*got[1], b);
}
