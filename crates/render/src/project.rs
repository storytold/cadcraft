//! Parallel 3D views ([`crate::Options::projection`]). A flat object lies in a plane `z =
//! elevation`, which a parallel view maps to the list by a 2D affine map: it draws through that
//! map like in plan view. 3D lines, polylines, faces and meshes are projected point by point.

use cadcraft_doc::{Entity, EntityKind, Face3d, Line, PolyfaceMesh, Polyline3d, Solid};
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
        EntityKind::Ray(r) | EntityKind::XLine(r) => r.base.z,
        _ => 0.0,
    }
}

/// A 3D object in view coordinates (x, y on the list; z towards the viewer), drawn as it is with
/// an identity transform; `None` for flat objects, which draw through [`Mat4::plane`].
pub(crate) fn in_view(e: &Entity, view: &Mat4) -> Option<Entity> {
    let p = |q: &Vec3| view.apply(*q);
    let kind = match &e.kind {
        EntityKind::Line(l) => EntityKind::Line(Line { a: p(&l.a), b: p(&l.b) }),
        EntityKind::Polyline3d(pl) => EntityKind::Polyline3d(Polyline3d { points: pl.points.iter().map(p).collect(), closed: pl.closed }),
        EntityKind::Face3d(f) => EntityKind::Face3d(Face3d { corners: f.corners.map(|c| p(&c)), hidden_edges: f.hidden_edges }),
        EntityKind::PolyfaceMesh(m) => {
            EntityKind::PolyfaceMesh(PolyfaceMesh { vertices: m.vertices.iter().map(p).collect(), faces: m.faces.clone() })
        }
        EntityKind::Solid(s) => EntityKind::Solid(Solid { corners: s.corners.map(|c| p(&c)) }),
        EntityKind::Trace(s) => EntityKind::Trace(Solid { corners: s.corners.map(|c| p(&c)) }),
        _ => return None,
    };
    Some(Entity { handle: e.handle, common: e.common.clone(), kind })
}

/// A block reference's transform in 3D: its 2D transform `m` (insertion point, rotation, x and y
/// scale, base point) with the insertion point's z and the z scale.
pub(crate) fn insert_transform(m: &cadcraft_geom::Mat3, ins: &cadcraft_doc::Insert, base: Vec3) -> Mat4 {
    let mut t = Mat4::from_mat3(m);
    t.m[2][2] = ins.scale.z;
    t.m[2][3] = ins.insert.z - base.z * ins.scale.z;
    t
}

/// A face of a shaded 3D view, waiting to be drawn back to front ([`crate::Options::shaded`]).
pub(crate) struct ShadedFace {
    pub handle: cadcraft_doc::Handle,
    pub rgb: cadcraft_color::Rgb,
    pub transparency: u8,
    /// When plotting, its layer's index ([`crate::DisplayList::prim_layers`]); `None` on screen.
    pub layer: Option<u32>,
    /// View z of the face's centre: larger is nearer the viewer.
    pub depth: f64,
    pub tris: Vec<cadcraft_geom::Vec2>,
}

/// Queue the faces of a 3D face, polyface mesh or solid already in view coordinates, lit by a
/// light from the viewer's upper left on both sides; false for other objects.
pub(crate) fn queue_faces(b: &mut crate::Builder, ctx: &crate::Ctx, e: &Entity, in_view: &Entity) -> bool {
    let quads: Vec<[Vec3; 4]> = match &in_view.kind {
        EntityKind::Face3d(f) => vec![f.corners],
        EntityKind::PolyfaceMesh(m) => m.faces3d().into_iter().map(|f| f.corners).collect(),
        EntityKind::Solid(s) | EntityKind::Trace(s) => vec![[s.corners[0], s.corners[1], s.corners[3], s.corners[2]]],
        _ => return false,
    };
    let (ink, _, _, _, visible, layer_transparency) = crate::resolve(ctx, e, b.plotting);
    if !visible {
        return true;
    }
    // Its own transparency, as `entity_parts` gives its other primitives (that runs after this).
    let transparency = if b.opts.transparency { crate::transparency(e.common.transparency, layer_transparency, ctx.block_transparency) } else { 0 };
    let layer = b.plotting.then(|| crate::entity_layer(ctx, e));
    // Model space plotted through a viewport carries the layer in the handle until it is clipped
    // into the layout's list, as the entity's other primitives do.
    let handle = match layer {
        Some(l) if ctx.top == crate::VIEWPORT_CONTENT => cadcraft_doc::Handle(u64::from(l)),
        _ => ctx.top,
    };
    let base = if ink.aci7 { cadcraft_color::Rgb(200, 200, 200) } else { ink.rgb };
    let light = Vec3::new(-0.3, 0.45, 1.0).normalized();
    for q in quads {
        let n = (q[1] - q[0]).cross(q[2] - q[0]);
        let n = if n.len() > 1e-300 { n.normalized() } else { (q[2] - q[0]).cross(q[3] - q[0]).normalized() };
        let k = 0.3 + 0.7 * n.dot(light).abs();
        let lit = |c: u8| (f64::from(c) * k).round().clamp(0.0, 255.0) as u8;
        let p = q.map(|c| c.xy());
        let mut tris = vec![p[0], p[1], p[2]];
        if q[3] != q[2] {
            tris.extend([p[0], p[2], p[3]]);
        }
        b.faces.push(ShadedFace {
            handle,
            rgb: cadcraft_color::Rgb(lit(base.0), lit(base.1), lit(base.2)),
            transparency,
            layer,
            depth: (q[0].z + q[1].z + q[2].z + q[3].z) / 4.0,
            tris,
        });
    }
    true
}
