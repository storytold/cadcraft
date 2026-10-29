use super::blocks::{insert, sub_ctx};
use super::dim_draw::{MAX_FILL_SOLIDS, MAX_FILL_TEXTS, block_text_box, dim_text_fill, dimension, dimension_in, solid_covers, starts_text_marks};
use super::display::{Kind, NO_LAYER, VIEWPORT_CONTENT};
use super::props::{resolve, transparency};
use super::text_entity::{mtext, place_text_entity};
use super::{
    Builder, Ctx, DimGeometry, Drawing, Entity, EntityKind, Handle, Ink, Mat3, Polyline, Prim, Vec2, dim, fill, hatch, linetype, linetype_text,
    mline, point, table, tolerance, wide,
};

/// The layer `e` is drawn on, as an index in the layer table: layer 0 inside a block is the
/// block reference's layer.
fn entity_layer(ctx: &Ctx, e: &Entity) -> u32 {
    let name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    ctx.d.layers.iter().position(|l| l.name.eq_ignore_ascii_case(name)).and_then(|i| u32::try_from(i).ok()).unwrap_or(NO_LAYER)
}

pub(crate) fn entity(b: &mut Builder, ctx: &Ctx, e: &Entity) {
    // Hidden by ISOLATEOBJECTS / HIDEOBJECTS (and so in viewports and plots too). A cancelled
    // build stops here (block contents come through here too).
    if ctx.d.is_hidden_object(e.handle) || b.opts.cancelled() {
        return;
    }
    // The entity's named plot style and transparency (set by `entity_parts`), for everything it
    // draws (block contents set their own).
    let (saved, saved_transparency) = (b.named, b.transparency);
    b.named = b.styles.and_then(|s| s.named_index(ctx, &e.common));
    entity_parts(b, ctx, e);
    b.named = saved;
    b.transparency = saved_transparency;
    if b.plotting && b.list.prim_layers.len() < b.list.prims.len() {
        // What it drew that its block contents didn't claim is on its own layer.
        let layer = entity_layer(ctx, e);
        let from = b.list.prim_layers.len();
        b.list.prim_layers.resize(b.list.prims.len(), layer);
        if ctx.top == VIEWPORT_CONTENT {
            // Model space seen through a viewport is clipped into the layout's list; the layer
            // travels in the handle until then (see [`take_viewport_layers`]).
            for p in b.list.prims.iter_mut().skip(from) {
                p.handle = Handle(u64::from(layer));
            }
        }
    }
}

fn entity_parts(b: &mut Builder, ctx: &Ctx, e: &Entity) {
    // A top-level annotative object draws at the view's annotation scale: a viewport's scale for
    // model space seen through it, else the drawing's (CANNOSCALE in model space, 1:1 on paper).
    if ctx.depth == 0 && cadcraft_doc::is_annotative(e) {
        let mode = if ctx.top == VIEWPORT_CONTENT {
            cadcraft_doc::anno_draw(e, 1.0 / ctx.xf.scale_factor().max(1e-12), ctx.d.anno_all_visible())
        } else {
            ctx.d.anno_draw(e)
        };
        match mode {
            cadcraft_doc::AnnoDraw::Hidden => return,
            cadcraft_doc::AnnoDraw::Scaled(f) => {
                let mut scaled = cadcraft_doc::anno_scaled(e, f);
                scaled.common.anno_scales.clear();
                return entity_parts(b, ctx, &scaled);
            }
            cadcraft_doc::AnnoDraw::Plain => {}
        }
    }
    let (rgb, lw, lt, ltscale, visible, layer_transparency) = resolve(ctx, e, b.plotting);
    if !visible {
        return;
    }
    if b.opts.transparency {
        b.transparency = transparency(e.common.transparency, layer_transparency, ctx.block_transparency);
    }
    // A plot style's linetype replaces the object's.
    let lt = match b.styles.and_then(|s| s.linetype(rgb, b.named)) {
        Some(over) => over,
        None => lt,
    };
    let tol = b.opts.tolerance / ctx.xf.scale_factor().max(1e-12);
    let lw = if b.opts.lineweights { lw } else { 0.0 };
    // Stroke a polyline with the entity's linetype.
    let stroke = |b: &mut Builder, pts: &[Vec2]| match &lt {
        Some(lt) => {
            let min = b.opts.min_dash / ctx.xf.scale_factor().max(1e-12);
            for dash in linetype::apply(pts, lt, ltscale, min) {
                if dash.len() == 1 {
                    if let Some(p) = dash.first() {
                        b.point(ctx, rgb, *p);
                    }
                } else {
                    b.polyline(ctx, rgb, lw, &dash);
                }
            }
            // Text embedded in the linetype (shapes need SHX files and are not drawn).
            if b.opts.text {
                for sh in linetype_text::texts(ctx.d, pts, lt, ltscale, min) {
                    b.shaped(ctx, rgb, lw, &sh);
                }
            }
        }
        None => b.polyline(ctx, rgb, lw, pts),
    };
    match &e.kind {
        EntityKind::Text(t) => {
            if !b.opts.text {
                return;
            }
            let (sh, _) = place_text_entity(ctx.d, t, &t.value);
            b.shaped(ctx, rgb, lw, &sh);
        }
        EntityKind::AttDef(a) => {
            let (sh, _) = place_text_entity(ctx.d, &a.text, &a.tag);
            b.shaped(ctx, rgb, lw, &sh);
        }
        EntityKind::MText(t) => {
            if !b.opts.text {
                return;
            }
            mtext(b, ctx, t, rgb, lw);
        }
        EntityKind::Insert(ins) => insert(b, ctx, e, ins, rgb),
        EntityKind::Dimension(dm) => draw_dimension(b, ctx, e, dm, lw, lt.as_ref(), ltscale),
        EntityKind::Hatch(h) => hatch::draw(b, ctx, e, h, rgb, lw, tol),
        EntityKind::MLine(m) => mline::draw(b, ctx, e, m, rgb, lw, tol),
        EntityKind::LwPolyline(p) if p.const_width > 0.0 || p.vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0) => {
            if b.opts.fill && ctx.d.header.i64("FILLMODE", 1) != 0 {
                // The GPU canvas has no MSAA: a band narrower than a pixel lights only where it
                // covers pixel centres, so whole sides vanish at some zooms (#1434). On screen the
                // centre line is drawn too, as a hairline (wide bands are drawn without lineweight),
                // so a thin band still shows as a one-pixel line. Always, not below a threshold:
                // the list serves a whole band of zooms. Plots and exports draw the band alone, and
                // a transparent band gets none (its alpha would add up along the centre).
                let hairline = !b.plotting && b.opts.min_dash > 0.0 && b.transparency == 0;
                match &lt {
                    // Dashed: each dash a filled piece of the band; dots a line across it.
                    Some(lt) => {
                        let min = b.opts.min_dash / ctx.xf.scale_factor().max(1e-12);
                        let (tris, ticks, centres) = wide::dashed(p, tol, lt, ltscale, min);
                        b.tris(ctx, rgb, &tris);
                        if hairline {
                            for c in &centres {
                                b.hairline(ctx, rgb, c);
                            }
                        }
                        for t in &ticks {
                            match t.as_slice() {
                                [q] => b.point(ctx, rgb, *q),
                                _ => b.polyline(ctx, rgb, lw, t),
                            }
                        }
                    }
                    None => {
                        b.tris(ctx, rgb, &fill::wide_polyline(p, tol));
                        if hairline {
                            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                            b.hairline(ctx, rgb, &pl.tessellate(tol));
                        }
                    }
                }
            } else {
                let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                stroke(b, &pl.tessellate(tol));
            }
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            if p.plinegen || lt.is_none() {
                stroke(b, &pl.tessellate(tol));
            } else {
                for s in pl.segments() {
                    let mut pts = Vec::new();
                    s.tessellate(tol, &mut pts);
                    stroke(b, &pts);
                }
            }
        }
        EntityKind::Solid(s) | EntityKind::Trace(s) => {
            let c = &s.corners;
            let q = [c[0].xy(), c[1].xy(), c[3].xy(), c[2].xy()];
            if b.opts.fill && ctx.d.header.i64("FILLMODE", 1) != 0 {
                b.tris(ctx, rgb, &[q[0], q[1], q[2], q[0], q[2], q[3]]);
            } else {
                b.polyline(ctx, rgb, lw, &[q[0], q[1], q[2], q[3], q[0]]);
            }
        }
        EntityKind::Wipeout(w) => draw_wipeout(b, ctx, w, rgb, lw),
        EntityKind::Image(i) => {
            let o = i.insert.xy();
            let u = i.u.xy() * i.size.x;
            let v = i.v.xy() * i.size.y;
            b.polyline(ctx, rgb, lw, &[o, o + u, o + u + v, o + v, o]);
            b.polyline(ctx, rgb, lw, &[o, o + u + v]);
            b.polyline(ctx, rgb, lw, &[o + u, o + v]);
        }
        EntityKind::Table(t) => table(b, ctx, t, rgb, lw),
        EntityKind::Tolerance(t) => tolerance::draw(b, ctx, e, t, lw, tol),
        EntityKind::Leader(l) => {
            let pts: Vec<Vec2> = l.vertices.iter().map(|v| v.xy()).collect();
            stroke(b, &pts);
            if l.arrow
                && let (Some(a), Some(n)) = (pts.first(), pts.get(1))
            {
                let st = ctx.d.dim_style(&l.style);
                // DIMASZ × DIMSCALE of the leader's style (the drawing's DIMSCALE when it is 0).
                let size = st.map(|s| s.arrow_size).unwrap_or(0.18) * leader_scale(ctx.d, st);
                // DIMLDRBLK ("" = closed filled).
                let kind = dim::Arrowhead::parse(st.map(|s| s.leader_arrow_block.as_str()).unwrap_or(""));
                let g = dim::arrowhead(kind, *a, (*a - *n).normalized(), size);
                for l in &g.lines {
                    b.polyline(ctx, rgb, lw, l);
                }
                b.tris(ctx, rgb, &g.tris);
            }
        }
        EntityKind::MLeader(m) => draw_mleader(b, ctx, e, m, rgb, lw),
        EntityKind::Point(p) => {
            // Relative sizes follow the view: undo the block / viewport scale.
            let view = b.opts.view_height / ctx.xf.scale_factor().max(1e-12);
            let size = point::size(ctx.d.header.f64("PDSIZE", 0.0), view);
            let fig = point::figure(ctx.d.header.i64("PDMODE", 0), size, p.p.xy(), p.angle, tol);
            if fig.dot {
                b.point(ctx, rgb, p.p.xy());
            }
            for l in &fig.lines {
                b.polyline(ctx, rgb, lw, l);
            }
        }
        kind => {
            for prim in kind.prims() {
                match prim {
                    Prim::Seg(s) => {
                        let mut pts = Vec::new();
                        s.tessellate(tol, &mut pts);
                        stroke(b, &pts);
                    }
                    Prim::Circle(c) => {
                        let mut pts = Vec::new();
                        c.tessellate(tol, &mut pts);
                        stroke(b, &pts);
                    }
                    Prim::Ellipse(el) => {
                        let mut pts = Vec::new();
                        el.tessellate(tol, &mut pts);
                        stroke(b, &pts);
                    }
                    Prim::Spline(s) => stroke(b, &s.tessellate(tol)),
                    Prim::Point(p) => b.point(ctx, rgb, p),
                    Prim::Infinite { base, dir, ray } => b.infinite(ctx, rgb, base, dir, ray),
                    Prim::Fill(pts) => {
                        let tris = fill::triangulate_evenodd(&[pts]);
                        b.tris(ctx, rgb, &tris);
                    }
                }
            }
        }
    }
}

fn draw_mleader(b: &mut Builder, ctx: &Ctx, e: &Entity, m: &cadcraft_doc::MLeader, rgb: Ink, lw: f32) {
    // Straight or spline leader lines; block content draws like a block reference.
    for pts in m.leader_paths() {
        b.polyline(ctx, rgb, lw, &pts);
        if let (Some(a), Some(n)) = (pts.first(), pts.get(1)) {
            b.tris(ctx, rgb, &dim::arrow(*a, (*a - *n).normalized(), m.arrow_size));
        }
    }
    if let Some(t) = &m.text {
        let land = m.landing.xy();
        let dir = if t.insert.x >= land.x { 1.0 } else { -1.0 };
        b.polyline(ctx, rgb, lw, &[land, land + Vec2::new(m.dogleg * dir, 0.0)]);
        mtext(b, ctx, t, rgb, lw);
    }
    if let Some(ins) = &m.block {
        if m.dogleg > 0.0 && !m.leaders.is_empty() {
            let land = m.landing.xy();
            let dir = if ins.insert.x >= land.x { 1.0 } else { -1.0 };
            b.polyline(ctx, rgb, lw, &[land, land + Vec2::new(m.dogleg * dir, 0.0)]);
        }
        insert(b, ctx, e, ins, rgb);
    }
}

fn draw_wipeout(b: &mut Builder, ctx: &Ctx, w: &cadcraft_doc::Wipeout, rgb: Ink, lw: f32) {
    // Hide what was drawn before it (draw order), then the frame per WIPEOUTFRAME:
    // 0 = none, 1 = shown and plotted, 2 = shown but not plotted.
    if b.opts.fill {
        let n = b.list.prims.len();
        b.tris(ctx, rgb, &fill::triangulate_evenodd(std::slice::from_ref(&w.boundary)));
        if let Some(p) = b.list.prims.get_mut(n) {
            p.kind = Kind::Mask;
        }
    }
    let frame = match ctx.d.header.i64("WIPEOUTFRAME", 1) {
        0 => false,
        2 => !b.plotting,
        _ => true,
    };
    if frame {
        let mut pts = w.boundary.clone();
        if let Some(f) = pts.first().copied() {
            pts.push(f);
        }
        b.polyline(ctx, rgb, lw, &pts);
    }
}

fn draw_dimension(b: &mut Builder, ctx: &Ctx, e: &Entity, dm: &cadcraft_doc::Dimension, lw: f32, lt: Option<&cadcraft_doc::Linetype>, ltscale: f64) {
    if let Some(blk) = dm.block.as_ref().and_then(|n| ctx.d.block(n))
        && ctx.depth < cadcraft_doc::MAX_BLOCK_DEPTH
    {
        let sub = Ctx { block_pstyle: b.named, block_transparency: b.transparency, ..sub_ctx(ctx, e, Mat3::IDENTITY) };
        // DIMTFILL: the block's MTEXT carries no background mask here, so a background
        // around each of the block's texts is drawn before the block's first text or first
        // line of the text's marks, except for a text that a SOLID of the block covers.
        let style = ctx.d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
        let g = if style.text_fill != 0 { dimension_in(ctx.d, dm) } else { DimGeometry::default() };
        let gap = style.text_gap.abs() * style.effective_scale(ctx.d.header.f64("DIMSCALE", 1.0));
        let boxes: Vec<Vec<Vec2>> = if g.text_fill.is_empty() {
            Vec::new()
        } else {
            let solids: Vec<&Entity> =
                blk.entities.iter().map(|be| &**be).filter(|be| matches!(be.kind, EntityKind::Solid(_))).take(MAX_FILL_SOLIDS).collect();
            // A text's own box (no gap): a SOLID holding all its corners is its background.
            let covered = |be: &Entity| block_text_box(ctx.d, be, 0.0).is_some_and(|t| solids.iter().any(|s| t.iter().all(|p| solid_covers(s, *p))));
            blk.entities
                .iter()
                .filter(|be| matches!(be.kind, EntityKind::MText(_) | EntityKind::Text(_)))
                .take(MAX_FILL_TEXTS)
                .filter(|be| !covered(be))
                .filter_map(|be| block_text_box(ctx.d, be, gap))
                .collect()
        };
        let mut fill_pending = !boxes.is_empty();
        for be in blk.entities.iter() {
            if !b.expand() {
                break;
            }
            if fill_pending && starts_text_marks(be, &g) {
                for corners in &boxes {
                    dim_text_fill(b, ctx, e, &style, corners);
                }
                fill_pending = false;
            }
            entity(b, &sub, be);
        }
    } else {
        dimension(b, ctx, e, dm, lw, lt, ltscale);
    }
}

/// Draw a polyline with a linetype (None: continuous).
pub(crate) fn stroke_with(b: &mut Builder, ctx: &Ctx, rgb: Ink, lw: f32, lt: Option<&cadcraft_doc::Linetype>, ltscale: f64, pts: &[Vec2]) {
    match lt {
        Some(lt) => {
            let min = b.opts.min_dash / ctx.xf.scale_factor().max(1e-12);
            for dash in linetype::apply(pts, lt, ltscale, min) {
                match dash.as_slice() {
                    [p] => b.point(ctx, rgb, *p),
                    _ => b.polyline(ctx, rgb, lw, &dash),
                }
            }
        }
        None => b.polyline(ctx, rgb, lw, pts),
    }
}

fn table(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::Table, rgb: Ink, lw: f32) {
    table::draw(b, ctx, t, rgb, lw);
}

/// The overall scale of a leader drawn with dimension style `st`: its DIMSCALE, else the
/// drawing's.
fn leader_scale(d: &Drawing, st: Option<&cadcraft_doc::DimStyle>) -> f64 {
    let dimscale = d.header.f64("DIMSCALE", 1.0);
    st.map(|s| s.effective_scale(dimscale)).unwrap_or_else(|| cadcraft_doc::DimStyle::default().effective_scale(dimscale))
}
