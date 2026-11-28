//! A closed fit-point spline is periodic: DXF flags it closed + periodic (group 70 bits 1 and 2)
//! and it reads back as the same smooth closed curve. Its fit data is written as AutoCAD 2024
//! writes it (the first fit point again at the end, the tangent where it closes as both end
//! tangents), so AutoCAD, which rebuilds a spline from its fit data, gets the same curve.

use cadcraft_doc::{Common, Drawing, EntityKind, Space};
use cadcraft_geom::{FitOptions, KnotParam, Spline, Vec2};

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
    // Its tangent where it closes is kept, as AutoCAD keeps it through fit-point edits.
    let d = Vec2::new(0.7326205476427477, -0.6806372992803439);
    assert_eq!((r.fit_opts.start_tangent, r.fit_opts.end_tangent), (Some(d), Some(d)));
    assert_eq!(r.fit_opts.tolerance, 0.0, "44 = 1e-10 is AutoCAD's no fit tolerance");
    // The same curve as the one built here from the fit points.
    let own = Spline::from_fit_points_closed(&fit);
    assert!(r.control.iter().zip(&own.control).all(|(a, b)| a.near(*b, 1e-12)) && r.control.len() == own.control.len());
    // A fit-point move rebuilds it as AutoCAD does, though its fit tolerance is 1e-10 (AutoCAD's
    // none): SPLINEDIT Fit data Move of (1, 4) to (1, 5), which keeps the tangent.
    let moved = r.refit(&[fit[0], fit[1], fit[2], Vec2::new(1.0, 5.0)], true);
    let want = [
        (0.0, 0.0),
        (0.9768273968569969, -0.9075163990404586),
        (4.139797324573113, -0.6528392109186482),
        (7.666093574908103, 3.731946546410586),
        (-0.03708934486751076, 7.208547566545824),
        (-1.245215489496467, 1.156860956903188),
        (0.0, 0.0),
    ];
    assert!(
        moved.control.len() == want.len() && moved.control.iter().zip(want).all(|(c, (x, y))| c.near(Vec2::new(x, y), 1e-9)),
        "{:?}",
        moved.control
    );
    let in_tags = spline_tags(dxf);
    // Written back: closed + periodic, and the knots, control points and fit data AutoCAD wrote.
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Spline(r)).unwrap();
    let text = String::from_utf8(cadcraft_io::write(&d, "b.dxf").unwrap()).unwrap();
    let out = spline_tags(&text);
    assert_eq!(values(out, "70"), [11.0]);
    for code in ["40", "10", "20", "74", "11", "21", "12", "22", "13", "23"] {
        assert_eq!(values(out, code), values(in_tags, code), "group {code}");
    }
}

#[test]
fn closed_splines_without_a_stored_tangent_reopen_as_they_were() {
    // Periodic knots, a fit tolerance or a kink: no tangent is kept where the curve closes, though
    // the file gives the curve's own as 12/13 for AutoCAD. Reopened, the spline still has none, so
    // SPLINEDIT Open (an open refit) gives free ends, as before the save.
    let fit = [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(6.0, 3.0), Vec2::new(1.0, 4.0), Vec2::new(-1.0, 2.0)];
    let k = Vec2::new(6.0, 3.0);
    let cases = [
        Spline::fit_with(&fit, true, FitOptions { knots: KnotParam::Uniform, ..FitOptions::default() }),
        Spline::fit_with(&fit, true, FitOptions { tolerance: 0.5, ..FitOptions::default() }),
        Spline::fit_with(&[fit[0], fit[1], k, k, fit[3], fit[4]], true, FitOptions::default()),
    ];
    for sp in cases {
        assert_eq!((sp.fit_opts.start_tangent, sp.fit_opts.end_tangent), (None, None));
        let mut d = Drawing::new_metric();
        d.add(&Space::Model, Common::default(), EntityKind::Spline(sp.clone())).unwrap();
        let bytes = cadcraft_io::write(&d, "k.dxf").unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(values(spline_tags(&text), "12").len(), 1, "12 is written for AutoCAD");
        let back = read_spline(&bytes);
        assert_eq!((back.fit_opts.start_tangent, back.fit_opts.end_tangent), (None, None), "{:?}", sp.fit_opts);
        let (open, again) = (sp.refit(&sp.fit, false), back.refit(&back.fit, false));
        assert!(open.control.len() == again.control.len() && open.control.iter().zip(&again.control).all(|(a, b)| a.near(*b, 1e-9)));
    }
}

#[test]
fn a_reversed_closed_spline_is_written_as_autocad_reverses_it() {
    // AutoCAD 2024's REVERSE of SPLINE (0,0) (4,0) (6,3) (1,4) Close (accoreconsole, `entget`):
    // the seam stays the first fit point, the others turn round, and so does the tangent where it
    // closes; the knots and control points are reversed. REVERSE and SPLINEDIT Reverse use
    // `Spline::reversed`.
    let fit = [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(6.0, 3.0), Vec2::new(1.0, 4.0)];
    let r = Spline::from_fit_points_closed(&fit).reversed();
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Spline(r.clone())).unwrap();
    let bytes = cadcraft_io::write(&d, "r.dxf").unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let tags = spline_tags(&text);
    assert_eq!(values(tags, "11"), [0.0, 1.0, 6.0, 4.0, 0.0]);
    assert_eq!(values(tags, "21"), [0.0, 4.0, 3.0, 0.0, 0.0]);
    let near = |a: Vec<f64>, b: &[f64], tol: f64| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < tol);
    for code in ["12", "13"] {
        assert!(near(values(tags, code), &[-0.7326205476427478], 1e-12), "{code}");
    }
    for code in ["22", "23"] {
        assert!(near(values(tags, code), &[0.6806372992803439], 1e-12), "{code}");
    }
    let knots = [
        0.0,
        0.0,
        0.0,
        0.0,
        4.12310562561766,
        9.222125139210444,
        12.82767641467443,
        16.82767641467443,
        16.82767641467443,
        16.82767641467443,
        16.82767641467443,
    ];
    assert!(near(values(tags, "40"), &knots, 1e-12), "{:?}", values(tags, "40"));
    let xs = [0.0, -1.006890633809634, 0.3665121443407817, 7.696509078097734, 4.110440110427767, 0.9768273968569975, 0.0];
    let ys = [0.0, 0.9354464925559991, 5.739691001923484, 3.854015801004462, -0.7122257786209955, -0.907516399040459, 0.0];
    assert!(near(values(tags, "10"), &xs, 1e-9) && near(values(tags, "20"), &ys, 1e-9));
    // Read back, it is the same spline, and a fit-point move keeps its seam and tangent: AutoCAD's
    // SPLINEDIT Fit data Move of the second fit point to (1, 5) after REVERSE.
    let back = read_spline(&bytes);
    assert_eq!(back, r);
    let moved = back.refit(&[fit[0], Vec2::new(1.0, 5.0), fit[2], fit[1]], true);
    let want = [
        (0.0, 0.0),
        (-1.245215489496467, 1.156860956903188),
        (-0.03708934486751225, 7.208547566545823),
        (7.666093574908103, 3.731946546410587),
        (4.139797324573113, -0.6528392109186484),
        (0.9768273968569966, -0.9075163990404583),
        (0.0, 0.0),
    ];
    assert!(
        moved.control.len() == want.len() && moved.control.iter().zip(want).all(|(c, (x, y))| c.near(Vec2::new(x, y), 1e-9)),
        "{:?}",
        moved.control
    );
}

#[test]
fn a_closed_fit_list_ending_with_its_first_point_survives_a_save() {
    let (a, b, c, d) = (Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(5.0, 3.0), Vec2::new(1.0, 4.0));
    // A kink where it closes given at the end (as a file can give it), the first point listed
    // again at the end, three points ending so, the kink at the start as a fit stores it, and three
    // points. The closing point is written after each but the three ending with the first, which
    // the reader keeps as they are.
    for (fit, written) in [
        (vec![a, b, c, a, a], vec![0.0, 4.0, 5.0, 0.0, 0.0, 0.0]),
        (vec![a, b, c, d, a], vec![0.0, 4.0, 5.0, 1.0, 0.0, 0.0]),
        (vec![a, b, a], vec![0.0, 4.0, 0.0]),
        (vec![a, a, b, c], vec![0.0, 0.0, 4.0, 5.0, 0.0]),
        (vec![a, b, c], vec![0.0, 4.0, 5.0, 0.0]),
    ] {
        let mut sp = Spline::fit_with(&[a, a, b, c], true, FitOptions::default());
        sp.fit = fit.clone();
        let mut dr = Drawing::new_metric();
        dr.add(&Space::Model, Common::default(), EntityKind::Spline(sp)).unwrap();
        let bytes = cadcraft_io::write(&dr, "a.dxf").unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(values(spline_tags(&text), "11"), written, "{fit:?}");
        // Read back with the same points, so a refit keeps the kink where it closes.
        assert_eq!(read_spline(&bytes).fit, fit);
    }
}
