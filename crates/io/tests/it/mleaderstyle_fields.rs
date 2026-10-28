//! MLEADERSTYLE content, leader, text and block settings (gap F10) through DXF and DWG: our own
//! styles round trip, a foreign record reads, hostile values are refused.

use cadcraft_doc::color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_geom::Vec3;

fn bubble() -> MLeaderStyle {
    MLeaderStyle {
        name: "Bubble".into(),
        arrow_size: 2.0,
        text_height: 3.0,
        landing_gap: 1.0,
        dogleg: 4.0,
        content: MLeaderContent::Block,
        leader_type: MLeaderLineType::Spline,
        max_points: 5,
        first_angle: std::f64::consts::FRAC_PI_4,
        second_angle: std::f64::consts::FRAC_PI_2,
        landing: false,
        leader_color: Color::Index(1),
        leader_linetype: "DASHED".into(),
        leader_lineweight: Lineweight::Mm100(50),
        text_color: Color::True(Rgb(10, 20, 30)),
        text_frame: true,
        default_text: "SEE DETAIL".into(),
        block: "PIN".into(),
        block_color: Color::Index(3),
        block_scale: 2.5,
        block_rotation: 0.5,
        block_center: false,
        scale: 4.0,
        break_size: 0.5,
        description: "Balloons".into(),
        ..MLeaderStyle::default()
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
    d.mleader_styles.push(bubble());
    d.mleader_styles.push(MLeaderStyle {
        name: "Plain".into(),
        leader_type: MLeaderLineType::None,
        content: MLeaderContent::None,
        ..MLeaderStyle::default()
    });
    d
}

fn styles(d: &Drawing) -> Vec<&MLeaderStyle> {
    d.mleader_styles.iter().filter(|s| s.name == "Bubble" || s.name == "Plain").collect()
}

#[test]
fn style_settings_survive_dxf() {
    let d = drawing();
    let text = String::from_utf8(cadcraft_io::write(&d, "x.dxf").unwrap()).unwrap();
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(styles(&back), styles(&d));
    // The block is in the file and PURGE-safe: its BLOCK_RECORD is what 343 points at.
    assert!(back.block("PIN").is_some());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn style_settings_survive_dwg() {
    let d = drawing();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let got = styles(&back);
    assert_eq!(got.len(), 2);
    // What the DWG bridge (acadrust 0.6) keeps of a style: content and leader type, landing,
    // leader colour, linetype and lineweight, text frame, the block and the overall scale. The
    // maximum points, angle constraints, default text, block colour, scale, rotation and
    // connection, break size, description and a true-colour text colour come back as defaults.
    let (a, b) = (got[0], bubble());
    assert_eq!((a.content, a.leader_type, a.landing, a.text_frame), (b.content, b.leader_type, b.landing, b.text_frame));
    assert_eq!((a.leader_color, a.leader_linetype.as_str(), a.leader_lineweight), (b.leader_color, b.leader_linetype.as_str(), b.leader_lineweight));
    assert_eq!(
        (a.block.as_str(), a.scale, a.arrow_size, a.text_height, a.dogleg),
        (b.block.as_str(), b.scale, b.arrow_size, b.text_height, b.dogleg)
    );
    assert_eq!(got[1], styles(&d)[1]);
}

/// A DXF file with one MLEADERSTYLE named "S" in its dictionary (`body`: its groups).
fn dxf_with(body: &str) -> String {
    format!(
        "0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\nC\n100\nAcDbDictionary\n3\nACAD_MLEADERSTYLE\n350\nD\n\
         0\nDICTIONARY\n5\nD\n330\nC\n100\nAcDbDictionary\n3\nS\n350\nE\n0\nMLEADERSTYLE\n5\nE\n330\nD\n100\nAcDbMLeaderStyle\n{body}0\nENDSEC\n0\nEOF\n"
    )
}

#[test]
fn a_foreign_style_reads() {
    let body = "179\n2\n170\n0\n90\n3\n40\n0.5235987755982988\n41\n0.0\n173\n2\n91\n-1023410175\n92\n30\n290\n1\n291\n0\n43\n2.0\n3\nNotes\n44\n1.5\n300\nTBD\n93\n-1056964608\n45\n2.5\n292\n1\n142\n10.0\n143\n2.0\n177\n1\n";
    let d = cadcraft_io::read_dxf(dxf_with(body).as_bytes()).unwrap();
    let s = d.mleader_styles.iter().find(|s| s.name == "S").expect("style S");
    assert_eq!((s.content, s.leader_type, s.max_points), (MLeaderContent::None, MLeaderLineType::Spline, 3));
    assert!((s.first_angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12 && s.second_angle == 0.0);
    assert_eq!((s.leader_color, s.leader_lineweight, s.text_color), (Color::Index(1), Lineweight::Mm100(30), Color::ByBlock));
    assert_eq!((s.landing, s.text_frame, s.block_center), (false, true, false));
    assert_eq!((s.scale, s.break_size, s.description.as_str(), s.default_text.as_str()), (10.0, 2.0, "Notes", "TBD"));
}

#[test]
fn hostile_style_values_read_as_defaults() {
    let body = "170\n99\n90\n-5\n40\nnan\n41\n1e300\n173\n-1\n91\nabc\n92\n99999\n142\n-3\n143\n-1\n47\n0\n141\n1e99\n340\nZZ\n343\nFFFFFFFFFFFFFFFFFFFFFF\n";
    let d = cadcraft_io::read_dxf(dxf_with(body).as_bytes()).unwrap();
    let s = d.mleader_styles.iter().find(|s| s.name == "S").expect("style S");
    let def = MLeaderStyle::default();
    assert_eq!((s.content, s.leader_type, s.max_points), (def.content, def.leader_type, def.max_points));
    assert_eq!((s.first_angle, s.second_angle, s.scale, s.block_scale, s.block_rotation), (0.0, 0.0, 1.0, 1.0, 0.0));
    assert_eq!((s.leader_linetype.as_str(), s.block.as_str()), ("ByBlock", ""));
    assert!(s.break_size >= 0.0);
    // And it saves.
    let _ = cadcraft_io::write(&d, "x.dxf").unwrap();
}
