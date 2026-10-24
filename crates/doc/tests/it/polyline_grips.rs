//! Dense contour lines retain all editable vertex and segment-midpoint grips.

use cadcraft_doc::{EntityKind, LwPolyline};
use cadcraft_geom::{PolyVertex, Vec2};

#[test]
fn dense_contour_preserves_every_vertex_and_midpoint() {
    let vertices: Vec<_> = (0..20_000).map(|i| PolyVertex::new(Vec2::new(f64::from(i), f64::from(i % 17)))).collect();
    let kind = EntityKind::LwPolyline(LwPolyline { vertices: vertices.clone(), closed: false, const_width: 0.0, elevation: 0.0, plinegen: false });
    let grips = kind.grips();
    assert_eq!(grips.len(), 39_999);
    for (i, v) in vertices.iter().enumerate() {
        assert_eq!(grips[2 * i], v.p);
        if let Some(next) = vertices.get(i + 1) {
            assert_eq!(grips[2 * i + 1], v.p.mid(next.p));
        }
    }
}

#[test]
fn closed_arcs_keep_their_curved_and_closing_midpoints() {
    for (bulge, y) in [(1.0, -5.0), (-1.0, 5.0)] {
        let kind = EntityKind::LwPolyline(LwPolyline {
            vertices: vec![PolyVertex { bulge, ..PolyVertex::new(Vec2::ZERO) }, PolyVertex::new(Vec2::new(10.0, 0.0))],
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        });
        let grips = kind.grips();
        assert_eq!(grips.len(), 4);
        for (actual, expected) in grips.iter().zip([Vec2::ZERO, Vec2::new(5.0, y), Vec2::new(10.0, 0.0), Vec2::new(5.0, 0.0)]) {
            assert!(actual.near(expected, 1e-9));
        }
    }
}
