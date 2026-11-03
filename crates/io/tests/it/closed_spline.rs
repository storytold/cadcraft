//! A closed fit-point spline is periodic: DXF flags it closed + periodic (group 70 bits 1 and 2)
//! and it reads back as the same smooth closed curve. Its fit data is written as AutoCAD 2024
//! writes it (the first fit point again at the end, the tangent where it closes as both end
//! tangents), so AutoCAD, which rebuilds a spline from its fit data, gets the same curve.

use cadcraft_doc::{Common, Drawing, EntityKind, Space};
use cadcraft_geom::{Spline, Vec2};

/// The tags after the AcDbSpline marker, up to the next entity.
fn spline_tags(text: &str) -> &str {
    let marker = "AcDbSpline\r\n";
    let s = &text[text.find(marker).unwrap() + marker.len()..];
    &s[..s.find("\r\n  0\r\n").unwrap()]
}

/// The values of group `code` in a tag list, as numbers.
fn values(tags: &str, code: &str) -> Vec<f64> {
    let lines: Vec<&str> = tags.lines().collect();
    lines.chunks(2).filter(|t| t.first().is_some_and(|c| c.trim() == code)).filter_map(|t| t.get(1)?.trim().parse().ok()).collect()
}

fn read_spline(bytes: &[u8]) -> Spline {
    let back = cadcraft_io::read(bytes, "a.dxf").unwrap();
    let Some(EntityKind::Spline(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no spline") };
    r
}

#[test]
fn closed_fit_spline_roundtrips_as_periodic() {
    let fit = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0), Vec2::new(0.0, 3.0)];
    let sp = Spline::from_fit_points_closed(&fit);
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Spline(sp.clone())).unwrap();
    let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let tags = spline_tags(&text);
    assert!(tags.contains(" 70\r\n11\r\n"), "planar + closed + periodic");
    // Five fit points, the first again last; the tangent where it closes at both ends.
    assert_eq!(values(tags, "74"), [5.0]);
    assert_eq!(values(tags, "11"), [0.0, 4.0, 4.0, 0.0, 0.0]);
    assert_eq!(values(tags, "21"), [0.0, 0.0, 3.0, 3.0, 0.0]);
    let (lo, hi) = sp.domain();
    let seam = sp.derivative(lo);
    assert!((seam.len() - 1.0).abs() < 1e-12 && sp.derivative(hi).near(seam, 1e-12));
    for (x, y) in [("12", "22"), ("13", "23")] {
        let t = Vec2::new(values(tags, x)[0], values(tags, y)[0]);
        assert!(t.near(seam, 1e-12), "{x}: {t:?} / {seam:?}");
    }
    let r = read_spline(&bytes);
    assert!(r.closed && r.is_periodic() && !r.wraps());
    assert_eq!(r.fit.len(), 4, "the closing fit point is implied");
    assert_eq!((&r.knots, &r.control), (&sp.knots, &sp.control));
    // Two fit points (no loop to close smoothly) are written and read as they are.
    let two = Spline::from_fit_points_closed(&[Vec2::ZERO, Vec2::new(4.0, 0.0)]);
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Spline(two.clone())).unwrap();
    let back = read_spline(&cadcraft_io::write(&d, "c.dxf").unwrap());
    assert!(back.closed && back.fit == two.fit, "{:?}", back.fit);
}

#[test]
fn autocad_closed_fit_spline_roundtrips() {
    // The SPLINE AutoCAD 2024 writes for SPLINE (0,0) (4,0) (6,3) (1,4) Close (accoreconsole,
    // `entget`): flags 1067, clamped chord-length knots, five fit points and the tangent where
    // it closes as 12 and 13.
    let dxf = "  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\nSPLINE\r\n  5\r\n2A\r\n  8\r\n0\r\n100\r\nAcDbSpline\r\n 70\r\n1067\r\n 71\r\n3\r\n 72\r\n11\r\n 73\r\n7\r\n 74\r\n5\r\n 42\r\n1e-09\r\n 43\r\n1e-10\r\n 44\r\n1e-10\r\n 12\r\n0.7326205476427477\r\n 22\r\n-0.6806372992803439\r\n 32\r\n0.0\r\n 13\r\n0.7326205476427477\r\n 23\r\n-0.6806372992803439\r\n 33\r\n0.0\r\n 40\r\n0.0\r\n 40\r\n0.0\r\n 40\r\n0.0\r\n 40\r\n0.0\r\n 40\r\n4.0\r\n 40\r\n7.605551275463989\r\n 40\r\n12.70457078905677\r\n 40\r\n16.82767641467443\r\n 40\r\n16.82767641467443\r\n 40\r\n16.82767641467443\r\n 40\r\n16.82767641467443\r\n 10\r\n0.0\r\n 20\r\n0.0\r\n 30\r\n0.0\r\n 10\r\n0.9768273968569969\r\n 20\r\n-0.9075163990404586\r\n 30\r\n0.0\r\n 10\r\n4.110440110427768\r\n 20\r\n-0.7122257786209955\r\n 30\r\n0.0\r\n 10\r\n7.696509078097733\r\n 20\r\n3.854015801004462\r\n 30\r\n0.0\r\n 10\r\n0.3665121443407813\r\n 20\r\n5.739691001923484\r\n 30\r\n0.0\r\n 10\r\n-1.006890633809635\r\n 20\r\n0.9354464925559993\r\n 30\r\n0.0\r\n 10\r\n0.0\r\n 20\r\n0.0\r\n 30\r\n0.0\r\n 11\r\n0.0\r\n 21\r\n0.0\r\n 31\r\n0.0\r\n 11\r\n4.0\r\n 21\r\n0.0\r\n 31\r\n0.0\r\n 11\r\n6.0\r\n 21\r\n3.0\r\n 31\r\n0.0\r\n 11\r\n1.0\r\n 21\r\n4.0\r\n 31\r\n0.0\r\n 11\r\n0.0\r\n 21\r\n0.0\r\n 31\r\n0.0\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
    let r = read_spline(dxf.as_bytes());
    let fit = [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(6.0, 3.0), Vec2::new(1.0, 4.0)];
    assert!(r.closed && r.is_periodic() && !r.wraps(), "{r:?}");
    assert_eq!(r.fit, fit, "the closing fit point is implied");
    assert_eq!((r.fit_opts.start_tangent, r.fit_opts.end_tangent), (None, None), "the tangent is the curve's");
    // The same curve as the one built here from the fit points.
    let own = Spline::from_fit_points_closed(&fit);
    assert!(r.control.iter().zip(&own.control).all(|(a, b)| a.near(*b, 1e-12)) && r.control.len() == own.control.len());
    let in_tags = spline_tags(dxf);
    // Written back: closed + periodic, and the knots, control points and fit data AutoCAD wrote.
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Spline(r)).unwrap();
    let text = String::from_utf8(cadcraft_io::write(&d, "b.dxf").unwrap()).unwrap();
    let out = spline_tags(&text);
    assert_eq!(values(out, "70"), [11.0]);
    for code in ["40", "10", "20", "74", "11", "21"] {
        assert_eq!(values(out, code), values(in_tags, code), "group {code}");
    }
    // The tangent where it closes is the curve's: AutoCAD's, to rounding.
    for code in ["12", "22", "13", "23"] {
        let (a, b) = (values(out, code), values(in_tags, code));
        assert!(a.len() == 1 && b.len() == 1 && (a[0] - b[0]).abs() < 1e-12, "group {code}: {a:?} / {b:?}");
    }
}
