//! DXF → Drawing.

use std::collections::HashMap;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::{PolyVertex, Spline, Vec2, Vec3};

use crate::{IoError, Result};

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
    if (c.extrusion.z - 1.0).abs() < 1e-12 && c.extrusion.x.abs() < 1e-12 && c.extrusion.y.abs() < 1e-12 {
        p
    } else {
        cadcraft_geom::Mat4::ocs(c.extrusion).apply(p)
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
    Text {
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
    }
}

/// Parse one entity record. POLYLINE/INSERT followers are handled by the caller.
fn entity(kind: &str, tags: &[Tag]) -> Option<(Common, EntityKind)> {
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
        "ELLIPSE" => EntityKind::Ellipse(Ellipse {
            center: t.p(10),
            major: t.p(11),
            ratio: t.fd(40, 1.0).clamp(1e-9, 1.0),
            start: t.fd(41, 0.0),
            end: t.fd(42, std::f64::consts::TAU),
        }),
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
            EntityKind::LwPolyline(LwPolyline {
                vertices: verts,
                closed: flags & 1 != 0,
                const_width: t.fd(43, 0.0),
                elevation: t.fd(38, 0.0),
                plinegen: flags & 128 != 0,
            })
        }
        "SPLINE" => {
            let degree = t.i(71).unwrap_or(3).clamp(1, 10) as usize;
            let knots = t.all_f(40);
            let control = t.pts(10);
            let weights = t.all_f(41);
            let fit = t.pts(11);
            let mut sp = Spline { degree, knots, control, weights, fit: fit.clone(), closed: t.i(70).unwrap_or(0) & 1 != 0 };
            if !sp.is_valid() && fit.len() >= 2 {
                sp = Spline::from_fit(&fit, sp.closed);
            } else if !sp.is_valid() && sp.control.len() >= 2 {
                sp = Spline::from_control(sp.control.clone(), degree);
            }
            EntityKind::Spline(sp)
        }
        "RAY" => EntityKind::Ray(RayLine { base: t.p(10), dir: t.p(11) }),
        "XLINE" => EntityKind::XLine(RayLine { base: t.p(10), dir: t.p(11) }),
        "TEXT" => EntityKind::Text(text_from(&t)),
        "MTEXT" => {
            // Contents: code 3 chunks followed by code 1.
            let mut contents = String::new();
            for tg in tags.iter().filter(|x| x.code == 3) {
                contents.push_str(&tg.str());
            }
            contents.push_str(&t.s(1).unwrap_or_default());
            let rotation = match (t.f(11), t.f(21)) {
                (Some(x), Some(y)) if x != 0.0 || y != 0.0 => Vec2::new(x, y).angle(),
                _ => t.fd(50, 0.0).to_radians(),
            };
            EntityKind::MText(MText {
                insert: t.p(10),
                height: t.fd(40, 0.2).max(1e-9),
                width: t.fd(41, 0.0).max(0.0),
                attach: t.i(71).unwrap_or(1).clamp(1, 9) as u8,
                rotation,
                style: t.s(7).unwrap_or_else(|| "Standard".into()),
                contents,
                line_spacing: t.fd(44, 1.0),
            })
        }
        "ATTDEF" => EntityKind::AttDef(Attrib {
            tag: t.s(2).unwrap_or_default(),
            text: text_from(&t),
            invisible: t.i(70).unwrap_or(0) & 1 != 0,
            constant: t.i(70).unwrap_or(0) & 2 != 0,
            prompt: t.s(3).unwrap_or_default(),
        }),
        "INSERT" => EntityKind::Insert(Insert {
            block: t.s(2).unwrap_or_default(),
            insert: t.p(10),
            scale: Vec3::new(t.fd(41, 1.0), t.fd(42, 1.0), t.fd(43, 1.0)),
            rotation: t.fd(50, 0.0).to_radians(),
            attribs: Vec::new(),
            cols: t.i(70).unwrap_or(1).clamp(1, 10_000) as u32,
            rows: t.i(71).unwrap_or(1).clamp(1, 10_000) as u32,
            col_spacing: t.fd(44, 0.0),
            row_spacing: t.fd(45, 0.0),
        }),
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
                text_mid: t.p(11),
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
            })
        }
        "LEADER" => EntityKind::Leader(Leader {
            vertices: t.pts(10).into_iter().map(|p| p.to3(0.0)).collect(),
            arrow: t.i(71).unwrap_or(1) != 0,
            spline: t.i(72).unwrap_or(0) != 0,
            style: t.s(3).unwrap_or_else(|| "Standard".into()),
        }),
        "SOLID" | "TRACE" => {
            let corners = [t.p(10), t.p(11), t.p(12), if t.f(13).is_some() { t.p(13) } else { t.p(12) }];
            let corners = corners.map(|p| ocs(&c, p));
            if kind == "SOLID" { EntityKind::Solid(Solid { corners }) } else { EntityKind::Trace(Solid { corners }) }
        }
        "3DFACE" => EntityKind::Face3d(Face3d { corners: [t.p(10), t.p(11), t.p(12), t.p(13)], hidden_edges: t.i(70).unwrap_or(0) as u8 }),
        "HATCH" => hatch(tags, &t)?,
        "ACAD_TABLE" => EntityKind::Table(acad_table(tags)),
        "VIEWPORT" => EntityKind::Viewport(Viewport {
            center: t.p(10),
            width: t.fd(40, 1.0),
            height: t.fd(41, 1.0),
            view_center: Vec2::new(t.fd(12, 0.0), t.fd(22, 0.0)),
            view_height: t.fd(45, 1.0),
            id: t.i(69).unwrap_or(0) as u32,
            locked: t.i(90).unwrap_or(0) & 16384 != 0,
            frozen_layers: crate::dxf_ext::read_frozen(tags),
            layer_colors: Vec::new(),
        }),
        "WIPEOUT" => {
            let o = t.p(10);
            let u = t.p(11);
            let v = t.p(12);
            // Clip vertices are in pixel space: origin at the image's top-left corner, y pointing down.
            let pts: Vec<Vec2> = t.pts(14).into_iter().map(|q| o.xy() + u.xy() * (q.x + 0.5) + v.xy() * (0.5 - q.y)).collect();
            EntityKind::Wipeout(Wipeout { boundary: pts })
        }
        "IMAGE" => {
            EntityKind::Image(Image { insert: t.p(10), u: t.p(11), v: t.p(12), size: Vec2::new(t.fd(13, 1.0), t.fd(23, 1.0)), path: String::new() })
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
fn acad_table(tags: &[Tag]) -> Table {
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
        let merged = match (c.i(173).unwrap_or(0) != 0, span(176), span(175)) {
            (true, Some(rs), Some(cs)) if rs >= 1 && cs >= 1 && (rs, cs) != (1, 1) => Some((rs, cs)),
            _ => None,
        };
        flat.push(TableCell { text, merged });
    }
    flat.resize(rows * cols, TableCell { text: String::new(), merged: None });
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
        None => (cols > 1 && cells.first().and_then(|r| r.first()).and_then(|c| c.merged) == Some((1, cols as u32)), false, 0.18, "Standard".into()),
    };
    Table { insert: T(tags).p(10), col_widths, row_heights, cells, style, text_height, title, header }
}

fn hatch(tags: &[Tag], t: &T) -> Option<EntityKind> {
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
                        if let Some(l) = verts.last_mut() {
                            l.bulge = cadcraft_geom::arc_to_bulge(sweep);
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
                        i += 8;
                        let e = cadcraft_geom::Ellipse { center: c, major: m, ratio, start: sa, end: ea };
                        let mut pts = Vec::new();
                        e.tessellate(m.len() * 1e-3, &mut pts);
                        verts.extend(pts.into_iter().map(PolyVertex::new));
                    }
                    4 => {
                        // Spline edge: 94 degree, 73 rational, 74 periodic, 95 knots, 96 ctrl.
                        let _deg = get(i).map(Tag::i64).unwrap_or(3);
                        while get(i).is_some_and(|x| x.code != 96) {
                            i += 1;
                        }
                        let nc = get(i).map(Tag::i64).unwrap_or(0).clamp(0, 100_000) as usize;
                        i += 1;
                        let mut ctrl = Vec::new();
                        for _ in 0..nc {
                            if get(i).is_some_and(|t| t.code == 10) {
                                ctrl.push(Vec2::new(f(i), f(i + 1)));
                                i += 2;
                                if get(i).is_some_and(|t| t.code == 42) {
                                    i += 1;
                                }
                            }
                        }
                        let sp = Spline::from_control(ctrl, 3);
                        verts.extend(sp.tessellate(1e-3).into_iter().map(PolyVertex::new));
                    }
                    _ => {}
                }
            }
        }
        if verts.len() > 1 && verts.first().map(|f| f.p).zip(verts.last().map(|l| l.p)).is_some_and(|(a, b)| a.near(b, 1e-9)) {
            verts.pop();
        }
        loops.push(HatchLoop { vertices: verts, outer });
    }
    Some(EntityKind::Hatch(Hatch {
        pattern: pattern.to_ascii_uppercase(),
        solid: solid || pattern.eq_ignore_ascii_case("SOLID"),
        loops,
        scale: t.fd(41, 1.0),
        angle: t.fd(52, 0.0).to_radians(),
        associative: t.i(71).unwrap_or(0) != 0,
        style: t.i(75).unwrap_or(0) as u8,
        elevation: 0.0,
        gradient: gradient(tags),
        origin: Vec2::ZERO,
        background: None,
    }))
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
    /// Upper-case names of anonymous `*T` blocks drawn by ACAD_TABLE entities.
    table_blocks: std::collections::HashSet<String>,
    /// Upper-case block names used by INSERTs.
    inserted: std::collections::HashSet<String>,
    /// Tables whose style is known only by TABLESTYLE handle: (table, style handle).
    table_style_fix: Vec<(Handle, String)>,
}

/// Dimension overrides and associativity, table references: data of an entity record that
/// needs the file's handle maps.
fn entity_extras(kind: &str, tags: &[Tag], h: Handle, k: &mut EntityKind, rx: &mut Rx) {
    match (kind, k) {
        ("DIMENSION", EntityKind::Dimension(dm)) => {
            dm.overrides = crate::dxf_ext::read_dstyle(tags, &rx.styles, &rx.brs);
            dm.assoc = crate::dxf_ext::read_assoc(tags);
            if let Some(center) = crate::dxf_ext::read_arclen(tags) {
                dm.kind = DimKind::ArcLength;
                dm.p15 = center;
            }
        }
        ("ACAD_TABLE", EntityKind::Table(_)) => {
            let t = T(tags);
            if let Some(b) = t.s(2) {
                rx.table_blocks.insert(b.to_ascii_uppercase());
            }
            let ours = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP).iter().any(|t| t.code == 1000 && t.str() == "TABLE");
            if !ours && let Some(sh) = t.s(342) {
                rx.table_style_fix.push((h, sh.trim().to_ascii_uppercase()));
            }
        }
        ("INSERT", EntityKind::Insert(ins)) => {
            rx.inserted.insert(ins.block.to_ascii_uppercase());
        }
        _ => {}
    }
}

/// Parse a run of entity records (ENTITIES section or a block's body) into entities.
fn parse_entities(recs: &[(String, Vec<Tag>)], d: &mut Drawing, rx: &mut Rx) -> Vec<(Option<String>, bool, Entity)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some((kind, tags)) = recs.get(i) {
        let t = T(tags);
        let owner = crate::dxf_ext::owner(tags);
        let paper = t.i(67) == Some(1);
        let handle = t.s(5).and_then(|h| Handle::parse_hex(&h));
        let mut kindent = entity(kind, tags);
        i += 1;
        match kind.as_str() {
            "POLYLINE" => {
                let flags = t.i(70).unwrap_or(0);
                let mut pts: Vec<(Vec3, f64, f64, f64)> = Vec::new();
                while let Some((k2, t2)) = recs.get(i) {
                    if k2 == "VERTEX" {
                        let tt = T(t2);
                        if tt.i(70).unwrap_or(0) & 16 == 0 {
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
                let k = if flags & (8 | 16 | 64) != 0 {
                    EntityKind::Polyline3d(Polyline3d { points: pts.iter().map(|p| p.0).collect(), closed: flags & 1 != 0 })
                } else {
                    EntityKind::LwPolyline(LwPolyline {
                        vertices: pts.iter().map(|p| PolyVertex { p: p.0.xy(), bulge: p.1, start_width: p.2, end_width: p.3 }).collect(),
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
                    while let Some((k2, t2)) = recs.get(i) {
                        if k2 == "ATTRIB" {
                            let tt = T(t2);
                            attribs.push(Attrib {
                                tag: tt.s(2).unwrap_or_default(),
                                text: text_from(&tt),
                                invisible: tt.i(70).unwrap_or(0) & 1 != 0,
                                constant: false,
                                prompt: String::new(),
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
            "SEQEND" | "VERTEX" | "ATTRIB" => continue,
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
            out.push((owner, paper, Entity { handle: h, common: c, kind: k }));
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

fn tables(tags: &[Tag], d: &mut Drawing, rx: &mut Rx) {
    let recs = records(tags);
    // Handle maps first: DIMSTYLE records refer to text styles and arrow blocks by handle.
    for (kind, tg) in &recs {
        let t = T(tg);
        let (Some(h), Some(n)) = (t.s(5), t.s(2)) else { continue };
        match kind.as_str() {
            "STYLE" => {
                rx.styles.insert(h.trim().to_ascii_uppercase(), n);
            }
            "BLOCK_RECORD" => {
                rx.brs.insert(h.trim().to_ascii_uppercase(), n);
            }
            _ => {}
        }
    }
    for (kind, tg) in recs {
        let t = T(&tg);
        match kind.as_str() {
            "LAYER" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() {
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
                let alpha = crate::dxf_ext::xdata(&tg, crate::dxf_ext::LAYER_TRANSPARENCY_APP).iter().find(|x| x.code == 1071);
                if let Some(pct) = alpha.and_then(|x| crate::dxf_ext::transparency_from_dxf(x.i64())) {
                    l.transparency = pct;
                }
                if let Some(desc) = crate::dxf_ext::xdata(&tg, crate::dxf_ext::LAYER_DESCRIPTION_APP).iter().filter(|x| x.code == 1000).nth(1) {
                    l.description = desc.str();
                }
                match d.layer_mut(&name) {
                    Some(x) => *x = l,
                    None => d.layers.push(l),
                }
            }
            "LTYPE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.linetype(&name).is_some() {
                    continue;
                }
                let pattern = tg.iter().filter(|x| x.code == 49).map(|x| DashElement::dash(x.f64())).collect();
                d.linetypes.push(Linetype { name, description: t.s(3).unwrap_or_default(), pattern });
            }
            "STYLE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                // 70 bit 4: vertical; 71 bits 2/4: backwards / upside down.
                let gen_flags = t.i(71).unwrap_or(0);
                let st = TextStyle {
                    name: name.clone(),
                    font: t.s(3).unwrap_or_default(),
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
            "DIMSTYLE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                // Every variable with a DIMSTYLE group code; values the style can't take are
                // left at their defaults.
                let mut fields = serde_json::Map::new();
                for tag in tg.iter().take_while(|x| x.code < 1000) {
                    let Some((field, kind)) = crate::dxf_ext::dim_field(tag.code) else { continue };
                    if let Some(v) = crate::dxf_ext::decode(kind, tag, &rx.styles, &rx.brs) {
                        fields.insert(field.to_string(), v);
                    }
                }
                let mut st = DimStyle { name: name.clone(), annotative: annotative(&tg), ..DimStyle::default() };
                st.apply_fields(&fields);
                match d.dim_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = st,
                    None => d.dim_styles.push(st),
                }
            }
            _ => {}
        }
    }
}

/// Read a DXF file into a drawing.
pub fn read(bytes: &[u8]) -> Result<Drawing> {
    let tags = cadcraft_dxf::parse(bytes).map_err(|e| IoError::Format(e.to_string()))?;
    let secs = sections(&tags);
    if secs.is_empty() {
        return Err(IoError::Format("no sections found".into()));
    }
    let mut d = Drawing::new_imperial();
    d.layouts.clear();
    let mut rx = Rx::default();
    let mut blocks: Vec<(String, Vec3, Vec<(Option<String>, bool, Entity)>, bool)> = Vec::new();
    let mut layout_objs: Vec<(String, u32, String, PageSetup)> = Vec::new();
    let mut entities = Vec::new();
    let mut objs = Objects::default();
    for s in &secs {
        match s.name.as_str() {
            "HEADER" => header(&s.tags, &mut d),
            "TABLES" => tables(&s.tags, &mut d, &mut rx),
            "BLOCKS" => {
                let recs = records(&s.tags);
                let mut i = 0;
                while let Some((k, tg)) = recs.get(i) {
                    if k == "BLOCK" {
                        let t = T(tg);
                        let name = t.s(2).unwrap_or_default();
                        let base = t.p(10);
                        let xref = t.i(70).unwrap_or(0) & 4 != 0;
                        let mut j = i + 1;
                        while recs.get(j).is_some_and(|(k, _)| k != "ENDBLK") {
                            j += 1;
                        }
                        let body = recs.get(i + 1..j).unwrap_or(&[]);
                        let ents = parse_entities(body, &mut d, &mut rx);
                        blocks.push((name, base, ents, xref));
                        i = j + 1;
                    } else {
                        i += 1;
                    }
                }
            }
            "ENTITIES" => entities = parse_entities(&records(&s.tags), &mut d, &mut rx),
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
    let first_layout = d.layouts.first().map(|l| l.name.clone()).unwrap_or_else(|| "Layout1".into());
    // Blocks: model/paper space blocks hold entities in newer files.
    for (name, base, ents, xref) in blocks {
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
        // nothing inserts.
        if rx.table_blocks.contains(&up) || (crate::dxf_ext::is_standard_arrow(&name) && !rx.inserted.contains(&up)) {
            continue;
        }
        let mut b = Block::new(&name);
        b.base = base;
        if xref {
            b.xref_path = Some(String::new());
        }
        for (_, _, e) in ents {
            b.entities.push(e);
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
    objs.apply(&mut d, &rx);
    // Keep handles unique against the header's seed.
    let seed = d.header.str("HANDSEED", "");
    if let Some(h) = Handle::parse_hex(&seed) {
        d.bump_handseed(Handle(h.0.saturating_sub(1)));
    }
    if d.layer("0").is_none() {
        d.layers.insert(0, Layer::default());
    }
    Ok(d)
}

/// Non-graphical objects applied once every entity is placed.
#[derive(Default)]
struct Objects {
    /// Dictionary entry handle (upper case) → entry name.
    names: HashMap<String, String>,
    /// XRECORD handle (upper case) → its groups.
    xrecords: HashMap<String, Vec<Tag>>,
    /// TABLESTYLE objects: (handle, groups).
    table_styles: Vec<(String, Vec<Tag>)>,
    /// DIMASSOC objects.
    dimassocs: Vec<Vec<Tag>>,
}

const MAX_OBJECTS: usize = 1_000_000;

impl Objects {
    fn add(&mut self, kind: &str, tags: &[Tag]) {
        let h = T(tags).s(5).map(|h| h.trim().to_ascii_uppercase()).unwrap_or_default();
        match kind {
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
            _ => {}
        }
    }

    fn apply(self, d: &mut Drawing, rx: &Rx) {
        // Table styles (named by their ACAD_TABLESTYLE dictionary entries).
        for (h, tags) in &self.table_styles {
            let Some(name) = self.names.get(h).filter(|n| !n.is_empty()) else { continue };
            let t = T(tags);
            let def = TableStyle::default();
            let pos = |v: f64, d: f64| if v.is_finite() && v > 0.0 { v } else { d };
            let st = TableStyle {
                name: name.clone(),
                text_height: pos(t.fd(140, def.text_height), def.text_height),
                margin: t.f(40).filter(|m| m.is_finite() && *m >= 0.0).unwrap_or(def.margin),
                title: t.i(280).unwrap_or(0) == 0,
                header: t.i(281).unwrap_or(0) == 0,
            };
            match d.table_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name)) {
                Some(x) => *x = st,
                None => d.table_styles.push(st),
            }
        }
        for (h, sh) in &rx.table_style_fix {
            if let Some(name) = self.names.get(sh) {
                let _ = d.modify_entity(*h, |e| {
                    if let EntityKind::Table(t) = &mut e.kind {
                        t.style = name.clone();
                    }
                });
            }
        }
        // Parametric constraints and parameters.
        let key = self.names.iter().find(|(_, n)| n.as_str() == crate::dxf_ext::CONSTRAINTS_KEY).map(|(h, _)| h.clone());
        if let Some(tags) = key.and_then(|k| self.xrecords.get(&k)) {
            let mut text = String::new();
            for t in tags.iter().filter(|t| t.code == 1 || t.code == 3) {
                if text.len() > crate::dxf_ext::MAX_PAYLOAD {
                    break;
                }
                text.push_str(&t.str());
            }
            if let Some((c, p)) = crate::dxf_ext::parse_constraints(&text) {
                d.constraints = c;
                d.parametric = p;
            }
        }
        // Standard associativity, for dimensions without CADCraft's exact links.
        for tags in &self.dimassocs {
            dimassoc(d, tags);
        }
    }
}

/// Associativity from a DIMASSOC object (files from other writers).
fn dimassoc(d: &mut Drawing, tags: &[Tag]) {
    let start = tags.iter().position(|x| x.code == 100 && x.str() == "AcDbDimAssoc").unwrap_or(0);
    let tail = tags.get(start..).unwrap_or(&[]);
    let Some(dim_h) = tail.iter().find(|x| x.code == 330).and_then(|x| Handle::parse_hex(&x.str())) else { return };
    let Some(EntityKind::Dimension(dm)) = d.entity(dim_h).map(|e| &e.kind) else { return };
    if !dm.assoc.is_empty() {
        return;
    }
    let slots: &[&str] = match dm.kind {
        DimKind::Linear { .. } | DimKind::Aligned => &["p13", "p14"],
        DimKind::Ordinate { .. } => &["p13"],
        _ => return,
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
        if let Some(snap) = infer_snap(d, r.i(72).unwrap_or(0), main, other, at) {
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

/// A CADCraft snap from a DIMASSOC object snap type and point.
fn infer_snap(d: &Drawing, osnap: i64, main: Handle, other: Option<Handle>, at: Vec2) -> Option<AssocSnap> {
    let e = d.entity(main)?;
    Some(match osnap {
        1 | 13 => match &e.kind {
            EntityKind::Line(l) => {
                if osnap == 13 || l.a.xy().dist(at) <= l.b.xy().dist(at) {
                    AssocSnap::Start
                } else {
                    AssocSnap::End
                }
            }
            EntityKind::Arc(a) => {
                let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
                if osnap == 13 || g.start_point().dist(at) <= g.end_point().dist(at) { AssocSnap::Start } else { AssocSnap::End }
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
    p
}
