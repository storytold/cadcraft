use super::common::{T, common, ocs};
use super::hatch::hatch;
use super::text::{attdef, attrib_modes, attrib_props, mtext_from, multiline_attrib, split_embedded, text_from};
use super::{
    Arc, Attrib, Circle, Common, DimKind, Dimension, Ellipse, EntityKind, Face3d, Handle, HashMap, Helix, HelixConstrain, Insert, Leader, Line,
    LwPolyline, MAX_FRAME_CHARS, Point, PolyVertex, Polyline3d, RawTag, RayLine, Rx, Solid, Spline, Table, TableCell, Tag, Tolerance, Unknown, Vec2,
    Vec3, Viewport, Wipeout, records,
};

/// Parse one entity record. POLYLINE/INSERT followers are handled by the caller.
pub(super) fn entity(kind: &str, tags: &[Tag]) -> Option<(Common, EntityKind)> {
    entity_with_table_budget(kind, tags, &mut 4_000_000)
}

/// Parse one entity while charging materialized table cells to the caller's shared import budget.
/// An ARC, with its angles run the other way in a mirrored OCS.
fn arc(c: &Common, t: &T) -> Arc {
    let mut a = Arc { center: ocs(c, t.p(10)), radius: t.fd(40, 1.0).abs(), start: t.fd(50, 0.0).to_radians(), end: t.fd(51, 360.0).to_radians() };
    if c.extrusion.z < 0.0 {
        // Mirrored OCS: angles run the other way.
        let (s, e) = (std::f64::consts::PI - a.end, std::f64::consts::PI - a.start);
        a.start = cadcraft_geom::norm_angle(s);
        a.end = cadcraft_geom::norm_angle(e);
    }
    a
}

fn ellipse(c: &Common, t: &T) -> Ellipse {
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
    e
}

/// A LWPOLYLINE, or the region CADCraft wrote as one.
fn lwpolyline(tags: &[Tag], c: &Common, t: &T) -> Option<EntityKind> {
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
    let pl = LwPolyline { vertices: verts, closed: flags & 1 != 0, const_width: t.fd(43, 0.0), elevation: t.fd(38, 0.0), plinegen: flags & 128 != 0 };
    // A region CADCraft wrote as polylines (`dxf_region`).
    crate::dxf_region::from_polyline(tags, pl)
}

fn spline(t: &T) -> Spline {
    let degree = t.i(71).unwrap_or(3).clamp(1, 10) as usize;
    let knots = t.all_f(40);
    let control = t.pts(10);
    let weights = t.all_f(41);
    let fit = t.pts(11);
    let tangent = |c: i32| t.f(c).map(|_| t.p(c).xy());
    let fit_opts =
        cadcraft_geom::FitOptions { knots: Default::default(), start_tangent: tangent(12), end_tangent: tangent(13), tolerance: t.fd(44, 0.0) }
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
    sp
}

pub(super) fn entity_with_table_budget(kind: &str, tags: &[Tag], table_cells_left: &mut usize) -> Option<(Common, EntityKind)> {
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
        "ARC" => EntityKind::Arc(arc(&c, &t)),
        "ELLIPSE" => EntityKind::Ellipse(ellipse(&c, &t)),
        "LWPOLYLINE" => lwpolyline(tags, &c, &t)?,
        "SPLINE" => EntityKind::Spline(spline(&t)),
        "HELIX" => EntityKind::Helix(helix(tags)),
        "RAY" => EntityKind::Ray(RayLine { base: t.p(10), dir: t.p(11) }),
        "XLINE" => EntityKind::XLine(RayLine { base: t.p(10), dir: t.p(11) }),
        "TEXT" => EntityKind::Text(text_from(&t)),
        "MTEXT" => EntityKind::MText(mtext_from(tags, &c)),
        "ATTDEF" => EntityKind::AttDef(attdef(tags, &c)),
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
            // Each group 10 starts a vertex. Scope Y/Z to it so an omitted Z cannot
            // shift a later vertex's elevation onto this one.
            vertices: t
                .all_f(10)
                .into_iter()
                .zip(tags.split(|tag| tag.code == 10).skip(1))
                .map(|(x, vertex)| {
                    let v = T(vertex);
                    Vec3::new(x, v.fd(20, 0.0), v.fd(30, 0.0))
                })
                .collect(),
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
pub(super) fn acad_table(tags: &[Tag], table_cells_left: &mut usize) -> Table {
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

/// The plot style names of a file: the entries of the `ACAD_PLOTSTYLENAME` dictionary of the
/// named object dictionary (the first object), by placeholder handle.
pub(super) fn plot_style_names(secs: &[cadcraft_dxf::Section]) -> HashMap<String, String> {
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
pub(super) fn entity_plot_style(tags: &[Tag], rx: &Rx) -> Option<String> {
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
pub(super) fn entity_extras(kind: &str, tags: &[Tag], h: Handle, k: &mut EntityKind, rx: &mut Rx) {
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
/// The entity of a POLYLINE, with its mesh faces (advancing `i` past its VERTEX records). `None`
/// for a mesh without faces.
pub(super) fn polyline_entity(recs: &[(String, Vec<Tag>)], i: &mut usize, t: &T) -> Option<(Common, EntityKind, Vec<EntityKind>)> {
    let flags = t.i(70).unwrap_or(0);
    let mut pts: Vec<(Vec3, f64, f64, f64)> = Vec::new();
    // Polyface face records (VERTEX 70 bit 128 without 64): vertex indices 71–74.
    let mut faces: Vec<[i64; 4]> = Vec::new();
    while let Some((k2, t2)) = recs.get(*i) {
        if k2 == "VERTEX" {
            let tt = T(t2);
            let vflags = tt.i(70).unwrap_or(0);
            if vflags & 128 != 0 && vflags & 64 == 0 {
                faces.push([71, 72, 73, 74].map(|c| tt.i(c).unwrap_or(0)));
            } else if vflags & 16 == 0 {
                pts.push((tt.p(10), tt.fd(42, 0.0), tt.fd(40, 0.0), tt.fd(41, 0.0)));
            }
            *i += 1;
        } else {
            if k2 == "SEQEND" {
                *i += 1;
            }
            break;
        }
    }
    let c = common(t);
    let mut extra: Vec<EntityKind> = Vec::new();
    let k = if flags & (16 | 64) != 0 {
        // Polyface (64) and polygon (16) meshes become 3D faces; the first takes the
        // polyline's place.
        let verts: Vec<Vec3> = pts.iter().map(|p| p.0).collect();
        let mut fs = if flags & 64 != 0 { polyface_faces(&verts, &faces) } else { mesh_faces(&verts, t, flags) }.into_iter().map(EntityKind::Face3d);
        let first = fs.next()?;
        extra.extend(fs);
        first
    } else if flags & 8 != 0 {
        EntityKind::Polyline3d(Polyline3d { points: pts.iter().map(|p| p.0).collect(), closed: flags & 1 != 0 })
    } else {
        // A 2D polyline's vertices are in its OCS: mirrored like LWPOLYLINE.
        let m = if c.extrusion.z < 0.0 { -1.0 } else { 1.0 };
        EntityKind::LwPolyline(LwPolyline {
            vertices: pts.iter().map(|p| PolyVertex { p: Vec2::new(m * p.0.x, p.0.y), bulge: m * p.1, start_width: p.2, end_width: p.3 }).collect(),
            closed: flags & 1 != 0,
            const_width: t.fd(40, 0.0).min(t.fd(41, 0.0)).max(0.0),
            elevation: t.p(10).z,
            plinegen: flags & 128 != 0,
        })
    };
    Some((c, k, extra))
}

/// The ATTRIB records after an INSERT, advancing `i` past them and their SEQEND.
pub(super) fn insert_attribs(recs: &[(String, Vec<Tag>)], i: &mut usize, ins_layer: &str) -> Vec<Attrib> {
    let mut attribs = Vec::new();
    while let Some((k2, t2)) = recs.get(*i) {
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
                constant: own.i(70).unwrap_or(0) & 2 != 0,
                prompt: String::new(),
                props: attrib_props(&tt, ins_layer),
                multiline,
                modes: attrib_modes(own.0),
            });
            *i += 1;
        } else {
            if k2 == "SEQEND" {
                *i += 1;
            }
            break;
        }
    }
    attribs
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
