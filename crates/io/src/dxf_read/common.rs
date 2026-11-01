use super::{Color, Common, Lineweight, PolyVertex, Rgb, Tag, Transparency, Vec2, Vec3};

/// Value lookup helpers over an entity's tags.
pub(super) struct T<'a>(pub(super) &'a [Tag]);

impl T<'_> {
    pub(super) fn f(&self, code: i32) -> Option<f64> {
        self.0.iter().find(|t| t.code == code).map(Tag::f64)
    }
    pub(super) fn fd(&self, code: i32, d: f64) -> f64 {
        self.f(code).unwrap_or(d)
    }
    pub(super) fn i(&self, code: i32) -> Option<i64> {
        self.0.iter().find(|t| t.code == code).map(Tag::i64)
    }
    pub(super) fn s(&self, code: i32) -> Option<String> {
        self.0.iter().find(|t| t.code == code).map(Tag::str)
    }
    pub(super) fn p(&self, code: i32) -> Vec3 {
        Vec3::new(self.fd(code, 0.0), self.fd(code + 10, 0.0), self.fd(code + 20, 0.0))
    }
    pub(super) fn all_f(&self, code: i32) -> Vec<f64> {
        self.0.iter().filter(|t| t.code == code).map(Tag::f64).collect()
    }
    pub(super) fn pts(&self, code: i32) -> Vec<Vec2> {
        let xs = self.all_f(code);
        let ys = self.all_f(code + 10);
        xs.into_iter().zip(ys).map(|(x, y)| Vec2::new(x, y)).collect()
    }
}

/// An entity's group 440: `0x01` in the top byte is ByBlock, `0x02` a fixed alpha (mapped to a
/// percentage as for layers), anything else ByLayer.
fn transparency_440(v: i64) -> Transparency {
    match (v >> 24) & 0xff {
        1 => Transparency::ByBlock,
        2 => {
            let alpha = u32::try_from(v & 0xff).unwrap_or(255);
            let opaque = (alpha * 100 + 127) / 255;
            Transparency::Percent(u8::try_from(100u32.saturating_sub(opaque).min(90)).unwrap_or(90))
        }
        _ => Transparency::ByLayer,
    }
}

/// The tags outside application-defined groups (`102 {` to `102 }`, whose codes are the
/// application's), with their positions in `tags`.
pub(super) fn outside_app(tags: &[Tag]) -> impl Iterator<Item = (usize, &Tag)> {
    let mut in_app = false;
    tags.iter().enumerate().filter(move |(_, t)| {
        if t.code == 102 {
            in_app = t.str().starts_with('{');
        }
        !in_app && t.code != 102
    })
}

/// The groups of the AcDbEntity subclass: the record up to the entity's own subclass marker (all
/// of it in files without markers, and in lists with no AcDbEntity marker, whose layer and colour
/// may follow the entity's marker), or up to an embedded object (101). Later groups reuse the
/// codes: an MTEXT's column width is 48. Markers inside an application-defined group (`102 {`
/// to `102 }`, whose codes are the application's) do not count.
fn entity_groups(tags: &[Tag]) -> &[Tag] {
    let outside_app = || outside_app(tags);
    let marked = outside_app().any(|(_, t)| t.code == 100 && t.str() == "AcDbEntity");
    let end = outside_app().find(|(_, t)| (marked && t.code == 100 && t.str() != "AcDbEntity") || t.code == 101).map_or(tags.len(), |(i, _)| i);
    tags.get(..end).unwrap_or(tags)
}

pub(super) fn common(t: &T) -> Common {
    let e = T(entity_groups(t.0));
    let mut c = Common { layer: e.s(8).unwrap_or_else(|| "0".into()), linetype: e.s(6).unwrap_or_else(|| "ByLayer".into()), ..Common::default() };
    if let Some(v) = e.i(62) {
        c.color = Color::from_aci(v as i16);
        if v < 0 {
            c.color = Color::ByLayer;
            c.visible = true;
        }
    }
    if let Some(tc) = e.i(420) {
        c.color = Color::True(Rgb::from_u32(tc as u32));
    }
    if let Some(lw) = e.i(370) {
        c.lineweight = Lineweight::from_dxf(lw as i16);
    }
    if let Some(s) = e.f(48) {
        c.ltscale = if s > 0.0 { s } else { 1.0 };
    }
    if e.i(60) == Some(1) {
        c.visible = false;
    }
    if let Some(v) = e.i(440) {
        c.transparency = transparency_440(v);
    }
    c.thickness = t.fd(39, 0.0);
    c.extrusion = Vec3::new(t.fd(210, 0.0), t.fd(220, 0.0), t.fd(230, 1.0));
    c
}

/// Apply the OCS → WCS transform for 2D entities with a non-default extrusion.
pub(super) fn ocs(c: &Common, p: Vec3) -> Vec3 {
    ocs_n(c.extrusion, p)
}

pub(super) fn ocs_n(n: Vec3, p: Vec3) -> Vec3 {
    if (n.z - 1.0).abs() < 1e-12 && n.x.abs() < 1e-12 && n.y.abs() < 1e-12 { p } else { cadcraft_geom::Mat4::ocs(n).apply(p) }
}

/// Whether an object with OCS points `pts` (x, y and elevation) lies in a plane tilted off the
/// XY plane: drawing its points as plan coordinates (x flipped for a mirrored plane, as the
/// readers do) would put some of them off their place seen from above by more than a
/// billionth of the object's size and distance from the origin. A normal that is the Z axis
/// or the Z axis turned over moves nothing, and noise in one moves points by a negligible
/// amount, so those objects keep their arcs; however small the tilt, an object far enough from
/// the origin to be moved visibly is projected.
pub(super) fn tilted(n: Vec3, pts: impl IntoIterator<Item = Vec3>) -> bool {
    if !n.is_finite() || n.len() < 1e-12 {
        return false;
    }
    let m = cadcraft_geom::Mat4::ocs(n);
    let flip = if n.z < 0.0 { -1.0 } else { 1.0 };
    let (mut moved, mut reach) = (0.0f64, 1.0f64);
    for p in pts.into_iter().filter(|p| p.is_finite()) {
        let shown = Vec2::new(flip * p.x, p.y);
        moved = moved.max((m.apply(p).xy() - shown).len());
        reach = reach.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
    }
    moved > 1e-9 * reach
}

/// Points along a run of OCS vertices at `elevation`, for [`tilted`]: its vertices and, on its
/// arcs, points between them (an arc can bulge where its ends don't move).
pub(super) fn run_points(vertices: &[PolyVertex], elevation: f64, closed: bool) -> Vec<Vec3> {
    let mut pts = Vec::with_capacity(vertices.len());
    for (i, v) in vertices.iter().enumerate() {
        pts.push(v.p);
        let next = if i + 1 < vertices.len() {
            vertices.get(i + 1)
        } else if closed {
            vertices.first()
        } else {
            None
        };
        if let Some(w) = next.filter(|_| v.bulge != 0.0) {
            // Each arc sampled from its own size, so a small one next to a long edge counts too.
            let arc = cadcraft_geom::Polyline { vertices: vec![*v, PolyVertex::new(w.p)], closed: false };
            let b = arc.bounds();
            let size = (b.max - b.min).len();
            pts.extend(arc.tessellate(if size.is_finite() && size > 0.0 { size * 0.05 } else { 1e-6 }));
        }
    }
    pts.into_iter().map(|p| p.to3(elevation)).collect()
}

/// A run of OCS vertices at `elevation` in the plane of `n` as seen from above in the WCS. A
/// tilted arc is an elliptical arc there, so each bulged edge is flattened (within a thousandth
/// of the edge's own size), its width spread evenly along it; vertices that start straight
/// edges keep their widths. Also gives the mean WCS Z, the elevation the flattened run takes.
pub(super) fn plan_of_tilted(n: Vec3, elevation: f64, vertices: &[PolyVertex], closed: bool) -> (Vec<PolyVertex>, f64) {
    let m = cadcraft_geom::Mat4::ocs(n);
    let mut local: Vec<PolyVertex> = Vec::with_capacity(vertices.len());
    for (i, v) in vertices.iter().enumerate() {
        let next = if i + 1 < vertices.len() {
            vertices.get(i + 1)
        } else if closed {
            vertices.first()
        } else {
            None
        };
        match next {
            Some(w) if v.bulge != 0.0 => {
                let arc = cadcraft_geom::Polyline { vertices: vec![*v, PolyVertex::new(w.p)], closed: false };
                // From the edge's own size, so a long edge elsewhere can't flatten a small arc away.
                let b = arc.bounds();
                let size = (b.max - b.min).len();
                let edge = arc.tessellate(if size.is_finite() && size > 0.0 { size * 1e-3 } else { 1e-6 });
                // The edge runs from this vertex to the next one in equal turns of its arc; its
                // width changes evenly along it, as a tapered segment's does.
                let steps = edge.len().saturating_sub(1).max(1);
                let width = |k: usize| v.start_width + (v.end_width - v.start_width) * k as f64 / steps as f64;
                local.extend(edge.iter().take(steps).enumerate().map(|(k, p)| PolyVertex {
                    p: *p,
                    bulge: 0.0,
                    start_width: width(k),
                    end_width: width(k + 1),
                }));
            }
            _ => local.push(PolyVertex { bulge: 0.0, ..*v }),
        }
    }
    let to_plan = |p: Vec2| m.apply(Vec3::new(p.x, p.y, elevation));
    let z = if local.is_empty() { elevation } else { local.iter().map(|v| to_plan(v.p).z).sum::<f64>() / local.len() as f64 };
    (local.into_iter().map(|v| PolyVertex { p: to_plan(v.p).xy(), ..v }).collect(), z)
}
