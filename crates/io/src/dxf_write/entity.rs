use super::blocks::table_size;
use super::common::{attrib_common, common, common_x, dim_xdata, layer_and_props, multiline_tags, oblique_52, text_tags};
use super::ctx::Ctx;
use super::layout::paper_view;
use super::{
    Bounds2, Color, DimKind, Drawing, Entity, EntityKind, Gradient, Handle, HashMap, Hatch, Leader, MLeader, Point, Vec2, Vec3, Version, W, dxf_ext,
};

/// The persistent reactors of boundary objects: each source object of an associative hatch in
/// the same space or block → the hatches it bounds (AutoCAD links the two both ways; without the
/// reactor the hatch no longer follows its boundary there).
pub(super) fn hatch_reactors(d: &Drawing) -> HashMap<Handle, Vec<String>> {
    let mut out: HashMap<Handle, Vec<String>> = HashMap::new();
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for st in stores {
        for e in st.iter() {
            let EntityKind::Hatch(h) = &e.kind else { continue };
            if !h.associative {
                continue;
            }
            for s in h.loops.iter().flat_map(|l| &l.sources) {
                if *s != e.handle && st.contains(*s) {
                    let r = out.entry(*s).or_default();
                    let own = e.handle.hex();
                    if !r.contains(&own) {
                        r.push(own);
                    }
                }
            }
        }
    }
    out
}

pub(super) fn entity(w: &mut W, d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    write_basic_entity(w, d, e, owner, paper, cx);
    write_text_entity(w, d, e, owner, paper, cx);
    write_insert_entity(w, d, e, owner, paper, cx);
    write_dimension_entity(w, d, e, owner, paper, cx);
    write_table_entity(w, d, e, owner, paper, cx);
    write_annotations_entity(w, d, e, owner, paper, cx);
    write_hatch_entity(w, d, e, owner, paper, cx);
    write_viewport_entity(w, d, e, owner, paper, cx);
    write_wipeout_entity(w, d, e, owner, paper, cx);
    write_other_entity(w, d, e, owner, paper, cx);
}

fn write_basic_entity(w: &mut W, _d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    match &e.kind {
        EntityKind::Line(l) => {
            w.s(0, "LINE");
            common(w, e, owner, paper, "AcDbLine");
            thickness(w, e);
            w.p(10, l.a);
            w.p(11, l.b);
        }
        EntityKind::Point(p) => {
            w.s(0, "POINT");
            common(w, e, owner, paper, "AcDbPoint");
            w.p(10, p.p);
            thickness(w, e);
            if p.angle != 0.0 {
                w.f(50, p.angle.to_degrees());
            }
        }
        EntityKind::Circle(c) => {
            w.s(0, "CIRCLE");
            common(w, e, owner, paper, "AcDbCircle");
            thickness(w, e);
            w.p(10, c.center);
            w.f(40, c.radius);
        }
        EntityKind::Arc(a) => {
            w.s(0, "ARC");
            common(w, e, owner, paper, "AcDbCircle");
            thickness(w, e);
            w.p(10, a.center);
            w.f(40, a.radius);
            w.s(100, "AcDbArc");
            w.f(50, a.start.to_degrees());
            w.f(51, a.end.to_degrees());
        }
        EntityKind::Ellipse(el) => {
            w.s(0, "ELLIPSE");
            common(w, e, owner, paper, "AcDbEllipse");
            w.p(10, el.center);
            w.p(11, el.major);
            w.f(40, el.ratio);
            w.f(41, el.start);
            w.f(42, el.end);
        }
        EntityKind::LwPolyline(p) => {
            w.s(0, "LWPOLYLINE");
            common(w, e, owner, paper, "AcDbPolyline");
            w.i(90, p.vertices.len() as i64);
            w.i(70, i64::from(p.closed) | if p.plinegen { 128 } else { 0 });
            if p.const_width > 0.0 {
                w.f(43, p.const_width);
            }
            if p.elevation != 0.0 {
                w.f(38, p.elevation);
            }
            thickness(w, e);
            for v in &p.vertices {
                w.p2(10, v.p);
                if v.start_width != 0.0 || v.end_width != 0.0 {
                    w.f(40, v.start_width);
                    w.f(41, v.end_width);
                }
                if v.bulge != 0.0 {
                    w.f(42, v.bulge);
                }
            }
        }
        EntityKind::MLine(m) => {
            w.s(0, "MLINE");
            common(w, e, owner, paper, "AcDbMline");
            w.t.extend(crate::dxf_mline::entity_tags(m, &cx.mlines.handle(&m.style)));
        }
        EntityKind::Polyline3d(p) => {
            w.s(0, "POLYLINE");
            common(w, e, owner, paper, "AcDb3dPolyline");
            w.i(66, 1);
            w.p(10, Vec3::ZERO);
            thickness(w, e);
            w.i(70, 8 | i64::from(p.closed));
            for q in &p.points {
                let vh = w.h();
                w.s(0, "VERTEX");
                w.s(5, vh);
                w.s(330, e.handle.hex());
                w.s(100, "AcDbEntity");
                w.s(8, &e.common.layer);
                w.s(100, "AcDbVertex");
                w.s(100, "AcDb3dPolylineVertex");
                w.p(10, *q);
                w.i(70, 32);
            }
            let sh = w.h();
            w.s(0, "SEQEND");
            w.s(5, sh);
            w.s(330, e.handle.hex());
            w.s(100, "AcDbEntity");
            w.s(8, &e.common.layer);
        }
        EntityKind::Spline(s) => {
            w.s(0, "SPLINE");
            common(w, e, owner, paper, "AcDbSpline");
            let flags = 8 | i64::from(s.closed) | if s.is_periodic() { 2 } else { 0 } | if s.weights.is_empty() { 0 } else { 4 };
            w.i(70, flags);
            w.i(71, s.degree as i64);
            w.i(72, s.knots.len() as i64);
            w.i(73, s.control.len() as i64);
            w.i(74, s.fit.len() as i64);
            w.f(42, 1e-10);
            w.f(43, 1e-10);
            // Fit data: tolerance and end tangents (the knot parametrisation is implied by the knots).
            if !s.fit.is_empty() {
                w.f(44, s.fit_opts.tolerance);
                if let Some(t) = s.fit_opts.start_tangent {
                    w.p(12, t.to3(0.0));
                }
                if let Some(t) = s.fit_opts.end_tangent {
                    w.p(13, t.to3(0.0));
                }
            }
            for k in &s.knots {
                w.f(40, *k);
            }
            for (i, c) in s.control.iter().enumerate() {
                w.p(10, c.to3(0.0));
                if let Some(wt) = s.weights.get(i) {
                    w.f(41, *wt);
                }
            }
            for f in &s.fit {
                w.p(11, f.to3(0.0));
            }
        }
        EntityKind::Helix(h) => {
            // A cubic spline through points on the helix, then the helix data (DXF Reference).
            w.s(0, "HELIX");
            common(w, e, owner, paper, "AcDbSpline");
            let (s, z) = h.spline3();
            w.i(70, 0);
            w.i(71, s.degree as i64);
            w.i(72, s.knots.len() as i64);
            w.i(73, s.control.len() as i64);
            w.i(74, 0);
            w.f(42, 1e-10);
            w.f(43, 1e-10);
            for k in &s.knots {
                w.f(40, *k);
            }
            for (c, z) in s.control.iter().zip(&z) {
                w.p(10, c.to3(*z));
            }
            w.s(100, "AcDbHelix");
            w.i(90, 29);
            w.i(91, 63);
            w.p(10, h.center);
            w.p(11, h.start_point());
            w.p(12, Vec3::Z);
            w.f(40, h.base_radius);
            w.f(41, h.turns);
            w.f(42, h.turn_height());
            w.i(290, i64::from(h.ccw));
            w.i(280, h.constrain.to_dxf());
        }
        EntityKind::Ray(r) | EntityKind::XLine(r) => {
            let ray = matches!(e.kind, EntityKind::Ray(_));
            w.s(0, if ray { "RAY" } else { "XLINE" });
            common(w, e, owner, paper, if ray { "AcDbRay" } else { "AcDbXline" });
            w.p(10, r.base);
            w.p(11, r.dir);
        }
        _ => {}
    }
}

fn write_text_entity(w: &mut W, d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    match &e.kind {
        EntityKind::Text(t) => {
            w.s(0, "TEXT");
            common(w, e, owner, paper, "AcDbText");
            thickness(w, e);
            text_tags(w, d, t, None, None);
        }
        // Closed polylines, one per loop (`dxf_region`): REGION's own data is ACIS.
        EntityKind::Region(r) => {
            for (i, tags) in crate::dxf_region::polylines(r).into_iter().enumerate() {
                w.s(0, "LWPOLYLINE");
                if i == 0 {
                    common(w, e, owner, paper, "AcDbPolyline");
                } else {
                    // The other loops get handles of their own (only the handle and properties are written).
                    let h = Handle::parse_hex(&w.h()).unwrap_or(e.handle);
                    let part = Entity { handle: h, common: e.common.clone(), kind: EntityKind::Point(Point { p: Vec3::ZERO, angle: 0.0 }) };
                    common(w, &part, owner, paper, "AcDbPolyline");
                }
                w.t.extend(tags);
            }
        }
        EntityKind::AttDef(a) => {
            w.s(0, "ATTDEF");
            common(w, e, owner, paper, "AcDbText");
            text_tags(w, d, &a.text, Some((a, cx.target)), Some(&a.prompt));
            if cx.target >= Version::R2007 {
                multiline_tags(w, a, true);
            }
        }
        EntityKind::MText(t) => {
            w.s(0, "MTEXT");
            common(w, e, owner, paper, "AcDbMText");
            w.p(10, t.insert);
            w.f(40, t.height);
            w.f(41, t.width);
            w.i(71, i64::from(t.attach));
            w.i(72, 1);
            // Long contents go in 250-char 3-chunks, the remainder in 1.
            let chars: Vec<char> = crate::dxf_read::mtext_dxf::encode_contents(&t.contents).chars().collect();
            let chunks: Vec<String> = chars.chunks(250).map(|c| c.iter().collect()).collect();
            let n = chunks.len();
            for (i, c) in chunks.iter().enumerate() {
                w.s(if i + 1 == n { 1 } else { 3 }, c.clone());
            }
            if n == 0 {
                w.s(1, "");
            }
            w.s(7, &t.style);
            if t.rotation != 0.0 {
                w.f(50, t.rotation.to_degrees());
            }
            // Line spacing style: 1 = at least, 2 = exactly.
            w.i(73, if t.line_spacing_exact { 2 } else { 1 });
            w.f(44, t.line_spacing);
            w.t.extend(crate::dxf_read::mtext_dxf::column_tags(t));
        }
        _ => {}
    }
}

fn write_insert_entity(w: &mut W, d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    if let EntityKind::Insert(i) = &e.kind {
        w.s(0, "INSERT");
        common(w, e, owner, paper, "AcDbBlockReference");
        if !i.attribs.is_empty() {
            w.i(66, 1);
        }
        w.s(2, &i.block);
        w.p(10, i.insert);
        w.f(41, i.scale.x);
        w.f(42, i.scale.y);
        w.f(43, i.scale.z);
        w.f(50, i.rotation.to_degrees());
        if i.cols > 1 || i.rows > 1 {
            w.i(70, i64::from(i.cols));
            w.i(71, i64::from(i.rows));
            w.f(44, i.col_spacing);
            w.f(45, i.row_spacing);
        }
        if !i.attribs.is_empty() {
            for a in &i.attribs {
                let ah = w.h();
                w.s(0, "ATTRIB");
                w.s(5, ah);
                w.s(330, e.handle.hex());
                w.s(100, "AcDbEntity");
                let (c, base) = attrib_common(d, e, i, a);
                layer_and_props(w, &c, Some(&base));
                // Hidden with its definition (a dynamic block's visibility state hides it).
                if !base.visible {
                    w.i(60, 1);
                }
                w.s(100, "AcDbText");
                text_tags(w, d, &a.text, Some((a, cx.target)), None);
                if cx.target >= Version::R2007 {
                    multiline_tags(w, a, false);
                }
            }
            let sh = w.h();
            w.s(0, "SEQEND");
            w.s(5, sh);
            w.s(330, e.handle.hex());
            w.s(100, "AcDbEntity");
            w.s(8, &e.common.layer);
        }
    }
}

fn write_dimension_entity(w: &mut W, d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    if let EntityKind::Dimension(dm) = &e.kind {
        w.s(0, "DIMENSION");
        common_x(w, e, owner, paper, "AcDbDimension", cx.assoc.get(&e.handle));
        let bname = cx.dim_blocks.get(&e.handle).cloned().or_else(|| dm.block.clone()).unwrap_or_default();
        w.s(2, bname);
        w.p(10, dm.defpt);
        let style = d.dim_style(&dm.style).cloned().unwrap_or_default();
        let g = cadcraft_render::dimension_geometry(dm, &style, d.header.f64("DIMSCALE", 1.0));
        w.p(11, if dm.user_text_pos { dm.text_mid } else { g.text_pos.to3(0.0) });
        let ty: i64 = match dm.kind {
            DimKind::Linear { .. } => 0,
            DimKind::Aligned => 1,
            DimKind::Angular => 2,
            DimKind::Diameter => 3,
            DimKind::Radius => 4,
            DimKind::Angular3P => 5,
            DimKind::Ordinate { x_type } => 6 | if x_type { 64 } else { 0 },
            // Written as aligned, marked by CADCraft xdata (see `dim_xdata`).
            DimKind::ArcLength => 1,
        };
        w.i(70, ty | 32 | if dm.user_text_pos { 128 } else { 0 });
        // The actual measurement (read-only in DXF): AutoLISP's entget shows it.
        w.f(42, g.measurement);
        if !dm.text.is_empty() {
            w.s(1, &dm.text);
        }
        w.s(3, &dm.style);
        if dm.text_rotation != 0.0 {
            w.f(53, dm.text_rotation.to_degrees());
        }
        match dm.kind {
            DimKind::Linear { rotation } => {
                w.s(100, "AcDbAlignedDimension");
                w.p(13, dm.p13);
                w.p(14, dm.p14);
                w.f(50, rotation.to_degrees());
                oblique_52(w, dm, rotation);
                w.s(100, "AcDbRotatedDimension");
            }
            DimKind::Aligned | DimKind::ArcLength => {
                w.s(100, "AcDbAlignedDimension");
                w.p(13, dm.p13);
                w.p(14, dm.p14);
                if matches!(dm.kind, DimKind::Aligned) {
                    oblique_52(w, dm, (dm.p14.xy() - dm.p13.xy()).angle());
                }
            }
            DimKind::Radius => {
                w.s(100, "AcDbRadialDimension");
                w.p(15, dm.p15);
                w.f(40, 0.0);
            }
            DimKind::Diameter => {
                w.s(100, "AcDbDiametricDimension");
                w.p(15, dm.p15);
                w.f(40, 0.0);
            }
            DimKind::Angular => {
                w.s(100, "AcDb2LineAngularDimension");
                w.p(13, dm.p13);
                w.p(14, dm.p14);
                w.p(15, dm.p15);
                w.p(16, dm.p16);
            }
            DimKind::Angular3P => {
                w.s(100, "AcDb3PointAngularDimension");
                w.p(13, dm.p13);
                w.p(14, dm.p14);
                w.p(15, dm.p15);
            }
            DimKind::Ordinate { .. } => {
                w.s(100, "AcDbOrdinateDimension");
                w.p(13, dm.p13);
                w.p(14, dm.p14);
            }
        }
        dim_xdata(w, dm, cx);
    }
}

fn write_table_entity(w: &mut W, _d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    if let EntityKind::Table(t) = &e.kind {
        // Tables without a generated block (none expected) are not written.
        let Some((bname, brh)) = cx.tables.get(&e.handle) else { return };
        w.s(0, "ACAD_TABLE");
        common(w, e, owner, paper, "AcDbBlockReference");
        w.s(2, bname);
        w.p(10, t.insert);
        w.s(100, "AcDbTable");
        w.i(280, 0);
        w.s(342, cx.table_style(&t.style));
        w.s(343, brh);
        w.p(11, Vec3::new(1.0, 0.0, 0.0));
        w.i(90, 0);
        let (rows, cols) = table_size(t);
        w.i(91, rows as i64);
        w.i(92, cols as i64);
        for c in [93, 94, 95, 96] {
            w.i(c, 0);
        }
        for h in t.row_heights.iter().take(rows) {
            w.f(141, *h);
        }
        for c in t.col_widths.iter().take(cols) {
            w.f(142, *c);
        }
        let cov = cadcraft_render::table_covered(t);
        let span = |v: u32| i64::from(v.min(i16::MAX as u32));
        for r in 0..rows {
            for c in 0..cols {
                let cell = t.cells.get(r).and_then(|x| x.get(c));
                let covered = cov.get(r).and_then(|x| x.get(c)).copied().unwrap_or(false);
                let merge = cell.and_then(|x| x.merged).filter(|&(a, b)| a >= 1 && b >= 1 && (a, b) != (1, 1));
                w.i(171, 1);
                w.i(172, 0);
                w.i(173, i64::from(covered || merge.is_some()));
                w.i(174, 0);
                let (rs, cs) = if covered { (0, 0) } else { merge.map(|(a, b)| (span(a), span(b))).unwrap_or((1, 1)) };
                w.i(175, cs);
                w.i(176, rs);
                w.i(91, 0);
                w.i(178, 0);
                w.f(145, 0.0);
                if let Some(h) = cell.and_then(|x| x.text_height).filter(|h| h.is_finite() && *h > 0.0) {
                    w.f(140, h);
                }
                // Long text: 250-character code-2 chunks, the rest in code 1.
                let chars: Vec<char> = cell.map(|x| x.text.chars().collect()).unwrap_or_default();
                let chunks: Vec<String> = chars.chunks(250).map(|c| c.iter().collect()).collect();
                let n = chunks.len();
                for (i, ch) in chunks.into_iter().enumerate() {
                    w.s(if i + 1 == n { 1 } else { 2 }, ch);
                }
                if n == 0 {
                    w.s(1, "");
                }
            }
        }
        w.s(1001, dxf_ext::APP);
        w.s(1000, "TABLE");
        w.i(1070, i64::from(t.title));
        w.i(1070, i64::from(t.header));
        w.f(1040, t.text_height);
        w.s(1000, &t.style);
        for c in crate::dxf_table::table_xdata(t) {
            w.s(1000, c);
        }
    }
}

fn write_annotations_entity(w: &mut W, d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    match &e.kind {
        EntityKind::MLeader(m) if !cx.native_mleader() => mleader_fallback(w, d, e, m, owner, paper, cx),
        EntityKind::MLeader(m) => {
            w.s(0, "MULTILEADER");
            common(w, e, owner, paper, "AcDbMLeader");
            let refs = crate::dxf_mleader::Refs {
                style: cx.mleader_style(&m.style),
                text_style: m.text.as_ref().and_then(|t| cx.style(&t.style)),
                block: m.block.as_ref().and_then(|b| cx.block_records.get(&b.block.to_ascii_uppercase()).cloned()),
            };
            w.t.extend(crate::dxf_mleader::entity_tags(d, m, &refs));
        }
        EntityKind::Leader(l) => {
            w.s(0, "LEADER");
            common(w, e, owner, paper, "AcDbLeader");
            w.s(3, &l.style);
            w.i(71, i64::from(l.arrow));
            w.i(72, i64::from(l.spline));
            w.i(76, l.vertices.len() as i64);
            for v in &l.vertices {
                w.p(10, *v);
            }
        }
        EntityKind::Image(i) => {
            // Every image has its IMAGEDEF (planned in `write`); the fallback never happens.
            let Some((def, reactor)) = cx.images.by_entity.get(&e.handle) else { return };
            w.s(0, "IMAGE");
            common(w, e, owner, paper, "AcDbRasterImage");
            w.t.extend(crate::dxf_image::entity_tags(i, def, reactor));
        }
        EntityKind::Solid(s) | EntityKind::Trace(s) => {
            let solid = matches!(e.kind, EntityKind::Solid(_));
            w.s(0, if solid { "SOLID" } else { "TRACE" });
            common(w, e, owner, paper, "AcDbTrace");
            w.p(10, s.corners[0]);
            w.p(11, s.corners[1]);
            w.p(12, s.corners[2]);
            w.p(13, s.corners[3]);
            thickness(w, e);
        }
        EntityKind::Face3d(f) => {
            w.s(0, "3DFACE");
            common(w, e, owner, paper, "AcDbFace");
            w.p(10, f.corners[0]);
            w.p(11, f.corners[1]);
            w.p(12, f.corners[2]);
            w.p(13, f.corners[3]);
            w.i(70, i64::from(f.hidden_edges));
        }
        _ => {}
    }
}

fn write_hatch_entity(w: &mut W, d: &Drawing, e: &Entity, owner: &str, paper: bool, _cx: &Ctx) {
    if let EntityKind::Hatch(h) = &e.kind {
        w.s(0, "HATCH");
        common(w, e, owner, paper, "AcDbHatch");
        w.p(10, Vec3::new(0.0, 0.0, h.elevation));
        w.p(210, Vec3::Z);
        w.s(2, &h.pattern);
        w.i(70, i64::from(h.solid));
        w.i(71, i64::from(h.associative));
        w.i(91, h.loops.len() as i64);
        for l in &h.loops {
            w.i(92, 2 | if l.outer { 1 } else { 0 });
            let has_bulge = l.vertices.iter().any(|v| v.bulge != 0.0);
            w.i(72, i64::from(has_bulge));
            w.i(73, 1);
            w.i(93, l.vertices.len() as i64);
            for v in &l.vertices {
                w.p2(10, v.p);
                if has_bulge {
                    w.f(42, v.bulge);
                }
            }
            // Source boundary objects of an associative hatch (the ones still beside it, in
            // its space or block, which carry the hatch as a reactor).
            let own = e.handle.hex();
            let sources: Vec<String> =
                l.sources.iter().filter(|s| h.associative && w.reactors.get(*s).is_some_and(|r| r.contains(&own))).map(|s| s.hex()).collect();
            w.i(97, sources.len() as i64);
            for s in &sources {
                w.s(330, s);
            }
        }
        w.i(75, i64::from(h.style));
        w.i(76, 1);
        if !h.solid {
            w.f(52, h.angle.to_degrees());
            w.f(41, h.scale);
            w.i(77, 0);
            // The lines read from the file, else the library's pattern of that name.
            let lines = if h.pattern_lines.is_empty() {
                cadcraft_doc::library::pattern_for(d, &h.pattern).map(|p| p.lines).unwrap_or_default()
            } else {
                h.pattern_lines.clone()
            };
            w.i(78, lines.len() as i64);
            for pl in lines {
                let ang = pl.angle.to_radians() + h.angle;
                w.f(53, ang.to_degrees());
                let o = h.origin + Vec2::new(pl.origin.0, pl.origin.1).rotate(h.angle) * h.scale;
                w.f(43, o.x);
                w.f(44, o.y);
                let dl = Vec2::new(pl.delta.0, pl.delta.1).rotate(ang) * h.scale;
                w.f(45, dl.x);
                w.f(46, dl.y);
                w.i(79, pl.dashes.len() as i64);
                for dsh in pl.dashes {
                    w.f(49, dsh * h.scale);
                }
            }
        }
        w.i(98, 0);
        if let Some(g) = &h.gradient {
            gradient(w, g);
        }
        hatch_xdata(w, h);
    }
}

fn write_viewport_entity(w: &mut W, d: &Drawing, e: &Entity, owner: &str, paper: bool, cx: &Ctx) {
    if let EntityKind::Viewport(v) = &e.kind {
        paper_view(w, d, e, owner, cx);
        w.s(0, "VIEWPORT");
        common(w, e, owner, paper, "AcDbViewport");
        w.p(10, v.center);
        w.f(40, v.width);
        w.f(41, v.height);
        // Status 68: a positive value is "on" (the stacking order; 1 is the active viewport), 0 off.
        w.i(68, if v.on { i64::from(v.id.clamp(1, 32767)) } else { 0 });
        w.i(69, i64::from(v.id));
        w.p2(12, v.view_center);
        w.f(45, v.view_height);
        // Status flags: 16384 = display locked, 32768 = always set, 65536 = clipped by the
        // 340 entity, 131072 = off.
        let clip = v.clip.filter(|h| *h != e.handle && d.entity(*h).is_some());
        let flags = 32768 | if v.locked { 16384 } else { 0 } | if clip.is_some() { 65536 } else { 0 } | if v.on { 0 } else { 131072 };
        w.i(90, flags);
        if let Some(h) = clip {
            w.s(340, h.hex());
        }
        // AutoCAD's frozen-layer list: one soft pointer per layer, to its LAYER record. Other
        // programs read only this; CADCraft prefers its own xdata below when present.
        for name in &v.frozen_layers {
            if let Some(h) = w.layers.get(&name.to_ascii_uppercase()).cloned() {
                w.s(341, h);
            }
        }
        // One CADCraft xdata group holds both lists: readers stop at the first group of an app.
        if !v.frozen_layers.is_empty() || !v.layer_colors.is_empty() {
            w.s(1001, dxf_ext::APP);
            if !v.frozen_layers.is_empty() {
                w.xdata(dxf_ext::frozen_xdata(&v.frozen_layers));
            }
            if !v.layer_colors.is_empty() {
                w.xdata(dxf_ext::layer_colors_xdata(&v.layer_colors));
            }
        }
    }
}

fn write_wipeout_entity(w: &mut W, _d: &Drawing, e: &Entity, owner: &str, paper: bool, _cx: &Ctx) {
    if let EntityKind::Wipeout(wo) = &e.kind {
        // A 1x1 image spanning the boundary's extents. Clip vertices are in pixel space, whose origin is
        // the image's top-left corner with y pointing down: the reader maps each vertex back through
        // insert + u * (x + 0.5) + v * (0.5 - y).
        // Polygonal clip boundaries are closed, so the first vertex is repeated when needed.
        let b = Bounds2::from_points(wo.boundary.iter().copied());
        let o = if b.is_empty() { Vec2::ZERO } else { b.min };
        let (sx, sy) = (if b.width() > 0.0 { b.width() } else { 1.0 }, if b.height() > 0.0 { b.height() } else { 1.0 });
        w.s(0, "WIPEOUT");
        common(w, e, owner, paper, "AcDbWipeout");
        w.i(90, 0);
        w.p(10, Vec3::new(o.x, o.y, 0.0));
        w.p(11, Vec3::new(sx, 0.0, 0.0));
        w.p(12, Vec3::new(0.0, sy, 0.0));
        w.p2(13, Vec2::new(1.0, 1.0));
        // Display flags 7 = show image, show unaligned, use clipping boundary; clipping on; default brightness/contrast/fade.
        w.i(70, 7);
        w.i(280, 1);
        w.i(281, 50);
        w.i(282, 50);
        w.i(283, 0);
        w.i(71, 2);
        let mut pts = wo.boundary.clone();
        if let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied())
            && first != last
        {
            pts.push(first);
        }
        w.i(91, pts.len() as i64);
        for q in pts {
            w.p2(14, Vec2::new((q.x - o.x) / sx - 0.5, 0.5 - (q.y - o.y) / sy));
        }
    }
}

fn write_other_entity(w: &mut W, _d: &Drawing, e: &Entity, owner: &str, paper: bool, _cx: &Ctx) {
    match &e.kind {
        EntityKind::Tolerance(t) => {
            w.s(0, "TOLERANCE");
            common(w, e, owner, paper, "AcDbFcf");
            w.s(3, &t.style);
            w.p(10, t.insert);
            w.s(1, &t.text);
            w.p(11, t.dir);
        }
        // Entity types CADCraft doesn't model aren't written; the save command reports them.
        EntityKind::Unknown(_) => {}
        _ => {}
    }
}

/// A multileader for a file older than 2007 (no MULTILEADER): a LEADER per leader line, ending
/// at the landing, plus its MTEXT or its block's INSERT, with the multileader's properties.
fn mleader_fallback(w: &mut W, d: &Drawing, e: &Entity, m: &MLeader, owner: &str, paper: bool, cx: &Ctx) {
    let part = |w: &mut W, kind: EntityKind| Entity { handle: Handle::parse_hex(&w.h()).unwrap_or(Handle(0)), common: e.common.clone(), kind };
    let style = d.header.str("DIMSTYLE", "Standard");
    for arm in m.leaders.iter().take(64) {
        let mut vertices = arm.clone();
        vertices.push(m.landing);
        let le = part(w, EntityKind::Leader(Leader { vertices, arrow: true, spline: m.spline, style: style.clone() }));
        entity(w, d, &le, owner, paper, cx);
    }
    if let Some(t) = &m.text {
        let te = part(w, EntityKind::MText(t.clone()));
        entity(w, d, &te, owner, paper, cx);
    }
    if let Some(ins) = &m.block {
        let ie = part(w, EntityKind::Insert(ins.clone()));
        entity(w, d, &ie, owner, paper, cx);
    }
}

/// A hatch's gradient fill (DXF Reference, HATCH group codes 450–470): two-colour gradient
/// with its rotation (radians), shift (0 = centered, 1 = shifted) and per-colour 463 records
/// carrying ACI (63) and, for true colours, the RGB value (421). Those groups belong to R2004+
/// files, so the same gradient also travels as `CADCRAFT` xdata, which survives this R2000
/// file's conversion to DWG (where the native fields don't exist).
fn gradient(w: &mut W, g: &Gradient) {
    w.i(450, 1);
    w.i(451, 0);
    w.f(460, g.angle);
    w.f(461, if g.centered { 0.0 } else { 1.0 });
    w.i(452, 0);
    w.f(462, 0.0);
    w.i(453, 2);
    for (k, c) in [(0.0, g.color1), (1.0, g.color2)] {
        w.f(463, k);
        match c {
            Color::True(rgb) => {
                w.i(63, i64::from(cadcraft_color::nearest_aci(rgb)));
                w.i(421, i64::from(rgb.to_u32()));
            }
            c => w.i(63, i64::from(c.to_aci())),
        }
    }
    w.s(470, &g.name);
    w.s(1001, dxf_ext::APP);
    w.s(1000, "GRADIENT");
    // A 1000 group holds at most 255 bytes; cut at a character boundary.
    let cut = g.name.char_indices().map(|(i, c)| i + c.len_utf8()).take_while(|end| *end <= 255).last().unwrap_or(0);
    w.s(1000, g.name.get(..cut).unwrap_or_default());
    w.f(1040, g.angle);
    w.i(1070, i64::from(g.centered));
    w.s(1000, g.color1.name());
    w.s(1000, g.color2.name());
}

/// Group 39 (thickness) of the entities whose DXF Reference record has one; only when set.
fn thickness(w: &mut W, e: &Entity) {
    if e.common.thickness != 0.0 {
        w.f(39, e.common.thickness);
    }
}

/// Hatch origin and background colour as `CADCRAFT` xdata: `1000 HATCH`, the origin as a
/// world position (`1011`, moved with the hatch by other programs) and the background colour
/// name (`1000`, empty for none). The DXF Reference has no HATCH groups for either: other
/// readers get the origin through the pattern lines' base points (43/44), the background isn't
/// carried. Appended to the gradient's `CADCRAFT` xdata when there is one.
fn hatch_xdata(w: &mut W, h: &Hatch) {
    if h.origin == Vec2::ZERO && h.background.is_none() {
        return;
    }
    if h.gradient.is_none() {
        w.s(1001, dxf_ext::APP);
    }
    w.s(1000, "HATCH");
    w.p(1011, h.origin.to3(0.0));
    w.s(1000, h.background.map(Color::name).unwrap_or_default());
}
