//! DXF → Drawing.

use std::collections::HashMap;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::{PolyVertex, Spline, Vec2, Vec3};

use crate::{IoError, Result};

pub(crate) mod mtext_dxf;

/// Value lookup helpers over an entity's tags.
struct T<'a>(&'a [Tag]);

impl T<'_> {
    fn f(&self, code: i32) -> Option<f64> {
        self.0.iter().find(|t| t.code == code).map(Tag::f64)
    }
    fn fd(&self, code: i32, d: f64) -> f64 {
        self.f(code).unwrap_or(d)
    }
    fn i(&self, code: i32) -> Option<i64> {
        self.0.iter().find(|t| t.code == code).map(Tag::i64)
    }
    fn s(&self, code: i32) -> Option<String> {
        self.0.iter().find(|t| t.code == code).map(Tag::str)
    }
    fn p(&self, code: i32) -> Vec3 {
        Vec3::new(self.fd(code, 0.0), self.fd(code + 10, 0.0), self.fd(code + 20, 0.0))
    }
    fn all_f(&self, code: i32) -> Vec<f64> {
        self.0.iter().filter(|t| t.code == code).map(Tag::f64).collect()
    }
    fn pts(&self, code: i32) -> Vec<Vec2> {
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

fn common(t: &T) -> Common {
    let mut c = Common { layer: t.s(8).unwrap_or_else(|| "0".into()), linetype: t.s(6).unwrap_or_else(|| "ByLayer".into()), ..Common::default() };
    if let Some(v) = t.i(62) {
        c.color = Color::from_aci(v as i16);
        if v < 0 {
            c.color = Color::ByLayer;
            c.visible = true;
        }
    }
    if let Some(tc) = t.i(420) {
        c.color = Color::True(Rgb::from_u32(tc as u32));
    }
    if let Some(lw) = t.i(370) {
        c.lineweight = Lineweight::from_dxf(lw as i16);
    }
    if let Some(s) = t.f(48) {
        c.ltscale = if s > 0.0 { s } else { 1.0 };
    }
    if t.i(60) == Some(1) {
        c.visible = false;
    }
    if let Some(v) = t.i(440) {
        c.transparency = transparency_440(v);
    }
    c.thickness = t.fd(39, 0.0);
    c.extrusion = Vec3::new(t.fd(210, 0.0), t.fd(220, 0.0), t.fd(230, 1.0));
    c
}

/// Apply the OCS → WCS transform for 2D entities with a non-default extrusion.
fn ocs(c: &Common, p: Vec3) -> Vec3 {
    ocs_n(c.extrusion, p)
}

fn ocs_n(n: Vec3, p: Vec3) -> Vec3 {
    if (n.z - 1.0).abs() < 1e-12 && n.x.abs() < 1e-12 && n.y.abs() < 1e-12 { p } else { cadcraft_geom::Mat4::ocs(n).apply(p) }
}

/// The properties an ATTRIB carries itself: those it has groups for. Its layer counts only when
/// it differs from its block reference's (an attribute defined on layer 0 goes on the reference's
/// layer, as it is drawn anyway).
fn attrib_props(t: &T, insert_layer: &str) -> AttribProps {
    let c = common(t);
    AttribProps {
        layer: t.s(8).filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case(insert_layer)),
        color: (t.i(62).is_some() || t.i(420).is_some()).then_some(c.color),
        linetype: t.s(6).filter(|l| !l.is_empty()),
        lineweight: t.i(370).is_some().then_some(c.lineweight),
    }
}

/// An attribute's modes from its own groups: flags (70) 4 verify and 8 preset, and Lock
/// position, the 280 after its tag in its subclass (a 280 before the tag is the subclass version).
fn attrib_modes(own: &[Tag]) -> AttribModes {
    let flags = own.iter().find(|t| t.code == 70).map(Tag::i64).unwrap_or(0);
    let sub = own.iter().position(|t| t.code == 100 && t.str().trim().starts_with("AcDbAttribute")).unwrap_or(0);
    let rest = own.get(sub..).unwrap_or_default();
    let lock_position = rest.iter().position(|t| t.code == 2).is_some_and(|at| rest.iter().skip(at + 1).any(|t| t.code == 280 && t.i64() != 0));
    AttribModes { verify: flags & 4 != 0, preset: flags & 8 != 0, lock_position }
}

/// An ATTRIB's or ATTDEF's groups split at its embedded multiline text (`101 Embedded Object`):
/// the attribute's own groups and, for a multiline attribute, the MTEXT groups after the marker.
fn split_embedded(tags: &[Tag]) -> (&[Tag], Option<&[Tag]>) {
    match tags.iter().position(|t| t.code == 101) {
        Some(i) => (tags.get(..i).unwrap_or(tags), tags.get(i + 1..)),
        None => (tags, None),
    }
}

/// A multiline attribute's text, from the MTEXT groups embedded in it.
fn multiline_attrib(embedded: Option<&[Tag]>, c: &Common) -> Option<MultilineAttrib> {
    let mtext = mtext_from(embedded.filter(|e| !e.is_empty())?, c);
    let read_as = cadcraft_fonts::plain_mtext(&mtext.contents);
    Some(MultilineAttrib { mtext, read_as })
}

/// Multiline text from MTEXT groups (an MTEXT entity's, or a multiline attribute's embedded ones).
fn mtext_from(tags: &[Tag], c: &Common) -> MText {
    let t = T(tags);
    // Contents: code 3 chunks followed by code 1.
    let mut contents = String::new();
    for tg in tags.iter().filter(|x| x.code == 3) {
        contents.push_str(&tg.str());
    }
    contents.push_str(&t.s(1).unwrap_or_default());
    let contents = mtext_dxf::decode_contents(&contents);
    let columns = mtext_dxf::read_columns(tags);
    // The direction comes from the MTEXT's own groups only: in an R2018+ embedded object
    // (group 101), 11 is the insertion point, not the X-axis direction (#667, as in #834).
    let own = T(mtext_dxf::before_columns(tags));
    let mut rotation = match (own.f(11), own.f(21)) {
        (Some(x), Some(y)) if x != 0.0 || y != 0.0 => Vec2::new(x, y).angle(),
        _ => own.fd(50, 0.0).to_radians(),
    };
    let mut attach = t.i(71).unwrap_or(1).clamp(1, 9) as u8;
    if c.extrusion.z < 0.0 {
        // Insertion point and direction are WCS, but the line height runs along extrusion ×
        // direction: under (0,0,-1) the glyphs are mirrored. Drawn readable (MIRRTEXT = 0) with
        // the same footprint, the text turns round and left/right attachment swap.
        rotation = cadcraft_geom::norm_angle(rotation + std::f64::consts::PI);
        attach = match attach {
            1 | 4 | 7 => attach + 2,
            3 | 6 | 9 => attach - 2,
            a => a,
        };
    }
    MText {
        insert: t.p(10),
        height: t.fd(40, 0.2).max(1e-9),
        width: match &columns {
            Some((_, w)) if t.fd(41, 0.0) <= 0.0 => *w,
            _ => t.fd(41, 0.0).max(0.0),
        },
        attach,
        rotation,
        style: t.s(7).unwrap_or_else(|| "Standard".into()),
        contents,
        line_spacing: t.fd(44, 1.0),
        line_spacing_exact: t.i(73) == Some(2),
        columns: columns.map(|(c, _)| c),
    }
}

fn text_from(t: &T) -> Text {
    let h = match t.i(72).unwrap_or(0) {
        1 => HAlign::Center,
        2 => HAlign::Right,
        3 => HAlign::Aligned,
        4 => HAlign::Middle,
        5 => HAlign::Fit,
        _ => HAlign::Left,
    };
    let v = match t.i(73).or_else(|| t.i(74)).unwrap_or(0) {
        1 => VAlign::Bottom,
        2 => VAlign::Middle,
        3 => VAlign::Top,
        _ => VAlign::Baseline,
    };
    let align = if t.f(11).is_some() { Some(t.p(11)) } else { None };
    let mut text = Text {
        insert: t.p(10),
        align_pt: if h == HAlign::Left && v == VAlign::Baseline { None } else { align },
        height: t.fd(40, 0.2).max(1e-9),
        value: t.s(1).unwrap_or_default(),
        rotation: t.fd(50, 0.0).to_radians(),
        width_factor: t.fd(41, 1.0),
        oblique: t.fd(51, 0.0).to_radians(),
        style: t.s(7).unwrap_or_else(|| "Standard".into()),
        halign: h,
        valign: v,
    };
    let n = Vec3::new(t.fd(210, 0.0), t.fd(220, 0.0), t.fd(230, 1.0));
    if n.z < 0.0 {
        mirror_text(&mut text, align.map(|a| ocs_n(n, a)), ocs_n(n, t.p(10)));
    }
    text
}

/// TEXT / ATTRIB / ATTDEF with extrusion (0,0,-1): the points are OCS and the text runs from its
/// insertion point towards -x in the WCS, with mirrored glyphs. CADCraft draws text readable
/// (MIRRTEXT = 0), so it keeps the mirrored footprint: the direction turns to -rotation and
/// left/right justification swap ends (start ↔ end point).
fn mirror_text(t: &mut Text, align: Option<Vec3>, insert: Vec3) {
    t.rotation = cadcraft_geom::norm_angle(-t.rotation);
    let baseline_left = |t: &Text, p: Vec3| if t.valign == VAlign::Baseline { None } else { Some(p) };
    match t.halign {
        HAlign::Left => {
            // TL/ML/BL are placed by their alignment point, baseline-left text by its start point.
            t.halign = HAlign::Right;
            t.insert = insert;
            t.align_pt = Some(if t.valign == VAlign::Baseline { insert } else { align.unwrap_or(insert) });
        }
        HAlign::Right => {
            t.halign = HAlign::Left;
            t.insert = align.unwrap_or(insert);
            t.align_pt = baseline_left(t, t.insert);
        }
        HAlign::Aligned | HAlign::Fit => {
            t.insert = align.unwrap_or(insert);
            t.align_pt = Some(insert);
        }
        HAlign::Center | HAlign::Middle => {
            t.insert = insert;
            t.align_pt = align;
        }
    }
}

/// Parse one entity record. POLYLINE/INSERT followers are handled by the caller.
fn entity(kind: &str, tags: &[Tag]) -> Option<(Common, EntityKind)> {
    entity_with_table_budget(kind, tags, &mut 4_000_000)
}

/// Parse one entity while charging materialized table cells to the caller's shared import budget.
fn entity_with_table_budget(kind: &str, tags: &[Tag], table_cells_left: &mut usize) -> Option<(Common, EntityKind)> {
    // The paper-space viewport our writer adds (dxf_write::paper_view) stays implicit.
    if kind == "VIEWPORT" && crate::dxf_ext::is_paper_view(tags) {
        return None;
    }
    let t = T(tags);
    let c = common(&t);
    let k = match kind {
        "LINE" => EntityKind::Line(Line { a: t.p(10), b: t.p(11) }),
        "POINT" => EntityKind::Point(Point { p: t.p(10), angle: t.fd(50, 0.0).to_radians() }),
        "CIRCLE" => EntityKind::Circle(Circle { center: ocs(&c, t.p(10)), radius: t.fd(40, 1.0).abs() }),
        "ARC" => {
            let mut a =
                Arc { center: ocs(&c, t.p(10)), radius: t.fd(40, 1.0).abs(), start: t.fd(50, 0.0).to_radians(), end: t.fd(51, 360.0).to_radians() };
            if c.extrusion.z < 0.0 {
                // Mirrored OCS: angles run the other way.
                let (s, e) = (std::f64::consts::PI - a.end, std::f64::consts::PI - a.start);
                a.start = cadcraft_geom::norm_angle(s);
                a.end = cadcraft_geom::norm_angle(e);
            }
            EntityKind::Arc(a)
        }
        "ELLIPSE" => {
            let mut e = Ellipse {
                center: t.p(10),
                major: t.p(11),
                ratio: t.fd(40, 1.0).clamp(1e-9, 1.0),
                start: t.fd(41, 0.0),
                end: t.fd(42, std::f64::consts::TAU),
            };
            if c.extrusion.z < 0.0 && (cadcraft_geom::ccw_sweep(e.start, e.end) - std::f64::consts::TAU).abs() > 1e-9 {
                // Centre and major axis are WCS, but the minor axis is extrusion × major: with (0,0,-1)
                // it points the other way, so parameter t lands where -t does in the XY plane. A full
                // ellipse is symmetric and stays as it is.
                let (s, e2) = (-e.end, -e.start);
                e.start = cadcraft_geom::norm_angle(s);
                e.end = cadcraft_geom::norm_angle(e2);
            }
            EntityKind::Ellipse(e)
        }
        "LWPOLYLINE" => {
            let mut verts = Vec::new();
            let mut cur: Option<PolyVertex> = None;
            for tg in tags {
                match tg.code {
                    10 => {
                        if let Some(v) = cur.take() {
                            verts.push(v);
                        }
                        cur = Some(PolyVertex { p: Vec2::new(tg.f64(), 0.0), ..Default::default() });
                    }
                    20 => {
                        if let Some(v) = cur.as_mut() {
                            v.p.y = tg.f64();
                        }
                    }
                    40 => {
                        if let Some(v) = cur.as_mut() {
                            v.start_width = tg.f64();
                        }
                    }
                    41 => {
                        if let Some(v) = cur.as_mut() {
                            v.end_width = tg.f64();
                        }
                    }
                    42 => {
                        if let Some(v) = cur.as_mut() {
                            v.bulge = tg.f64();
                        }
                    }
                    _ => {}
                }
            }
            verts.extend(cur);
            let flags = t.i(70).unwrap_or(0);
            if c.extrusion.z < 0.0 {
                for v in &mut verts {
                    v.p.x = -v.p.x;
                    v.bulge = -v.bulge;
                }
            }
            let pl = LwPolyline {
                vertices: verts,
                closed: flags & 1 != 0,
                const_width: t.fd(43, 0.0),
                elevation: t.fd(38, 0.0),
                plinegen: flags & 128 != 0,
            };
            // A region CADCraft wrote as polylines (`dxf_region`).
            crate::dxf_region::from_polyline(tags, pl)?
        }
        "SPLINE" => {
            let degree = t.i(71).unwrap_or(3).clamp(1, 10) as usize;
            let knots = t.all_f(40);
            let control = t.pts(10);
            let weights = t.all_f(41);
            let fit = t.pts(11);
            let tangent = |c: i32| t.f(c).map(|_| t.p(c).xy());
            let fit_opts = cadcraft_geom::FitOptions {
                knots: Default::default(),
                start_tangent: tangent(12),
                end_tangent: tangent(13),
                tolerance: t.fd(44, 0.0),
            }
            .sanitized();
            let closed = t.i(70).unwrap_or(0) & 1 != 0;
            let mut sp = Spline { degree, knots, control, weights, fit: fit.clone(), closed, fit_opts };
            if !sp.is_valid() && fit.len() >= 2 {
                sp = Spline::fit_with(&fit, sp.closed, fit_opts);
            } else if !sp.is_valid() && sp.control.len() >= 2 {
                sp = Spline::from_control(sp.control.clone(), degree);
            } else if fit.len() >= 3 {
                // DXF keeps the knots, not how they were spaced: recover it for later refits.
                sp.fit_opts.knots = sp.infer_knot_param();
            }
            EntityKind::Spline(sp)
        }
        "HELIX" => EntityKind::Helix(helix(tags)),
        "RAY" => EntityKind::Ray(RayLine { base: t.p(10), dir: t.p(11) }),
        "XLINE" => EntityKind::XLine(RayLine { base: t.p(10), dir: t.p(11) }),
        "TEXT" => EntityKind::Text(text_from(&t)),
        "MTEXT" => EntityKind::MText(mtext_from(tags, &c)),
        "ATTDEF" => {
            // A multiline definition has its text as embedded MTEXT after its own groups.
            let (own, embedded) = split_embedded(tags);
            let multiline = multiline_attrib(embedded, &c);
            let own = T(own);
            let mut text = text_from(&own);
            if let Some(m) = &multiline {
                text.value.clone_from(&m.read_as);
            }
            EntityKind::AttDef(Attrib {
                tag: own.s(2).unwrap_or_default(),
                text,
                invisible: own.i(70).unwrap_or(0) & 1 != 0,
                constant: own.i(70).unwrap_or(0) & 2 != 0,
                prompt: own.s(3).unwrap_or_default(),
                props: Default::default(),
                multiline,
                modes: attrib_modes(own.0),
            })
        }
        "INSERT" => {
            // A mirrored block reference comes with extrusion (0,0,-1): its insertion point, scale,
            // rotation and column spacing are in that OCS, where x runs the other way. In the WCS that
            // is the mirrored point, a negative x scale, the opposite rotation and a negative column
            // spacing. Noise of up to 1e-3 in the extrusion's x/y moves the projection by ~1e-7 of the
            // size, so it still counts as mirrored. A truly tilted extrusion (block plane not parallel
            // to XY) needs the full 3D block transform, which Insert cannot hold; it is left as read.
            let n = c.extrusion;
            let mirrored = n.z < 0.0 && n.x.hypot(n.y) <= 1e-3 * n.z.abs();
            let mut p = t.p(10);
            let mut scale = Vec3::new(t.fd(41, 1.0), t.fd(42, 1.0), t.fd(43, 1.0));
            let mut rotation = t.fd(50, 0.0).to_radians();
            let mut col_spacing = t.fd(44, 0.0);
            if mirrored {
                p = ocs(&c, p);
                scale.x = -scale.x;
                rotation = -rotation;
                col_spacing = -col_spacing;
            }
            EntityKind::Insert(Insert {
                block: t.s(2).unwrap_or_default(),
                insert: p,
                scale,
                rotation,
                attribs: Vec::new(),
                cols: t.i(70).unwrap_or(1).clamp(1, 10_000) as u32,
                rows: t.i(71).unwrap_or(1).clamp(1, 10_000) as u32,
                col_spacing,
                row_spacing: t.fd(45, 0.0),
            })
        }
        "DIMENSION" => {
            let ty = t.i(70).unwrap_or(0) & 0x0f;
            let kind = match ty {
                0 => DimKind::Linear { rotation: t.fd(50, 0.0).to_radians() },
                1 => DimKind::Aligned,
                2 => DimKind::Angular,
                3 => DimKind::Diameter,
                4 => DimKind::Radius,
                5 => DimKind::Angular3P,
                6 => DimKind::Ordinate { x_type: t.i(70).unwrap_or(0) & 64 != 0 },
                8 => DimKind::ArcLength,
                _ => DimKind::Aligned,
            };
            EntityKind::Dimension(Dimension {
                kind,
                defpt: t.p(10),
                // The text midpoint is the one OCS point of a DIMENSION.
                text_mid: if c.extrusion.z < 0.0 { ocs(&c, t.p(11)) } else { t.p(11) },
                p13: t.p(13),
                p14: t.p(14),
                p15: t.p(15),
                p16: t.p(16),
                text: t.s(1).unwrap_or_default(),
                style: t.s(3).unwrap_or_else(|| "Standard".into()),
                measurement: t.fd(42, 0.0),
                text_rotation: t.fd(53, 0.0).to_radians(),
                user_text_pos: t.i(70).unwrap_or(0) & 128 != 0,
                block: t.s(2).filter(|s| !s.is_empty()),
                overrides: Default::default(),
                assoc: Vec::new(),
                marks: Default::default(),
            })
        }
        "LEADER" => EntityKind::Leader(Leader {
            vertices: t.pts(10).into_iter().map(|p| p.to3(0.0)).collect(),
            arrow: t.i(71).unwrap_or(1) != 0,
            spline: t.i(72).unwrap_or(0) != 0,
            style: t.s(3).unwrap_or_else(|| "Standard".into()),
        }),
        // Names behind handles follow in `entity_extras` and `Objects::apply`.
        "MULTILEADER" | "MLEADER" => EntityKind::MLeader(crate::dxf_mleader::read(tags)),
        "SOLID" | "TRACE" => {
            let corners = [t.p(10), t.p(11), t.p(12), if t.f(13).is_some() { t.p(13) } else { t.p(12) }];
            let corners = corners.map(|p| ocs(&c, p));
            if kind == "SOLID" { EntityKind::Solid(Solid { corners }) } else { EntityKind::Trace(Solid { corners }) }
        }
        "MLINE" => EntityKind::MLine(crate::dxf_mline::read_entity(tags)),
        "3DFACE" => EntityKind::Face3d(Face3d { corners: [t.p(10), t.p(11), t.p(12), t.p(13)], hidden_edges: t.i(70).unwrap_or(0) as u8 }),
        "HATCH" => {
            let mut k = hatch(tags, &t)?;
            if c.extrusion.z < 0.0
                && let EntityKind::Hatch(h) = &mut k
            {
                // Boundary in the mirrored OCS, like LWPOLYLINE; the pattern direction mirrors too.
                for v in h.loops.iter_mut().flat_map(|l| l.vertices.iter_mut()) {
                    v.p.x = -v.p.x;
                    v.bulge = -v.bulge;
                }
                h.angle = cadcraft_geom::norm_angle(std::f64::consts::PI - h.angle);
            }
            k
        }
        "ACAD_TABLE" => EntityKind::Table(acad_table(tags, table_cells_left)),
        "VIEWPORT" => EntityKind::Viewport(Viewport {
            center: t.p(10),
            width: t.fd(40, 1.0),
            height: t.fd(41, 1.0),
            view_center: Vec2::new(t.fd(12, 0.0), t.fd(22, 0.0)),
            view_height: t.fd(45, 1.0),
            id: t.i(69).unwrap_or(0) as u32,
            locked: t.i(90).unwrap_or(0) & 16384 != 0,
            frozen_layers: crate::dxf_ext::read_frozen(tags),
            layer_colors: crate::dxf_ext::read_layer_colors(tags),
            // Status flags: 65536 = non-rectangular clipping (by the 340 entity), 131072 = off.
            clip: t.s(340).and_then(|h| Handle::parse_hex(&h)).filter(|h| h.0 != 0 && t.i(90).unwrap_or(0) & 65536 != 0),
            on: t.i(90).unwrap_or(0) & 131072 == 0,
        }),
        "WIPEOUT" => {
            let o = t.p(10);
            let u = t.p(11);
            let v = t.p(12);
            // Clip vertices are in pixel space: origin at the image's top-left corner, y pointing down.
            let pts: Vec<Vec2> = t.pts(14).into_iter().map(|q| o.xy() + u.xy() * (q.x + 0.5) + v.xy() * (0.5 - q.y)).collect();
            EntityKind::Wipeout(Wipeout { boundary: pts })
        }
        // The file path comes from the IMAGEDEF object (`dxf_image::resolve`).
        "IMAGE" => EntityKind::Image(crate::dxf_image::read_entity(tags)),
        "TOLERANCE" => {
            let dir = t.p(11);
            let dir = if dir.xy().is_finite() && dir.xy().len() > 1e-12 { dir } else { Vec3::new(1.0, 0.0, 0.0) };
            EntityKind::Tolerance(Tolerance {
                insert: t.p(10),
                dir,
                style: t.s(3).unwrap_or_else(|| "Standard".into()),
                text: t.s(1).unwrap_or_default().chars().take(MAX_FRAME_CHARS).collect(),
            })
        }
        other => EntityKind::Unknown(Unknown {
            dxf_type: other.to_string(),
            tags: tags.iter().map(|tg| RawTag { code: tg.code, value: tg.str() }).collect(),
        }),
    };
    Some((c, k))
}

/// An ACAD_TABLE entity: sizes, cell texts and merges (DXF Reference), plus CADCraft's
/// title/header flags, text height and style name from xdata when present.
fn acad_table(tags: &[Tag], table_cells_left: &mut usize) -> Table {
    const MAX_CELLS: usize = 1_000_000;
    let start = tags.iter().position(|x| x.code == 100 && x.str() == "AcDbTable").unwrap_or(0);
    let tail = tags.get(start..).unwrap_or(&[]);
    let first_cell = tail.iter().position(|x| x.code == 171).unwrap_or(tail.len());
    let head = T(tail.get(..first_cell).unwrap_or(&[]));
    let mut rows = usize::try_from(head.i(91).unwrap_or(0)).unwrap_or(0).min(10_000);
    let mut cols = usize::try_from(head.i(92).unwrap_or(0)).unwrap_or(0).min(10_000);
    // Large declared sizes must be backed by cell records (no allocation from a bare count).
    let n_cells = tail.iter().filter(|x| x.code == 171).count();
    let declared = rows.saturating_mul(cols);
    if declared > MAX_CELLS || (declared > 10_000 && n_cells < declared) {
        rows = 0;
        cols = 0;
    }
    // Charge before any cell vector is materialized, including discarded tables.
    let count = rows.saturating_mul(cols);
    match table_cells_left.checked_sub(count) {
        Some(left) => *table_cells_left = left,
        None => return Table { insert: T(tags).p(10), ..Table::default() },
    }
    let size = |v: f64| if v.is_finite() && v > 0.0 { v } else { 0.27 };
    let mut row_heights: Vec<f64> = head.all_f(141).into_iter().take(rows).map(size).collect();
    row_heights.resize(rows, 0.27);
    let mut col_widths: Vec<f64> = head.all_f(142).into_iter().take(cols).map(size).collect();
    col_widths.resize(cols, 2.5);
    // Cells, row-major, each starting at a 171 group; xdata ends the last one.
    let body = tail.get(first_cell..).unwrap_or(&[]);
    let body = body.get(..body.iter().position(|x| x.code >= 1000).unwrap_or(body.len())).unwrap_or(&[]);
    let mut flat: Vec<TableCell> = Vec::new();
    let mut i = 0;
    while let Some(t0) = body.get(i) {
        if flat.len() >= rows * cols {
            break;
        }
        i += 1;
        if t0.code != 171 {
            continue;
        }
        let end = body.get(i..).and_then(|r| r.iter().position(|x| x.code == 171)).map(|p| i + p).unwrap_or(body.len());
        let cell = body.get(i..end).unwrap_or(&[]);
        i = end;
        let c = T(cell);
        let mut text: String = cell.iter().filter(|x| x.code == 2 || x.code == 3).map(Tag::str).collect();
        text.push_str(&c.s(1).unwrap_or_default());
        let span = |code: i32| c.i(code).and_then(|v| u32::try_from(v).ok());
        // A merge starts at the cell whose span (rows 176, columns 175) is more than one cell.
        // Its own merged flag (173) varies by writer: CADCraft sets it, while AutoCAD-style
        // writers (acadrust 0.6.4) set it only on the cells the merge covers, whose span is 0.
        let merged = match (span(176), span(175)) {
            (Some(rs), Some(cs)) if rs >= 1 && cs >= 1 && (rs, cs) != (1, 1) => Some((rs, cs)),
            _ => None,
        };
        // The cell's own text height: 140 outside the value block (301 … 304), where it is a number.
        let mut in_value = false;
        let mut text_height = None;
        for x in cell {
            match x.code {
                301 => in_value = true,
                304 => in_value = false,
                140 if !in_value => text_height = Some(x.f64()).filter(|h| h.is_finite() && *h > 0.0 && *h < 1e9),
                _ => {}
            }
        }
        flat.push(TableCell { text, merged, text_height, ..TableCell::default() });
    }
    flat.resize(rows * cols, TableCell::default());
    let cells: Vec<Vec<TableCell>> = if cols == 0 { vec![Vec::new(); rows] } else { flat.chunks(cols).map(<[TableCell]>::to_vec).collect() };
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let ours = x.iter().position(|t| t.code == 1000 && t.str() == "TABLE").map(|p| x.get(p + 1..).unwrap_or(&[]));
    let (title, header, text_height, style) = match ours {
        Some(o) => {
            let flags: Vec<i64> = o.iter().filter(|t| t.code == 1070).map(Tag::i64).collect();
            let h = o.iter().find(|t| t.code == 1040).map(Tag::f64).filter(|h| h.is_finite() && *h > 0.0).unwrap_or(0.18);
            let style = o.iter().find(|t| t.code == 1000).map(Tag::str).unwrap_or_else(|| "Standard".into());
            (flags.first().is_some_and(|v| *v != 0), flags.get(1).is_some_and(|v| *v != 0), h, style)
        }
        // Other writers: a first row merged across every column reads as a title.
        // Their text height is resolved to the table style's once the styles are read (`apply`).
        None => (cols > 1 && cells.first().and_then(|r| r.first()).and_then(|c| c.merged) == Some((1, cols as u32)), false, 0.0, "Standard".into()),
    };
    let mut t = Table { insert: T(tags).p(10), col_widths, row_heights, cells, style, text_height, title, header, ..Table::default() };
    crate::dxf_table::read_table_xdata(tags, &mut t);
    t
}

fn hatch(tags: &[Tag], t: &T) -> Option<EntityKind> {
    let (origin, background) = hatch_xdata(tags);
    let pattern = t.s(2).unwrap_or_else(|| "SOLID".into());
    let solid = t.i(70).unwrap_or(0) == 1;
    let n_loops = t.i(91).unwrap_or(0).clamp(0, 100_000) as usize;
    let mut loops = Vec::new();
    // Walk the boundary data after the 91 tag.
    let start = tags.iter().position(|x| x.code == 91).map(|i| i + 1)?;
    let mut i = start;
    let get = |i: usize| tags.get(i);
    for _ in 0..n_loops {
        // 92: loop type flags.
        while get(i).is_some_and(|x| x.code != 92) {
            i += 1;
        }
        let flags = get(i)?.i64();
        i += 1;
        let outer = flags & 1 != 0;
        let mut verts: Vec<PolyVertex> = Vec::new();
        if flags & 2 != 0 {
            // Polyline boundary: 72 has-bulge, 73 closed, 93 count, then 10/20/42.
            let has_bulge = get(i).filter(|x| x.code == 72).map(Tag::i64).unwrap_or(0) != 0;
            while get(i).is_some_and(|x| x.code != 93) {
                i += 1;
            }
            let n = get(i)?.i64().clamp(0, 1_000_000) as usize;
            i += 1;
            for _ in 0..n {
                let x = get(i).filter(|t| t.code == 10)?.f64();
                let y = get(i + 1).filter(|t| t.code == 20)?.f64();
                i += 2;
                let mut b = 0.0;
                if has_bulge && get(i).is_some_and(|t| t.code == 42) {
                    b = get(i)?.f64();
                    i += 1;
                }
                verts.push(PolyVertex::with_bulge(Vec2::new(x, y), b));
            }
        } else {
            // Edge boundary: 93 edge count, each 72 edge type.
            while get(i).is_some_and(|x| x.code != 93) {
                i += 1;
            }
            let n = get(i)?.i64().clamp(0, 1_000_000) as usize;
            i += 1;
            for _ in 0..n {
                while get(i).is_some_and(|x| x.code != 72) {
                    i += 1;
                }
                let ty = get(i)?.i64();
                i += 1;
                let f = |i: usize| get(i).map(Tag::f64).unwrap_or(0.0);
                match ty {
                    1 => {
                        let a = Vec2::new(f(i), f(i + 1));
                        let b = Vec2::new(f(i + 2), f(i + 3));
                        i += 4;
                        if verts.last().is_none_or(|l| !l.p.near(a, 1e-9)) {
                            verts.push(PolyVertex::new(a));
                        }
                        verts.push(PolyVertex::new(b));
                    }
                    2 => {
                        let c = Vec2::new(f(i), f(i + 1));
                        let r = f(i + 2);
                        let sa = f(i + 3).to_radians();
                        let ea = f(i + 4).to_radians();
                        let ccw = get(i + 5).map(Tag::i64).unwrap_or(1) != 0;
                        i += 6;
                        let (s, e) = if ccw { (sa, ea) } else { (-sa, -ea) };
                        let p0 = Vec2::polar(c, r, s);
                        let p1 = Vec2::polar(c, r, e);
                        let sweep = if ccw { cadcraft_geom::ccw_sweep(s, e) } else { -cadcraft_geom::ccw_sweep(e, s) };
                        if verts.last().is_none_or(|l| !l.p.near(p0, 1e-9)) {
                            verts.push(PolyVertex::new(p0));
                        }
                        // A full circle (0 to 360) starts and ends on the same point, which one
                        // bulge can't span: it goes in as two half circles.
                        let full = (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9;
                        let bulge = cadcraft_geom::arc_to_bulge(if full { sweep / 2.0 } else { sweep });
                        if let Some(l) = verts.last_mut() {
                            l.bulge = bulge;
                        }
                        if full {
                            verts.push(PolyVertex::with_bulge(Vec2::polar(c, r, s + sweep / 2.0), bulge));
                        }
                        verts.push(PolyVertex::new(p1));
                    }
                    3 => {
                        // Elliptic arc: approximate with points.
                        let c = Vec2::new(f(i), f(i + 1));
                        let m = Vec2::new(f(i + 2), f(i + 3));
                        let ratio = f(i + 4);
                        let sa = f(i + 5).to_radians();
                        let ea = f(i + 6).to_radians();
                        let ccw = get(i + 7).filter(|t| t.code == 73).map(Tag::i64).unwrap_or(1) != 0;
                        i += 8;
                        // A clockwise edge stores its parameters mirrored, as an arc edge does: it
                        // runs from -start clockwise to -end, the ccw sweep from -end to -start
                        // walked backwards.
                        let (s, e) = if ccw { (sa, ea) } else { (-ea, -sa) };
                        let e = cadcraft_geom::Ellipse { center: c, major: m, ratio, start: s, end: e };
                        let mut pts = Vec::new();
                        e.tessellate(m.len() * 1e-3, &mut pts);
                        if !ccw {
                            pts.reverse();
                        }
                        verts.extend(pts.into_iter().map(PolyVertex::new));
                    }
                    4 => {
                        // Spline edge: 94 degree, 73 rational, 74 periodic, 95 knot count, 96
                        // control point count, then the 40 knots, the 10/20 control points (each
                        // with a 42 weight when rational), and 97 fit point count with 11/21 fit
                        // points and 12/22, 13/23 end tangents (R2010+).
                        let (mut degree, mut nk, mut nc) = (3, 0, 0);
                        while let Some(x) = get(i).filter(|x| matches!(x.code, 94 | 73 | 74 | 95 | 96)) {
                            let n = x.i64().clamp(0, 100_000) as usize;
                            match x.code {
                                94 => degree = n,
                                95 => nk = n,
                                96 => nc = n,
                                _ => {}
                            }
                            i += 1;
                        }
                        let mut knots = Vec::new();
                        while knots.len() < nk && get(i).is_some_and(|x| x.code == 40) {
                            knots.push(f(i));
                            i += 1;
                        }
                        let (mut control, mut weights) = (Vec::new(), Vec::new());
                        while control.len() < nc && get(i).is_some_and(|x| x.code == 10) {
                            control.push(Vec2::new(f(i), f(i + 1)));
                            i += 2;
                            if get(i).is_some_and(|t| t.code == 42) {
                                weights.push(f(i));
                                i += 1;
                            }
                        }
                        let mut fit = Vec::new();
                        if get(i).is_some_and(|x| x.code == 97) {
                            let n = get(i).map(Tag::i64).unwrap_or(0).clamp(0, 100_000) as usize;
                            i += 1;
                            while fit.len() < n && get(i).is_some_and(|x| x.code == 11) {
                                fit.push(Vec2::new(f(i), f(i + 1)));
                                i += 2;
                            }
                        }
                        if weights.len() != control.len() {
                            weights.clear();
                        }
                        let sp = Spline { degree, knots, control, weights, ..Spline::default() };
                        let sp = if sp.is_valid() {
                            sp
                        } else if sp.control.is_empty() && fit.len() > 1 {
                            Spline::from_fit_points(&fit)
                        } else {
                            Spline::from_control(sp.control, degree)
                        };
                        let pts = sp.tessellate(1e-3);
                        let skip = usize::from(verts.last().zip(pts.first()).is_some_and(|(l, p)| l.p.near(*p, 1e-9)));
                        verts.extend(pts.into_iter().skip(skip).map(PolyVertex::new));
                    }
                    _ => {}
                }
            }
        }
        if verts.len() > 1 && verts.first().map(|f| f.p).zip(verts.last().map(|l| l.p)).is_some_and(|(a, b)| a.near(b, 1e-9)) {
            verts.pop();
        }
        // 97: number of source boundary objects, then their handles (330). A spline edge's fit
        // data count is a 97 too, followed by its fit points instead.
        let mut sources = Vec::new();
        for _ in 0..2 {
            while get(i).is_some_and(|x| !matches!(x.code, 97 | 92 | 75)) {
                i += 1;
            }
            if get(i).is_none_or(|x| x.code != 97) {
                break;
            }
            let n = get(i).map(Tag::i64).unwrap_or(0).clamp(0, MAX_HATCH_SOURCES as i64) as usize;
            i += 1;
            if get(i).is_some_and(|x| !matches!(x.code, 330 | 92 | 75)) {
                continue;
            }
            while sources.len() < n
                && let Some(x) = get(i).filter(|x| x.code == 330)
            {
                sources.extend(Handle::parse_hex(&x.str()).filter(|h| h.0 != 0));
                i += 1;
            }
            break;
        }
        loops.push(HatchLoop { vertices: verts, outer, sources });
    }
    let solid = solid || pattern.eq_ignore_ascii_case("SOLID");
    let (scale, angle) = (t.fd(41, 1.0), t.fd(52, 0.0).to_radians());
    // Keep the file's definition lines even for a name the library knows: the library's versions
    // of some patterns (AR-CONC) only approximate AutoCAD's, and drawing them at the file's scale
    // could be hundreds of times denser than the hatch the author saw.
    let pattern_lines = if solid { Vec::new() } else { pattern_definition(tags, scale, angle, origin) };
    Some(EntityKind::Hatch(Hatch {
        pattern: pattern.to_ascii_uppercase(),
        solid,
        loops,
        scale,
        angle,
        associative: t.i(71).unwrap_or(0) != 0,
        style: t.i(75).unwrap_or(0) as u8,
        // The first 30 is the elevation point's z (boundary points are 2D).
        elevation: t.f(30).filter(|z| z.is_finite()).unwrap_or(0.0),
        gradient: gradient(tags),
        origin,
        background,
        pattern_lines,
    }))
}

/// Upper bound on the source boundary objects of one HATCH loop.
const MAX_HATCH_SOURCES: usize = 4096;

/// Upper bounds on a HATCH's pattern definition lines and on the dashes of one line.
const MAX_PATTERN_LINES: usize = 1024;
const MAX_PATTERN_DASHES: usize = 256;

/// The pattern definition lines of a HATCH (groups 78, then per line 53 angle, 43/44 base
/// point, 45/46 offset, 79 dash count, 49 dash lengths). A file holds them as drawn: scaled,
/// rotated with the hatch and the offset as a world vector. They are kept the library's way
/// (unit scale, no hatch rotation, base point relative to the hatch origin, offset as shift
/// along the line and spacing), which the writer and the renderer turn back. Lines with
/// non-finite values are skipped.
fn pattern_definition(tags: &[Tag], scale: f64, angle: f64, origin: Vec2) -> Vec<cadcraft_doc::library::PatternLine> {
    let Some(at) = tags.iter().position(|x| x.code == 78) else { return Vec::new() };
    let scale = if scale.is_finite() && scale > 1e-9 { scale } else { 1.0 };
    let angle = if angle.is_finite() { angle } else { 0.0 };
    // Raw lines: angle (degrees), base, offset, dashes.
    let mut raw: Vec<(f64, Vec2, Vec2, Vec<f64>)> = Vec::new();
    for x in tags.iter().skip(at + 1) {
        let cur = raw.last_mut();
        match (x.code, cur) {
            (53, _) => {
                if raw.len() >= MAX_PATTERN_LINES {
                    break;
                }
                raw.push((x.f64(), Vec2::ZERO, Vec2::ZERO, Vec::new()));
            }
            (43, Some(l)) => l.1.x = x.f64(),
            (44, Some(l)) => l.1.y = x.f64(),
            (45, Some(l)) => l.2.x = x.f64(),
            (46, Some(l)) => l.2.y = x.f64(),
            (79, Some(_)) => {}
            (49, Some(l)) => {
                if l.3.len() < MAX_PATTERN_DASHES {
                    l.3.push(x.f64());
                }
            }
            _ => break,
        }
    }
    raw.into_iter()
        .filter(|(a, b, o, d)| a.is_finite() && b.is_finite() && o.is_finite() && d.iter().all(|v| v.is_finite()))
        .map(|(a, base, offset, dashes)| {
            let local = (base - origin).rotate(-angle) * (1.0 / scale);
            let delta = offset.rotate(-a.to_radians()) * (1.0 / scale);
            cadcraft_doc::library::PatternLine {
                angle: a - angle.to_degrees(),
                origin: (local.x, local.y),
                delta: (delta.x, delta.y),
                dashes: dashes.into_iter().map(|d| d / scale).collect(),
            }
        })
        .collect()
}

/// Hatch origin and background colour from `CADCRAFT` xdata: `1000 HATCH`, `1011` origin,
/// `1000` background colour name (empty for none). Defaults when absent.
fn hatch_xdata(tags: &[Tag]) -> (Vec2, Option<Color>) {
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let Some(at) = x.windows(2).position(|w| matches!(w, [m, o] if m.code == 1000 && m.str() == "HATCH" && o.code == 1011)) else {
        return (Vec2::ZERO, None);
    };
    let val = |k: usize, code: i32| x.get(at + k).filter(|t| t.code == code);
    let coord = |k: usize, code: i32| val(k, code).map(Tag::f64).filter(|v| v.is_finite()).unwrap_or(0.0);
    let origin = Vec2::new(coord(1, 1011), coord(2, 1021));
    (origin, val(4, 1000).and_then(|t| Color::parse(&t.str())))
}

/// Gradient values missing from a file fall back to the GRADIENT command's defaults.
fn default_gradient() -> Gradient {
    Gradient { name: "LINEAR".into(), color1: Color::Index(5), color2: Color::Index(7), angle: 0.0, centered: true }
}

/// A hatch's gradient fill: the native groups when present (another program may have edited
/// them), otherwise CADCraft's `GRADIENT` xdata (kept by R2000 files and DWG conversions).
fn gradient(tags: &[Tag]) -> Option<Gradient> {
    if tags.iter().any(|x| x.code == 450) { native_gradient(tags) } else { xdata_gradient(tags) }
}

/// `CADCRAFT` xdata: `1000 GRADIENT`, then name, 1040 angle, 1070 centered, colour 1 and 2 names.
fn xdata_gradient(tags: &[Tag]) -> Option<Gradient> {
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let at = x.iter().position(|t| t.code == 1000 && t.str() == "GRADIENT")?;
    let o = x.get(at + 1..).unwrap_or(&[]);
    let d = default_gradient();
    let strs: Vec<String> = o.iter().filter(|t| t.code == 1000).take(3).map(Tag::str).collect();
    let color = |i: usize, d: Color| strs.get(i).and_then(|s| Color::parse(s)).unwrap_or(d);
    Some(Gradient {
        name: strs.first().cloned().unwrap_or(d.name),
        color1: color(1, d.color1),
        color2: color(2, d.color2),
        angle: o.iter().find(|t| t.code == 1040).map(Tag::f64).filter(|a| a.is_finite()).unwrap_or(d.angle),
        centered: o.iter().find(|t| t.code == 1070).map_or(d.centered, |t| t.i64() != 0),
    })
}

/// DXF Reference HATCH groups 450–470, when 450 says gradient. Each 463 starts a colour record
/// whose 63 (ACI) and 421 (true colour) groups follow.
fn native_gradient(tags: &[Tag]) -> Option<Gradient> {
    let start = tags.iter().position(|x| x.code == 450)?;
    if tags.get(start)?.i64() != 1 {
        return None;
    }
    let mut g = default_gradient();
    // Per colour record: (ACI, true colour); a true colour wins over its ACI fallback.
    let mut colors: [(Option<Color>, Option<Rgb>); 2] = [(None, None); 2];
    let mut slot: Option<usize> = None;
    for x in tags.iter().skip(start + 1).take_while(|x| x.code != 1001) {
        let rec = slot.and_then(|s| colors.get_mut(s));
        match x.code {
            460 => g.angle = Some(x.f64()).filter(|a| a.is_finite()).unwrap_or(0.0),
            461 => g.centered = x.f64().abs() < 1e-9,
            463 => slot = Some(slot.map_or(0, |s| s.saturating_add(1))),
            63 => {
                if let Some(r) = rec {
                    r.0 = Some(Color::from_aci(i16::try_from(x.i64()).unwrap_or(256)));
                }
            }
            421 => {
                if let Some(r) = rec {
                    r.1 = Some(Rgb::from_u32(u32::try_from(x.i64() & 0xff_ffff).unwrap_or(0)));
                }
            }
            470 => g.name = x.str(),
            _ => {}
        }
    }
    let [c1, c2] = colors;
    let pick = |(aci, rgb): (Option<Color>, Option<Rgb>), d: Color| rgb.map(Color::True).or(aci).unwrap_or(d);
    g.color1 = pick(c1, g.color1);
    g.color2 = pick(c2, g.color2);
    Some(g)
}

/// Handle maps and references gathered while reading.
#[derive(Default)]
struct Rx {
    /// STYLE record handle (upper case) → text style name.
    styles: HashMap<String, String>,
    /// BLOCK_RECORD handle (upper case) → block name.
    brs: HashMap<String, String>,
    /// LTYPE record handle (upper case) → linetype name (DIMLTYPE, DIMLTEX1, DIMLTEX2).
    ltypes: HashMap<String, String>,
    /// Upper-case block name → (insertion units, explodable) from its BLOCK_RECORD.
    block_props: HashMap<String, (u8, bool)>,
    /// Upper-case names of anonymous `*T` blocks drawn by ACAD_TABLE entities.
    table_blocks: std::collections::HashSet<String>,
    /// Upper-case block names used by INSERTs.
    inserted: std::collections::HashSet<String>,
    /// Tables whose style is known only by TABLESTYLE handle: (table, style handle).
    table_style_fix: Vec<(Handle, String)>,
    /// Multileaders' style handles and block attribute values, resolved with the objects.
    mleader_fix: Vec<(Handle, crate::dxf_mleader::Pending)>,
    /// Upper-case xref block name → the external drawing's path (BLOCK group 1).
    xref_paths: HashMap<String, String>,
    /// Plot style name placeholder handle (upper case) → plot style name (`ACAD_PLOTSTYLENAME`).
    pstyles: HashMap<String, String>,
    /// Upper-case block name → associative array parameters from its BLOCK_RECORD.
    block_arrays: HashMap<String, cadcraft_doc::ArrayParams>,
    /// LAYER record handle (upper case) → layer name (VIEWPORT group 341 frozen layers).
    layers: HashMap<String, String>,
}

/// The plot style names of a file: the entries of the `ACAD_PLOTSTYLENAME` dictionary of the
/// named object dictionary (the first object), by placeholder handle.
fn plot_style_names(secs: &[cadcraft_dxf::Section]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(objects) = secs.iter().find(|s| s.name == "OBJECTS") else { return out };
    let recs = records(&objects.tags);
    let entries = |tags: &[Tag]| {
        let mut v: Vec<(String, String)> = Vec::new();
        let mut name: Option<String> = None;
        for t in tags {
            match t.code {
                3 => name = Some(t.str()),
                350 | 360 => {
                    if let Some(n) = name.take() {
                        v.push((n, t.str().trim().to_ascii_uppercase()));
                    }
                }
                _ => {}
            }
        }
        v
    };
    let Some((_, root)) = recs.iter().find(|(k, _)| k == "DICTIONARY") else { return out };
    let Some((_, dict)) = entries(root).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(crate::dxf_ext::PLOT_STYLE_NAMES_KEY)) else {
        return out;
    };
    let is_dict = |k: &str| k == "DICTIONARY" || k == "ACDBDICTIONARYWDFLT";
    let Some((_, tags)) = recs.iter().find(|(k, tg)| is_dict(k) && T(tg).s(5).is_some_and(|h| h.trim().eq_ignore_ascii_case(&dict))) else {
        return out;
    };
    for (n, h) in entries(tags).into_iter().take(crate::dxf_ext::MAX_PLOT_STYLE_NAMES) {
        if !n.trim().is_empty() {
            out.insert(h, n.chars().take(255).collect());
        }
    }
    out
}

/// The named plot style an entity record points at (group 390 among its AcDbEntity groups).
fn entity_plot_style(tags: &[Tag], rx: &Rx) -> Option<String> {
    let mut in_entity = false;
    for t in tags {
        if t.code == 100 {
            if in_entity {
                return None;
            }
            in_entity = t.str() == "AcDbEntity";
        } else if t.code == 390 && in_entity {
            return rx.pstyles.get(&t.str().trim().to_ascii_uppercase()).cloned();
        }
    }
    None
}

/// Dimension overrides and associativity, table references: data of an entity record that
/// needs the file's handle maps.
fn entity_extras(kind: &str, tags: &[Tag], h: Handle, k: &mut EntityKind, rx: &mut Rx) {
    match (kind, k) {
        ("DIMENSION", EntityKind::Dimension(dm)) => {
            dm.overrides = crate::dxf_ext::read_dstyle(tags, &rx.styles, &rx.brs, &rx.ltypes);
            dm.assoc = crate::dxf_ext::read_assoc(tags);
            if let Some(center) = crate::dxf_ext::read_arclen(tags) {
                dm.kind = DimKind::ArcLength;
                dm.p15 = center;
            }
            dm.marks = crate::dxf_ext::read_marks(tags);
            // Group 52: the extension lines' angle relative to the dimension line (DIMEDIT Oblique).
            let base = match dm.kind {
                DimKind::Linear { rotation } => Some(rotation),
                DimKind::Aligned => Some((dm.p14.xy() - dm.p13.xy()).angle()),
                _ => None,
            };
            if let (Some(base), Some(o)) = (base, T(tags).f(52).filter(|o| o.is_finite() && o.rem_euclid(180.0) != 0.0)) {
                dm.marks.oblique = Some(cadcraft_geom::norm_angle(base + o.to_radians()));
            }
        }
        ("ACAD_TABLE", EntityKind::Table(_)) => {
            let t = T(tags);
            if let Some(b) = t.s(2) {
                rx.table_blocks.insert(b.to_ascii_uppercase());
            }
            let ours = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP).iter().any(|t| t.code == 1000 && t.str() == "TABLE");
            if !ours {
                rx.table_style_fix.push((h, t.s(342).map(|sh| sh.trim().to_ascii_uppercase()).unwrap_or_default()));
            }
        }
        ("INSERT", EntityKind::Insert(ins)) => {
            rx.inserted.insert(ins.block.to_ascii_uppercase());
        }
        ("MULTILEADER" | "MLEADER", EntityKind::MLeader(m)) => {
            let pending = crate::dxf_mleader::resolve(tags, m, &rx.styles, &rx.brs);
            if let Some(b) = &m.block {
                rx.inserted.insert(b.block.to_ascii_uppercase());
            }
            rx.mleader_fix.push((h, pending));
        }
        // CADCraft's VPFROZEN xdata wins; otherwise AutoCAD's group 341 layer handles.
        ("VIEWPORT", EntityKind::Viewport(v)) if v.frozen_layers.is_empty() => {
            v.frozen_layers = tags
                .iter()
                .take_while(|t| t.code < 1000)
                .filter(|t| t.code == 341)
                .filter_map(|t| rx.layers.get(&t.str().trim().to_ascii_uppercase()).cloned())
                .collect();
        }
        _ => {}
    }
}

/// Parse a run of entity records (ENTITIES section or a block's body) into entities. Records that
/// can't be placed are counted in `skipped`.
fn parse_entities(
    recs: &[(String, Vec<Tag>)],
    d: &mut Drawing,
    rx: &mut Rx,
    skipped: &mut usize,
    table_cells_left: &mut usize,
) -> Vec<(Option<String>, bool, Entity)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some((kind, tags)) = recs.get(i) {
        let t = T(tags);
        let owner = crate::dxf_ext::owner(tags);
        let paper = t.i(67) == Some(1);
        let handle = t.s(5).and_then(|h| Handle::parse_hex(&h));
        let mut kindent = entity_with_table_budget(kind, tags, table_cells_left);
        if let Some((c, _)) = kindent.as_mut()
            && let Some(n) = entity_plot_style(tags, rx)
        {
            c.plot_style = n;
        }
        // More entities from this record (the faces of a polyface or polygon mesh).
        let mut extra: Vec<EntityKind> = Vec::new();
        i += 1;
        match kind.as_str() {
            "POLYLINE" => {
                let flags = t.i(70).unwrap_or(0);
                let mut pts: Vec<(Vec3, f64, f64, f64)> = Vec::new();
                // Polyface face records (VERTEX 70 bit 128 without 64): vertex indices 71–74.
                let mut faces: Vec<[i64; 4]> = Vec::new();
                while let Some((k2, t2)) = recs.get(i) {
                    if k2 == "VERTEX" {
                        let tt = T(t2);
                        let vflags = tt.i(70).unwrap_or(0);
                        if vflags & 128 != 0 && vflags & 64 == 0 {
                            faces.push([71, 72, 73, 74].map(|c| tt.i(c).unwrap_or(0)));
                        } else if vflags & 16 == 0 {
                            pts.push((tt.p(10), tt.fd(42, 0.0), tt.fd(40, 0.0), tt.fd(41, 0.0)));
                        }
                        i += 1;
                    } else {
                        if k2 == "SEQEND" {
                            i += 1;
                        }
                        break;
                    }
                }
                let c = common(&t);
                let k = if flags & (16 | 64) != 0 {
                    // Polyface (64) and polygon (16) meshes become 3D faces; the first takes the
                    // polyline's place.
                    let verts: Vec<Vec3> = pts.iter().map(|p| p.0).collect();
                    let mut fs = if flags & 64 != 0 { polyface_faces(&verts, &faces) } else { mesh_faces(&verts, &t, flags) }
                        .into_iter()
                        .map(EntityKind::Face3d);
                    let Some(first) = fs.next() else {
                        *skipped = skipped.saturating_add(1);
                        continue;
                    };
                    extra.extend(fs);
                    first
                } else if flags & 8 != 0 {
                    EntityKind::Polyline3d(Polyline3d { points: pts.iter().map(|p| p.0).collect(), closed: flags & 1 != 0 })
                } else {
                    // A 2D polyline's vertices are in its OCS: mirrored like LWPOLYLINE.
                    let m = if c.extrusion.z < 0.0 { -1.0 } else { 1.0 };
                    EntityKind::LwPolyline(LwPolyline {
                        vertices: pts
                            .iter()
                            .map(|p| PolyVertex { p: Vec2::new(m * p.0.x, p.0.y), bulge: m * p.1, start_width: p.2, end_width: p.3 })
                            .collect(),
                        closed: flags & 1 != 0,
                        const_width: t.fd(40, 0.0).min(t.fd(41, 0.0)).max(0.0),
                        elevation: t.p(10).z,
                        plinegen: flags & 128 != 0,
                    })
                };
                kindent = Some((c, k));
            }
            "INSERT" => {
                if t.i(66) == Some(1) {
                    let mut attribs = Vec::new();
                    let ins_layer = kindent.as_ref().map(|(c, _)| c.layer.clone()).unwrap_or_default();
                    while let Some((k2, t2)) = recs.get(i) {
                        if k2 == "ATTRIB" {
                            let tt = T(t2);
                            // A multiline attribute has its text as embedded MTEXT after its own groups.
                            let (own, embedded) = split_embedded(t2);
                            let multiline = multiline_attrib(embedded, &common(&tt));
                            let own = T(own);
                            let mut text = text_from(&own);
                            if let Some(m) = &multiline {
                                text.value.clone_from(&m.read_as);
                            }
                            attribs.push(Attrib {
                                tag: own.s(2).unwrap_or_default(),
                                text,
                                invisible: own.i(70).unwrap_or(0) & 1 != 0,
                                constant: false,
                                prompt: String::new(),
                                props: attrib_props(&tt, &ins_layer),
                                multiline,
                                modes: attrib_modes(own.0),
                            });
                            i += 1;
                        } else {
                            if k2 == "SEQEND" {
                                i += 1;
                            }
                            break;
                        }
                    }
                    if let Some((_, EntityKind::Insert(ins))) = kindent.as_mut() {
                        ins.attribs = attribs;
                    }
                }
            }
            // A VERTEX or ATTRIB with no POLYLINE or INSERT before it; a stray SEQEND holds nothing.
            "VERTEX" | "ATTRIB" => {
                *skipped = skipped.saturating_add(1);
                continue;
            }
            "SEQEND" => continue,
            _ => {}
        }
        if let Some((c, mut k)) = kindent {
            let h = match handle {
                Some(h) => {
                    d.bump_handseed(h);
                    h
                }
                None => d.new_handle(),
            };
            entity_extras(kind, tags, h, &mut k, rx);
            let more: Vec<_> =
                extra.into_iter().map(|f| (owner.clone(), paper, Entity { handle: d.new_handle(), common: c.clone(), kind: f })).collect();
            out.push((owner, paper, Entity { handle: h, common: c, kind: k }));
            out.extend(more);
        } else if kind == "HATCH" {
            // Without boundary data. The other records `entity` returns nothing for are read
            // elsewhere (the writer's paper viewport, the loops of a region).
            *skipped = skipped.saturating_add(1);
        }
    }
    // Assign fresh handles to duplicates (hostile files).
    let mut seen = std::collections::HashSet::new();
    for (_, _, e) in &mut out {
        if !seen.insert(e.handle) {
            e.handle = d.new_handle();
            seen.insert(e.handle);
        }
    }
    out
}

/// Faces of a polyface mesh: each face record names three or four vertices by 1-based index;
/// a negative index hides the edge that starts at that vertex. Faces with fewer vertices or
/// an index outside the vertex list are skipped.
fn polyface_faces(verts: &[Vec3], faces: &[[i64; 4]]) -> Vec<Face3d> {
    let mut out = Vec::new();
    for f in faces {
        let mut corners: Vec<Vec3> = Vec::with_capacity(4);
        let mut hidden = 0u8;
        let mut ok = true;
        for &ix in f.iter().filter(|&&ix| ix != 0) {
            let at = usize::try_from(ix.unsigned_abs()).ok().and_then(|n| n.checked_sub(1));
            let Some(p) = at.and_then(|n| verts.get(n)) else {
                ok = false;
                break;
            };
            if ix < 0 {
                hidden |= 1 << corners.len();
            }
            corners.push(*p);
        }
        match (ok, corners.as_slice()) {
            // A triangle's closing edge is the 3DFACE's fourth (the third is degenerate).
            (true, [a, b, c]) => out.push(Face3d { corners: [*a, *b, *c, *c], hidden_edges: hidden | if hidden & 4 != 0 { 8 } else { 0 } }),
            (true, [a, b, c, d]) => out.push(Face3d { corners: [*a, *b, *c, *d], hidden_edges: hidden }),
            _ => {}
        }
    }
    out
}

/// Quads of an M×N polygon mesh (71/72, or the smoothed 73/74 when the vertex count matches
/// those), closed in M by bit 1 and in N by bit 32. Vertices run row by row.
fn mesh_faces(verts: &[Vec3], t: &T, flags: i64) -> Vec<Face3d> {
    let dims = |a: i32, b: i32| Some((usize::try_from(t.i(a)?).ok()?, usize::try_from(t.i(b)?).ok()?));
    let fits = |(m, n): (usize, usize)| m >= 2 && n >= 2 && m.checked_mul(n).is_some_and(|mn| mn <= verts.len());
    let exact = |(m, n): (usize, usize)| fits((m, n)) && m * n == verts.len();
    let Some((m, n)) = [dims(71, 72), dims(73, 74)].into_iter().flatten().find(|d| exact(*d)).or_else(|| dims(71, 72).filter(|d| fits(*d))) else {
        return Vec::new();
    };
    let at = |i: usize, j: usize| verts.get((i % m) * n + j % n).copied();
    let rows = if flags & 1 != 0 { m } else { m - 1 };
    let cols = if flags & 32 != 0 { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..rows {
        for j in 0..cols {
            if let (Some(a), Some(b), Some(c), Some(d)) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j)) {
                out.push(Face3d { corners: [a, b, c, d], hidden_edges: 0 });
            }
        }
    }
    out
}

fn header(tags: &[Tag], d: &mut Drawing) {
    let mut i = 0;
    while let Some(t) = tags.get(i) {
        if t.code == 9 {
            let name = t.str().trim_start_matches('$').to_ascii_uppercase();
            i += 1;
            let mut vals: Vec<&Tag> = Vec::new();
            while let Some(v) = tags.get(i) {
                if v.code == 9 {
                    break;
                }
                vals.push(v);
                i += 1;
            }
            // Legacy files keep spacing in HEADER; modern *ACTIVE VPORT entries override it.
            if matches!(name.as_str(), "SNAPUNIT" | "GRIDUNIT") {
                let (Some(x), Some(y)) = (vals.iter().find(|t| t.code == 10), vals.iter().find(|t| t.code == 20)) else { continue };
                let p = Vec2::new(x.f64(), y.f64());
                if p.is_finite() && if name == "GRIDUNIT" { p.x >= 0.0 && p.y >= 0.0 } else { p.x > 0.0 && p.y > 0.0 } {
                    d.header.set(&name, HVal::Point(p.to3(0.0)));
                }
                continue;
            }
            let hv = match vals.as_slice() {
                [x, y, z] if x.code == 10 => HVal::Point(Vec3::new(x.f64(), y.f64(), z.f64())),
                [x, y] if x.code == 10 => HVal::Point(Vec3::new(x.f64(), y.f64(), 0.0)),
                [v] => match &v.value {
                    cadcraft_dxf::Value::Real(r) => HVal::Real(*r),
                    cadcraft_dxf::Value::Int(n) => HVal::Int(*n),
                    cadcraft_dxf::Value::Bool(b) => HVal::Int(i64::from(*b)),
                    other => HVal::Str(other.as_str()),
                },
                _ => continue,
            };
            // $DIMDSEP holds a character code; the dimension style holds the character.
            let hv = match hv {
                HVal::Int(n) if name == "DIMDSEP" => match u32::try_from(n).ok().and_then(char::from_u32).filter(|c| !c.is_control()) {
                    Some(c) => HVal::Str(c.to_string()),
                    None => continue,
                },
                // $DIMJOGANG is in radians; the variable (like the style) holds degrees.
                HVal::Real(r) if name == "DIMJOGANG" => HVal::Real((r.to_degrees() * 1e9).round() / 1e9),
                // $ANGBASE is in degrees (a drawing measured from north has 90.0); the variable
                // holds radians.
                HVal::Real(r) if name == "ANGBASE" => HVal::Real(r.to_radians()),
                hv => hv,
            };
            d.header.set(&name, hv);
        } else {
            i += 1;
        }
    }
}

/// True when a style record carries `AcadAnnotative` xdata with the annotative flag set.
fn annotative(tags: &[Tag]) -> bool {
    let list = crate::dxf_ext::xdata_list(crate::dxf_ext::xdata(tags, "AcadAnnotative"), "AnnotativeData");
    list.iter().filter(|t| t.code == 1070).nth(1).is_some_and(|t| t.i64() != 0)
}

/// A text style's font: CADCraft's own font name from `CADCRAFT` xdata (`1000 FONT`, name)
/// while the font file is still the `txt` written for other readers; the TrueType family of the
/// style's `ACAD` xdata (`1000` typeface) when it has no font file (how AutoCAD stores TrueType
/// styles); otherwise the file.
fn style_font(tags: &[Tag], file: String) -> String {
    if file.trim().is_empty()
        && let Some(face) =
            crate::dxf_ext::xdata(tags, "ACAD").iter().find(|t| t.code == 1000).map(|t| t.str().trim().to_string()).filter(|f| !f.is_empty())
    {
        return face;
    }
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let own = x.windows(2).find_map(|w| match w {
        [m, n] if m.code == 1000 && m.str() == "FONT" && n.code == 1000 => Some(n.str()),
        _ => None,
    });
    let stem = file.trim().to_ascii_lowercase();
    match own {
        Some(name) if !name.trim().is_empty() && (stem == "txt" || stem == "txt.shx") => name,
        _ => file,
    }
}

/// Load symbol tables and resolve handle references, indexing layer names for case-insensitive updates.
fn tables(tags: &[Tag], d: &mut Drawing, rx: &mut Rx) {
    let recs = records(tags);
    // STYLE handle → name, or the shape file of an unnamed shape style (linetype elements).
    let ltype_styles: HashMap<String, String> = recs
        .iter()
        .filter(|(k, _)| k == "STYLE")
        .filter_map(|(_, tg)| {
            let t = T(tg);
            let name = t.s(2).filter(|n| !n.trim().is_empty());
            Some((t.s(5)?.trim().to_ascii_uppercase(), name.or_else(|| t.s(3))?))
        })
        .collect();
    // Handle maps first: DIMSTYLE records refer to text styles and arrow blocks by handle.
    for (kind, tg) in &recs {
        let t = T(tg);
        let (Some(h), Some(n)) = (t.s(5), t.s(2)) else { continue };
        match kind.as_str() {
            "STYLE" => {
                rx.styles.insert(h.trim().to_ascii_uppercase(), n);
            }
            "BLOCK_RECORD" => {
                let units = t.i(70).and_then(|u| u8::try_from(u).ok()).unwrap_or(0);
                rx.block_props.insert(n.to_ascii_uppercase(), (units, t.i(280).unwrap_or(1) != 0));
                if let Some(a) = crate::dxf_ext::read_array(tg) {
                    rx.block_arrays.insert(n.to_ascii_uppercase(), a);
                }
                rx.brs.insert(h.trim().to_ascii_uppercase(), n);
            }
            "LTYPE" => {
                rx.ltypes.insert(h.trim().to_ascii_uppercase(), n);
            }
            "LAYER" => {
                rx.layers.insert(h.trim().to_ascii_uppercase(), n);
            }
            _ => {}
        }
    }
    let mut active_vport = false;
    // Index of layers already appended, by lower-cased name, so each LAYER record updates or
    // appends in O(1) instead of scanning the growing layer list — otherwise many distinct layer
    // names make import quadratic (CWE-407).
    let mut layer_index: HashMap<String, usize> = d.layers.iter().enumerate().map(|(i, l)| (l.name.to_ascii_lowercase(), i)).collect();
    for (kind, tg) in recs {
        let t = T(&tg);
        match kind.as_str() {
            "LAYER" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.layers.len() >= MAX_OBJECTS {
                    continue;
                }
                let aci = t.i(62).unwrap_or(7);
                let flags = t.i(70).unwrap_or(0);
                let mut l = Layer {
                    name: name.clone(),
                    color: Color::Index(aci.unsigned_abs().clamp(1, 255) as u8),
                    linetype: t.s(6).unwrap_or_else(|| "Continuous".into()),
                    lineweight: t.i(370).map(|v| Lineweight::from_dxf(v as i16)).unwrap_or(Lineweight::Default),
                    on: aci >= 0,
                    frozen: flags & 1 != 0,
                    locked: flags & 4 != 0,
                    vp_freeze_new: flags & 2 != 0,
                    plot: t.i(290).unwrap_or(1) != 0,
                    ..Layer::default()
                };
                if let Some(tc) = t.i(420) {
                    l.color = Color::True(Rgb::from_u32(tc as u32));
                }
                if let Some(n) = t.s(390).and_then(|h| rx.pstyles.get(&h.trim().to_ascii_uppercase())) {
                    l.plot_style = n.clone();
                }
                let alpha = crate::dxf_ext::xdata(&tg, crate::dxf_ext::LAYER_TRANSPARENCY_APP).iter().find(|x| x.code == 1071);
                if let Some(pct) = alpha.and_then(|x| crate::dxf_ext::transparency_from_dxf(x.i64())) {
                    l.transparency = pct;
                }
                if let Some(desc) = crate::dxf_ext::xdata(&tg, crate::dxf_ext::LAYER_DESCRIPTION_APP).iter().filter(|x| x.code == 1000).nth(1) {
                    l.description = desc.str();
                }
                match layer_index.get(&name.to_ascii_lowercase()) {
                    Some(&i) => {
                        if let Some(x) = d.layers.get_mut(i) {
                            *x = l;
                        }
                    }
                    None => {
                        layer_index.insert(name.to_ascii_lowercase(), d.layers.len());
                        d.layers.push(l);
                    }
                }
            }
            "LTYPE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.linetype(&name).is_some() || d.linetypes.len() >= MAX_OBJECTS {
                    continue;
                }
                let pattern = ltype_pattern(&tg, &ltype_styles);
                d.linetypes.push(Linetype { name, description: t.s(3).unwrap_or_default(), pattern });
            }
            "STYLE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.text_styles.len() >= MAX_OBJECTS {
                    continue;
                }
                // 70 bit 4: vertical; 71 bits 2/4: backwards / upside down.
                let gen_flags = t.i(71).unwrap_or(0);
                let st = TextStyle {
                    name: name.clone(),
                    font: style_font(&tg, t.s(3).unwrap_or_default()),
                    big_font: t.s(4).unwrap_or_default(),
                    height: t.fd(40, 0.0),
                    width_factor: t.fd(41, 1.0),
                    oblique: t.fd(50, 0.0).to_radians(),
                    backwards: gen_flags & 2 != 0,
                    upside_down: gen_flags & 4 != 0,
                    vertical: t.i(70).unwrap_or(0) & 4 != 0,
                    annotative: annotative(&tg),
                };
                match d.text_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = st,
                    None => d.text_styles.push(st),
                }
            }
            // The first *ACTIVE viewport holds the current snap/grid settings.
            "VPORT" if !active_vport && t.s(2).is_some_and(|n| n.trim().eq_ignore_ascii_case("*ACTIVE")) => {
                active_vport = true;
                vport_record(d, &t);
                for (name, code) in [("SNAPUNIT", 14), ("GRIDUNIT", 15)] {
                    if let (Some(x), Some(y)) = (t.f(code), t.f(code + 10)) {
                        let p = Vec2::new(x, y);
                        if p.is_finite() && if name == "GRIDUNIT" { p.x >= 0.0 && p.y >= 0.0 } else { p.x > 0.0 && p.y > 0.0 } {
                            d.header.set(name, HVal::Point(p.to3(0.0)));
                        }
                    }
                }
                if let (Some(x), Some(y)) = (t.f(13), t.f(23))
                    && x.is_finite()
                    && y.is_finite()
                {
                    d.header.set("SNAPBASE", HVal::Point(Vec3::new(x, y, 0.0)));
                }
                if let Some(a) = t.f(50).filter(|a| a.is_finite()) {
                    d.header.set_f64("SNAPANG", a);
                }
            }
            "VIEW" => {
                let name = t.s(2).unwrap_or_default();
                let (h, w) = (t.fd(40, 0.0), t.fd(41, 0.0));
                if name.is_empty() || !(h.is_finite() && w.is_finite() && h > 0.0 && w > 0.0) || d.views.len() >= MAX_OBJECTS {
                    continue;
                }
                let center = t.p(10).xy();
                if !(center.x.is_finite() && center.y.is_finite()) {
                    continue;
                }
                let x = crate::dxf_ext::xdata(&tg, crate::dxf_ext::APP);
                let field = |key: &str| {
                    x.iter().position(|t| t.code == 1000 && t.str() == key).and_then(|i| x.get(i + 1)).filter(|t| t.code == 1000).map(Tag::str)
                };
                // 70 bit 1: a paper space view; its layout is CADCraft xdata.
                let layout = (t.i(70).unwrap_or(0) & 1 != 0).then(|| field("LAYOUT").unwrap_or_default());
                let twist = t.f(50).filter(|a| a.is_finite()).map_or(0.0, |a| a.rem_euclid(360.0).to_radians());
                let v = NamedView { name: name.clone(), center, height: h, width: w, layer_state: field("LAYERSTATE"), twist, layout };
                match d.views.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = v,
                    None => d.views.push(v),
                }
            }
            "UCS" => {
                let name = t.s(2).unwrap_or_default();
                let (origin, x_axis, y_axis) = (t.p(10), t.p(11), t.p(12));
                let finite = [origin, x_axis, y_axis].iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
                if name.is_empty() || !finite || d.ucss.len() >= MAX_OBJECTS {
                    continue;
                }
                let u = Ucs { name: name.clone(), origin, x_axis, y_axis };
                match d.ucss.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = u,
                    None => d.ucss.push(u),
                }
            }
            "DIMSTYLE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.dim_styles.len() >= MAX_OBJECTS {
                    continue;
                }
                // Every variable with a DIMSTYLE group code; values the style can't take are
                // left at their defaults.
                let mut fields = serde_json::Map::new();
                for tag in tg.iter().take_while(|x| x.code < 1000) {
                    let Some((field, kind)) = crate::dxf_ext::dim_field(tag.code) else { continue };
                    if let Some(v) = crate::dxf_ext::decode(kind, tag, &rx.styles, &rx.brs, &rx.ltypes) {
                        fields.insert(field.to_string(), v);
                    }
                }
                fields.extend(crate::dxf_ext::read_newer_dimvars(&tg, &rx.styles, &rx.brs, &rx.ltypes));
                let mut st = DimStyle { name: name.clone(), annotative: annotative(&tg), ..DimStyle::default() };
                st.apply_fields(&fields);
                match d.dim_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = st,
                    None => d.dim_styles.push(st),
                }
            }
            "VPORT" => vport_record(d, &t),
            _ => {}
        }
    }
    // One *Active viewport is no tiling: the view is the editor's (it zooms to the extents).
    d.vports.retain(|c| c.tiles.len() > usize::from(c.is_active()));
}

/// Most tiles a viewport configuration keeps.
const MAX_VPORT_TILES: usize = 64;

/// A VPORT record: one tile of the configuration it names (`*ACTIVE` the one shown). Corners
/// are fractions of the drawing area; unusable tiles are skipped.
fn vport_record(d: &mut Drawing, t: &T) {
    let name = t.s(2).map(|n| n.trim().to_string()).unwrap_or_default();
    let (min, max, center) = (t.p(10).xy(), t.p(11).xy(), t.p(12).xy());
    let height = t.fd(40, 0.0);
    let unit = |v: f64| v.is_finite() && (-1e-9..=1.0 + 1e-9).contains(&v);
    let ok = [min.x, min.y, max.x, max.y].into_iter().all(unit)
        && max.x > min.x + 1e-6
        && max.y > min.y + 1e-6
        && center.is_finite()
        && height.is_finite()
        && height > 0.0;
    if name.is_empty() || !ok {
        return;
    }
    let aspect = t.f(41).filter(|a| a.is_finite() && *a > 0.0).unwrap_or(0.0);
    let clamp = |p: Vec2| Vec2::new(p.x.clamp(0.0, 1.0), p.y.clamp(0.0, 1.0));
    let tile = VportTile { min: clamp(min), max: clamp(max), center, height, aspect };
    let name = if name.eq_ignore_ascii_case(VportConfig::ACTIVE) { VportConfig::ACTIVE.to_string() } else { name };
    let room = d.vports.len() < MAX_OBJECTS;
    match d.vports.iter_mut().find(|c| c.name.eq_ignore_ascii_case(&name)) {
        Some(c) if c.tiles.len() < MAX_VPORT_TILES => c.tiles.push(tile),
        Some(_) => {}
        None if room => d.vports.push(VportConfig { name, tiles: vec![tile] }),
        None => {}
    }
}

/// The dash elements of an LTYPE record with their embedded text and shapes (DXF Reference,
/// LTYPE): each `49` length starts an element; `74` type flags (1 absolute rotation, 2 text,
/// 4 shape), `75` shape number, `340` STYLE, `46` scale, `50` rotation (radians), `44`/`45`
/// offset and `9` text follow it.
fn ltype_pattern(tags: &[Tag], styles: &HashMap<String, String>) -> Vec<DashElement> {
    let mut out: Vec<DashElement> = Vec::new();
    for t in tags.iter().take_while(|t| t.code < 1000) {
        if t.code == 49 {
            out.push(DashElement::dash(t.f64()));
            continue;
        }
        let Some(el) = out.last_mut() else { continue };
        let complex = el.text.is_some() || el.shape.is_some();
        let v = t.f64();
        match t.code {
            74 => {
                let flags = t.i64();
                el.absolute = flags & 1 != 0;
                if flags & 2 != 0 {
                    el.text = Some(String::new());
                } else if flags & 4 != 0 {
                    el.shape = Some(0);
                }
            }
            75 if el.shape.is_some() => el.shape = Some(u16::try_from(t.i64()).unwrap_or(0)),
            340 if complex => el.style = styles.get(&t.str().trim().to_ascii_uppercase()).filter(|n| !n.trim().is_empty()).cloned(),
            9 if el.text.is_some() => el.text = Some(t.str()),
            46 if complex && v.is_finite() => el.scale = v,
            50 if complex && v.is_finite() => el.rotation = v,
            44 if complex && v.is_finite() => el.offset.x = v,
            45 if complex && v.is_finite() => el.offset.y = v,
            _ => {}
        }
    }
    out
}

/// Table cells one file may materialize, across its blocks and entity sections.
const TABLE_CELL_BUDGET: usize = 4_000_000;

/// Read a DXF file into a drawing.
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    read_budgeted(bytes, TABLE_CELL_BUDGET)
}

/// Import DXF sections while sharing one materialized-table-cell budget across blocks and entity sections.
fn read_budgeted(bytes: &[u8], table_cells_left: usize) -> Result<Drawing> {
    let tags = cadcraft_dxf::parse(bytes).map_err(|e| IoError::Format(e.to_string()))?;
    read_tags_budgeted(&tags, table_cells_left)
}

/// [`read`] from the parsed tags (a DWG's rendition arrives as tags, never as text).
pub fn read_tags(tags: &[Tag]) -> Result<Drawing> {
    read_tags_budgeted(tags, TABLE_CELL_BUDGET)
}

/// [`read_budgeted`] from the parsed tags.
fn read_tags_budgeted(tags: &[Tag], mut table_cells_left: usize) -> Result<Drawing> {
    let secs = sections(tags);
    if secs.is_empty() {
        return Err(IoError::Format("no sections found".into()));
    }
    let mut d = Drawing::new_imperial();
    d.layouts.clear();
    // A file without $INSUNITS declares no units (#1209); only new drawings take the template's.
    d.header.set_i64("INSUNITS", 0);
    let mut rx = Rx { pstyles: plot_style_names(&secs), ..Rx::default() };
    let mut blocks: Vec<(String, Vec3, String, Vec<(Option<String>, bool, Entity)>, i64)> = Vec::new();
    let mut layout_objs: Vec<(String, u32, String, PageSetup)> = Vec::new();
    let mut entities = Vec::new();
    let mut objs = Objects::default();
    // Every handle another program's file already uses is taken before anything gets a new one:
    // its $HANDSEED can be lower than its handles, and entities made while reading (the faces of
    // a polyface mesh) would otherwise get handles that records further on still have. A file
    // CADCraft wrote names its own next handle, above which its rebuilt records are numbered
    // (#495), so it is left to that.
    let ours = secs.iter().any(|s| s.name == "OBJECTS" && s.tags.iter().any(|t| t.code == 3 && t.str() == crate::dxf_ext::HANDSEED_KEY));
    for t in secs.iter().filter(|_| !ours).flat_map(|s| s.tags.iter()).filter(|t| t.code == 5 || t.code == 105) {
        if let Some(h) = Handle::parse_hex(t.str().trim()) {
            d.bump_handseed(h);
        }
    }
    let mut skipped = std::collections::BTreeMap::new();
    count_skipped(&mut skipped, "(no section)", outside_sections(tags));
    for s in &secs {
        match s.name.as_str() {
            "HEADER" => header(&s.tags, &mut d),
            "TABLES" => tables(&s.tags, &mut d, &mut rx),
            "BLOCKS" => {
                let recs = records(&s.tags);
                for (k, tg) in &recs {
                    let t = T(tg);
                    if k == "BLOCK" && t.i(70).unwrap_or(0) & 4 != 0 {
                        rx.xref_paths.insert(t.s(2).unwrap_or_default().to_ascii_uppercase(), t.s(1).unwrap_or_default());
                    }
                }
                let mut n = 0;
                let mut i = 0;
                while let Some((k, tg)) = recs.get(i) {
                    if k == "BLOCK" {
                        let t = T(tg);
                        let name = t.s(2).unwrap_or_default();
                        let base = t.p(10);
                        let description = t.s(4).unwrap_or_default();
                        // Block flags: 4 xref, 8 overlay.
                        let xref = t.i(70).unwrap_or(0) & 12;
                        let mut j = i + 1;
                        while recs.get(j).is_some_and(|(k, _)| k != "ENDBLK") {
                            j += 1;
                        }
                        let body = recs.get(i + 1..j).unwrap_or(&[]);
                        let ents = parse_entities(body, &mut d, &mut rx, &mut n, &mut table_cells_left);
                        blocks.push((name, base, description, ents, xref));
                        i = j + 1;
                    } else {
                        // A record outside any BLOCK ... ENDBLK pair.
                        if k != "ENDBLK" {
                            n = n.saturating_add(1);
                        }
                        i += 1;
                    }
                }
                count_skipped(&mut skipped, "BLOCKS", n);
            }
            // A damaged file may split its entities over several ENTITIES sections (#865).
            "ENTITIES" => {
                let mut n = 0;
                entities.extend(parse_entities(&records(&s.tags), &mut d, &mut rx, &mut n, &mut table_cells_left));
                count_skipped(&mut skipped, "ENTITIES", n);
            }
            "OBJECTS" => {
                for (k, tg) in records(&s.tags) {
                    objs.add(&k, &tg);
                    if k == "LAYOUT" {
                        // Fields after the AcDbLayout marker (AcDbPlotSettings also has a code 1).
                        let start = tg.iter().position(|x| x.code == 100 && x.str() == "AcDbLayout").unwrap_or(0);
                        let tail = tg.get(start..).unwrap_or(&[]);
                        let t = T(tail);
                        // LAYOUT has two 330s: owner dict and the block record; take the last.
                        let br = tg.iter().rfind(|x| x.code == 330).map(Tag::str).unwrap_or_default();
                        let plot = T(tg.get(..start).unwrap_or(&[]));
                        layout_objs.push((t.s(1).unwrap_or_default(), t.i(71).unwrap_or(0) as u32, br.to_ascii_uppercase(), page_setup(&plot)));
                    }
                }
            }
            _ => {}
        }
    }
    // A file CADCraft wrote names its own next handle (#495): its table records and objects
    // are numbered above it and rebuilt on save, so following $HANDSEED would drift every save.
    let own_seed = objs.xrecord_text(crate::dxf_ext::HANDSEED_KEY).and_then(|t| Handle::parse_hex(t.trim()));
    // Layouts.
    let mut paper_block_for: HashMap<String, String> = HashMap::new();
    layout_objs.sort_by_key(|l| l.1);
    for (name, order, br, page) in &layout_objs {
        if name.eq_ignore_ascii_case("Model") {
            continue;
        }
        d.layouts.push(Layout { page: page.clone(), ..Layout::new(name, *order) });
        if let Some(bname) = rx.brs.get(br) {
            paper_block_for.insert(bname.to_ascii_uppercase(), name.clone());
        }
    }
    if d.layouts.is_empty() {
        d.layouts.push(Layout::new("Layout1", 1));
        paper_block_for.insert("*PAPER_SPACE".into(), "Layout1".into());
    }
    // The layout in `*Paper_Space` is the active one, kept when it isn't the first tab (the first
    // is active by default); paper-space entities without an owner go there.
    let active = paper_block_for.get("*PAPER_SPACE").cloned();
    let first_tab = d.layouts.iter().min_by_key(|l| l.tab_order).map(|l| l.name.clone());
    d.active_layout = active.clone().filter(|a| Some(a) != first_tab.as_ref());
    let first_layout = active.or_else(|| d.layouts.first().map(|l| l.name.clone())).unwrap_or_else(|| "Layout1".into());
    // Blocks: model/paper space blocks hold entities in newer files.
    for (name, base, description, ents, xref) in blocks {
        let up = name.to_ascii_uppercase();
        if up == "*MODEL_SPACE" {
            for (_, _, e) in ents {
                d.ensure_layer(&e.common.layer);
                d.model.push(e);
            }
            continue;
        }
        if up.starts_with("*PAPER_SPACE") {
            let lname = paper_block_for.get(&up).cloned().unwrap_or_else(|| first_layout.clone());
            if let Some(l) = d.layouts.iter_mut().find(|l| l.name == lname) {
                for (_, _, e) in ents {
                    l.entities.push(e);
                }
            }
            continue;
        }
        // Generated geometry we rebuild on save: table blocks, and standard arrowhead blocks
        // nothing inserts whose content is the one CADCraft writes. An arrowhead block with other
        // content (AutoCAD's `_Dot`, a user's redefinition) is kept, so dimensions keep it.
        let generated_arrow = || {
            let own: Vec<&Entity> = ents.iter().map(|(_, _, e)| e).collect();
            crate::dxf_ext::is_standard_arrow(&name) && !rx.inserted.contains(&up) && crate::dxf_write::is_generated_arrow(&name, &own)
        };
        if rx.table_blocks.contains(&up) || generated_arrow() {
            continue;
        }
        let mut b = Block::new(&name);
        b.base = base;
        b.description = description;
        if let Some(&(units, explodable)) = rx.block_props.get(&up) {
            (b.units, b.explodable) = (units, explodable);
        }
        if xref & 4 != 0 {
            b.xref_path = Some(String::new());
            b.xref_overlay = xref & 8 != 0;
        } else {
            for (_, _, e) in ents {
                b.entities.push(e);
            }
        }
        // Array parameters, with the block names the items use (other programs may rename
        // anonymous blocks); dropped when the items no longer match them.
        b.array = rx.block_arrays.get(&up).cloned().and_then(|mut a| a.sync_names(b.entities.iter().map(|e| e.as_ref())).then_some(a));
        if let Some(path) = rx.xref_paths.remove(&up) {
            b.xref_path = Some(path);
        }
        d.blocks.insert(name, std::sync::Arc::new(b));
    }
    // ENTITIES section: model space, or paper space when flagged (67=1).
    for (owner, paper, e) in entities {
        d.ensure_layer(&e.common.layer);
        let owner_name = owner.and_then(|o| rx.brs.get(&o.trim().to_ascii_uppercase()).cloned()).map(|n| n.to_ascii_uppercase());
        let to_paper = paper || owner_name.as_deref().is_some_and(|n| n.starts_with("*PAPER_SPACE"));
        if to_paper {
            let lname = owner_name.and_then(|n| paper_block_for.get(&n).cloned()).unwrap_or_else(|| first_layout.clone());
            if let Some(l) = d.layouts.iter_mut().find(|l| l.name == lname) {
                l.entities.push(e);
            }
        } else {
            d.model.push(e);
        }
    }
    apply_sortents(&mut d, &objs.sortents, &rx.brs, &paper_block_for, &first_layout);
    objs.apply(&mut d, &rx);
    crate::dxf_image::resolve(&secs, &mut d);
    crate::dxf_mline::resolve(&secs, &mut d);
    // Keep handles unique against the header's seed.
    let seed = d.header.str("HANDSEED", "");
    if let Some(h) = own_seed.or_else(|| Handle::parse_hex(&seed)) {
        d.bump_handseed(Handle(h.0.saturating_sub(1)));
    }
    if d.layer("0").is_none() {
        d.layers.insert(0, Layer::default());
    }
    crate::dxf_raw::read(&secs, &mut d);
    d.skipped_records = skipped;
    Ok(d)
}

fn count_skipped(skipped: &mut std::collections::BTreeMap<String, usize>, section: &str, n: usize) {
    if n > 0 {
        let c = skipped.entry(section.to_string()).or_insert(0);
        *c = c.saturating_add(n);
    }
}

/// Records between sections, which [`sections`] passes over: everything after an ENDSEC up to the
/// next SECTION. Stray ENDSECs and the EOF hold nothing and don't count.
fn outside_sections(tags: &[Tag]) -> usize {
    let mut inside = false;
    let mut n = 0usize;
    for t in tags.iter().filter(|t| t.code == 0) {
        match t.str().as_str() {
            "SECTION" => inside = true,
            "ENDSEC" => inside = false,
            "EOF" => {}
            _ if !inside => n = n.saturating_add(1),
            _ => {}
        }
    }
    n
}

/// A SORTENTSTABLE (DXF Reference, OBJECTS section): after its AcDbSortentsTable marker, the
/// owning block record (330), then pairs of an entity handle (331) and the sort handle (5) it is
/// drawn by.
fn sortents_table(tags: &[Tag]) -> Option<(String, HashMap<Handle, u64>)> {
    let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbSortentsTable")?;
    let tail = tags.get(start + 1..)?;
    let owner = tail.iter().find(|t| t.code == 330)?.str().trim().to_ascii_uppercase();
    let mut map = HashMap::new();
    let mut ent: Option<Handle> = None;
    for t in tail {
        match t.code {
            331 => ent = Handle::parse_hex(&t.str()),
            5 => {
                if let (Some(e), Some(k)) = (ent.take(), Handle::parse_hex(&t.str()))
                    && map.len() < MAX_OBJECTS
                {
                    map.insert(e, k.0);
                }
            }
            _ => {}
        }
    }
    (!map.is_empty()).then_some((owner, map))
}

/// Draw each space with a SORTENTSTABLE in sort-handle order: an entity listed in the table by
/// its sort handle, any other by its own handle.
fn apply_sortents(
    d: &mut Drawing,
    tables: &[(String, HashMap<Handle, u64>)],
    brs: &HashMap<String, String>,
    paper_for: &HashMap<String, String>,
    first_layout: &str,
) {
    for (owner, map) in tables {
        let Some(name) = brs.get(owner) else { continue };
        let up = name.to_ascii_uppercase();
        let key = |e: &Entity| map.get(&e.handle).copied().unwrap_or(e.handle.0);
        if up == "*MODEL_SPACE" {
            d.model.sort_by_key(key);
        } else if up.starts_with("*PAPER_SPACE") {
            let lname = paper_for.get(&up).map(String::as_str).unwrap_or(first_layout);
            if let Some(l) = d.layouts.iter_mut().find(|l| l.name == lname) {
                l.entities.sort_by_key(key);
            }
        } else if let Some(b) = d.blocks.get_mut(name.as_str()) {
            std::sync::Arc::make_mut(b).entities.sort_by_key(key);
        }
    }
}

/// Non-graphical objects applied once every entity is placed.
#[derive(Default)]
struct Objects {
    /// Dictionary entry handle (upper case) → entry name.
    names: HashMap<String, String>,
    /// SCALE objects (the annotation scale list), in file order.
    scales: Vec<cadcraft_doc::AnnoScale>,
    /// XRECORD handle (upper case) → its groups.
    xrecords: HashMap<String, Vec<Tag>>,
    /// TABLESTYLE objects: (handle, groups).
    table_styles: Vec<(String, Vec<Tag>)>,
    /// DIMASSOC objects.
    dimassocs: Vec<Vec<Tag>>,
    /// MLEADERSTYLE objects: (handle, groups).
    mleader_styles: Vec<(String, Vec<Tag>)>,
    /// GROUP objects: (handle, groups).
    groups: Vec<(String, Vec<Tag>)>,
    /// Saved paper-space views of layouts: (layout name, view).
    layout_views: Vec<(String, (Vec2, f64))>,
    /// SORTENTSTABLE objects: (owning block record handle, entity handle → sort handle).
    sortents: Vec<(String, HashMap<Handle, u64>)>,
}

const MAX_OBJECTS: usize = 1_000_000;

impl Objects {
    fn add(&mut self, kind: &str, tags: &[Tag]) {
        let h = T(tags).s(5).map(|h| h.trim().to_ascii_uppercase()).unwrap_or_default();
        match kind {
            "SCALE" if self.scales.len() < cadcraft_doc::MAX_SCALES => {
                if let Some(s) = crate::dxf_ext::read_scale(tags)
                    && !self.scales.iter().any(|x| x.name.eq_ignore_ascii_case(&s.name))
                {
                    self.scales.push(s);
                }
            }
            "DICTIONARY" => {
                let mut name: Option<String> = None;
                for t in tags {
                    match t.code {
                        3 => name = Some(t.str()),
                        350 | 360 => {
                            if let Some(n) = name.take()
                                && self.names.len() < MAX_OBJECTS
                            {
                                self.names.insert(t.str().trim().to_ascii_uppercase(), n);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "XRECORD" if self.xrecords.len() < MAX_OBJECTS => {
                self.xrecords.insert(h, tags.to_vec());
            }
            "TABLESTYLE" if self.table_styles.len() < MAX_OBJECTS => self.table_styles.push((h, tags.to_vec())),
            "DIMASSOC" if self.dimassocs.len() < MAX_OBJECTS => self.dimassocs.push(tags.to_vec()),
            "MLEADERSTYLE" if self.mleader_styles.len() < MAX_OBJECTS => self.mleader_styles.push((h, tags.to_vec())),
            "GROUP" if self.groups.len() < MAX_OBJECTS => self.groups.push((h, tags.to_vec())),
            "SORTENTSTABLE" if self.sortents.len() < MAX_OBJECTS => {
                if let Some(t) = sortents_table(tags) {
                    self.sortents.push(t);
                }
            }
            "LAYOUT" if self.layout_views.len() < MAX_OBJECTS => {
                let start = tags.iter().position(|x| x.code == 100 && x.str() == "AcDbLayout").unwrap_or(0);
                if let (Some(name), Some(view)) = (T(tags.get(start..).unwrap_or(&[])).s(1), layout_view(tags)) {
                    self.layout_views.push((name, view));
                }
            }
            _ => {}
        }
    }

    /// The text of the CADCraft XRECORD stored under `key` in a dictionary (its 1/3 strings).
    fn xrecord_text(&self, key: &str) -> Option<String> {
        let h = self.names.iter().find(|(_, n)| n.as_str() == key).map(|(h, _)| h)?;
        let tags = self.xrecords.get(h)?;
        let mut text = String::new();
        for t in tags.iter().filter(|t| t.code == 1 || t.code == 3) {
            if text.len() > crate::dxf_ext::MAX_PAYLOAD {
                break;
            }
            text.push_str(&t.str());
        }
        Some(text)
    }

    fn apply(self, d: &mut Drawing, rx: &Rx) {
        if let Some(style) = self.xrecord_text(crate::dxf_ext::MLEADER_STYLE_KEY).filter(|s| !s.trim().is_empty()) {
            d.header.set_str("CMLEADERSTYLE", &style);
        }
        // Table styles (named by their ACAD_TABLESTYLE dictionary entries).
        for (h, tags) in &self.table_styles {
            let Some(name) = self.names.get(h).filter(|n| !n.is_empty()) else { continue };
            let st = crate::dxf_table::read_table_style(name, tags);
            match d.table_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name)) {
                Some(x) => *x = st,
                None => d.table_styles.push(st),
            }
        }
        for (h, sh) in &rx.table_style_fix {
            let name = self.names.get(sh).cloned();
            // A table from another writer has no scale of its own: its text height is its
            // style's, so every cell style height applies as it is.
            let height = {
                let style = name.as_deref().unwrap_or("Standard");
                d.table_styles
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(style))
                    .map_or(cadcraft_doc::TableStyle::default().text_height, |s| s.text_height)
            };
            let _ = d.modify_entity(*h, |e| {
                if let EntityKind::Table(t) = &mut e.kind {
                    if let Some(name) = name {
                        t.style = name;
                    }
                    t.text_height = height;
                }
            });
        }
        // Parametric constraints and parameters.
        if let Some((c, p)) = self.xrecord_text(crate::dxf_ext::CONSTRAINTS_KEY).and_then(|t| crate::dxf_ext::parse_constraints(&t)) {
            d.constraints = c;
            d.parametric = p;
        }
        // The annotation scale list (a metric drawing without one gets the metric defaults),
        // then CADCraft's annotative scaling record.
        if !self.scales.is_empty() {
            d.scales = self.scales.clone();
        } else if d.header.i64("MEASUREMENT", 0) == 1 {
            d.scales = cadcraft_doc::metric_scales();
        }
        if let Some(p) = self.xrecord_text(crate::dxf_ext::ANNOTATION_KEY).and_then(|t| crate::dxf_ext::parse_anno(&t)) {
            crate::dxf_ext::apply_anno(d, p);
        }
        // Saved layer states.
        if let Some(states) = self.xrecord_text(crate::dxf_ext::LAYER_STATES_KEY).and_then(|t| crate::dxf_ext::parse_layer_states(&t)) {
            d.layer_states = states;
        }
        // The drawing's plot style tables.
        if let Some(tables) = self.xrecord_text(crate::dxf_ext::PLOT_STYLES_KEY).and_then(|t| crate::dxf_ext::parse_plot_styles(&t)) {
            d.plot_style_tables = tables;
        }
        // Multileader styles (named by their ACAD_MLEADERSTYLE dictionary entries).
        for (h, tags) in &self.mleader_styles {
            let Some(name) = self.names.get(h).filter(|n| !n.is_empty()) else { continue };
            let t = T(tags);
            let def = MLeaderStyle::default();
            let num = |code: i32, d: f64, min: f64| t.f(code).filter(|v| v.is_finite() && *v >= min).unwrap_or(d);
            let text_style = t.s(342).and_then(|sh| rx.styles.get(&sh.trim().to_ascii_uppercase()).cloned()).filter(|n| !n.is_empty());
            let st = MLeaderStyle {
                name: name.clone(),
                arrow_size: num(44, def.arrow_size, 0.0),
                text_height: t.f(45).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(def.text_height),
                landing_gap: num(42, def.landing_gap, 0.0),
                dogleg: num(43, def.dogleg, 0.0),
                text_style: text_style.unwrap_or(def.text_style),
                annotative: t.i(296).unwrap_or(0) != 0,
            };
            match d.mleader_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name)) {
                Some(x) => *x = st,
                None => d.mleader_styles.push(st),
            }
        }
        crate::dxf_mleader::apply(d, &rx.mleader_fix, &self.names);
        for (name, view) in &self.layout_views {
            if let Some(l) = d.layouts.iter_mut().find(|l| l.name == *name) {
                l.view = Some(*view);
            }
        }
        // Groups (named by their ACAD_GROUP dictionary entries); members that exist.
        for (h, tags) in &self.groups {
            let Some(name) = self.names.get(h).filter(|n| !n.is_empty()) else { continue };
            let t = T(tags);
            let members: Vec<Handle> = tags
                .iter()
                .filter(|x| x.code == 340)
                .filter_map(|x| Handle::parse_hex(&x.str()))
                .filter(|m| d.entity(*m).is_some())
                .take(MAX_OBJECTS)
                .collect();
            let g = Group { name: name.clone(), description: t.s(300).unwrap_or_default(), selectable: t.i(71).unwrap_or(1) != 0, members };
            match d.groups.iter_mut().find(|x| x.name.eq_ignore_ascii_case(name)) {
                Some(x) => *x = g,
                None => d.groups.push(g),
            }
        }
        // Standard associativity, for dimensions without CADCraft's exact links.
        for tags in &self.dimassocs {
            dimassoc(d, tags);
        }
    }
}

/// Associativity from a DIMASSOC object (files from other writers). The dimension kind decides
/// which definition points the references name, in the order AutoCAD writes them: `p13`/`p14`
/// for linear, aligned and arc-length dimensions, both ends of both lines (`p13`..`p16`) for
/// angular dimensions, the three points of an angular-3-point dimension, `p13` for ordinate
/// dimensions, and for radial or diametric dimensions the single circle reference
/// ([`radial_links`]). CADCraft's own xdata links, when present, win.
fn dimassoc(d: &mut Drawing, tags: &[Tag]) {
    let start = tags.iter().position(|x| x.code == 100 && x.str() == "AcDbDimAssoc").unwrap_or(0);
    let tail = tags.get(start..).unwrap_or(&[]);
    let Some(dim_h) = tail.iter().find(|x| x.code == 330).and_then(|x| Handle::parse_hex(&x.str())) else { return };
    let Some(EntityKind::Dimension(dm)) = d.entity(dim_h).map(|e| &e.kind) else { return };
    if !dm.assoc.is_empty() {
        return;
    }
    // A radial or diametric dimension names its circle once, by its centre.
    if matches!(dm.kind, DimKind::Radius | DimKind::Diameter) {
        let links = radial_links(d, dm, tail);
        if !links.is_empty() {
            let _ = d.modify_entity(dim_h, |e| {
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.assoc = links;
                }
            });
        }
        return;
    }
    let slots: &[&str] = match dm.kind {
        DimKind::Linear { .. } | DimKind::Aligned => &["p13", "p14"],
        DimKind::Angular => &["p13", "p14", "p15", "p16"],
        DimKind::Angular3P => &["p13", "p14", "p15"],
        DimKind::ArcLength => &["p13", "p14"],
        DimKind::Ordinate { .. } => &["p13"],
        DimKind::Radius | DimKind::Diameter => return,
    };
    let flags = T(tail).i(90).unwrap_or(0);
    let used: Vec<&str> = slots.iter().enumerate().filter(|(i, _)| flags & (1 << i) != 0).map(|(_, s)| *s).collect();
    // Point references start at each `1 AcDbOsnapPointRef`.
    let starts: Vec<usize> = tail.iter().enumerate().filter(|(_, x)| x.code == 1).map(|(i, _)| i).take(8).collect();
    let mut links = Vec::new();
    for (k, s) in starts.iter().enumerate() {
        let Some(point) = used.get(k) else { break };
        let end = starts.get(k + 1).copied().unwrap_or(tail.len());
        let r = T(tail.get(s + 1..end).unwrap_or(&[]));
        let Some(main) = r.s(331).and_then(|h| Handle::parse_hex(&h)) else { continue };
        let other = r.s(332).and_then(|h| Handle::parse_hex(&h));
        let at = r.p(10).xy();
        // A polyline's segment (subentity type 73 = 2, edge; 91 its 1-based index).
        let segment = r.i(91).filter(|k| *k >= 1 && r.i(73) == Some(2));
        if let Some(snap) = infer_snap(d, r.i(72).unwrap_or(0), main, other, at, segment) {
            links.push(DimAssoc { point: (*point).to_string(), handle: main, snap });
        }
    }
    if !links.is_empty() {
        let _ = d.modify_entity(dim_h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.assoc = links;
            }
        });
    }
}

/// Associations of a radial or diametric dimension from its one standard reference: the circle
/// or arc it measures, by its centre (osnap 3). The definition points are then on that circle,
/// so the dimension follows its radius or diameter.
fn radial_links(d: &Drawing, dm: &Dimension, tail: &[Tag]) -> Vec<DimAssoc> {
    let starts: Vec<usize> = tail.iter().enumerate().filter(|(_, x)| x.code == 1 && x.str() == "AcDbOsnapPointRef").map(|(i, _)| i).take(8).collect();
    for (k, s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(tail.len());
        let r = T(tail.get(s + 1..end).unwrap_or(&[]));
        if !matches!(r.i(72), Some(3 | 4)) {
            continue;
        }
        let Some(handle) = r.s(331).and_then(|h| Handle::parse_hex(&h)) else { continue };
        let Some(centre) = circle_centre(d, handle) else { continue };
        let on = |point: &str, p: Vec3| {
            let angle = centre.angle_to(p.xy());
            angle.is_finite().then(|| DimAssoc { point: point.into(), handle, snap: AssocSnap::OnCircle { angle } })
        };
        let links = match dm.kind {
            DimKind::Radius => on("p15", dm.p15).map(|p15| vec![DimAssoc { point: "defpt".into(), handle, snap: AssocSnap::Center }, p15]),
            DimKind::Diameter => [on("defpt", dm.defpt), on("p15", dm.p15)].into_iter().collect::<Option<Vec<_>>>(),
            _ => None,
        };
        if let Some(links) = links {
            return links;
        }
    }
    Vec::new()
}

/// The planar centre of a circle or arc, or `None` for any other entity: the point a radial or
/// diametric dimension's single DIMASSOC reference names.
fn circle_centre(d: &Drawing, h: Handle) -> Option<Vec2> {
    match &d.entity(h)?.kind {
        EntityKind::Circle(c) => Some(c.center.xy()),
        EntityKind::Arc(a) => Some(a.center.xy()),
        _ => None,
    }
}

/// A CADCraft snap from a DIMASSOC object snap type and point, and for a polyline the segment
/// the point is an end of: AutoCAD writes no usable point there (10/20 are 0, #1049), the segment
/// and the osnap (1 its end, 13 its start) name the vertex. For a line or arc the osnap alone
/// says which end: 13 is the start and 1 the end, whatever the stored point (the point is
/// 0,0,0 for curves, and choosing by distance gave the wrong end for arcs). A point entity has
/// only one position, so both codes mean it.
fn infer_snap(d: &Drawing, osnap: i64, main: Handle, other: Option<Handle>, at: Vec2, segment: Option<i64>) -> Option<AssocSnap> {
    let e = d.entity(main)?;
    if let (EntityKind::LwPolyline(p), Some(k), 1 | 13) = (&e.kind, segment, osnap) {
        let n = p.vertices.len();
        let segments = if p.closed { n } else { n.saturating_sub(1) };
        let k = usize::try_from(k).ok().filter(|k| *k <= segments)?;
        let index = if osnap == 13 { k - 1 } else { k % n };
        return Some(AssocSnap::Vertex { index });
    }
    Some(match osnap {
        1 | 13 => match &e.kind {
            // AutoCAD names a curve's start with 13 and its end with 1 (the stored point is 0,0,0).
            // Other writers use 1 for either end of a line with the point stored: take the nearer.
            EntityKind::Line(l) => {
                if osnap == 13 || (at != Vec2::ZERO && l.a.xy().dist(at) <= l.b.xy().dist(at)) {
                    AssocSnap::Start
                } else {
                    AssocSnap::End
                }
            }
            EntityKind::Arc(_) => {
                if osnap == 13 {
                    AssocSnap::Start
                } else {
                    AssocSnap::End
                }
            }
            EntityKind::LwPolyline(p) => {
                let index = p.vertices.iter().enumerate().min_by(|x, y| x.1.p.dist(at).total_cmp(&y.1.p.dist(at))).map(|(i, _)| i)?;
                AssocSnap::Vertex { index }
            }
            EntityKind::Point(_) => AssocSnap::Start,
            _ => return None,
        },
        2 => AssocSnap::Mid,
        3 | 4 => AssocSnap::Center,
        5 | 10 => match &e.kind {
            EntityKind::Circle(Circle { center, .. }) | EntityKind::Arc(Arc { center, .. }) => {
                let v = at - center.xy();
                AssocSnap::OnCircle { angle: v.y.atan2(v.x) }
            }
            _ => return None,
        },
        6 => AssocSnap::Intersection { other: other.filter(|o| d.entity(*o).is_some())? },
        _ => return None,
    })
}

/// Page setup from a LAYOUT object's AcDbPlotSettings fields (sizes and margins in mm).
fn page_setup(t: &T) -> PageSetup {
    let mut p = PageSetup::default();
    let mm = |code: i32, d: f64| {
        let v = t.fd(code, d);
        if v.is_finite() && (0.0..=100_000.0).contains(&v) { v } else { d }
    };
    let (w, h) = (mm(44, 0.0), mm(45, 0.0));
    if w > 0.0 && h > 0.0 {
        p.width_mm = w.min(h);
        p.height_mm = w.max(h);
    }
    p.margins_mm = [mm(40, p.margins_mm[0]), mm(41, p.margins_mm[1]), mm(42, p.margins_mm[2]), mm(43, p.margins_mm[3])];
    // Plot paper units 1: millimetres. 0 (inches) is left to MEASUREMENT: earlier CADCraft
    // versions wrote 0 for every layout.
    p.paper_mm = t.i(72) == Some(1);
    // Plot rotation 1 or 3 turns the media a quarter turn; media defined wider than tall is
    // already landscape.
    let rotated = matches!(t.i(73), Some(1 | 3));
    if w > 0.0 && h > 0.0 {
        p.landscape = rotated != (w > h);
    } else if t.i(73).is_some() {
        p.landscape = rotated;
    }
    if let Some(name) = t.s(4).filter(|n| !n.is_empty()) {
        p.paper = name.replace('_', " ");
    }
    if let Some(dev) = t.s(2).map(|s| s.trim().to_string()) {
        p.device = if dev.is_empty() || dev.eq_ignore_ascii_case("none_device") { "None".into() } else { dev };
    }
    // Plot layout flags: 2 display plot styles, 4 centred, 16 standard scale (75 = 0: scaled
    // to fit), 128 lineweights.
    if let Some(flags) = t.i(70) {
        p.show_plot_styles = flags & 2 != 0;
        p.center = flags & 4 != 0;
        p.lineweights = flags & 128 != 0;
        p.scale_to_fit = flags & 16 != 0 && t.i(75) == Some(0);
    }
    // Custom scale: 142 paper units per 143 drawing units; 147 is the scale factor.
    let pos = |v: Option<f64>| v.filter(|v| v.is_finite() && *v > 0.0);
    let ratio = pos(t.f(142)).zip(pos(t.f(143))).map(|(n, d)| n / d);
    if let Some(s) = pos(ratio).or_else(|| pos(t.f(147))) {
        p.scale = s;
    }
    p.plot_area = match t.i(74) {
        Some(0) => "display",
        Some(1) => "extents",
        Some(2) => "limits",
        Some(4) => "window",
        _ => "layout",
    }
    .into();
    p.plot_window = [t.fd(48, 0.0), t.fd(49, 0.0), t.fd(140, 0.0), t.fd(141, 0.0)].map(|v| if v.is_finite() { v } else { 0.0 });
    p.plot_style_table = t.s(7).unwrap_or_default();
    p
}

/// A layout's saved paper-space view (centre, height) from CADCraft xdata.
fn layout_view(tags: &[Tag]) -> Option<(Vec2, f64)> {
    let list = crate::dxf_ext::xdata_list(crate::dxf_ext::xdata(tags, crate::dxf_ext::APP), "PSVIEW");
    let g = |c: i32| list.iter().find(|t| t.code == c).map(Tag::f64).filter(|v| v.is_finite());
    Some((Vec2::new(g(1010)?, g(1020)?), g(1040)?))
}

/// A HELIX (DXF Reference): the axis base point, start point, axis, radius, turns, turn height,
/// handedness and constraint after the AcDbHelix marker; the top radius is how far the end of its
/// spline (AcDbSpline: the last control point, else fit point) lies from the axis.
fn helix(tags: &[Tag]) -> Helix {
    let hs = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbHelix").unwrap_or(tags.len());
    let ss = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbSpline").unwrap_or(0).min(hs);
    let spline = tags.get(ss..hs).unwrap_or(&[]);
    let t = T(tags.get(hs..).unwrap_or(&[]));
    // The last point of the spline (3D) with group `code`.
    let last = |code: i32| {
        let i = spline.iter().rposition(|g| g.code == code)?;
        let f = |c: i32| spline.get(i..).and_then(|r| r.iter().find(|g| g.code == c)).map(Tag::f64).unwrap_or(0.0);
        Some(Vec3::new(f(code), f(code + 10), f(code + 20)))
    };
    let center = t.p(10);
    let start = t.p(11);
    let axis = t.p(12);
    let axis = if axis.is_finite() && axis.len() > 1e-12 { axis.normalized() } else { Vec3::Z };
    // Distance from the axis.
    let radius = |p: Vec3| {
        let v = Vec3::new(p.x - center.x, p.y - center.y, p.z - center.z);
        let along = v.dot(axis);
        Vec3::new(v.x - axis.x * along, v.y - axis.y * along, v.z - axis.z * along).len()
    };
    let base_radius = t.f(40).filter(|r| r.is_finite() && *r >= 0.0).unwrap_or_else(|| radius(start));
    let turns = t.fd(41, 1.0);
    let end = last(10).or_else(|| last(11));
    let height = match (t.f(42), end) {
        (Some(th), _) if th.is_finite() => th * turns,
        (_, Some(e)) => e.z - center.z,
        _ => 0.0,
    };
    Helix {
        center,
        base_radius,
        top_radius: end.map(radius).unwrap_or(base_radius),
        height,
        turns,
        start_angle: center.xy().angle_to(start.xy()),
        ccw: t.i(290).unwrap_or(1) != 0,
        constrain: HelixConstrain::from_dxf(t.i(280).unwrap_or(2)),
    }
    .sanitized()
}

/// An entity from the groups of a DXF record of type `kind` (without its `0` group), as the
/// reader makes it. AutoLISP's `entmake` and `entmod` are built on it.
pub fn entity_from_groups(kind: &str, tags: &[Tag]) -> Option<(Common, EntityKind)> {
    entity(&kind.to_ascii_uppercase(), tags)
}

#[cfg(test)]
mod table_budget_tests {
    use super::*;
    /// Build synthetic table tags with a requested row and column count for allocation-budget checks.
    fn table_tags(rows: i64, cols: i64) -> Vec<Tag> {
        vec![Tag::s(100, "AcDbTable"), Tag::i(91, rows), Tag::i(92, cols), Tag::i(171, 1), Tag::s(1, "cell")]
    }
    /// A table that cannot fit allocates no cell grid, while a smaller table spends only its cell count.
    #[test]
    fn exhausted_budget_rejects_before_materializing_cells() {
        let mut left = 3;
        let table = acad_table(&table_tags(2, 2), &mut left);
        assert!(table.cells.is_empty() && table.row_heights.is_empty() && table.col_widths.is_empty());
        assert_eq!(left, 3);
        let table = acad_table(&table_tags(1, 2), &mut left);
        assert_eq!(table.cells[0].len(), 2);
        assert_eq!(left, 1);
    }
    /// Tables in blocks and repeated entity sections consume one document-wide cell allowance.
    #[test]
    fn blocks_and_entity_sections_share_one_budget() {
        let table = "0\nACAD_TABLE\n100\nAcDbTable\n91\n1\n92\n2\n171\n1\n1\ncell\n";
        let dxf = format!(
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nTEST\n10\n0\n20\n0\n{table}0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{table}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{table}0\nENDSEC\n0\nEOF\n"
        );
        let d = read_budgeted(dxf.as_bytes(), 2).unwrap();
        let block = d.block("TEST").unwrap();
        let count = |e: &Entity| match &e.kind {
            EntityKind::Table(t) => t.cells.iter().map(Vec::len).sum::<usize>(),
            _ => 0,
        };
        assert_eq!(block.entities.iter().map(|e| count(e)).sum::<usize>(), 2);
        assert_eq!(d.model.iter().map(|e| count(e)).sum::<usize>(), 0);
    }
}
