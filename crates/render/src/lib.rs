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
mod linetype_text;
mod mline;
mod outline;
pub mod paper;
pub mod pen;
mod plotstyle;
mod point;
pub mod raster;
mod text;
mod tolerance;
pub mod units;
mod wide;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, Lineweight, Prim, Space};
use cadcraft_geom::{Bounds2, Mat3, Polyline, Vec2};
use pen::Pen;

pub use dim::{ArrowGeom, Arrowhead, DimGeometry, DimText, LineRole, arrowhead, dimension_geometry, dimension_geometry_with, format_linear_value};
pub use fill::triangulate_evenodd;
pub use hatch::{fill_loops as hatch_fill_loops, pattern_swatch};
pub use paper::{PAPER_SIZES, PaperSize, Sheet, paper_size, sheet};
pub use tolerance::{gdt_points, tolerance_in};

/// Handle carried by model-space geometry drawn inside a paper-space viewport (it belongs to no
/// single paper-space entity, so selection highlighting never matches it).
pub const VIEWPORT_CONTENT: Handle = Handle(0);

/// [`DisplayList::prim_layers`] of a primitive whose layer isn't in the layer table.
pub const NO_LAYER: u32 = u32::MAX;

/// What a display-list primitive draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `verts[start..start+len]` as a connected polyline.
    Polyline,
    /// `tris[start..start+len]`, three vertices per triangle.
    Tris,
    /// A wipeout: triangles laid out like [`Kind::Tris`], filled with the background colour
    /// (paper white when plotting, the canvas colour on screen) so they hide what was drawn
    /// before them.
    Mask,
    /// A single point marker at `verts[start]`.
    Point,
    /// An infinite line or ray: `verts[start]` = base, `verts[start+1]` = unit direction.
    Infinite { ray: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DPrim {
    pub handle: Handle,
    pub color: Rgb,
    /// Colour 7 (directly, by layer or by block): drawn white on a dark background and black on
    /// a light one, see [`DPrim::display_rgb`]. Other colours keep their RGB.
    pub aci7: bool,
    /// Lineweight in mm (0 = thinnest).
    pub lw: f32,
    /// Transparency in percent (0 = opaque, at most 90): the object's own, its layer's when
    /// ByLayer, the block reference's when ByBlock. Always 0 unless [`Options::transparency`].
    pub transparency: u8,
    pub kind: Kind,
    pub start: u32,
    pub len: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub prims: Vec<DPrim>,
    pub verts: Vec<Vec2>,
    pub tris: Vec<Vec2>,
    /// Plots ([`build_plot`]): for each primitive, the index in [`Drawing::layers`] of the layer
    /// it is drawn on (block contents on layer 0: the block reference's; [`NO_LAYER`] when the
    /// layer isn't in the table), for PDF layers. Empty for other lists.
    pub prim_layers: Vec<u32>,
    /// Plots: outline-font glyphs that a plotter can write as text instead of drawing the
    /// filled outline primitive they stand for ([`PlotText::prim`]), in drawing order.
    pub texts: Vec<PlotText>,
    pub bounds: Bounds2,
    /// The paper sheet when the list was built for a layout (paper space); the UI draws the
    /// white sheet, its shadow and the printable-area outline from it.
    pub sheet: Option<Sheet>,
    /// Block contents expanded while building (see [`MAX_BLOCK_EXPANSION`]).
    pub expanded: usize,
    /// Plot style end, join and fill style per primitive (parallel to `prims`; missing entries
    /// are "use object"), turned into geometry by [`pen::apply`].
    pub pens: Vec<Pen>,
}

/// A glyph of plotted text: the font glyph and, in world coordinates, where it sits. It is
/// drawn by primitive [`PlotText::prim`] (a filled outline in the primitive's colour); output
/// with real text writes the glyph instead of that primitive.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotText {
    /// The filled outline primitive (index in [`DisplayList::prims`]); [`PlotText::NONE`] when
    /// the glyph has none in the list (clipped by a viewport).
    pub prim: u32,
    /// Triangle vertices of the outline when it was drawn, and their bounding box (a clipped
    /// outline has others).
    pub verts: u32,
    pub bounds: Bounds2,
    /// The glyph; its origin and em axes are in world coordinates.
    pub glyph: cadcraft_fonts::TextGlyph,
}

impl PlotText {
    pub const NONE: u32 = u32::MAX;
    /// [`PlotText::prim`] of a glyph cut by a viewport edge.
    pub const CLIPPED: u32 = u32::MAX - 1;
}

impl DPrim {
    /// The colour to draw with on `background`.
    pub fn display_rgb(&self, background: Rgb) -> Rgb {
        cadcraft_color::display_rgb(self.color, self.aci7, background)
    }
    /// The opacity to draw with: 255 when opaque, down to 26 at 90 % transparency.
    pub fn alpha(&self) -> u8 {
        alpha(self.transparency)
    }
}

/// The opacity (0..=255) of a transparency percentage, which is clamped to 0..=90 first.
pub fn alpha(transparency: u8) -> u8 {
    let t = u32::from(transparency.min(MAX_TRANSPARENCY));
    u8::try_from((255 * (100 - t) + 50) / 100).unwrap_or(u8::MAX)
}

/// The most transparent an object can be, in percent.
pub const MAX_TRANSPARENCY: u8 = 90;

/// A resolved colour: its RGB, whether it is colour 7, and its index colour (0 = a true
/// colour), which picks the style of a colour-dependent plot style table.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ink {
    rgb: Rgb,
    aci7: bool,
    aci: u8,
}

/// Resolve `c` like [`Color::resolve`], noting whether it comes out as colour 7.
fn ink(c: Color, layer: Color, block: Color) -> Ink {
    // ByLayer / ByBlock left unresolved (layer or block colour itself logical) mean colour 7.
    let is7 = |c: Color| matches!(c, Color::ByLayer | Color::ByBlock | Color::Index(7));
    let aci7 = match c {
        Color::ByLayer => is7(layer),
        Color::ByBlock => is7(block),
        c => c == Color::Index(7),
    };
    let aci = match c {
        Color::ByLayer => layer,
        Color::ByBlock => block,
        c => c,
    };
    let aci = match aci {
        Color::Index(i) => i,
        Color::True(_) => 0,
        Color::ByLayer | Color::ByBlock => 7,
    };
    Ink { rgb: c.resolve(layer, block), aci7, aci }
}

impl DisplayList {
    pub fn segment_count(&self) -> usize {
        self.prims.iter().filter(|p| p.kind == Kind::Polyline).map(|p| p.len.saturating_sub(1) as usize).sum()
    }
    /// The polyline points of a primitive.
    pub fn points(&self, p: &DPrim) -> &[Vec2] {
        match p.kind {
            Kind::Tris | Kind::Mask => self.tris.get(p.start as usize..(p.start + p.len) as usize).unwrap_or(&[]),
            _ => self.verts.get(p.start as usize..(p.start + p.len) as usize).unwrap_or(&[]),
        }
    }
    /// The boundary of a filled primitive ([`Kind::Tris`], [`Kind::Mask`]) as closed polylines,
    /// for highlighting it; empty for other kinds.
    pub fn fill_outlines(&self, p: &DPrim) -> Vec<Vec<Vec2>> {
        match p.kind {
            Kind::Tris | Kind::Mask => outline::fill_outlines(self.points(p)),
            _ => Vec::new(),
        }
    }
}

/// Build options.
#[derive(Clone, Debug)]
pub struct Options {
    /// Chord tolerance in world units (about half a pixel at the current zoom).
    pub tolerance: f64,
    /// Two pixels in world units: shorter dashes are continuous and filled font text uses
    /// screen-size detail. Zero keeps full detail (plots and image/vector exports).
    pub min_dash: f64,
    /// Draw text (off for very coarse previews).
    pub text: bool,
    /// Highlighted (selected) handles are not special here; the canvas overlays them.
    pub fill: bool,
    pub lineweights: bool,
    /// Height of the visible area in world units of the space being built. Relative point
    /// sizes (`PDSIZE` <= 0) are a percentage of it; 0 = the height of the space's extents.
    pub view_height: f64,
    /// Plot style table to draw with. When `None`, [`build`] of a layout whose page setup has
    /// "Display plot styles" on uses the layout's table; [`build_plot`] uses none.
    pub plot_style_table: Option<std::sync::Arc<cadcraft_doc::PlotStyleTable>>,
    /// The area being looked at (world units of the space being built), for on-screen builds:
    /// hatch patterns are only generated inside it. `None` = everywhere (plots, exports, picks).
    pub view: Option<Bounds2>,
    /// Draw objects with their transparency ([`DPrim::transparency`]). Off by default: the screen
    /// follows TRANSPARENCYDISPLAY, plots the page setup's "Plot transparency".
    pub transparency: bool,
    /// A layout viewport whose model-space contents are left out (its border is still drawn):
    /// the canvas draws the active viewport's contents itself, see [`viewport_content`].
    pub skip_viewport: Option<Handle>,
    /// Set from another thread to stop the build early (the canvas cancels a background
    /// rebuild the view no longer needs). A cancelled build returns a partial list, which the
    /// caller must discard; see [`Options::cancelled`].
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl Options {
    /// Whether [`Options::cancel`] has been set.
    pub fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            tolerance: 0.001,
            min_dash: 0.0,
            text: true,
            fill: true,
            lineweights: false,
            view_height: 0.0,
            plot_style_table: None,
            view: None,
            skip_viewport: None,
            transparency: false,
            cancel: None,
        }
    }
}

/// The largest axis scale of a transform's linear part (its largest singular value): a length
/// `l` in the transform's own units measures at most `l * max_scale` after it, in any direction.
/// A uniform scale gives its factor, where [`Mat3::scale_factor`] gives the mean of the axes.
fn max_scale(m: &Mat3) -> f64 {
    let (a, b, c, d) = (m.a, m.b, m.c, m.d);
    let e = (a * a + b * b + c * c + d * d) / 2.0;
    let f = (((a * a + b * b - c * c - d * d) / 2.0).powi(2) + (a * c + b * d).powi(2)).sqrt();
    let s = (e + f).max(0.0).sqrt();
    if s.is_finite() { s } else { 0.0 }
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
    /// Extra linetype scale for model space seen through a viewport: 1 / viewport scale when
    /// `PSLTSCALE` is on (dashes keep their paper-space length at any viewport scale), else 1.
    lt_factor: f64,
    /// Named plot style (table index) of the block reference being expanded, for `ByBlock`.
    block_pstyle: Option<usize>,
    /// Transparency (percent) of the block reference being expanded, for `ByBlock`.
    block_transparency: u8,
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
    /// The plot style table drawn with.
    styles: Option<&'a plotstyle::Styler>,
    /// Named plot style (table index) of the entity being drawn.
    named: Option<usize>,
    /// The block-expansion budget shared by the parts of a list built in parallel.
    shared_budget: Option<&'a std::sync::atomic::AtomicUsize>,
    /// Transparency (percent) of the entity being drawn.
    transparency: u8,
}

/// Upper bound on the block contents drawn per display list (each block reference, MINSERT copy
/// and entity drawn inside a block counts one). Nested, self-referencing or arrayed block
/// references in a hostile file would otherwise expand exponentially (`MAX_BLOCK_DEPTH` only
/// bounds the depth); past the limit the remaining block contents are not drawn.
pub const MAX_BLOCK_EXPANSION: usize = 2_000_000;

impl Builder<'_> {
    /// `color` and `lw` as the plot style table prints them.
    fn styled(&self, color: Ink, lw: f32) -> (Ink, f32) {
        match self.styles {
            Some(s) => s.apply(color, self.named, lw, self.opts.lineweights),
            None => (color, lw),
        }
    }
    /// Record the end, join and fill style `color` prints with for the primitive pushed next.
    fn note_pen(&mut self, color: Ink) {
        let Some(s) = self.styles else { return };
        let pen = s.pen(color, self.named);
        if !pen.is_object() {
            let i = self.list.prims.len();
            self.list.pens.resize(i, Pen::default());
            self.list.pens.push(pen);
        }
    }
    fn polyline(&mut self, ctx: &Ctx, color: Ink, lw: f32, pts: &[Vec2]) {
        if pts.len() < 2 {
            return;
        }
        self.note_pen(color);
        let (color, lw) = self.styled(color, lw);
        self.push_polyline(ctx, color, lw, pts);
    }
    /// A one-pixel line in `color` as the plot style prints it, without the style's lineweight
    /// or pen: the centre line drawn on screen over a thin wide polyline.
    fn hairline(&mut self, ctx: &Ctx, color: Ink, pts: &[Vec2]) {
        if pts.len() < 2 {
            return;
        }
        let (color, _) = self.styled(color, 0.0);
        self.push_polyline(ctx, color, 0.0, pts);
    }
    fn push_polyline(&mut self, ctx: &Ctx, color: Ink, lw: f32, pts: &[Vec2]) {
        let start = self.list.verts.len() as u32;
        for p in pts {
            let q = ctx.xf.apply(*p);
            self.list.bounds.add(q);
            self.list.verts.push(q);
        }
        self.list.prims.push(DPrim {
            handle: ctx.top,
            color: color.rgb,
            aci7: color.aci7,
            lw,
            transparency: self.transparency,
            kind: Kind::Polyline,
            start,
            len: pts.len() as u32,
        });
    }
    fn tris(&mut self, ctx: &Ctx, color: Ink, tris: &[Vec2]) {
        self.tris_at(ctx, color, tris, ctx.xf);
    }
    fn tris_at(&mut self, ctx: &Ctx, color: Ink, tris: &[Vec2], xf: Mat3) {
        if tris.len() < 3 {
            return;
        }
        self.note_pen(color);
        let (color, _) = self.styled(color, 0.0);
        let start = self.list.tris.len() as u32;
        for p in tris {
            let q = xf.apply(*p);
            self.list.bounds.add(q);
            self.list.tris.push(q);
        }
        self.list.prims.push(DPrim {
            handle: ctx.top,
            color: color.rgb,
            aci7: color.aci7,
            lw: 0.0,
            transparency: self.transparency,
            kind: Kind::Tris,
            start,
            len: tris.len() as u32,
        });
    }
    /// Shaped text: strokes as polylines; TrueType glyphs filled (TEXTFILL) or outlined.
    fn shaped(&mut self, ctx: &Ctx, color: Ink, lw: f32, sh: &cadcraft_fonts::Shaped) {
        for s in &sh.strokes {
            self.polyline(ctx, color, lw, s);
        }
        if sh.glyphs.is_empty() {
            return;
        }
        let fill = self.opts.fill && ctx.d.header.i64("TEXTFILL", 1) != 0;
        // Plots note which filled outlines are font glyphs (see `DisplayList::texts`).
        let mut text = sh.text.iter().peekable();
        let pixel = if self.plotting { 0.0 } else { self.opts.min_dash * 0.5 };
        for (i, g) in sh.glyphs.iter().enumerate() {
            if fill {
                while text.next_if(|t| t.outline < i).is_some() {}
                let glyph = text.next_if(|t| t.outline == i);
                let at = self.list.prims.len();
                let len = if let Some(t) = glyph {
                    let xf = crate::text::placement(t, ctx.xf);
                    let mesh = crate::text::mesh(t, xf, pixel);
                    self.tris_at(ctx, color, &mesh.triangles, xf);
                    mesh.triangles.len()
                } else {
                    let tris = fill::triangulate_evenodd(g);
                    self.tris(ctx, color, &tris);
                    tris.len()
                };
                if let Some(t) = glyph.filter(|_| self.plotting)
                    && self.list.prims.len() > at
                    && let (Ok(prim), Ok(verts)) = (u32::try_from(at), u32::try_from(len))
                {
                    let xf = &ctx.xf;
                    let glyph = cadcraft_fonts::TextGlyph { origin: xf.apply(t.origin), x: xf.apply_vec(t.x), y: xf.apply_vec(t.y), ..t.clone() };
                    let bounds = self.list.prims.get(at).map(|p| Bounds2::from_points(self.list.points(p).iter().copied())).unwrap_or(Bounds2::EMPTY);
                    self.list.texts.push(PlotText { prim, verts, bounds, glyph });
                }
            } else {
                for c in g {
                    self.polyline(ctx, color, lw, c);
                }
            }
        }
    }
    fn point(&mut self, ctx: &Ctx, color: Ink, p: Vec2) {
        let (color, _) = self.styled(color, 0.0);
        let q = ctx.xf.apply(p);
        self.list.bounds.add(q);
        let start = self.list.verts.len() as u32;
        self.list.verts.push(q);
        self.list.prims.push(DPrim {
            handle: ctx.top,
            color: color.rgb,
            aci7: color.aci7,
            lw: 0.0,
            transparency: self.transparency,
            kind: Kind::Point,
            start,
            len: 1,
        });
    }
    fn infinite(&mut self, ctx: &Ctx, color: Ink, base: Vec2, dir: Vec2, ray: bool) {
        let (color, _) = self.styled(color, 0.0);
        let start = self.list.verts.len() as u32;
        self.list.verts.push(ctx.xf.apply(base));
        self.list.verts.push(ctx.xf.apply_vec(dir).normalized());
        self.list.prims.push(DPrim {
            handle: ctx.top,
            color: color.rgb,
            aci7: color.aci7,
            lw: 0.0,
            transparency: self.transparency,
            kind: Kind::Infinite { ray },
            start,
            len: 2,
        });
    }
    /// Take one unit of the block-expansion budget; false once it is spent.
    fn expand(&mut self) -> bool {
        self.list.expanded = self.list.expanded.saturating_add(1);
        match self.shared_budget {
            Some(spent) => spent.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < MAX_BLOCK_EXPANSION,
            None => self.list.expanded <= MAX_BLOCK_EXPANSION,
        }
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
        lt_factor: 1.0,
        block_pstyle: None,
        block_transparency: 0,
    }
}

fn build_space(d: &Drawing, space: &Space, opts: &Options, plotting: bool) -> DisplayList {
    let opts = &with_point_view(d, space, opts);
    let styler = style_table(d, space, opts, plotting).map(|t| plotstyle::Styler::new(d, t));
    let mut b = Builder { list: DisplayList::default(), opts, plotting, styles: styler.as_ref(), named: None, shared_budget: None, transparency: 0 };
    if let Space::Paper(name) = space {
        b.list.sheet = paper::sheet(d, name);
    }
    if let Some(store) = d.space(space) {
        let paper = matches!(space, Space::Paper(_));
        #[cfg(not(target_arch = "wasm32"))]
        if !paper && !plotting && b.styles.is_none() && store.len() >= 2 * PARALLEL_CHUNK {
            build_parallel(&mut b, d, store);
            return b.list;
        }
        let mut viewports = 0usize;
        for e in store.iter() {
            if paper && let EntityKind::Viewport(vp) = &e.kind {
                if viewports < MAX_VIEWPORTS {
                    viewports += 1;
                    viewport(&mut b, d, e, vp);
                    if plotting {
                        take_viewport_layers(&mut b.list);
                    }
                }
                continue;
            }
            entity(&mut b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
        }
    }
    // A layout is drawn in paper units, so its pens become geometry now.
    if let Some(sheet) = b.list.sheet {
        pen::apply(&mut b.list, sheet.unit_mm);
    }
    b.list
}

/// Top-level entities per task when a display list is built on all cores.
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_CHUNK: usize = 256;

/// Build model space for the screen on all cores: the entities are split into chunks built
/// side by side, then joined in drawing order, so the list is the same as a one-thread build.
/// The canvas rebuilds the whole list at every 2x zoom step (in the background since zooming
/// stopped blocking the UI) and after every edit; one core made both slow on real drawings. Plots, layouts and plot styles keep the one-thread
/// path (they run once, and their pens and per-primitive layers are built along the way).
#[cfg(not(target_arch = "wasm32"))]
fn build_parallel(b: &mut Builder, d: &Drawing, store: &cadcraft_doc::EntityStore) {
    use rayon::prelude::*;
    let ents: Vec<&std::sync::Arc<Entity>> = store.iter().collect();
    let budget = std::sync::atomic::AtomicUsize::new(0);
    let (opts, plotting) = (b.opts, b.plotting);
    let parts: Vec<DisplayList> = ents
        .par_chunks(PARALLEL_CHUNK)
        .map(|chunk| {
            let mut part =
                Builder { list: DisplayList::default(), opts, plotting, styles: None, named: None, shared_budget: Some(&budget), transparency: 0 };
            for e in chunk {
                entity(&mut part, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
            }
            part.list
        })
        .collect();
    // One allocation per array for the joined list.
    b.list.prims.reserve(parts.iter().map(|p| p.prims.len()).sum());
    b.list.verts.reserve(parts.iter().map(|p| p.verts.len()).sum());
    b.list.tris.reserve(parts.iter().map(|p| p.tris.len()).sum());
    for part in parts {
        append(&mut b.list, part);
    }
}

/// Append `src` to `dst`: its primitives index into `dst`'s vertex and triangle arrays.
#[cfg(not(target_arch = "wasm32"))]
fn append(dst: &mut DisplayList, src: DisplayList) {
    let (verts, tris, prims) = (dst.verts.len() as u32, dst.tris.len() as u32, dst.prims.len() as u32);
    // Pens are parallel to the primitives, with missing entries meaning "use object".
    if !src.pens.is_empty() {
        dst.pens.resize(dst.prims.len(), Pen::default());
    }
    dst.prims.extend(src.prims.into_iter().map(|mut p| {
        p.start += if matches!(p.kind, Kind::Tris | Kind::Mask) { tris } else { verts };
        p
    }));
    dst.verts.extend(src.verts);
    dst.tris.extend(src.tris);
    dst.prim_layers.extend(src.prim_layers);
    dst.texts.extend(src.texts.into_iter().map(|mut t| {
        if t.prim < PlotText::CLIPPED {
            t.prim += prims;
        }
        t
    }));
    dst.pens.extend(src.pens);
    dst.bounds = dst.bounds.union(&src.bounds);
    dst.expanded = dst.expanded.saturating_add(src.expanded);
}

/// The plot style table a build draws with: the one in `opts`, else (on screen) the layout's
/// when its page setup has "Display plot styles" on.
fn style_table(d: &Drawing, space: &Space, opts: &Options, plotting: bool) -> Option<std::sync::Arc<cadcraft_doc::PlotStyleTable>> {
    opts.plot_style_table.clone().or_else(|| match space {
        Space::Paper(n) if !plotting => d
            .layout(n)
            .filter(|l| l.page.show_plot_styles)
            .and_then(|l| cadcraft_doc::plot_style_table(d, &l.page.plot_style_table))
            .map(std::sync::Arc::new),
        _ => None,
    })
}

/// `opts` with a view height for relative point sizes: the extents height when none is given
/// and points draw as figures sized relative to the view.
fn with_point_view(d: &Drawing, space: &Space, opts: &Options) -> Options {
    let mut o = opts.clone();
    let relative = d.header.f64("PDSIZE", 0.0) <= 0.0 && point::sized(d.header.i64("PDMODE", 0));
    if !(o.view_height.is_finite() && o.view_height > 0.0) && relative {
        let ext = d.extents(space);
        o.view_height = if ext.is_empty() { 0.0 } else { ext.height().max(ext.width() * 1e-3) };
    }
    o
}

/// Upper bound on viewports drawn per layout (hostile files).
const MAX_VIEWPORTS: usize = 256;

/// A paper-space viewport: its border (as an ordinary entity) and model space seen through it.
/// Viewport id 1 is the paper-space view itself and is not drawn. A clipped viewport shows
/// model space through its clip boundary's outline (the boundary entity draws itself, so the
/// rectangle is not drawn); a viewport turned off shows only its border.
fn viewport(b: &mut Builder, d: &Drawing, e: &Entity, vp: &cadcraft_doc::Viewport) {
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

/// Build the display list for a set of loose entities (previews, rubber bands).
pub fn build_entities<'a, I: IntoIterator<Item = &'a Entity>>(d: &Drawing, ents: I, opts: &Options) -> DisplayList {
    let ents: Vec<&Entity> = ents.into_iter().collect();
    // Only previews holding points need the model extents for relative point sizes.
    let with_view;
    let opts = if ents.iter().any(|e| matches!(e.kind, EntityKind::Point(_))) {
        with_view = with_point_view(d, &Space::Model, opts);
        &with_view
    } else {
        opts
    };
    let mut b = Builder { list: DisplayList::default(), opts, plotting: false, styles: None, named: None, shared_budget: None, transparency: 0 };
    for e in ents {
        entity(&mut b, &top_ctx(d, Mat3::IDENTITY, e.handle, &[], &[]), e);
    }
    b.list
}

/// The layer `e` is drawn on, as an index in the layer table: layer 0 inside a block is the
/// block reference's layer.
fn entity_layer(ctx: &Ctx, e: &Entity) -> u32 {
    let name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    ctx.d.layers.iter().position(|l| l.name.eq_ignore_ascii_case(name)).and_then(|i| u32::try_from(i).ok()).unwrap_or(NO_LAYER)
}

/// After a viewport is appended to a plot: the layers its contents carry in their handles
/// (see [`entity`]) go to [`DisplayList::prim_layers`], and the handles back to
/// [`VIEWPORT_CONTENT`]. The viewport's border was tagged when it was drawn.
///
/// Text glyphs seen through the viewport (marked in the handles' high half) keep their outline
/// primitive when it came through the clip whole; a glyph cut by the viewport edge stays
/// geometry.
fn take_viewport_layers(list: &mut DisplayList) {
    let start = list.prim_layers.len();
    for i in start..list.prims.len() {
        let Some(p) = list.prims.get(i).copied() else { break };
        list.prim_layers.push(u32::try_from(p.handle.0 & 0xFFFF_FFFF).unwrap_or(NO_LAYER));
        let bounds = Bounds2::from_points(list.points(&p).iter().copied());
        if let Some(t) = (p.handle.0 >> 32).checked_sub(1).and_then(|k| list.texts.get_mut(k as usize)) {
            let same = |a: Vec2, b: Vec2| a.dist(b) <= 1e-9 * (1.0 + a.len());
            let whole = t.prim == PlotText::NONE
                && p.kind == Kind::Tris
                && p.len == t.verts
                && same(bounds.min, t.bounds.min)
                && same(bounds.max, t.bounds.max);
            // A second piece (or a cut one) means the clip changed the glyph.
            t.prim = if whole { u32::try_from(i).unwrap_or(PlotText::NONE) } else { PlotText::CLIPPED };
        }
        if let Some(p) = list.prims.get_mut(i) {
            p.handle = VIEWPORT_CONTENT;
        }
    }
}

/// An object's colour, lineweight (mm), linetype, linetype scale, visibility and the transparency
/// of the layer it is drawn on (for ByLayer, see [`transparency`]).
type Resolved = (Ink, f32, Option<cadcraft_doc::Linetype>, f64, bool, u8);

fn resolve(ctx: &Ctx, e: &Entity, plotting: bool) -> Resolved {
    resolve_common(ctx, &e.common, plotting)
}

/// [`resolve`] for an object's properties `c`.
fn resolve_common(ctx: &Ctx, c: &cadcraft_doc::Common, plotting: bool) -> Resolved {
    let d = ctx.d;
    // Layer "0" inside a block takes the insert's layer.
    let layer_name = if c.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { c.layer.as_str() };
    let layer = d.layer(layer_name);
    let visible =
        c.visible && layer.is_none_or(|l| l.visible() && (!plotting || l.plot)) && !ctx.frozen.iter().any(|f| f.eq_ignore_ascii_case(layer_name));
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let rgb = ink(c.color, layer_color, ctx.block_color);
    let lw = match c.lineweight {
        Lineweight::ByLayer => layer.map(|l| l.lineweight).unwrap_or(Lineweight::Default),
        Lineweight::ByBlock => ctx.block_lw,
        x => x,
    };
    let lw_mm = match lw {
        Lineweight::Mm100(v) => f32::from(v) / 100.0,
        _ => default_lineweight(d),
    };
    let lt_name = match c.linetype.to_ascii_lowercase().as_str() {
        "bylayer" => layer.map(|l| l.linetype.clone()).unwrap_or_else(|| "Continuous".into()),
        "byblock" => ctx.block_ltype.clone(),
        _ => c.linetype.clone(),
    };
    let lt = d.linetype(&lt_name).filter(|l| !l.pattern.is_empty()).cloned();
    let scale = d.header.f64("LTSCALE", 1.0) * c.ltscale * ctx.lt_factor;
    (rgb, lw_mm, lt, scale, visible, layer.map_or(0, |l| l.transparency))
}

/// The transparency (percent, 0..=90) of an object whose own setting is `t`: the percentage, its
/// layer's `layer` when ByLayer (layer 0 inside a block: the block reference's layer, as
/// [`resolve_common`] finds it), the block reference's `block` when ByBlock (opaque outside
/// blocks).
fn transparency(t: cadcraft_doc::Transparency, layer: u8, block: u8) -> u8 {
    let t = match t {
        cadcraft_doc::Transparency::Percent(p) => p,
        cadcraft_doc::Transparency::ByBlock => block,
        cadcraft_doc::Transparency::ByLayer => layer,
    };
    t.min(MAX_TRANSPARENCY)
}

/// The width in mm that the "Default" lineweight stands for: `LWDEFAULT` (hundredths of a mm,
/// 0 to 211), 0.25 mm when it is unset or out of range.
fn default_lineweight(d: &Drawing) -> f32 {
    match d.header.i64("LWDEFAULT", 25) {
        v @ 0..=211 => v as f32 / 100.0,
        _ => 0.25,
    }
}

fn entity(b: &mut Builder, ctx: &Ctx, e: &Entity) {
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
        EntityKind::Dimension(dm) => {
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
                    let covered =
                        |be: &Entity| block_text_box(ctx.d, be, 0.0).is_some_and(|t| solids.iter().any(|s| t.iter().all(|p| solid_covers(s, *p))));
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
                dimension(b, ctx, e, dm, lw, lt.as_ref(), ltscale);
            }
        }
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
        EntityKind::Wipeout(w) => {
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
        EntityKind::MLeader(m) => {
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
        line_spacing_exact: t.line_spacing_exact,
        font: cadcraft_fonts::TextFont::resolve(&ts.font),
        width_factor: ts.width_factor,
        oblique: ts.oblique,
        columns: t.columns.as_ref().map(|c| cadcraft_fonts::MTextColumnSpec {
            dynamic: c.kind == cadcraft_doc::MTextColumnType::Dynamic,
            count: usize::from(c.count.max(1)),
            gutter: c.gutter,
            height: c.height,
            heights: if c.kind == cadcraft_doc::MTextColumnType::Dynamic && !c.auto_height { c.heights.clone() } else { Vec::new() },
            reversed: c.flow_reversed,
        }),
    };
    cadcraft_fonts::layout_mtext_with(&t.contents, &params)
}

fn mtext_color(c: Option<cadcraft_fonts::MTextColor>, rgb: Ink) -> Ink {
    match c {
        Some(cadcraft_fonts::MTextColor::Aci(i)) => {
            u8::try_from(i).ok().filter(|i| *i > 0).map(|i| ink(Color::Index(i), Color::Index(7), Color::Index(7))).unwrap_or(rgb)
        }
        Some(cadcraft_fonts::MTextColor::Rgb(v)) => {
            Ink { rgb: Rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8), aci7: false, aci: 0 }
        }
        None => rgb,
    }
}

fn mtext(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::MText, rgb: Ink, lw: f32) {
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

/// Draw a polyline with a linetype (None: continuous).
fn stroke_with(b: &mut Builder, ctx: &Ctx, rgb: Ink, lw: f32, lt: Option<&cadcraft_doc::Linetype>, ltscale: f64, pts: &[Vec2]) {
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

/// `lw` / `lt` are the dimension's own lineweight and linetype (ByBlock in DIMLWD, DIMLWE,
/// DIMLTYPE, DIMLTEX1 and DIMLTEX2).
fn dimension(b: &mut Builder, ctx: &Ctx, e: &Entity, dm: &cadcraft_doc::Dimension, lw: f32, lt: Option<&cadcraft_doc::Linetype>, ltscale: f64) {
    let d = ctx.d;
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
    let g = dimension_in(d, dm);
    // DIMCLRD / DIMCLRE / DIMCLRT: ByBlock = the dimension's own colour.
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let own = match e.common.color {
        Color::ByLayer => layer_color,
        Color::ByBlock => ctx.block_color,
        c => c,
    };
    let dim_rgb = ink(style.dim_line_color, layer_color, own);
    let ext_rgb = ink(style.ext_line_color, layer_color, own);
    let txt_rgb = ink(style.text_color, layer_color, own);
    let layer = d.layer(layer_name);
    let weight = |v: i16| -> f32 {
        if !b.opts.lineweights {
            return 0.0;
        }
        match Lineweight::from_dxf(v) {
            Lineweight::ByBlock => lw,
            Lineweight::Mm100(x) => f32::from(x) / 100.0,
            Lineweight::ByLayer => match layer.map(|l| l.lineweight) {
                Some(Lineweight::Mm100(x)) => f32::from(x) / 100.0,
                _ => default_lineweight(d),
            },
            Lineweight::Default => default_lineweight(d),
        }
    };
    let ltype = |name: &str| -> Option<cadcraft_doc::Linetype> {
        match name.trim().to_ascii_lowercase().as_str() {
            "" | "byblock" => lt.cloned(),
            "bylayer" => layer.and_then(|l| d.linetype(&l.linetype)).filter(|l| !l.pattern.is_empty()).cloned(),
            _ => d.linetype(name.trim()).filter(|l| !l.pattern.is_empty()).cloned(),
        }
    };
    let (dim_lw, ext_lw) = (weight(style.dim_lineweight), weight(style.ext_lineweight));
    let (dim_lt, ext1_lt, ext2_lt) = (ltype(&style.dim_linetype), ltype(&style.ext_linetype1), ltype(&style.ext_linetype2));
    let line_style = |i: usize| match g.line_roles.get(i).unwrap_or(&LineRole::Dim) {
        LineRole::Ext if g.ext2_lines.contains(&i) => (ext_rgb, ext_lw, ext2_lt.as_ref()),
        LineRole::Ext => (ext_rgb, ext_lw, ext1_lt.as_ref()),
        LineRole::Dim => (dim_rgb, dim_lw, dim_lt.as_ref()),
    };
    // The text's marks (arc length symbol, DIMINSPECT frame) come last, over the text background.
    for (i, l) in g.lines.iter().enumerate().filter(|(i, _)| !g.text_marks.contains(i)) {
        let (rgb, w, t) = line_style(i);
        stroke_with(b, ctx, rgb, w, t, ltscale, l);
    }
    for t in &g.fills {
        b.tris(ctx, dim_rgb, t);
    }
    dim_text_fill(b, ctx, e, &style, &g.text_fill);
    for (i, l) in g.lines.iter().enumerate().filter(|(i, _)| g.text_marks.contains(i)) {
        let (rgb, w, t) = line_style(i);
        stroke_with(b, ctx, rgb, w, t, ltscale, l);
    }
    if b.opts.text {
        let sh = cadcraft_fonts::Shaped { strokes: g.text.clone(), glyphs: g.text_glyphs.clone(), width: 0.0, text: g.text_runs.clone() };
        b.shaped(ctx, txt_rgb, lw, &sh);
    }
}

/// DIMTFILL: a dimension's text background (`corners` from [`DimGeometry::text_fill`]) hides
/// what is drawn before it, in the drawing's background color (1) or DIMTFILLCLR (2).
fn dim_text_fill(b: &mut Builder, ctx: &Ctx, e: &Entity, style: &cadcraft_doc::DimStyle, corners: &[Vec2]) {
    let [p0, p1, p2, p3] = corners else { return };
    if !b.opts.fill || style.text_fill == 0 {
        return;
    }
    // DIMTFILLCLR ByBlock = the dimension's own color, as DIMCLRT.
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let own = match e.common.color {
        Color::ByLayer => layer_color,
        Color::ByBlock => ctx.block_color,
        c => c,
    };
    let n = b.list.prims.len();
    b.tris(ctx, ink(style.text_fill_color, layer_color, own), &[*p0, *p1, *p2, *p0, *p2, *p3]);
    if style.text_fill == 1
        && let Some(p) = b.list.prims.get_mut(n)
    {
        p.kind = Kind::Mask;
    }
}

/// A dimension block's text, or a line that is a segment of one of the text's marks in `g`
/// (as the `*D` block writer splits them): where the background goes.
fn starts_text_marks(be: &Entity, g: &DimGeometry) -> bool {
    match &be.kind {
        EntityKind::MText(_) | EntityKind::Text(_) => true,
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            g.text_marks
                .iter()
                .filter_map(|i| g.lines.get(*i))
                .any(|m| m.windows(2).any(|w| matches!(w, [p, q] if p.dist(a) < 1e-6 && q.dist(b) < 1e-6)))
        }
        _ => false,
    }
}

/// The background of a dimension block's MTEXT or TEXT: its box widened by `gap` on every side,
/// turned with the text (corners in order); `None` for other entities.
fn block_text_box(d: &Drawing, be: &Entity, gap: f64) -> Option<Vec<Vec2>> {
    let t = match &be.kind {
        EntityKind::MText(t) => t,
        EntityKind::Text(t) => return text_box(d, t, gap),
        _ => return None,
    };
    let (bounds, at, rot) = (layout_mtext_entity(d, &cadcraft_doc::MText { rotation: 0.0, ..t.clone() }).bounds, t.insert.xy(), t.rotation);
    if !(bounds.width().is_finite() && bounds.height().is_finite()) || bounds.width() <= 0.0 {
        return None;
    }
    let (lo, hi) = (bounds.min - Vec2::new(gap, gap), bounds.max + Vec2::new(gap, gap));
    let (x, y) = (Vec2::from_angle(rot), Vec2::from_angle(rot).perp());
    let turn = |p: Vec2| at + x * (p.x - at.x) + y * (p.y - at.y);
    Some([lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)].into_iter().map(turn).collect())
}

/// The background of a TEXT as placed: the box of its strokes and glyph outlines along the text's
/// direction (the baseline from the insertion to the alignment point for Aligned and Fit),
/// widened by `gap` on every side.
fn text_box(d: &Drawing, t: &cadcraft_doc::Text, gap: f64) -> Option<Vec<Vec2>> {
    let (sh, _) = place_text_entity(d, t, &t.value);
    let rot = match (t.halign, t.align_pt) {
        (cadcraft_doc::HAlign::Aligned | cadcraft_doc::HAlign::Fit, Some(p)) if p.xy().dist(t.insert.xy()) > 1e-12 => t.insert.xy().angle_to(p.xy()),
        _ => t.rotation,
    };
    let (x, y) = (Vec2::from_angle(rot), Vec2::from_angle(rot).perp());
    let pts: Vec<Vec2> = sh.strokes.iter().flatten().chain(sh.glyphs.iter().flatten().flatten()).copied().collect();
    let first = pts.first()?;
    let start = (first.dot(x), first.dot(y), first.dot(x), first.dot(y));
    let (s0, u0, s1, u1) = pts.iter().fold(start, |(s0, u0, s1, u1), p| (s0.min(p.dot(x)), u0.min(p.dot(y)), s1.max(p.dot(x)), u1.max(p.dot(y))));
    if !(s1 - s0 > 0.0 && (s0 + s1 + u0 + u1).is_finite()) {
        return None;
    }
    let at = |s: f64, u: f64| x * s + y * u;
    Some(vec![at(s0 - gap, u0 - gap), at(s1 + gap, u0 - gap), at(s1 + gap, u1 + gap), at(s0 - gap, u1 + gap)])
}

/// Most texts of one dimension block that get a DIMTFILL background, and most of its SOLIDs
/// checked for covering one.
const MAX_FILL_TEXTS: usize = 64;
const MAX_FILL_SOLIDS: usize = 256;

/// Whether `be` is a SOLID that covers `p` (a text background the block holds already, as the
/// `*D` block of DIMTFILL 2 does).
fn solid_covers(be: &Entity, p: Vec2) -> bool {
    let EntityKind::Solid(s) = &be.kind else { return false };
    // SOLID corners go 1, 2, 4, 3 around the outline.
    let [c1, c2, c3, c4] = s.corners.map(|c| c.xy());
    let ring = [c1, c2, c4, c3];
    let next = [c2, c4, c3, c1];
    let sides: Vec<f64> = ring.iter().zip(next.iter()).map(|(a, b)| (*b - *a).cross(p - *a)).filter(|c| c.abs() > 1e-12).collect();
    !sides.is_empty() && (sides.iter().all(|c| *c > 0.0) || sides.iter().all(|c| *c < 0.0))
}

mod table;
pub use table::{MAX_LAYOUT_CELLS, TableFill, TableGeom, TableLine, TableText, table_covered, table_geometry, table_style_of};

fn table(b: &mut Builder, ctx: &Ctx, t: &cadcraft_doc::Table, rgb: Ink, lw: f32) {
    table::draw(b, ctx, t, rgb, lw);
}

/// A TEXT's justification as the fonts' alignment (used where text is placed outside a build).
pub fn text_align(h: cadcraft_doc::HAlign, v: cadcraft_doc::VAlign) -> (cadcraft_fonts::Align, cadcraft_fonts::VAlign) {
    align(h, v)
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
        // The reference's own linetype scale (group 48) multiplies that of the objects inside,
        // as LTSCALE does for the drawing (tests/block_ltscale.rs).
        lt_factor: ctx.lt_factor * if e.common.ltscale.is_finite() && e.common.ltscale > 0.0 { e.common.ltscale } else { 1.0 },
        block_pstyle: ctx.block_pstyle,
        // The reference's own, set by the caller (`Builder::transparency` while it draws).
        block_transparency: ctx.block_transparency,
    }
}

fn insert(b: &mut Builder, ctx: &Ctx, e: &Entity, ins: &cadcraft_doc::Insert, rgb: Ink) {
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
                let sub = Ctx { block_pstyle: b.named, block_transparency: b.transparency, ..sub_ctx(ctx, e, m) };
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
        // ATTMODE (ATTDISP): 0 shows no attributes, 1 the visible ones, 2 all of them.
        let attmode = ctx.d.header.i64("ATTMODE", 1);
        // The reference's transparency; each attribute draws with its own.
        let reference = b.transparency;
        for a in &ins.attribs {
            if attmode == 0 || (a.invisible && attmode != 2) {
                continue;
            }
            // A multiline attribute is drawn as its multiline text.
            let multiline = a.multiline_text();
            let sh = if multiline.is_none() { Some(place_text_entity(ctx.d, &a.text, &a.text.value).0) } else { None };
            // Properties of the attribute's definition (as AutoCAD copies them to the attribute
            // on insertion), except those set on the attribute itself: ByBlock = the reference's,
            // layer 0 = the reference's layer.
            let def = ctx.d.block(&ins.block).and_then(|blk| blk.entities.iter().find(|be| attdef_tag(be, &a.tag)));
            let (ink, lw, shown, t) = match (def, a.props.is_empty()) {
                (None, true) => (rgb, resolve(ctx, e, b.plotting).1, true, reference),
                (def, _) => {
                    let c = a.props.over(&def.map(|d| d.common.clone()).unwrap_or_else(by_block));
                    let (ink, lw, _, _, shown, layer_t) = resolve_common(&sub_ctx(ctx, e, Mat3::IDENTITY), &c, b.plotting);
                    let t = if b.opts.transparency { transparency(c.transparency, layer_t, reference) } else { 0 };
                    (ink, lw, shown, t)
                }
            };
            if shown {
                b.transparency = t;
                let lw = if b.opts.lineweights { lw } else { 0.0 };
                match (&multiline, &sh) {
                    (Some(m), _) => mtext(b, ctx, m, ink, lw),
                    (None, Some(sh)) => b.shaped(ctx, ink, lw, sh),
                    (None, None) => {}
                }
            }
        }
        b.transparency = reference;
    }
}

/// Properties that follow the block reference's (an attribute without a definition).
fn by_block() -> cadcraft_doc::Common {
    cadcraft_doc::Common {
        color: Color::ByBlock,
        linetype: "ByBlock".into(),
        lineweight: Lineweight::ByBlock,
        transparency: cadcraft_doc::Transparency::ByBlock,
        ..cadcraft_doc::Common::default()
    }
}

/// The attribute definition with tag `tag` (any case).
fn attdef_tag(e: &Entity, tag: &str) -> bool {
    matches!(&e.kind, EntityKind::AttDef(d) if d.tag.eq_ignore_ascii_case(tag))
}

/// The overall scale of a leader drawn with dimension style `st`: its DIMSCALE, else the
/// drawing's.
fn leader_scale(d: &Drawing, st: Option<&cadcraft_doc::DimStyle>) -> f64 {
    let dimscale = d.header.f64("DIMSCALE", 1.0);
    st.map(|s| s.effective_scale(dimscale)).unwrap_or_else(|| cadcraft_doc::DimStyle::default().effective_scale(dimscale))
}

#[cfg(test)]
mod tests;
