//! CADCraft rendering.
//!
//! [`build`] turns a drawing space into a [`DisplayList`]: world-space polylines, filled
//! triangles and points, each tagged with its top-level entity handle and resolved colour.
//! Linetypes, hatches, text, block references and dimensions are expanded here. The UI uploads
//! the list to the GPU; [`raster`] draws it on the CPU (PNG export, plot preview, tests).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod clip;
mod dim;
mod fill;
mod hatch;
mod linetype;
pub mod paper;
pub mod raster;
pub mod units;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, Lineweight, Prim, Space};
use cadcraft_geom::{Bounds2, Mat3, Polyline, Vec2};

pub use dim::{ArrowGeom, Arrowhead, DimGeometry, DimText, LineRole, arrowhead, dimension_geometry, dimension_geometry_with, format_linear_value};
pub use fill::triangulate_evenodd;
pub use paper::{PAPER_SIZES, PaperSize, Sheet, paper_size, sheet};

/// Handle carried by model-space geometry drawn inside a paper-space viewport (it belongs to no
/// single paper-space entity, so selection highlighting never matches it).
pub const VIEWPORT_CONTENT: Handle = Handle(0);

/// What a display-list primitive draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `verts[start..start+len]` as a connected polyline.
    Polyline,
    /// `tris[start..start+len]`, three vertices per triangle.
    Tris,
    /// A single point marker at `verts[start]`.
    Point,
    /// An infinite line or ray: `verts[start]` = base, `verts[start+1]` = unit direction.
    Infinite { ray: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DPrim {
    pub handle: Handle,
    pub color: Rgb,
    /// Lineweight in mm (0 = thinnest).
    pub lw: f32,
    pub kind: Kind,
    pub start: u32,
    pub len: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub prims: Vec<DPrim>,
    pub verts: Vec<Vec2>,
    pub tris: Vec<Vec2>,
    pub bounds: Bounds2,
    /// The paper sheet when the list was built for a layout (paper space); the UI draws the
    /// white sheet, its shadow and the printable-area outline from it.
    pub sheet: Option<Sheet>,
    /// Block contents expanded while building (see [`MAX_BLOCK_EXPANSION`]).
    pub expanded: usize,
}

impl DisplayList {
    pub fn segment_count(&self) -> usize {
        self.prims.iter().filter(|p| p.kind == Kind::Polyline).map(|p| p.len.saturating_sub(1) as usize).sum()
    }
    /// The polyline points of a primitive.
    pub fn points(&self, p: &DPrim) -> &[Vec2] {
        match p.kind {
            Kind::Tris => self.tris.get(p.start as usize..(p.start + p.len) as usize).unwrap_or(&[]),
            _ => self.verts.get(p.start as usize..(p.start + p.len) as usize).unwrap_or(&[]),
        }
    }
}

/// Build options.
#[derive(Clone, Debug)]
pub struct Options {
    /// Chord tolerance in world units (about half a pixel at the current zoom).
    pub tolerance: f64,
    /// Dashes shorter than this (world units) are drawn as continuous lines.
    pub min_dash: f64,
    /// Draw text (off for very coarse previews).
    pub text: bool,
    /// Highlighted (selected) handles are not special here; the canvas overlays them.
    pub fill: bool,
    pub lineweights: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { tolerance: 0.001, min_dash: 0.0, text: true, fill: true, lineweights: false }
    }
}

/// Graphics state inherited through block references.
#[derive(Clone)]
struct Ctx<'a> {
    d: &'a Drawing,
    xf: Mat3,
    block_color: Color,
    block_layer: Option<String>,
    block_lw: Lineweight,
    block_ltype: String,
    depth: usize,
    top: Handle,
    /// Layers frozen in the viewport being drawn.
    frozen: &'a [String],
    /// Per-viewport layer colour overrides of the viewport being drawn.
    vp_colors: &'a [(String, Color)],
}

impl Ctx<'_> {
    /// A layer's colour, honouring the viewport's override.
    fn layer_color(&self, name: &str) -> Option<Color> {
        if let Some((_, c)) = self.vp_colors.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            return Some(*c);
        }
        self.d.layer(name).map(|l| l.color)
    }
}

struct Builder<'a> {
    list: DisplayList,
    opts: &'a Options,
    /// Plotting: skip layers marked "do not plot".
    plotting: bool,
}

/// Upper bound on the block contents drawn per display list (each block reference, MINSERT copy
/// and entity drawn inside a block counts one). Nested, self-referencing or arrayed block
/// references in a hostile file would otherwise expand exponentially (`MAX_BLOCK_DEPTH` only
/// bounds the depth); past the limit the remaining block contents are not drawn.
pub const MAX_BLOCK_EXPANSION: usize = 2_000_000;

impl Builder<'_> {
    /// Take one unit of the block-expansion budget; false once it is spent.
    fn expand(&mut self) -> bool {
        self.list.expanded = self.list.expanded.saturating_add(1);
        self.list.expanded <= MAX_BLOCK_EXPANSION
    }
    fn polyline(&mut self, ctx: &Ctx, color: Rgb, lw: f32, pts: &[Vec2]) {
        if pts.len() < 2 {
            return;
        }
        let start = self.list.verts.len() as u32;
        for p in pts {
            let q = ctx.xf.apply(*p);
            self.list.bounds.add(q);
            self.list.verts.push(q);
        }
        self.list.prims.push(DPrim { handle: ctx.top, color, lw, kind: Kind::Polyline, start, len: pts.len() as u32 });
    }
    fn tris(&mut self, ctx: &Ctx, color: Rgb, tris: &[Vec2]) {
        if tris.len() < 3 {
            return;
        }
        let start = self.list.tris.len() as u32;
        for p in tris {
            let q = ctx.xf.apply(*p);
            self.list.bounds.add(q);
            self.list.tris.push(q);
        }
        self.list.prims.push(DPrim { handle: ctx.top, color, lw: 0.0, kind: Kind::Tris, start, len: tris.len() as u32 });
    }
    /// Shaped text: strokes as polylines; TrueType glyphs filled (TEXTFILL) or outlined.
    fn shaped(&mut self, ctx: &Ctx, color: Rgb, lw: f32, sh: &cadcraft_fonts::Shaped) {
        for s in &sh.strokes {
            self.polyline(ctx, color, lw, s);
        }
        if sh.glyphs.is_empty() {
            return;
        }
        let fill = self.opts.fill && ctx.d.header.i64("TEXTFILL", 1) != 0;
        for g in &sh.glyphs {
            if fill {
                let tris = fill::triangulate_evenodd(g);
                self.tris(ctx, color, &tris);
            } else {
                for c in g {
                    self.polyline(ctx, color, lw, c);
                }
            }
        }
    }
    fn point(&mut self, ctx: &Ctx, color: Rgb, p: Vec2) {
        let q = ctx.xf.apply(p);
        self.list.bounds.add(q);
        let start = self.list.verts.len() as u32;
        self.list.verts.push(q);
        self.list.prims.push(DPrim { handle: ctx.top, color, lw: 0.0, kind: Kind::Point, start, len: 1 });
    }
    fn infinite(&mut self, ctx: &Ctx, color: Rgb, base: Vec2, dir: Vec2, ray: bool) {
        let start = self.list.verts.len() as u32;
        self.list.verts.push(ctx.xf.apply(base));
        self.list.verts.push(ctx.xf.apply_vec(dir).normalized());
        self.list.prims.push(DPrim { handle: ctx.top, color, lw: 0.0, kind: Kind::Infinite { ray }, start, len: 2 });
    }
}

/// Build the display list for a space. For a layout (paper space) this includes the model-space
/// geometry seen through each viewport, clipped to the viewport, and [`DisplayList::sheet`].
pub fn build(d: &Drawing, space: &Space, opts: &Options) -> DisplayList {
    build_space(d, space, opts, false)
}

/// Like [`build`] but for plotting: layers marked "do not plot" (such as `Defpoints`) are left out.
pub fn build_plot(d: &Drawing, space: &Space, opts: &Options) -> DisplayList {
    build_space(d, space, opts, true)
}

fn top_ctx<'a>(d: &'a Drawing, xf: Mat3, top: Handle, frozen: &'a [String], vp_colors: &'a [(String, Color)]) -> Ctx<'a> {
    Ctx {
        d,
        xf,
        block_color: Color::Index(7),
        block_layer: None,
        block_lw: Lineweight::Default,
        block_ltype: "Continuous".into(),
        depth: 0,
        top,
        frozen,
        vp_colors,
    }
}

fn build_space(d: &Drawing, space: &Space, opts: &Options, plotting: bool) -> DisplayList {
    let mut b = Builder { list: DisplayList::default(), opts, plotting };
    if let Space::Paper(name) = space {
        b.list.sheet = paper::sheet(d, name);
    }
    if let Some(store) = d.space(space) {
        let paper = matches!(space, Space::Paper(_));
        let mut viewports = 0usize;
        for e in store.iter() {
            if paper && let EntityKind::Viewport(vp) = &e.kind {
                if viewports < MAX_VIEWPORTS {
                    viewports += 1;
                    viewport(&mut b, d, e, vp);
                }
                continue;
            }
            entity(&mut b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
        }
    }
    b.list
}

/// Upper bound on viewports drawn per layout (hostile files).
const MAX_VIEWPORTS: usize = 256;

/// A paper-space viewport: its border (as an ordinary entity) and model space seen through it.
/// Viewport id 1 is the paper-space view itself and is not drawn.
fn viewport(b: &mut Builder, d: &Drawing, e: &Entity, vp: &cadcraft_doc::Viewport) {
    if vp.id == 1 || !e.common.visible {
        return;
    }
    // Border (on the viewport's layer; layer off hides only the border).
    entity(b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
    if b.list.expanded >= MAX_BLOCK_EXPANSION {
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
    let half = Vec2::new(vp.width / 2.0, vp.height / 2.0);
    let rect = Bounds2::new(center - half, center + half);
    let xf = Mat3::translate(center).then_before(Mat3::scale(s, s)).then_before(Mat3::translate(-vp.view_center));
    let mhalf = half / s;
    // Model window with slack: entity bounds are approximate (text, dimensions).
    let win = Bounds2::new(vp.view_center - mhalf, vp.view_center + mhalf).expand(mhalf.x.max(mhalf.y) * 0.1);
    let mut sub = Builder { list: DisplayList::default(), opts: b.opts, plotting: b.plotting };
    for me in d.model.iter() {
        if !matches!(me.kind, EntityKind::Ray(_) | EntityKind::XLine(_) | EntityKind::Viewport(_)) {
            let eb = cadcraft_doc::entity_bounds(d, me, 0);
            if !eb.is_empty() && !eb.intersects(&win) {
                continue;
            }
        }
        if matches!(me.kind, EntityKind::Viewport(_)) {
            continue;
        }
        entity(&mut sub, &top_ctx(d, xf, VIEWPORT_CONTENT, &vp.frozen_layers, &vp.layer_colors), me);
    }
    b.list.expanded = b.list.expanded.saturating_add(sub.list.expanded);
    append_clipped(&mut b.list, &sub.list, &rect);
}

/// Append `src` to `dst`, clipped to `rect`.
fn append_clipped(dst: &mut DisplayList, src: &DisplayList, rect: &Bounds2) {
    for p in &src.prims {
        let pts = src.points(p);
        match p.kind {
            Kind::Polyline => {
                for piece in clip::clip_polyline(pts, rect) {
                    push_raw(dst, p, Kind::Polyline, &piece);
                }
            }
            Kind::Tris => {
                let t = clip::clip_triangles(pts, rect);
                push_raw(dst, p, Kind::Tris, &t);
            }
            Kind::Point => {
                if let Some(q) = pts.first()
                    && rect.contains(*q)
                {
                    push_raw(dst, p, Kind::Point, &[*q]);
                }
            }
            Kind::Infinite { ray } => {
                if let (Some(base), Some(dir)) = (pts.first(), pts.get(1))
                    && let Some((a, c)) = clip::clip_infinite(*base, *dir, ray, rect)
                {
                    push_raw(dst, p, Kind::Polyline, &[a, c]);
                }
            }
        }
    }
}

fn push_raw(dst: &mut DisplayList, p: &DPrim, kind: Kind, pts: &[Vec2]) {
    let min = match kind {
        Kind::Polyline => 2,
        Kind::Tris => 3,
        _ => 1,
    };
    if pts.len() < min {
        return;
    }
    let tris = kind == Kind::Tris;
    let start = if tris { dst.tris.len() } else { dst.verts.len() } as u32;
    for q in pts {
        dst.bounds.add(*q);
    }
    if tris {
        dst.tris.extend_from_slice(pts);
    } else {
        dst.verts.extend_from_slice(pts);
    }
    dst.prims.push(DPrim { handle: p.handle, color: p.color, lw: p.lw, kind, start, len: pts.len() as u32 });
}

/// Build the display list for a set of loose entities (previews, rubber bands).
pub fn build_entities<'a, I: IntoIterator<Item = &'a Entity>>(d: &Drawing, ents: I, opts: &Options) -> DisplayList {
    let mut b = Builder { list: DisplayList::default(), opts, plotting: false };
    for e in ents {
        entity(&mut b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
    }
    b.list
}

fn resolve(ctx: &Ctx, e: &Entity, plotting: bool) -> (Rgb, f32, Option<cadcraft_doc::Linetype>, f64, bool) {
    let d = ctx.d;
    // Layer "0" inside a block takes the insert's layer.
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer = d.layer(layer_name);
    let visible = e.common.visible
        && layer.is_none_or(|l| l.visible() && (!plotting || l.plot))
        && !ctx.frozen.iter().any(|f| f.eq_ignore_ascii_case(layer_name));
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let rgb = e.common.color.resolve(layer_color, ctx.block_color);
    let lw = match e.common.lineweight {
        Lineweight::ByLayer => layer.map(|l| l.lineweight).unwrap_or(Lineweight::Default),
        Lineweight::ByBlock => ctx.block_lw,
        x => x,
    };
    let lw_mm = match lw {
        Lineweight::Mm100(v) => f32::from(v) / 100.0,
        _ => 0.25,
    };
    let lt_name = match e.common.linetype.to_ascii_lowercase().as_str() {
        "bylayer" => layer.map(|l| l.linetype.clone()).unwrap_or_else(|| "Continuous".into()),
        "byblock" => ctx.block_ltype.clone(),
        _ => e.common.linetype.clone(),
    };
    let lt = d.linetype(&lt_name).filter(|l| !l.pattern.is_empty()).cloned();
    let scale = d.header.f64("LTSCALE", 1.0) * e.common.ltscale;
    (rgb, lw_mm, lt, scale, visible)
}

fn entity(b: &mut Builder, ctx: &Ctx, e: &Entity) {
    let (rgb, lw, lt, ltscale, visible) = resolve(ctx, e, b.plotting);
    if !visible {
        return;
    }
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
        EntityKind::Dimension(dm) => {
            if let Some(blk) = dm.block.as_ref().and_then(|n| ctx.d.block(n))
                && ctx.depth < cadcraft_doc::MAX_BLOCK_DEPTH
            {
                let sub = sub_ctx(ctx, e, Mat3::IDENTITY);
                for be in blk.entities.iter() {
                    if !b.expand() {
                        break;
                    }
                    entity(b, &sub, be);
                }
            } else {
                dimension(b, ctx, e, dm, lw);
            }
        }
        EntityKind::Hatch(h) => {
            if h.solid || h.pattern.eq_ignore_ascii_case("SOLID") || h.gradient.is_some() {
                if b.opts.fill {
                    let loops: Vec<Vec<Vec2>> =
                        h.loops.iter().map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.tessellate(tol)).collect();
                    let tris = fill::triangulate_evenodd(&loops);
                    let c = match &h.gradient {
                        Some(g) => g.color1.resolve(Color::Index(7), ctx.block_color),
                        None => rgb,
                    };
                    b.tris(ctx, c, &tris);
                }
            } else {
                for seg in hatch::pattern_lines(h, tol) {
                    if seg.len() == 1 {
                        if let Some(p) = seg.first() {
                            b.point(ctx, rgb, *p);
                        }
                    } else {
                        b.polyline(ctx, rgb, lw, &seg);
                    }
                }
            }
        }
        EntityKind::LwPolyline(p) if p.const_width > 0.0 || p.vertices.iter().any(|v| v.start_width > 0.0 || v.end_width > 0.0) => {
            if b.opts.fill && ctx.d.header.i64("FILLMODE", 1) != 0 {
                let tris = fill::wide_polyline(p, tol);
                b.tris(ctx, rgb, &tris);
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
        EntityKind::Wipeout(w) => {
            let mut pts = w.boundary.clone();
            if let Some(f) = pts.first().copied() {
                pts.push(f);
            }
            b.polyline(ctx, rgb, lw, &pts);
        }
        EntityKind::Image(i) => {
            let o = i.insert.xy();
            let u = i.u.xy() * i.size.x;
            let v = i.v.xy() * i.size.y;
            b.polyline(ctx, rgb, lw, &[o, o + u, o + u + v, o + v, o]);
            b.polyline(ctx, rgb, lw, &[o, o + u + v]);
            b.polyline(ctx, rgb, lw, &[o + u, o + v]);
        }
        EntityKind::Table(t) => table(b, ctx, t, rgb, lw),
        EntityKind::Leader(l) => {
            let pts: Vec<Vec2> = l.vertices.iter().map(|v| v.xy()).collect();
            stroke(b, &pts);
            if l.arrow
                && let (Some(a), Some(n)) = (pts.first(), pts.get(1))
            {
                let st = ctx.d.dim_style(&l.style);
                let size = st.map(|s| s.arrow_size).unwrap_or(0.18) * ctx.d.header.f64("DIMSCALE", 1.0);
                let kind = dim::Arrowhead::parse(st.map(|s| s.arrow_block.as_str()).unwrap_or(""));
                let g = dim::arrowhead(kind, *a, (*a - *n).normalized(), size);
                for l in &g.lines {
                    b.polyline(ctx, rgb, lw, l);
                }
                b.tris(ctx, rgb, &g.tris);
            }
        }
        EntityKind::MLeader(m) => {
            for l in &m.leaders {
                let pts: Vec<Vec2> = l.iter().map(|v| v.xy()).chain(std::iter::once(m.landing.xy())).collect();
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
        }
        EntityKind::Point(p) => b.point(ctx, rgb, p.p.xy()),
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

/// The text style of an entity (Standard defaults when missing).
fn text_style(d: &Drawing, name: &str) -> cadcraft_doc::TextStyle {
    d.text_style(name).cloned().unwrap_or_default()
}

/// Shape a TEXT / ATTRIB / ATTDEF with its style's font and generation flags.
pub fn place_text_entity(d: &Drawing, t: &cadcraft_doc::Text, value: &str) -> (cadcraft_fonts::Shaped, Bounds2) {
    let ts = text_style(d, &t.style);
    let font = cadcraft_fonts::TextFont::resolve(&ts.font);
    let (h, v) = align(t.halign, t.valign);
    let p = cadcraft_fonts::TextParams {
        insert: t.insert.xy(),
        align_pt: t.align_pt.map(|p| p.xy()),
        height: t.height,
        rotation: t.rotation,
        width_factor: t.width_factor,
        oblique: t.oblique,
        h,
        v,
        backwards: ts.backwards,
        upside_down: ts.upside_down,
    };
    cadcraft_fonts::place(&font, value, &p)
}

/// Lay out an MTEXT with its style's font.
pub fn layout_mtext_entity(d: &Drawing, t: &cadcraft_doc::MText) -> cadcraft_fonts::MTextLayout {
    let ts = text_style(d, &t.style);
    let params = cadcraft_fonts::MTextParams {
        insert: t.insert.xy(),
        height: t.height,
        width: t.width,
        attach: t.attach,
        rotation: t.rotation,
        line_spacing: t.line_spacing,
        font: cadcraft_fonts::TextFont::resolve(&ts.font),
        width_factor: ts.width_factor,
        oblique: ts.oblique,
    };
    cadcraft_fonts::layout_mtext_with(&t.contents, &params)
}

fn mtext_color(c: Option<cadcraft_fonts::MTextColor>, rgb: Rgb) -> Rgb {
    match c {
        Some(cadcraft_fonts::MTextColor::Aci(i)) => {
            u8::try_from(i).ok().filter(|i| *i > 0).map(|i| Color::Index(i).resolve(Color::Index(7), Color::Index(7))).unwrap_or(rgb)
        }
        Some(cadcraft_fonts::MTextColor::Rgb(v)) => Rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8),
        None => rgb,
    }
}

fn mtext(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::MText, rgb: Rgb, lw: f32) {
    let l = layout_mtext_entity(ctx.d, t);
    for p in &l.pieces {
        b.shaped(ctx, mtext_color(p.color, rgb), lw, &p.shaped);
    }
}

/// The font settings of a dimension style's text style.
pub fn dim_text(d: &Drawing, st: &cadcraft_doc::DimStyle) -> DimText {
    let ts = text_style(d, &st.text_style);
    DimText { font: cadcraft_fonts::TextFont::resolve(&ts.font), fixed_height: ts.height, width_factor: ts.width_factor, oblique: ts.oblique }
}

/// Geometry of a dimension in a drawing: its style, overrides and text font.
pub fn dimension_in(d: &Drawing, dm: &cadcraft_doc::Dimension) -> DimGeometry {
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default();
    let st = style.with_overrides(&dm.overrides);
    dim::dimension_geometry_with(dm, &style, d.header.f64("DIMSCALE", 1.0), &dim_text(d, &st))
}

fn dimension(b: &mut Builder, ctx: &Ctx, e: &Entity, dm: &cadcraft_doc::Dimension, lw: f32) {
    let d = ctx.d;
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
    let g = dimension_in(d, dm);
    // DIMCLRD / DIMCLRE / DIMCLRT: ByBlock = the dimension's own colour.
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = d.layer(layer_name).map(|l| l.color).unwrap_or(Color::Index(7));
    let own = match e.common.color {
        Color::ByLayer => layer_color,
        Color::ByBlock => ctx.block_color,
        c => c,
    };
    let dim_rgb = style.dim_line_color.resolve(layer_color, own);
    let ext_rgb = style.ext_line_color.resolve(layer_color, own);
    let txt_rgb = style.text_color.resolve(layer_color, own);
    for (l, role) in g.lines.iter().zip(g.line_roles.iter().chain(std::iter::repeat(&LineRole::Dim))) {
        b.polyline(ctx, if *role == LineRole::Ext { ext_rgb } else { dim_rgb }, lw, l);
    }
    for t in &g.fills {
        b.tris(ctx, dim_rgb, t);
    }
    if b.opts.text {
        let sh = cadcraft_fonts::Shaped { strokes: g.text.clone(), glyphs: g.text_glyphs.clone(), width: 0.0 };
        b.shaped(ctx, txt_rgb, lw, &sh);
    }
}

/// Covered cells of a table (inside another cell's merge, not its anchor).
pub fn table_covered(t: &cadcraft_doc::Table) -> Vec<Vec<bool>> {
    let rows = t.row_heights.len().min(10_000);
    let cols = t.col_widths.len().min(10_000);
    let mut cov = vec![vec![false; cols]; rows];
    for (r, row) in t.cells.iter().enumerate().take(rows) {
        for (c, cell) in row.iter().enumerate().take(cols) {
            if let Some((rs, cs)) = cell.merged {
                for rr in r..(r + rs.max(1) as usize).min(rows) {
                    for cc in c..(c + cs.max(1) as usize).min(cols) {
                        if (rr, cc) != (r, c)
                            && let Some(x) = cov.get_mut(rr).and_then(|v| v.get_mut(cc))
                        {
                            *x = true;
                        }
                    }
                }
            }
        }
    }
    cov
}

fn table(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::Table, rgb: Rgb, lw: f32) {
    let o = t.insert.xy();
    let rows = t.row_heights.len().min(10_000);
    let cols = t.col_widths.len().min(10_000);
    let xs: Vec<f64> = std::iter::once(0.0)
        .chain(t.col_widths.iter().take(cols).scan(0.0, |a, w| {
            *a += w;
            Some(*a)
        }))
        .collect();
    let ys: Vec<f64> = std::iter::once(0.0)
        .chain(t.row_heights.iter().take(rows).scan(0.0, |a, h| {
            *a += h;
            Some(*a)
        }))
        .collect();
    let cov = table_covered(t);
    let style = ctx.d.table_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&t.style)).cloned().unwrap_or_default();
    let ts = text_style(ctx.d, "Standard");
    let font = cadcraft_fonts::TextFont::resolve(&ts.font);
    let x_at = |c: usize| xs.get(c).copied().unwrap_or(0.0);
    let y_at = |r: usize| ys.get(r).copied().unwrap_or(0.0);
    for r in 0..rows {
        for c in 0..cols {
            if cov.get(r).and_then(|v| v.get(c)).copied().unwrap_or(false) {
                continue;
            }
            let cell = t.cells.get(r).and_then(|row| row.get(c));
            let (rs, cs) = cell.and_then(|x| x.merged).map(|(a, b)| (a.max(1) as usize, b.max(1) as usize)).unwrap_or((1, 1));
            let (r2, c2) = ((r + rs).min(rows), (c + cs).min(cols));
            let (x0, x1, y0, y1) = (x_at(c), x_at(c2), y_at(r), y_at(r2));
            let p = |x: f64, y: f64| o + Vec2::new(x, -y);
            // Each cell draws its bottom and right edges; the outer top/left come from row 0 / col 0.
            b.polyline(ctx, rgb, lw, &[p(x0, y1), p(x1, y1), p(x1, y0)]);
            if r == 0 {
                b.polyline(ctx, rgb, lw, &[p(x0, y0), p(x1, y0)]);
            }
            if c == 0 {
                b.polyline(ctx, rgb, lw, &[p(x0, y0), p(x0, y1)]);
            }
            let Some(cell) = cell.filter(|x| !x.text.is_empty() && b.opts.text) else { continue };
            let title = t.title && r == 0;
            let header = t.header && r == usize::from(t.title);
            let h = if title { t.text_height * 1.4 } else { t.text_height };
            // Titles and headers are centred; data sits middle-left inside the cell margin.
            let params = if title || header {
                cadcraft_fonts::TextParams::centered(p((x0 + x1) / 2.0, (y0 + y1) / 2.0), h, 0.0)
            } else {
                let at = p(x0 + style.margin.max(0.0), (y0 + y1) / 2.0);
                cadcraft_fonts::TextParams {
                    align_pt: Some(at),
                    h: cadcraft_fonts::Align::Left,
                    v: cadcraft_fonts::VAlign::Middle,
                    ..cadcraft_fonts::TextParams::new(at, h)
                }
            };
            let (sh, _) = cadcraft_fonts::place(&font, &cell.text, &params);
            b.shaped(ctx, rgb, lw, &sh);
        }
    }
}

fn align(h: cadcraft_doc::HAlign, v: cadcraft_doc::VAlign) -> (cadcraft_fonts::Align, cadcraft_fonts::VAlign) {
    use cadcraft_fonts::{Align as A, VAlign as V};
    let ha = match h {
        cadcraft_doc::HAlign::Left => A::Left,
        cadcraft_doc::HAlign::Center => A::Center,
        cadcraft_doc::HAlign::Right => A::Right,
        cadcraft_doc::HAlign::Aligned => A::Aligned,
        cadcraft_doc::HAlign::Middle => A::Middle,
        cadcraft_doc::HAlign::Fit => A::Fit,
    };
    let va = match v {
        cadcraft_doc::VAlign::Baseline => V::Baseline,
        cadcraft_doc::VAlign::Bottom => V::Bottom,
        cadcraft_doc::VAlign::Middle => V::Middle,
        cadcraft_doc::VAlign::Top => V::Top,
    };
    (ha, va)
}

fn sub_ctx<'a>(ctx: &Ctx<'a>, e: &Entity, m: Mat3) -> Ctx<'a> {
    let layer = if e.common.layer == "0" { ctx.block_layer.clone() } else { Some(e.common.layer.clone()) };
    let color = match e.common.color {
        Color::ByBlock => ctx.block_color,
        Color::ByLayer => ctx.layer_color(layer.as_deref().unwrap_or("0")).unwrap_or(Color::Index(7)),
        c => c,
    };
    let lw = match e.common.lineweight {
        Lineweight::ByBlock => ctx.block_lw,
        Lineweight::ByLayer => ctx.d.layer(layer.as_deref().unwrap_or("0")).map(|l| l.lineweight).unwrap_or(Lineweight::Default),
        x => x,
    };
    let lt = match e.common.linetype.to_ascii_lowercase().as_str() {
        "byblock" => ctx.block_ltype.clone(),
        "bylayer" => ctx.d.layer(layer.as_deref().unwrap_or("0")).map(|l| l.linetype.clone()).unwrap_or_else(|| "Continuous".into()),
        _ => e.common.linetype.clone(),
    };
    Ctx {
        d: ctx.d,
        xf: ctx.xf.then_before(m),
        block_color: color,
        block_layer: layer,
        block_lw: lw,
        block_ltype: lt,
        depth: ctx.depth + 1,
        top: ctx.top,
        frozen: ctx.frozen,
        vp_colors: ctx.vp_colors,
    }
}

fn insert(b: &mut Builder, ctx: &Ctx, e: &Entity, ins: &cadcraft_doc::Insert, rgb: Rgb) {
    if ctx.depth >= cadcraft_doc::MAX_BLOCK_DEPTH {
        return;
    }
    if let Some(blk) = ctx.d.block(&ins.block) {
        let cols = ins.cols.clamp(1, 10_000);
        let rows = ins.rows.clamp(1, 10_000);
        'copies: for r in 0..rows {
            for c in 0..cols {
                if !b.expand() {
                    break 'copies;
                }
                let off = Vec2::new(ins.col_spacing * f64::from(c), ins.row_spacing * f64::from(r)).rotate(ins.rotation);
                let m = Mat3::translate(off).then_before(ins.transform(blk.base.xy()));
                let sub = sub_ctx(ctx, e, m);
                for be in blk.entities.iter() {
                    // Constant/visible attribute definitions inside blocks are not drawn; attribs are.
                    if matches!(be.kind, EntityKind::AttDef(_)) {
                        continue;
                    }
                    if !b.expand() {
                        break 'copies;
                    }
                    entity(b, &sub, be);
                }
            }
        }
    }
    if b.opts.text {
        for a in &ins.attribs {
            if a.invisible {
                continue;
            }
            let (sh, _) = place_text_entity(ctx.d, &a.text, &a.text.value);
            b.shaped(ctx, rgb, 0.0, &sh);
        }
    }
}

#[cfg(test)]
mod tests;
