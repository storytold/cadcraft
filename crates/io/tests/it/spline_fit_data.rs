//! SPLINE fit data in DXF: fit tolerance (group 44) and end tangents (12/13) are written and read
//! back; the knot parametrisation, which DXF does not store, is recovered from the knots (#408).

use cadcraft_doc::{Common, Drawing, EntityKind, Space};
use cadcraft_geom::{FitOptions, KnotParam, Spline, Vec2};

fn roundtrip(sp: &Spline) -> Spline {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Spline(sp.clone())).unwrap();
    let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
    let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
    let Some(EntityKind::Spline(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no spline") };
    r
}

#[test]
fn fit_options_roundtrip_through_dxf() {
    let fit = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.2), Vec2::new(5.0, 3.0), Vec2::new(6.0, 3.1), Vec2::new(9.0, 0.0)];
    for knots in KnotParam::ALL {
        let opts =
            FitOptions { knots, start_tangent: Some(Vec2::new(0.0, 1.0)), end_tangent: Some(Vec2::new(1.0, -1.0).normalized()), tolerance: 0.0 };
        let sp = Spline::fit_with(&fit, false, opts);
        let r = roundtrip(&sp);
        assert_eq!(r.fit_opts.knots, knots);
        assert!(r.fit_opts.start_tangent.is_some_and(|t| t.near(Vec2::Y, 1e-9)));
        assert!(r.fit_opts.end_tangent.is_some_and(|t| t.near(Vec2::new(1.0, -1.0).normalized(), 1e-9)));
        assert_eq!(r.control.len(), sp.control.len());
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            assert!(r.eval(t).near(sp.eval(t), 1e-9));
        }
        // Closed (periodic) splines keep their parametrisation too.
        let c = roundtrip(&Spline::fit_with(&fit, true, FitOptions { knots, ..FitOptions::default() }));
        assert!(c.is_periodic() && c.fit_opts.knots == knots);
    }
    let fit: Vec<Vec2> = (0..20).map(|i| Vec2::new(i as f64, (i as f64 * 0.4).sin())).collect();
    let sp = Spline::fit_with(&fit, false, FitOptions { tolerance: 0.05, ..FitOptions::default() });
    let r = roundtrip(&sp);
    assert_eq!(r.fit_opts.tolerance, 0.05);
    assert_eq!(r.control.len(), sp.control.len(), "the approximation is kept, not re-interpolated");
}

#[test]
fn fit_data_is_written_with_the_documented_groups() {
    let sp = Spline::fit_with(
        &[Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(8.0, 2.0)],
        false,
        FitOptions { start_tangent: Some(Vec2::Y), tolerance: 0.25, ..FitOptions::default() },
    );
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Spline(sp)).unwrap();
    let text = String::from_utf8(cadcraft_io::write(&d, "a.dxf").unwrap()).unwrap();
    let spline = &text[text.find("AcDbSpline").unwrap()..];
    let spline = &spline[..spline.find("\r\n  0\r\n").unwrap()];
    assert!(spline.contains(" 44\r\n0.25\r\n"), "fit tolerance");
    assert!(spline.contains(" 12\r\n0.0\r\n 22\r\n1.0\r\n"), "start tangent");
    assert!(!spline.contains(" 13\r\n"), "no end tangent");
    // A fit spline whose stored knots don't match its control points is rebuilt from the fit
    // data, tangents included.
    let dxf = "0\r\nSECTION\r\n2\r\nENTITIES\r\n0\r\nSPLINE\r\n8\r\n0\r\n100\r\nAcDbSpline\r\n70\r\n8\r\n71\r\n3\r\n74\r\n3\r\n12\r\n0.0\r\n22\r\n1.0\r\n32\r\n0.0\r\n11\r\n0.0\r\n21\r\n0.0\r\n31\r\n0.0\r\n11\r\n4.0\r\n21\r\n0.0\r\n31\r\n0.0\r\n11\r\n8.0\r\n21\r\n2.0\r\n31\r\n0.0\r\n0\r\nENDSEC\r\n0\r\nEOF\r\n";
    let back = cadcraft_io::read(dxf.as_bytes(), "b.dxf").unwrap();
    let Some(EntityKind::Spline(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no spline") };
    assert!(r.is_valid() && r.fit_opts.start_tangent == Some(Vec2::Y) && r.control.len() == 4);
}
