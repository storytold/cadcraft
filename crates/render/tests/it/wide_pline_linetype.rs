//! Wide polylines are drawn with their linetype: each dash a filled piece of the band (issue #293).

use cadcraft_doc::{Common, Drawing, EntityKind, LwPolyline, Space, library};
use cadcraft_geom::{PolyVertex, Vec2};
use cadcraft_render::{Kind, Options, build};

/// A 0.2-wide polyline through `xs` (on y = 0) with linetype `lt` (DASHED: 0.5 on, 0.25 off).
fn drawing(xs: &[f64], lt: &str, plinegen: bool) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.linetypes.extend(library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
    let p = LwPolyline {
        vertices: xs.iter().map(|&x| PolyVertex::new(Vec2::new(x, 0.0))).collect(),
        closed: false,
        const_width: 0.2,
        elevation: 0.0,
        plinegen,
    };
    d.add(&Space::Model, Common { linetype: lt.into(), ..Default::default() }, EntityKind::LwPolyline(p)).unwrap();
    d
}

/// Filled triangles of the drawing.
fn tris(d: &Drawing) -> Vec<Vec2> {
    let l = build(d, &Space::Model, &Options::default());
    l.prims.iter().filter(|p| p.kind == Kind::Tris).flat_map(|p| l.points(p).to_vec()).collect()
}

fn area(t: &[Vec2]) -> f64 {
    t.chunks(3).map(|t| ((t[1] - t[0]).cross(t[2] - t[0]) / 2.0).abs()).sum()
}

#[test]
fn wide_polyline_dashes_are_filled_bands() {
    let full = tris(&drawing(&[0.0, 3.0], "Continuous", false));
    assert!((area(&full) - 0.6).abs() < 1e-9);
    // 3.0 long = four 0.5 dashes with 0.25 gaps between.
    let dashed = tris(&drawing(&[0.0, 3.0], "DASHED", false));
    assert!((area(&dashed) - 0.4).abs() < 1e-9, "{}", area(&dashed));
    assert!(dashed.iter().all(|q| !(q.x > 0.5 + 1e-9 && q.x < 0.75 - 1e-9)), "the first gap is empty");
    assert!(dashed.iter().all(|q| (q.y.abs() - 0.1).abs() < 1e-9), "dashes keep the full width");
}

#[test]
fn a_band_more_than_50000_periods_long_keeps_its_dashes_without_a_level_of_detail() {
    // 50,001 DASHED periods: two thirds of the band is dashes (0.5 of each 0.75, the end ones
    // halves), as a plot or export draws it; a display build, with a level of detail, draws it
    // whole (#1069).
    let len = 50_001.0 * 0.75;
    let d = drawing(&[0.0, len], "DASHED", false);
    let dashed = area(&tris(&d));
    assert!((dashed - 0.2 * len * 2.0 / 3.0).abs() < 1e-4, "{dashed}");
    let l = build(&d, &Space::Model, &Options { min_dash: 1e-6, ..Default::default() });
    let whole = area(&l.prims.iter().filter(|p| p.kind == Kind::Tris).flat_map(|p| l.points(p).to_vec()).collect::<Vec<_>>());
    assert!((whole - 0.2 * len).abs() < 1e-4, "{whole}");
}

#[test]
fn a_closed_plinegen_band_is_stretched_around_as_a_thin_one_is() {
    // A closed 10.5 square is 42 around: 56 DASHED periods laid from its second vertex, 10.5 along
    // (a whole number of periods), so the first side has dashes at 0-0.5, 0.75-1.25…; laid as an
    // open run they would be centred, at 0-0.25, 0.5-1.0….
    let mut d = Drawing::new_imperial();
    d.linetypes.extend(library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
    let p = LwPolyline {
        vertices: [(0.0, 0.0), (10.5, 0.0), (10.5, 10.5), (0.0, 10.5)].map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).to_vec(),
        closed: true,
        const_width: 0.2,
        elevation: 0.0,
        plinegen: true,
    };
    d.add(&Space::Model, Common { linetype: "DASHED".into(), ..Default::default() }, EntityKind::LwPolyline(p)).unwrap();
    let t = tris(&d);
    let on_first_side = |x: f64| t.iter().any(|q| (q.x - x).abs() < 1e-9 && q.y.abs() <= 0.1 + 1e-9);
    assert!(on_first_side(0.75) && on_first_side(1.25) && !on_first_side(1.0), "{t:?}");

    // With fill off only the centre line is drawn, laid out the same way.
    d.header.set_i64("FILLMODE", 0);
    let l = build(&d, &Space::Model, &Options::default());
    let starts: Vec<Vec2> = l.prims.iter().filter(|p| p.kind == Kind::Polyline).filter_map(|p| l.points(p).first().copied()).collect();
    let starts_at = |x: f64| starts.iter().any(|q| (q.x - x).abs() < 1e-9 && q.y.abs() < 1e-9);
    assert!(starts_at(0.75) && !starts_at(0.5), "{starts:?}");
}

#[test]
fn a_closed_plinegen_run_is_anchored_at_its_second_vertex_after_an_arc() {
    // The first segment is an arc (bulge 0.5): the pattern is measured along the run as drawn, so
    // a dash starts right at the second vertex, without width and with fill off alike.
    for width in [0.0, 0.2] {
        let mut d = Drawing::new_imperial();
        d.linetypes.extend(library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
        d.header.set_i64("FILLMODE", 0);
        let mut vertices: Vec<PolyVertex> =
            [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).to_vec();
        if let Some(v) = vertices.first_mut() {
            v.bulge = 0.5;
        }
        let p = LwPolyline { vertices, closed: true, const_width: width, elevation: 0.0, plinegen: true };
        d.add(&Space::Model, Common { linetype: "DASHED".into(), ..Default::default() }, EntityKind::LwPolyline(p)).unwrap();
        let l = build(&d, &Space::Model, &Options::default());
        let starts: Vec<Vec2> = l.prims.iter().filter(|p| p.kind == Kind::Polyline).filter_map(|p| l.points(p).first().copied()).collect();
        assert!(starts.iter().any(|q| q.dist(Vec2::new(10.0, 0.0)) < 1e-9), "{width}: {starts:?}");
    }
}

#[test]
fn a_closed_plinegen_run_whose_first_two_vertices_coincide_is_anchored_at_them() {
    // The zero-length first segment is left out of the drawn run, but the second vertex is still
    // where the run starts: the 40-unit square is stretched to 53 DASHED periods from there, so
    // dashes start at 0 and 40/53 (not 10 + k * 40/53, from the next vertex). Without width, with
    // fill off, and with fill on (the band's centre line) alike.
    for (width, fill) in [(0.0, 0), (0.2, 0), (0.2, 1)] {
        let mut d = Drawing::new_imperial();
        d.linetypes.extend(library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
        d.header.set_i64("FILLMODE", fill);
        let vertices: Vec<PolyVertex> =
            [(0.0, 0.0), (0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).to_vec();
        let p = LwPolyline { vertices, closed: true, const_width: width, elevation: 0.0, plinegen: true };
        d.add(&Space::Model, Common { linetype: "DASHED".into(), ..Default::default() }, EntityKind::LwPolyline(p)).unwrap();
        let l = build(&d, &Space::Model, &Options { min_dash: 1e-6, ..Default::default() });
        let starts: Vec<Vec2> = l.prims.iter().filter(|p| p.kind == Kind::Polyline).filter_map(|p| l.points(p).first().copied()).collect();
        for x in [0.0, 40.0 / 53.0] {
            assert!(starts.iter().any(|q| q.dist(Vec2::new(x, 0.0)) < 1e-9), "{width} {fill} {x}: {starts:?}");
        }
    }
}

#[test]
fn plinegen_runs_the_pattern_along_the_whole_polyline() {
    // Two 0.6 segments: per segment each is shorter than the 0.75 pattern, so drawn continuous;
    // along the whole 1.2 the centred pattern gives 0.475 + 0.475.
    let per_segment = area(&tris(&drawing(&[0.0, 0.6, 1.2], "DASHED", false)));
    let whole = area(&tris(&drawing(&[0.0, 0.6, 1.2], "DASHED", true)));
    assert!((per_segment - 0.24).abs() < 1e-9, "{per_segment}");
    assert!((whole - 0.19).abs() < 1e-9, "{whole}");
}
