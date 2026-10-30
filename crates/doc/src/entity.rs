//! Entities: the graphical objects of a drawing.

use cadcraft_color::Color;
use cadcraft_geom::{
    Arc as GArc, Bounds2, Circle as GCircle, Ellipse as GEllipse, Line as GLine, Mat3, PolyVertex, Polyline, Segment, Spline as GSpline, Vec2, Vec3,
};
use serde::{Deserialize, Serialize};

mod mline;
pub use mline::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub handle: Handle,
    #[serde(default)]
    pub common: Common,
    pub kind: EntityKind,
}

impl Entity {
    pub fn new(handle: Handle, kind: EntityKind) -> Self {
        Entity { handle, common: Common::default(), kind }
    }
}
fn one() -> f64 {
    1.0
}
fn standard() -> String {
    "Standard".into()
}

mod helix;
pub use helix::*;
fn yes() -> bool {
    true
}
mod region;
pub use region::*;

mod block;
mod common;
mod curves;
mod dim;
mod fill;
mod layout;
mod text;
mod wipeout;

pub use block::{Attrib, AttribModes, AttribProps, Insert, MultilineAttrib, mtext_literal};
pub use common::{Common, Handle, Lineweight, RawTag, Transparency};
pub use curves::{Arc, Circle, Ellipse, Face3d, Line, LwPolyline, Point, Polyline3d, RayLine, Solid};
pub use dim::{AssocSnap, DimAssoc, DimBreak, DimKind, DimMarks, Dimension, Inspection, JoggedRadius, Leader, MLeader};
pub use fill::{Gradient, Hatch, HatchLoop};
pub use layout::{Image, Table, TableCell, Viewport};
pub use text::{HAlign, MText, MTextColumnType, MTextColumns, Text, VAlign};
pub use wipeout::Wipeout;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unknown {
    pub dxf_type: String,
    pub tags: Vec<RawTag>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntityKind {
    Line(Line),
    Point(Point),
    Circle(Circle),
    Arc(Arc),
    Ellipse(Ellipse),
    LwPolyline(LwPolyline),
    Polyline3d(Polyline3d),
    Spline(GSpline),
    Helix(Helix),
    Ray(RayLine),
    XLine(RayLine),
    Text(Text),
    MText(MText),
    AttDef(Attrib),
    Insert(Insert),
    Dimension(Dimension),
    MLine(MLine),
    Leader(Leader),
    MLeader(MLeader),
    Hatch(Hatch),
    Solid(Solid),
    Trace(Solid),
    Face3d(Face3d),
    Region(Region),
    Viewport(Viewport),
    Image(Image),
    Wipeout(Wipeout),
    Table(Table),
    Tolerance(crate::Tolerance),
    Unknown(Unknown),
}

/// A primitive the entity decomposes into, for snapping, selection, trimming and rendering.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Seg(Segment),
    Circle(GCircle),
    Ellipse(GEllipse),
    Spline(GSpline),
    Point(Vec2),
    /// An infinite line (`ray == false`) or a ray from `base` along `dir`.
    Infinite {
        base: Vec2,
        dir: Vec2,
        ray: bool,
    },
    /// A filled polygon (solids, wide polylines, hatch fill) — the outline is also selectable.
    Fill(Vec<Vec2>),
}

impl EntityKind {
    /// AutoCAD's object type name as shown in the Properties palette.
    pub fn type_name(&self) -> &'static str {
        match self {
            EntityKind::Line(_) => "Line",
            EntityKind::Point(_) => "Point",
            EntityKind::Circle(_) => "Circle",
            EntityKind::Arc(_) => "Arc",
            EntityKind::Ellipse(_) => "Ellipse",
            EntityKind::LwPolyline(_) => "Polyline",
            EntityKind::Polyline3d(_) => "3D Polyline",
            EntityKind::Spline(_) => "Spline",
            EntityKind::Helix(_) => "Helix",
            EntityKind::Ray(_) => "Ray",
            EntityKind::XLine(_) => "Xline",
            EntityKind::Text(_) => "Text",
            EntityKind::MText(_) => "MText",
            EntityKind::AttDef(_) => "Attribute Definition",
            EntityKind::Insert(_) => "Block Reference",
            EntityKind::Dimension(_) => "Dimension",
            EntityKind::MLine(_) => "MLine",
            EntityKind::Leader(_) => "Leader",
            EntityKind::MLeader(_) => "Multileader",
            EntityKind::Hatch(_) => "Hatch",
            EntityKind::Solid(_) => "2D Solid",
            EntityKind::Trace(_) => "Trace",
            EntityKind::Face3d(_) => "3D Face",
            EntityKind::Region(_) => "Region",
            EntityKind::Viewport(_) => "Viewport",
            EntityKind::Image(_) => "Raster Image",
            EntityKind::Wipeout(_) => "Wipeout",
            EntityKind::Table(_) => "Table",
            EntityKind::Tolerance(_) => "Tolerance",
            EntityKind::Unknown(_) => "Proxy",
        }
    }
    /// DXF entity type name.
    pub fn dxf_name(&self) -> &str {
        match self {
            EntityKind::Line(_) => "LINE",
            EntityKind::Point(_) => "POINT",
            EntityKind::Circle(_) => "CIRCLE",
            EntityKind::Arc(_) => "ARC",
            EntityKind::Ellipse(_) => "ELLIPSE",
            EntityKind::LwPolyline(_) => "LWPOLYLINE",
            EntityKind::Polyline3d(_) => "POLYLINE",
            EntityKind::Spline(_) => "SPLINE",
            EntityKind::Helix(_) => "HELIX",
            EntityKind::Ray(_) => "RAY",
            EntityKind::XLine(_) => "XLINE",
            EntityKind::Text(_) => "TEXT",
            EntityKind::MText(_) => "MTEXT",
            EntityKind::AttDef(_) => "ATTDEF",
            EntityKind::Insert(_) => "INSERT",
            EntityKind::Dimension(_) => "DIMENSION",
            EntityKind::MLine(_) => "MLINE",
            EntityKind::Leader(_) => "LEADER",
            EntityKind::MLeader(_) => "MULTILEADER",
            EntityKind::Hatch(_) => "HATCH",
            EntityKind::Solid(_) => "SOLID",
            EntityKind::Trace(_) => "TRACE",
            EntityKind::Face3d(_) => "3DFACE",
            EntityKind::Region(_) => "REGION",
            EntityKind::Viewport(_) => "VIEWPORT",
            EntityKind::Image(_) => "IMAGE",
            EntityKind::Wipeout(_) => "WIPEOUT",
            EntityKind::Table(_) => "ACAD_TABLE",
            EntityKind::Tolerance(_) => "TOLERANCE",
            EntityKind::Unknown(u) => u.dxf_type.as_str(),
        }
    }

    /// Geometric primitives in WCS (2D projection). Text/blocks/dimensions return their
    /// insertion points only; their full geometry comes from the renderer's expansion.
    pub fn prims(&self) -> Vec<Prim> {
        match self {
            EntityKind::Line(l) => vec![Prim::Seg(Segment::Line(GLine::new(l.a.xy(), l.b.xy())))],
            EntityKind::Point(p) => vec![Prim::Point(p.p.xy())],
            EntityKind::Circle(c) => vec![Prim::Circle(GCircle::new(c.center.xy(), c.radius))],
            EntityKind::Arc(a) => vec![Prim::Seg(Segment::Arc { arc: GArc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true })],
            EntityKind::Ellipse(e) => {
                vec![Prim::Ellipse(GEllipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end })]
            }
            EntityKind::LwPolyline(p) => {
                let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                pl.segments().into_iter().map(Prim::Seg).collect()
            }
            EntityKind::Polyline3d(p) => {
                let mut v: Vec<Prim> =
                    p.points.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(w.first()?.xy(), w.get(1)?.xy()))))).collect();
                if p.closed
                    && let (Some(a), Some(b)) = (p.points.last(), p.points.first())
                {
                    v.push(Prim::Seg(Segment::Line(GLine::new(a.xy(), b.xy()))));
                }
                v
            }
            EntityKind::Spline(s) => vec![Prim::Spline(s.clone())],
            EntityKind::Helix(h) => vec![Prim::Spline(h.spline())],
            EntityKind::Ray(r) => vec![Prim::Infinite { base: r.base.xy(), dir: r.dir.xy().normalized(), ray: true }],
            EntityKind::XLine(r) => vec![Prim::Infinite { base: r.base.xy(), dir: r.dir.xy().normalized(), ray: false }],
            EntityKind::Solid(s) | EntityKind::Trace(s) => {
                let c = &s.corners;
                vec![Prim::Fill(vec![c[0].xy(), c[1].xy(), c[3].xy(), c[2].xy()])]
            }
            EntityKind::Face3d(f) => {
                let c = &f.corners;
                let pts = [c[0].xy(), c[1].xy(), c[2].xy(), c[3].xy(), c[0].xy()];
                pts.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(*w.first()?, *w.get(1)?))))).collect()
            }
            EntityKind::Region(r) => r.segments().into_iter().map(Prim::Seg).collect(),
            EntityKind::Hatch(h) => {
                h.loops.iter().flat_map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.segments()).map(Prim::Seg).collect()
            }
            EntityKind::Wipeout(w) => {
                w.closed_boundary().windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(*w.first()?, *w.get(1)?))))).collect()
            }
            EntityKind::Viewport(v) => {
                let c = v.center.xy();
                let h = Vec2::new(v.width / 2.0, v.height / 2.0);
                let b = Bounds2::new(c - h, c + h).corners();
                (0..4).map(|i| Prim::Seg(Segment::Line(GLine::new(b[i], b[(i + 1) % 4])))).collect()
            }
            EntityKind::Leader(l) => {
                l.vertices.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(w.first()?.xy(), w.get(1)?.xy()))))).collect()
            }
            EntityKind::Text(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::MText(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::AttDef(a) => vec![Prim::Point(a.text.insert.xy())],
            EntityKind::Insert(i) => vec![Prim::Point(i.insert.xy())],
            EntityKind::Dimension(d) => vec![Prim::Point(d.defpt.xy())],
            EntityKind::MLine(m) => m.segments().into_iter().map(Prim::Seg).collect(),
            EntityKind::MLeader(m) => m
                .leaders
                .iter()
                .flat_map(|l| l.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(w.first()?.xy(), w.get(1)?.xy()))))))
                .chain(m.block.iter().map(|b| Prim::Point(b.insert.xy())))
                .collect(),
            EntityKind::Image(i) => vec![Prim::Point(i.insert.xy())],
            EntityKind::Table(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::Tolerance(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::Unknown(_) => Vec::new(),
        }
    }

    /// Apply a 2D affine transform (move/rotate/uniform scale/mirror). Non-uniform scale turns
    /// circles into ellipses only where the kind supports it; otherwise radii use the mean scale.
    pub fn transform(&mut self, m: &Mat3) {
        let t3 = |p: &mut Vec3| {
            let q = m.apply(p.xy());
            p.x = q.x;
            p.y = q.y;
        };
        let s = m.scale_factor();
        let rot = m.rotation();
        let mirror = m.is_mirroring();
        match self {
            EntityKind::Line(l) => {
                t3(&mut l.a);
                t3(&mut l.b);
            }
            EntityKind::Point(p) => t3(&mut p.p),
            EntityKind::Circle(c) => {
                t3(&mut c.center);
                c.radius *= s;
            }
            EntityKind::Arc(a) => {
                let sp = m.apply(Vec2::polar(a.center.xy(), a.radius, a.start));
                let ep = m.apply(Vec2::polar(a.center.xy(), a.radius, a.end));
                t3(&mut a.center);
                a.radius *= s;
                let c = a.center.xy();
                if mirror {
                    a.start = c.angle_to(ep);
                    a.end = c.angle_to(sp);
                } else {
                    a.start = c.angle_to(sp);
                    a.end = c.angle_to(ep);
                }
            }
            EntityKind::Ellipse(e) => {
                let sp = m.apply(e.center.xy() + e.major.xy() * e.start.cos() + e.major.xy().perp() * (e.ratio * e.start.sin()));
                let ep = m.apply(e.center.xy() + e.major.xy() * e.end.cos() + e.major.xy().perp() * (e.ratio * e.end.sin()));
                t3(&mut e.center);
                let mj = m.apply_vec(e.major.xy());
                e.major = mj.to3(0.0);
                let full = (cadcraft_geom::ccw_sweep(e.start, e.end) - cadcraft_geom::TAU).abs() < 1e-9;
                if !full {
                    let ge = GEllipse { center: e.center.xy(), major: mj, ratio: e.ratio, start: 0.0, end: cadcraft_geom::TAU };
                    let (ps, pe) = (ge.param_of(sp), ge.param_of(ep));
                    if mirror {
                        e.start = pe;
                        e.end = ps;
                    } else {
                        e.start = ps;
                        e.end = pe;
                    }
                }
            }
            EntityKind::LwPolyline(p) => {
                for v in &mut p.vertices {
                    v.p = m.apply(v.p);
                    v.start_width *= s;
                    v.end_width *= s;
                    if mirror {
                        v.bulge = -v.bulge;
                    }
                }
                p.const_width *= s;
            }
            EntityKind::Polyline3d(p) => p.points.iter_mut().for_each(t3),
            EntityKind::Spline(sp) => sp.transform(m),
            EntityKind::Helix(h) => {
                let sp = m.apply(h.start_point().xy());
                t3(&mut h.center);
                h.base_radius *= s;
                h.top_radius *= s;
                h.height *= s;
                h.start_angle = h.center.xy().angle_to(sp);
                // Mirrored, the twist turns the other way.
                h.ccw ^= mirror;
            }
            EntityKind::Ray(r) | EntityKind::XLine(r) => {
                t3(&mut r.base);
                r.dir = m.apply_vec(r.dir.xy()).normalized().to3(0.0);
            }
            EntityKind::Text(t) => transform_text(t, m, s, rot, mirror),
            EntityKind::AttDef(a) => {
                transform_text(&mut a.text, m, s, rot, mirror);
                a.transform_multiline(m);
            }
            EntityKind::MText(t) => {
                t3(&mut t.insert);
                t.height *= s;
                t.width *= s;
                if let Some(c) = &mut t.columns {
                    c.scale(s);
                }
                t.rotation = cadcraft_geom::norm_angle(t.rotation + rot);
            }
            EntityKind::Insert(i) => {
                t3(&mut i.insert);
                i.scale.x *= s;
                i.scale.y *= s;
                i.scale.z *= s;
                i.rotation = cadcraft_geom::norm_angle(i.rotation + rot);
                if mirror {
                    i.scale.y = -i.scale.y;
                    i.rotation = cadcraft_geom::norm_angle(rot - (i.rotation - rot));
                }
                for a in &mut i.attribs {
                    transform_text(&mut a.text, m, s, rot, false);
                    a.transform_multiline(m);
                }
                i.col_spacing *= s;
                i.row_spacing *= s;
            }
            EntityKind::Dimension(d) => {
                for p in [&mut d.defpt, &mut d.text_mid, &mut d.p13, &mut d.p14, &mut d.p15, &mut d.p16] {
                    t3(p);
                }
                if let DimKind::Linear { rotation } = &mut d.kind {
                    *rotation = cadcraft_geom::norm_angle(*rotation + rot);
                }
                d.marks.transform(&t3, rot, s, mirror);
                d.block = None;
            }
            EntityKind::MLine(ml) => {
                ml.vertices.iter_mut().for_each(|v| t3(&mut v.p));
                ml.vertices.iter_mut().flat_map(|v| v.cuts.iter_mut()).for_each(|c| (c.start, c.end) = (c.start * s, c.end * s));
                // Mirrored, the elements change sides.
                ml.scale *= if mirror { -s } else { s };
            }
            EntityKind::Leader(l) => l.vertices.iter_mut().for_each(t3),
            EntityKind::MLeader(ml) => {
                for l in &mut ml.leaders {
                    l.iter_mut().for_each(t3);
                }
                t3(&mut ml.landing);
                if let Some(t) = &mut ml.text {
                    t3(&mut t.insert);
                    t.height *= s;
                    t.width *= s;
                    if let Some(c) = &mut t.columns {
                        c.scale(s);
                    }
                }
                if let Some(b) = &mut ml.block {
                    // Transformed exactly like a block reference.
                    let mut k = EntityKind::Insert(b.clone());
                    k.transform(m);
                    if let EntityKind::Insert(nb) = k {
                        *b = nb;
                    }
                }
            }
            EntityKind::Hatch(h) => {
                for l in &mut h.loops {
                    for v in &mut l.vertices {
                        v.p = m.apply(v.p);
                        if mirror {
                            v.bulge = -v.bulge;
                        }
                    }
                }
                h.scale *= s;
                h.angle = cadcraft_geom::norm_angle(h.angle + rot);
                h.origin = m.apply(h.origin);
            }
            EntityKind::Solid(so) | EntityKind::Trace(so) => so.corners.iter_mut().for_each(t3),
            EntityKind::Face3d(f) => f.corners.iter_mut().for_each(t3),
            EntityKind::Region(r) => {
                for v in r.loops.iter_mut().flat_map(|l| l.vertices.iter_mut()) {
                    v.p = m.apply(v.p);
                    if mirror {
                        v.bulge = -v.bulge;
                    }
                }
            }
            EntityKind::Viewport(v) => {
                t3(&mut v.center);
                v.width *= s;
                v.height *= s;
            }
            EntityKind::Image(i) => {
                t3(&mut i.insert);
                i.u = m.apply_vec(i.u.xy()).to3(0.0);
                i.v = m.apply_vec(i.v.xy()).to3(0.0);
            }
            EntityKind::Wipeout(w) => w.boundary.iter_mut().for_each(|p| *p = m.apply(*p)),
            EntityKind::Table(t) => {
                if let Some(b) = &mut t.breaks {
                    b.height *= s;
                    b.spacing *= s;
                    b.heights.iter_mut().for_each(|h| *h *= s);
                    b.offsets.iter_mut().for_each(|o| *o = m.apply_vec(*o));
                }
                t3(&mut t.insert);
                t.col_widths.iter_mut().for_each(|w| *w *= s);
                t.row_heights.iter_mut().for_each(|h| *h *= s);
                t.text_height *= s;
            }
            EntityKind::Tolerance(t) => {
                t3(&mut t.insert);
                // Like text with MIRRTEXT = 0: a mirrored frame keeps reading left to right.
                let a = if mirror { t.rotation() } else { t.rotation() + rot };
                t.dir = Vec2::from_angle(cadcraft_geom::norm_angle(a)).to3(0.0);
            }
            EntityKind::Unknown(_) => {}
        }
    }

    /// Grip points (editable defining points), in display order.
    pub fn grips(&self) -> Vec<Vec2> {
        // A broken table has a grip for each part.
        if let EntityKind::Table(t) = self {
            return t.grip_points();
        }
        match self {
            EntityKind::Line(l) => vec![l.a.xy(), l.a.xy().mid(l.b.xy()), l.b.xy()],
            EntityKind::Point(p) => vec![p.p.xy()],
            EntityKind::Circle(c) => {
                let o = c.center.xy();
                vec![o, o + Vec2::X * c.radius, o + Vec2::Y * c.radius, o - Vec2::X * c.radius, o - Vec2::Y * c.radius]
            }
            EntityKind::Arc(a) => {
                let g = GArc::new(a.center.xy(), a.radius, a.start, a.end);
                vec![g.start_point(), g.mid_point(), g.end_point(), a.center.xy()]
            }
            EntityKind::Ellipse(e) => {
                let c = e.center.xy();
                let mj = e.major.xy();
                let mn = mj.perp() * e.ratio;
                vec![c, c + mj, c + mn, c - mj, c - mn]
            }
            EntityKind::LwPolyline(p) => {
                let mut g: Vec<Vec2> = Vec::new();
                let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                // Compute the segments once, not once per vertex: rebuilding the whole segment
                // vector inside the loop is quadratic in the vertex count (CWE-407).
                let segs = pl.segments();
                for (i, v) in p.vertices.iter().enumerate() {
                    g.push(v.p);
                    if let Some(s) = segs.get(i) {
                        g.push(s.mid());
                    }
                }
                g
            }
            EntityKind::Polyline3d(p) => p.points.iter().map(|q| q.xy()).collect(),
            EntityKind::Spline(s) => {
                if s.fit.is_empty() {
                    s.control.clone()
                } else {
                    s.fit.clone()
                }
            }
            EntityKind::Helix(h) => vec![h.center.xy(), h.start_point().xy(), h.end_point().xy()],
            EntityKind::Ray(r) | EntityKind::XLine(r) => vec![r.base.xy(), r.base.xy() + r.dir.xy()],
            EntityKind::Text(t) => vec![t.justify_point()],
            EntityKind::MText(t) => vec![t.insert.xy()],
            EntityKind::AttDef(a) => vec![a.text.justify_point()],
            EntityKind::Insert(i) => std::iter::once(i.insert.xy()).chain(i.attribs.iter().map(|a| a.text.justify_point())).collect(),
            EntityKind::Dimension(d) => vec![d.p13.xy(), d.p14.xy(), d.defpt.xy(), d.text_mid.xy()],
            EntityKind::MLine(m) => m.points(),
            EntityKind::Leader(l) => l.vertices.iter().map(|v| v.xy()).collect(),
            EntityKind::MLeader(m) => m.leaders.iter().flat_map(|l| l.iter().map(|v| v.xy())).chain(std::iter::once(m.landing.xy())).collect(),
            EntityKind::Hatch(h) => {
                let b = Bounds2::from_points(h.loops.iter().flat_map(|l| l.vertices.iter().map(|v| v.p)));
                vec![b.center()]
            }
            EntityKind::Solid(s) | EntityKind::Trace(s) => s.corners.iter().map(|c| c.xy()).collect(),
            EntityKind::Face3d(f) => f.corners.iter().map(|c| c.xy()).collect(),
            EntityKind::Region(r) => vec![r.bounds().center()],
            EntityKind::Viewport(v) => {
                let c = v.center.xy();
                let h = Vec2::new(v.width / 2.0, v.height / 2.0);
                Bounds2::new(c - h, c + h).corners().to_vec()
            }
            EntityKind::Image(i) => vec![i.insert.xy()],
            EntityKind::Wipeout(w) => w.vertices().to_vec(),
            EntityKind::Table(t) => vec![t.insert.xy()],
            EntityKind::Tolerance(t) => vec![t.insert.xy()],
            EntityKind::Unknown(_) => Vec::new(),
        }
    }
}

fn transform_text(t: &mut Text, m: &Mat3, s: f64, rot: f64, mirror: bool) {
    let q = m.apply(t.insert.xy());
    t.insert.x = q.x;
    t.insert.y = q.y;
    if let Some(a) = &mut t.align_pt {
        let q = m.apply(a.xy());
        a.x = q.x;
        a.y = q.y;
    }
    t.height *= s;
    // MIRRTEXT = 0 (the default): mirrored text keeps reading left to right.
    t.rotation = cadcraft_geom::norm_angle(t.rotation + if mirror { 0.0 } else { rot });
}

#[cfg(test)]
mod grip_tests {
    use super::*;
    use cadcraft_geom::PolyVertex;

    #[test]
    fn handle_parse_hex_accepts_only_hex_digits() {
        assert_eq!(Handle::parse_hex("1a"), Some(Handle(0x1A)));
        assert_eq!(Handle::parse_hex(" 1A "), Some(Handle(0x1A)));
        for bad in ["+1A", "-1A", "", "+", "1 A"] {
            assert_eq!(Handle::parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn polyline_grips_are_vertices_and_midpoints() {
        // Grips are each vertex plus the midpoint of each segment; the segment vector is computed
        // once rather than rebuilt per vertex (CWE-407, row 35).
        let verts: Vec<PolyVertex> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
        let e = EntityKind::LwPolyline(LwPolyline { vertices: verts, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false });
        let g = e.grips();
        // 3 vertices + 2 segment midpoints.
        assert_eq!(g.len(), 5, "{g:?}");
        assert!(g.contains(&Vec2::new(0.0, 0.0)) && g.contains(&Vec2::new(5.0, 0.0)));
    }
}
