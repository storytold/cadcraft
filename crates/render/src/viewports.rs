#[cfg(not(target_arch = "wasm32"))]
use super::display::append;
use super::display::{DPrim, DisplayList, Kind, PlotText, VIEWPORT_CONTENT};
use super::draw::entity;
#[cfg(not(target_arch = "wasm32"))]
use super::pipeline::PARALLEL_CHUNK;
use super::pipeline::{Options, style_table, top_ctx, with_point_view};
use super::{Bounds2, Builder, Ctx, Drawing, Entity, EntityKind, Handle, MAX_BLOCK_EXPANSION, Mat3, Space, Vec2, clip, paper, pen, plotstyle};

/// Upper bound on viewports drawn per layout (hostile files).
pub(crate) const MAX_VIEWPORTS: usize = 256;

/// A paper-space viewport: its border (as an ordinary entity) and model space seen through it.
/// Viewport id 1 is the paper-space view itself and is not drawn. A clipped viewport shows
/// model space through its clip boundary's outline (the boundary entity draws itself, so the
/// rectangle is not drawn); a viewport turned off shows only its border.
pub(crate) fn viewport(b: &mut Builder, d: &Drawing, e: &Entity, vp: &cadcraft_doc::Viewport) {
    if !viewport_shown(d, e, vp) {
        return;
    }
    let poly = viewport_clip(d, e.handle, vp);
    // Border (on the viewport's layer; layer off hides only the border).
    if poly.is_none() {
        entity(b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
    }
    if b.opts.skip_viewport == Some(e.handle) {
        return;
    }
    let unit_mm = paper::unit_mm_of(d, e.handle);
    match &poly {
        Some(p) => contents(b, d, vp, p.bounds(), &Clip::Poly(p), unit_mm),
        None => {
            if let Some(rect) = viewport_rect(vp) {
                contents(b, d, vp, rect, &Clip::Rect(rect), unit_mm);
            }
        }
    }
}

/// Whether a viewport draws anything: id 1 is the paper-space view itself, and a hidden viewport
/// or one on a frozen layer shows neither its border nor its contents.
fn viewport_shown(d: &Drawing, e: &Entity, vp: &cadcraft_doc::Viewport) -> bool {
    vp.id != 1 && e.common.visible && !d.is_hidden_object(e.handle) && !d.layer(&e.common.layer).is_some_and(|l| l.frozen)
}

/// A viewport's rectangle on paper; `None` for an unusable size or centre.
fn viewport_rect(vp: &cadcraft_doc::Viewport) -> Option<Bounds2> {
    let center = vp.center.xy();
    let ok = |v: f64| v.is_finite() && v > 0.0;
    (ok(vp.width) && ok(vp.height) && center.is_finite()).then(|| {
        let half = Vec2::new(vp.width / 2.0, vp.height / 2.0);
        Bounds2::new(center - half, center + half)
    })
}

/// Model space seen through the layout viewport `handle` of `space`, in paper units, clipped to
/// `area` (paper units) instead of to the viewport: what [`build`] draws inside that viewport,
/// over an area the caller chooses. The canvas builds the active viewport this way for an area
/// around it and draws it clipped to the viewport, so panning and zooming inside it moves the
/// geometry instead of rebuilding the layout (see [`Options::skip_viewport`]). Empty when the
/// viewport shows nothing.
pub fn viewport_content(d: &Drawing, space: &Space, handle: Handle, area: Bounds2, opts: &Options) -> DisplayList {
    let Some(e) = d.space(space).and_then(|s| s.get(handle)) else { return DisplayList::default() };
    let EntityKind::Viewport(vp) = &e.kind else { return DisplayList::default() };
    if !viewport_shown(d, e, vp) || area.is_empty() || !(area.min.is_finite() && area.max.is_finite()) {
        return DisplayList::default();
    }
    let opts = &with_point_view(d, space, opts);
    let styler = style_table(d, space, opts, false).map(|t| plotstyle::Styler::new(d, t));
    let mut b =
        Builder { list: DisplayList::default(), opts, plotting: false, styles: styler.as_ref(), named: None, shared_budget: None, transparency: 0 };
    contents(&mut b, d, vp, area, &Clip::Rect(area), paper::unit_mm_of(d, handle));
    b.list
}

/// Model space seen through `vp`, in paper units, appended to `b`'s list clipped to `clip`;
/// `area` (the clip's bounds on paper) picks the model objects to draw.
fn contents(b: &mut Builder, d: &Drawing, vp: &cadcraft_doc::Viewport, area: Bounds2, clip: &Clip, unit_mm: f64) {
    if b.list.expanded >= MAX_BLOCK_EXPANSION || !vp.on {
        return;
    }
    let center = vp.center.xy();
    let ok = |v: f64| v.is_finite() && v > 0.0;
    if !(ok(vp.width) && ok(vp.height) && ok(vp.view_height) && center.is_finite() && vp.view_center.is_finite()) {
        return;
    }
    let s = vp.height / vp.view_height;
    if !ok(s) {
        return;
    }
    let xf = Mat3::translate(center).then_before(Mat3::scale(s, s)).then_before(Mat3::translate(-vp.view_center));
    let mhalf = Vec2::new(area.width(), area.height()) / (2.0 * s);
    let mcenter = vp.view_center + (area.center() - center) / s;
    // Model window with slack: entity bounds are approximate (text, dimensions).
    let win = Bounds2::new(mcenter - mhalf, mcenter + mhalf).expand(mhalf.x.max(mhalf.y) * 0.1);
    // PSLTSCALE on: model-space dashes are scaled so they measure the same on paper as in
    // paper space, whatever the viewport scale.
    let lt_factor = if d.header.i64("PSLTSCALE", 1) != 0 { 1.0 / s } else { 1.0 };
    let ctx = || Ctx { lt_factor, ..top_ctx(d, xf, VIEWPORT_CONTENT, &vp.frozen_layers, &vp.layer_colors) };
    let seen = |me: &Entity| {
        if matches!(me.kind, EntityKind::Viewport(_)) {
            return false;
        }
        if matches!(me.kind, EntityKind::Ray(_) | EntityKind::XLine(_)) {
            return true;
        }
        let eb = cadcraft_doc::entity_bounds(d, me, 0);
        eb.is_empty() || eb.intersects(&win)
    };
    // One expansion budget for the whole build: seed it with what the enclosing list has already
    // spent so a viewport's contents cannot start over with a fresh allowance and exceed the
    // documented limit (CWE-400). Both the serial sub-builder and the parallel chunks share it.
    let budget = std::sync::atomic::AtomicUsize::new(b.list.expanded);
    let mut sub = Builder {
        list: DisplayList::default(),
        opts: b.opts,
        plotting: b.plotting,
        styles: b.styles,
        named: None,
        shared_budget: Some(&budget),
        transparency: 0,
    };
    #[cfg(not(target_arch = "wasm32"))]
    let parallel = !b.plotting && b.styles.is_none() && d.model.len() >= 2 * PARALLEL_CHUNK;
    #[cfg(target_arch = "wasm32")]
    let parallel = false;
    if parallel {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // As `build_parallel`: chunks built side by side, joined in drawing order.
            use rayon::prelude::*;
            let ents: Vec<&std::sync::Arc<Entity>> = d.model.iter().collect();
            let opts = b.opts;
            let parts: Vec<DisplayList> = ents
                .par_chunks(PARALLEL_CHUNK)
                .map(|chunk| {
                    let mut part = Builder {
                        list: DisplayList::default(),
                        opts,
                        plotting: false,
                        styles: None,
                        named: None,
                        shared_budget: Some(&budget),
                        transparency: 0,
                    };
                    let c = ctx();
                    for me in chunk.iter().filter(|me| seen(me)) {
                        entity(&mut part, &c, me);
                    }
                    part.list
                })
                .collect();
            for part in parts {
                append(&mut sub.list, part);
            }
        }
    } else {
        let c = ctx();
        for me in d.model.iter().filter(|me| seen(me)) {
            entity(&mut sub, &c, me);
        }
    }
    // Plotted text glyphs move to the layout's list; their outlines carry the glyph's index there
    // in their handle's high half until clipped (see `take_viewport_layers`).
    for t in std::mem::take(&mut sub.list.texts) {
        let k = b.list.texts.len() as u64 + 1;
        if let Some(p) = sub.list.prims.get_mut(t.prim as usize)
            && k < u64::from(u32::MAX)
        {
            p.handle = Handle(p.handle.0 | (k << 32));
            b.list.texts.push(PlotText { prim: PlotText::NONE, ..t });
        }
    }
    // The contents are in paper units now: draw their pens before clipping.
    pen::apply(&mut sub.list, unit_mm);
    b.list.expanded = b.list.expanded.saturating_add(sub.list.expanded);
    append_clipped(&mut b.list, &sub.list, clip);
}

/// The clip region of a clipped viewport: the outline of its clip boundary entity, when that
/// is a closed shape in the drawing. `None` for a rectangular viewport (or a dangling,
/// unusable or self-referencing boundary, which falls back to the rectangle).
pub fn viewport_clip(d: &Drawing, handle: Handle, vp: &cadcraft_doc::Viewport) -> Option<clip::PolyClip> {
    let h = vp.clip.filter(|h| *h != handle)?;
    let be = d.entity(h)?;
    clip::PolyClip::new(&clip::boundary_polygon(&be.kind)?)
}

/// What viewport contents are clipped to.
enum Clip<'a> {
    /// The viewport rectangle (the fast path).
    Rect(Bounds2),
    /// A clip boundary's outline.
    Poly(&'a clip::PolyClip),
}

/// Append `src` to `dst`, clipped to `clip`.
fn append_clipped(dst: &mut DisplayList, src: &DisplayList, clip: &Clip) {
    for p in &src.prims {
        let pts = src.points(p);
        match (p.kind, clip) {
            (Kind::Polyline, Clip::Rect(rect)) => {
                for piece in clip::clip_polyline(pts, rect) {
                    push_raw(dst, p, Kind::Polyline, &piece);
                }
            }
            (Kind::Polyline, Clip::Poly(c)) => {
                for piece in c.clip_polyline(pts) {
                    push_raw(dst, p, Kind::Polyline, &piece);
                }
            }
            (Kind::Tris | Kind::Mask, Clip::Rect(rect)) => {
                let t = clip::clip_triangles(pts, rect);
                push_raw(dst, p, p.kind, &t);
            }
            (Kind::Tris | Kind::Mask, Clip::Poly(c)) => {
                let t = c.clip_triangles(pts);
                push_raw(dst, p, p.kind, &t);
            }
            (Kind::Point, _) => {
                let inside = |q: Vec2| match clip {
                    Clip::Rect(rect) => rect.contains(q),
                    Clip::Poly(c) => c.contains(q),
                };
                if let Some(q) = pts.first()
                    && inside(*q)
                {
                    push_raw(dst, p, Kind::Point, &[*q]);
                }
            }
            (Kind::Infinite { ray }, Clip::Rect(rect)) => {
                if let (Some(base), Some(dir)) = (pts.first(), pts.get(1))
                    && let Some((a, c)) = clip::clip_infinite(*base, *dir, ray, rect)
                {
                    push_raw(dst, p, Kind::Polyline, &[a, c]);
                }
            }
            (Kind::Infinite { ray }, Clip::Poly(c)) => {
                if let (Some(base), Some(dir)) = (pts.first(), pts.get(1)) {
                    for piece in c.clip_infinite(*base, *dir, ray) {
                        push_raw(dst, p, Kind::Polyline, &piece);
                    }
                }
            }
        }
    }
}

fn push_raw(dst: &mut DisplayList, p: &DPrim, kind: Kind, pts: &[Vec2]) {
    let min = match kind {
        Kind::Polyline => 2,
        Kind::Tris | Kind::Mask => 3,
        _ => 1,
    };
    if pts.len() < min {
        return;
    }
    let tris = matches!(kind, Kind::Tris | Kind::Mask);
    let start = if tris { dst.tris.len() } else { dst.verts.len() } as u32;
    for q in pts {
        dst.bounds.add(*q);
    }
    if tris {
        dst.tris.extend_from_slice(pts);
    } else {
        dst.verts.extend_from_slice(pts);
    }
    dst.prims.push(DPrim {
        handle: p.handle,
        color: p.color,
        aci7: p.aci7,
        lw: p.lw,
        transparency: p.transparency,
        kind,
        start,
        len: pts.len() as u32,
    });
}
