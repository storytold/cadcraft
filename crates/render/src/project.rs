//! Parallel 3D views ([`crate::Options::projection`]). A flat object lies in a plane `z =
//! elevation`, which a parallel view maps to the list by a 2D affine map: it draws through that
//! map like in plan view. 3D lines, polylines, faces, meshes, helices and infinite lines are
//! projected point by point.

use cadcraft_doc::{EntityKind, Face3d, Line, PolyfaceMesh, Polyline3d, RayLine};
use cadcraft_geom::{Mat4, Vec3};

/// The elevation of the plane a flat object lies in.
pub(crate) fn elevation(k: &EntityKind) -> f64 {
    match k {
        EntityKind::Point(p) => p.p.z,
        EntityKind::Circle(c) => c.center.z,
        EntityKind::Arc(a) => a.center.z,
        EntityKind::Ellipse(el) => el.center.z,
        EntityKind::LwPolyline(p) => p.elevation,
        EntityKind::Text(t) => t.insert.z,
        EntityKind::AttDef(a) => a.text.insert.z,
        EntityKind::MText(t) => t.insert.z,
        EntityKind::Insert(i) => i.insert.z,
        EntityKind::Hatch(h) => h.elevation,
        EntityKind::Dimension(dm) => dm.defpt.z,
        EntityKind::Leader(l) => l.vertices.first().map_or(0.0, |v| v.z),
        EntityKind::MLeader(m) => m.landing.z,
        EntityKind::Image(i) => i.insert.z,
        EntityKind::Table(t) => t.insert.z,
        EntityKind::Tolerance(t) => t.insert.z,
        EntityKind::Solid(s) | EntityKind::Trace(s) => s.corners[0].z,
        _ => 0.0,
    }
}

/// A 3D object in view coordinates (x, y on the list; z towards the viewer), drawn as it is with
/// an identity transform; `None` for flat objects, which draw through [`Mat4::plane`].
pub(crate) fn in_view(e: &cadcraft_doc::Entity, view: &Mat4) -> Option<EntityKind> {
    let p = |q: &Vec3| view.apply(*q);
    Some(match &e.kind {
        EntityKind::Line(l) => EntityKind::Line(Line { a: p(&l.a), b: p(&l.b) }),
        EntityKind::Polyline3d(pl) => EntityKind::Polyline3d(Polyline3d { points: pl.points.iter().map(p).collect(), closed: pl.closed }),
        EntityKind::Face3d(f) => EntityKind::Face3d(Face3d { corners: f.corners.map(|c| p(&c)), hidden_edges: f.hidden_edges }),
        EntityKind::PolyfaceMesh(m) => {
            EntityKind::PolyfaceMesh(PolyfaceMesh { vertices: m.vertices.iter().map(p).collect(), faces: m.faces.clone() })
        }
        EntityKind::Helix(h) => EntityKind::Polyline3d(Polyline3d { points: h.fit_points().iter().map(p).collect(), closed: false }),
        EntityKind::Ray(r) | EntityKind::XLine(r) => {
            let ray = RayLine { base: p(&r.base), dir: p(&(r.base + r.dir)) - p(&r.base) };
            if matches!(e.kind, EntityKind::Ray(_)) { EntityKind::Ray(ray) } else { EntityKind::XLine(ray) }
        }
        _ => return None,
    })
}

/// The uniform scale of a view transform's linear part (the cube root of its determinant): 1 for
/// a view of model space, the block's scale inside a scaled block reference.
pub(crate) fn scale(m: &Mat4) -> f64 {
    let a = &m.m;
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    let k = det.abs().cbrt();
    if k.is_finite() && k > 0.0 { k } else { 1.0 }
}

/// A block reference's transform in 3D: its 2D transform `m` (insertion point, rotation, x and y
/// scale, base point) with the insertion point's z and the z scale.
pub(crate) fn insert_transform(m: &cadcraft_geom::Mat3, ins: &cadcraft_doc::Insert, base: Vec3) -> Mat4 {
    let mut t = Mat4::from_mat3(m);
    t.m[2][2] = ins.scale.z;
    t.m[2][3] = ins.insert.z - base.z * ins.scale.z;
    t
}
