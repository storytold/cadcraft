//! A 3D face draws only its visible edges: `hidden_edges` bit i hides the side from corner i to
//! the next, as DXF group 70 defines it, and a triangle's zero-length side isn't drawn.

use cadcraft_doc::{EntityKind, Face3d, Prim};
use cadcraft_geom::Vec3;

fn sides(f: Face3d) -> usize {
    EntityKind::Face3d(f).prims().iter().filter(|p| matches!(p, Prim::Seg(_))).count()
}

#[test]
fn hidden_edges_are_not_drawn() {
    let quad = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
    assert_eq!(sides(Face3d { corners: quad, hidden_edges: 0 }), 4);
    assert_eq!(sides(Face3d { corners: quad, hidden_edges: 1 | 4 }), 2);
    assert_eq!(sides(Face3d { corners: quad, hidden_edges: 15 }), 0);
    let triangle = [quad[0], quad[1], quad[2], quad[2]];
    assert_eq!(sides(Face3d { corners: triangle, hidden_edges: 0 }), 3);
}
