//! Multileaders with several landings (leader roots, gap FF2): each root keeps its own landing,
//! dogleg and leader lines through DXF and DWG instead of being folded into the first root's
//! lines, and hostile `LEADER{` sections read without panics.

use cadcraft_doc::*;
use cadcraft_geom::Vec3;

fn mleaders(d: &Drawing) -> Vec<&MLeader> {
    d.model.iter().filter_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m) } else { None }).collect()
}

/// Text between two landings: two leaders to the left landing, one to the right one.
fn two_landings() -> MLeader {
    MLeader {
        leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)], vec![Vec3::new(0.0, 12.0, 0.0), Vec3::new(3.0, 11.0, 0.0)]],
        landing: Vec3::new(10.0, 5.0, 0.0),
        dogleg: 2.0,
        text: Some(MText {
            insert: Vec3::new(12.625, 5.0, 0.0),
            height: 2.5,
            width: 0.0,
            attach: 4,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "LEFT AND RIGHT".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }),
        style: "Standard".into(),
        arrow_size: 2.5,
        roots: vec![MLeaderRoot { landing: Vec3::new(40.0, 5.0, 0.0), dogleg: 1.5, leaders: vec![vec![Vec3::new(50.0, -4.0, 0.0)]] }],
        ..MLeader::default()
    }
}

fn dxf_text(d: &Drawing) -> String {
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2018), binary: false };
    String::from_utf8(cadcraft_io::write_with(d, "x.dxf", &opts).unwrap()).unwrap()
}

/// The groups of the first MULTILEADER record of a DXF text, as (code, value) pairs.
fn record(dxf: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let pairs: Vec<(i32, String)> = lines.chunks(2).filter_map(|p| Some((p.first()?.parse().ok()?, p.get(1)?.to_string()))).collect();
    let start = pairs.iter().position(|(c, v)| *c == 0 && v == "MULTILEADER").unwrap();
    pairs[start + 1..].iter().take_while(|(c, _)| *c != 0).cloned().collect()
}

#[test]
fn several_landings_survive_dxf() {
    let mut d = Drawing::new_metric();
    let m = two_landings();
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m.clone())).unwrap();
    let text = dxf_text(&d);
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let got = mleaders(&back);
    assert_eq!(got.len(), 1);
    assert_eq!(*got[0], m, "every landing keeps its own dogleg and leader lines");
    // Written as two LEADER sections, the second dogleg pointing back at the text.
    let rec = record(&text);
    assert_eq!(rec.iter().filter(|(c, v)| *c == 302 && v == "LEADER{").count(), 2);
    let dirs: Vec<f64> = rec.iter().filter(|(c, _)| *c == 11).filter_map(|(_, v)| v.parse().ok()).collect();
    assert!(dirs.contains(&-1.0) && dirs.contains(&1.0), "{dirs:?}");
    // Leader lines are numbered over both roots, the roots in order.
    let lines: Vec<&str> = rec.iter().filter(|(c, _)| *c == 91).map(|(_, v)| v.as_str()).collect();
    for n in ["0", "1", "2"] {
        assert!(lines.contains(&n), "{lines:?}");
    }
    // Saved again, nothing changes.
    let again = cadcraft_io::read_dxf(dxf_text(&back).as_bytes()).unwrap();
    assert_eq!(mleaders(&again).first().copied(), Some(&m));
}

#[test]
fn a_root_without_lines_is_not_written_and_older_files_get_a_leader_per_line() {
    let mut d = Drawing::new_metric();
    let mut m = two_landings();
    m.roots.push(MLeaderRoot { landing: Vec3::new(60.0, 5.0, 0.0), dogleg: 1.0, leaders: Vec::new() });
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m.clone())).unwrap();
    let back = cadcraft_io::read_dxf(dxf_text(&d).as_bytes()).unwrap();
    assert_eq!(mleaders(&back)[0].roots.len(), 1, "a landing without leader lines is dropped");
    // R2000 has no MULTILEADER: one LEADER per line, each ending at its own landing.
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2000), binary: false };
    let old = cadcraft_io::read_dxf(&cadcraft_io::write_with(&d, "x.dxf", &opts).unwrap()).unwrap();
    let ends: Vec<Vec3> =
        old.model.iter().filter_map(|e| if let EntityKind::Leader(l) = &e.kind { l.vertices.last().copied() } else { None }).collect();
    assert_eq!(ends, vec![m.landing, m.landing, Vec3::new(40.0, 5.0, 0.0)]);
}

/// A DXF file with one MULTILEADER record (`body`: group/value lines).
fn dxf_with(body: &str) -> String {
    format!("0\nSECTION\n2\nENTITIES\n0\nMULTILEADER\n5\nA0\n100\nAcDbEntity\n8\n0\n100\nAcDbMLeader\n{body}0\nENDSEC\n0\nEOF\n")
}

#[test]
fn a_foreign_first_root_without_lines_gives_way_to_the_next() {
    // Three LEADER sections; the first has no line, the third has the dogleg switched off by
    // the entity (291 0).
    let root = |x: f64, line: bool| {
        let l = if line { format!("304\nLEADER_LINE{{\n10\n{}\n20\n0.0\n30\n0.0\n305\n}}\n", x + 1.0) } else { String::new() };
        format!("302\nLEADER{{\n10\n{x}\n20\n5.0\n30\n0.0\n40\n0.75\n{l}303\n}}\n")
    };
    let body = format!(
        "270\n2\n300\nCONTEXT_DATA{{\n40\n1.0\n41\n2.5\n290\n1\n304\nNOTE\n12\n6.0\n22\n5.0\n32\n0.0\n171\n1\n296\n0\n{}{}{}301\n}}\n170\n1\n172\n2\n",
        root(1.0, false),
        root(2.0, true),
        root(20.0, true)
    );
    let d = cadcraft_io::read_dxf(dxf_with(&body).as_bytes()).unwrap();
    let m = mleaders(&d)[0];
    assert_eq!((m.landing, m.dogleg, m.leaders.clone()), (Vec3::new(2.0, 5.0, 0.0), 0.75, vec![vec![Vec3::new(3.0, 0.0, 0.0)]]));
    assert_eq!(m.roots.len(), 1);
    assert_eq!(m.roots[0].landing, Vec3::new(20.0, 5.0, 0.0));
    let off = cadcraft_io::read_dxf(dxf_with(&body.replace("170\n1\n172", "170\n1\n291\n0\n172")).as_bytes()).unwrap();
    let m = mleaders(&off)[0];
    assert_eq!((m.dogleg, m.roots[0].dogleg), (0.0, 0.0), "doglegs switched off");
}

#[test]
fn hostile_roots_do_not_panic() {
    let roots: String =
        (0..300).map(|i| format!("302\nLEADER{{\n10\n{i}\n20\n1e999\n40\nnan\n304\nLEADER_LINE{{\n10\n{i}\n20\n0\n305\n}}\n303\n}}\n")).collect();
    let cases = [
        format!("300\nCONTEXT_DATA{{\n12\n1\n22\n1\n290\n1\n304\nX\n41\n1e308\n{roots}301\n}}\n172\n2\n"),
        // Roots with lines but no landing, unterminated roots.
        "300\nCONTEXT_DATA{\n302\nLEADER{\n304\nLEADER_LINE{\n10\n1\n305\n}\n303\n}\n302\nLEADER{\n304\nLEADER_LINE{\n10\n2\n".to_string(),
        "300\nCONTEXT_DATA{\n302\nLEADER{\n40\n-4\n10\n5\n304\nLEADER_LINE{\n10\n1\n305\n}\n303\n}\n302\nLEADER{\n10\n9\n40\n-1\n304\nLEADER_LINE{\n10\n2\n305\n}\n301\n}\n"
            .to_string(),
    ];
    for body in &cases {
        let d = cadcraft_io::read_dxf(dxf_with(body).as_bytes()).unwrap();
        for m in mleaders(&d) {
            assert!(m.roots.len() <= 256 && m.line_count() <= 4096);
            for r in m.all_roots() {
                assert!(r.landing.x.is_finite() && r.landing.y.is_finite() && r.dogleg.is_finite() && r.dogleg >= 0.0);
            }
            assert!(m.roots.iter().all(|r| !r.leaders.is_empty()));
        }
        let again = cadcraft_io::read_dxf(dxf_text(&d).as_bytes()).unwrap();
        assert_eq!(mleaders(&again).len(), mleaders(&d).len(), "{body}");
        let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
    }
}

/// Through the DWG bridge (acadrust converts our DXF to DWG and back).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn several_landings_survive_dwg() {
    let mut d = Drawing::new_metric();
    let m = two_landings();
    d.add(&Space::Model, Common::default(), EntityKind::MLeader(m.clone())).unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let got = mleaders(&back);
    assert_eq!(got.len(), 1);
    assert_eq!(*got[0], m);
}
