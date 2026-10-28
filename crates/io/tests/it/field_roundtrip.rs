//! Fields the model now holds come back from a saved DXF: a multiline
//! text's defined height, a dimension's text attachment, a block's uniform scaling, a scale's
//! unit flag; and the text style group is written only when it isn't the default.

use cadcraft_doc::*;
use cadcraft_dxf::{Tag, parse, records, sections};
use cadcraft_geom::Vec3;
use cadcraft_io::{Version, read_dxf};

fn save(d: &Drawing) -> String {
    let mut d = d.clone();
    d.header.set_str("ACADVER", Version::R2018.code());
    String::from_utf8(cadcraft_io::write(&d, "x.dxf").unwrap()).unwrap()
}

fn all(text: &str) -> Vec<(String, Vec<Tag>)> {
    let tags = parse(text.as_bytes()).unwrap();
    sections(&tags).into_iter().flat_map(|s| records(&s.tags)).collect()
}

fn first<T>(d: &Drawing, f: impl Fn(&EntityKind) -> Option<T>) -> T {
    d.model.iter().find_map(|e| f(&e.kind)).expect("entity")
}

#[test]
fn mtext_defined_height_and_default_style() {
    let mut d = Drawing::new_imperial();
    let m = MText {
        insert: Vec3::ZERO,
        height: 0.18,
        defined_height: 2.5,
        width: 4.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "Note".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(m)).unwrap();
    let text = save(&d);
    let (_, g) = all(&text).into_iter().find(|(k, _)| k == "MTEXT").unwrap();
    assert!(g.iter().any(|t| t.code == 46 && (t.f64() - 2.5).abs() < 1e-12));
    assert!(!g.iter().any(|t| t.code == 7), "STANDARD is the default style: not written");
    let back = read_dxf(text.as_bytes()).unwrap();
    let m = first(&back, |k| if let EntityKind::MText(m) = k { Some(m.clone()) } else { None });
    assert_eq!((m.defined_height, m.style.as_str()), (2.5, "Standard"));
    // Scaling scales it.
    let mut e = back.model.iter().next().unwrap().as_ref().clone();
    e.kind.transform(&cadcraft_geom::Mat3::scale(2.0, 2.0));
    if let EntityKind::MText(m) = &e.kind {
        assert_eq!(m.defined_height, 5.0);
    }
}

#[test]
fn dimension_attachment_and_version() {
    let src = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
0\nDIMENSION\n5\n40\n100\nAcDbEntity\n8\n0\n100\nAcDbDimension\n280\n0\n2\n*D0\n10\n10.0\n20\n-2.0\n30\n0.0\n\
11\n5.0\n21\n-2.0\n31\n0.0\n70\n32\n71\n8\n42\n10.0\n3\nStandard\n100\nAcDbAlignedDimension\n\
13\n0.0\n23\n0.0\n33\n0.0\n14\n10.0\n24\n0.0\n34\n0.0\n100\nAcDbRotatedDimension\n0\nENDSEC\n0\nEOF\n";
    let d = read_dxf(src.as_bytes()).unwrap();
    let dm = first(&d, |k| if let EntityKind::Dimension(x) = k { Some(x.clone()) } else { None });
    assert_eq!(dm.attachment, 8);
    let text = save(&d);
    let (_, g) = all(&text).into_iter().find(|(k, _)| k == "DIMENSION").unwrap();
    let after_marker: Vec<i32> = g.iter().skip_while(|t| !(t.code == 100 && t.str() == "AcDbDimension")).skip(1).map(|t| t.code).collect();
    assert_eq!(after_marker.first(), Some(&280), "the subclass version comes first");
    assert!(g.iter().any(|t| t.code == 71 && t.i64() == 8));
    assert!(!g.iter().any(|t| t.code == 50), "a horizontal dimension's rotation (0) is left out");
}

#[test]
fn block_uniform_scaling_and_space_flags() {
    let mut d = Drawing::new_imperial();
    let mut b = Block::new("BOLT");
    b.scale_uniformly = true;
    d.blocks.insert("BOLT".into(), std::sync::Arc::new(b));
    let text = save(&d);
    let recs = all(&text);
    let rec = |name: &str| {
        recs.iter()
            .find(|(k, g)| k == "BLOCK_RECORD" && g.iter().any(|t| t.code == 2 && t.str().eq_ignore_ascii_case(name)))
            .map(|(_, g)| g.clone())
            .unwrap()
    };
    assert!(rec("BOLT").iter().any(|t| t.code == 281 && t.i64() == 1));
    let ms = rec("*Model_Space");
    assert!(ms.iter().any(|t| t.code == 70) && ms.iter().any(|t| t.code == 280 && t.i64() == 1) && ms.iter().any(|t| t.code == 281));
    assert!(read_dxf(text.as_bytes()).unwrap().block("BOLT").unwrap().scale_uniformly);
}

#[test]
fn scale_unit_flag_is_kept() {
    let mut d = Drawing::new_imperial();
    let mut s = AnnoScale::new("1'-0\" = 1'-0\"", 12.0, 12.0);
    s.unit_scale = Some(false);
    d.scales = vec![AnnoScale::unit(), s];
    let back = read_dxf(save(&d).as_bytes()).unwrap();
    let s = back.scales.iter().find(|s| s.name == "1'-0\" = 1'-0\"").unwrap();
    assert_eq!(s.unit_scale, Some(false), "a 12:12 scale the file didn't mark as the unit scale");
    assert_eq!(back.scales.iter().find(|s| s.name == "1:1").and_then(|s| s.unit_scale), Some(true));
}
