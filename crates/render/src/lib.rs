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

mod table;
pub use table::{MAX_LAYOUT_CELLS, TableFill, TableGeom, TableLine, TableText, table_covered, table_geometry, table_style_of};

mod blocks;
mod dim_draw;
mod display;
mod draw;
mod pipeline;
mod props;
mod text_entity;
mod viewports;

pub use dim_draw::{dim_text, dimension_in};
pub use display::{DPrim, DisplayList, Kind, MAX_TRANSPARENCY, NO_LAYER, PlotText, VIEWPORT_CONTENT, alpha};
pub use pipeline::{Options, build, build_entities, build_plot};
use props::{default_lineweight, resolve_common};
use text_entity::text_style;
pub use text_entity::{layout_mtext_entity, place_text_entity, text_align};
pub use viewports::{viewport_clip, viewport_content};

#[cfg(test)]
mod tests;
