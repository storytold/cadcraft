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
    // Three fit points and a start tangent: five control points, the other end without a second
    // derivative, as AutoCAD 2024 builds it.
    assert!(r.is_valid() && r.fit_opts.start_tangent == Some(Vec2::Y) && r.control.len() == 5);
}

/// A SPLINE stored with fit points and end tangents only.
fn fit_only_dxf(fit: &[(f64, f64)], start: Option<(f64, f64)>, end: Option<(f64, f64)>) -> String {
    let mut s =
        format!("0\r\nSECTION\r\n2\r\nENTITIES\r\n0\r\nSPLINE\r\n8\r\n0\r\n100\r\nAcDbSpline\r\n70\r\n8\r\n71\r\n3\r\n74\r\n{}\r\n", fit.len());
    for (code, t) in [(12, start), (13, end)] {
        if let Some((x, y)) = t {
            s += &format!("{code}\r\n{x:?}\r\n{}\r\n{y:?}\r\n{}\r\n0.0\r\n", code + 10, code + 20);
        }
    }
    for (x, y) in fit {
        s += &format!("11\r\n{x:?}\r\n21\r\n{y:?}\r\n31\r\n0.0\r\n");
    }
    s + "0\r\nENDSEC\r\n0\r\nEOF\r\n"
}

#[test]
fn a_fit_only_spline_takes_its_end_tangents_with_their_length() {
    // AutoCAD 2024 (accoreconsole, entget of the spline it builds): each stored tangent is the end
    // derivative as it stands, so 12 = (0, 3) is not 12 = (0, 1), and 13 = (1, -1) is not a unit.
    let fit = [(0.0, 0.0), (4.0, 0.0), (8.0, 2.0), (12.0, 1.0), (15.0, 4.0)];
    let total = 16.83788226773652;
    let knots = [0.0, 0.0, 0.0, 0.0, 4.0, 8.47213595499958, 12.59524158061724, total, total, total, total];
    let cases = [
        (
            Some((0.0, 3.0)),
            None,
            [
                (0.0, 0.0),
                (0.0, 4.0),
                (5.032653247327764, -2.9587578599554),
                (7.49793879823205, 3.8423576404002),
                (12.40378970003823, -0.4300472739512),
                (14.12639202373297, 2.5093177413937),
                (15.0, 4.0),
            ],
        ),
        (
            Some((0.0, 2.0)),
            Some((1.0, -1.0)),
            [
                (0.0, 0.0),
                (0.0, 2.6666666666666),
                (5.051273572432346, -2.3615552390813),
                (7.426977592306234, 4.0442234834397),
                (12.64672696003663, -1.6844325426585),
                (13.5857864376269, 5.4142135623731),
                (15.0, 4.0),
            ],
        ),
    ];
    let v = |t: Option<(f64, f64)>| t.map(|(x, y)| Vec2::new(x, y));
    for (start, end, control) in cases {
        let back = cadcraft_io::read(fit_only_dxf(&fit, start, end).as_bytes(), "c.dxf").unwrap();
        let Some(EntityKind::Spline(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no spline") };
        assert_eq!(r.knots.len(), knots.len());
        assert!(r.knots.iter().zip(knots).all(|(a, b)| (a - b).abs() < 1e-9), "{:?}", r.knots);
        assert_eq!(r.control.len(), control.len());
        for (c, (x, y)) in r.control.iter().zip(control) {
            assert!(c.near(Vec2::new(x, y), 1e-9), "{start:?} {end:?}: {c:?} vs ({x}, {y})");
        }
        // The tangents are kept as stored, and written back so.
        assert_eq!((r.fit_opts.start_tangent, r.fit_opts.end_tangent), (v(start), v(end)));
        let again = roundtrip(&r);
        assert_eq!((again.fit_opts.start_tangent, again.fit_opts.end_tangent), (v(start), v(end)));
        assert!(again.control.iter().zip(&r.control).all(|(a, b)| a.near(*b, 1e-12)));
    }
}
