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
fn plinegen_runs_the_pattern_along_the_whole_polyline() {
    // Two 0.6 segments: per segment each gets one 0.5 dash; along the whole 1.2 the pattern
    // gives 0.5 + 0.45.
    let per_segment = area(&tris(&drawing(&[0.0, 0.6, 1.2], "DASHED", false)));
    let whole = area(&tris(&drawing(&[0.0, 0.6, 1.2], "DASHED", true)));
    assert!((per_segment - 0.2).abs() < 1e-9, "{per_segment}");
    assert!((whole - 0.19).abs() < 1e-9, "{whole}");
}
