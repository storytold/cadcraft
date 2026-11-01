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

/// 3D Orbit orientation for model space: azimuth (`yaw`) around world Z, elevation tilt (`pitch`)
/// from the Top 2D plan view (`0.0`), and the 3D `pivot` point in world coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Orbit3d {
    pub yaw: f64,
    pub pitch: f64,
    pub pivot: cadcraft_geom::Vec3,
}

impl Orbit3d {
    /// True when looking straight down `-Z` with zero twist (the standard 2D Top plan view).
    pub fn is_top(&self) -> bool {
        self.yaw.abs() <= 1e-9 && self.pitch.abs() <= 1e-9
    }

    /// Orthographically project a 3D world point `p` onto the 2D view plane around `self.pivot`.
    pub fn project(&self, p: cadcraft_geom::Vec3) -> Vec2 {
        if self.is_top() {
            return p.xy();
        }
        let d = p - self.pivot;
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let x1 = d.x * cy - d.y * sy;
        let y1 = d.x * sy + d.y * cy;
        let y2 = y1 * cp + d.z * sp;
        Vec2::new(self.pivot.x + x1, self.pivot.y + y2)
    }

    /// Project a 3D direction vector onto the 2D view plane.
    pub fn project_vec(&self, v: cadcraft_geom::Vec3) -> Vec2 {
        if self.is_top() {
            return v.xy();
        }
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let x1 = v.x * cy - v.y * sy;
        let y1 = v.x * sy + v.y * cy;
        let y2 = y1 * cp + v.z * sp;
        Vec2::new(x1, y2)
    }

    /// The 2D affine transform mapping a point `(x, y)` on the world `z = 0` work plane to its
    /// projected 2D view-plane coordinates ([`Self::project`] at `z = 0`).
    pub fn xy_mat3(&self) -> cadcraft_geom::Mat3 {
        if self.is_top() {
            return cadcraft_geom::Mat3::IDENTITY;
        }
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let a = cy;
        let c = -sy;
        let b = sy * cp;
        let d = cy * cp;
        let e = self.pivot.x - a * self.pivot.x - c * self.pivot.y;
        let f = self.pivot.y - self.pivot.z * sp - b * self.pivot.x - d * self.pivot.y;
        cadcraft_geom::Mat3 { a, b, c, d, e, f }
    }

    /// Whether the 3D orbit looks along the `z = 0` work plane edge-on (`cos(pitch)` ≈ 0, e.g.
    /// Front, Back, Left, or Right views), so 2D view-plane points cannot be unprojected onto `z = 0`.
    pub fn is_edge_on(&self) -> bool {
        !self.is_top() && self.pitch.cos().abs() < 1e-4
    }

    /// Map a 2D view-plane point `p` back onto the world `z = 0` work plane (the inverse of
    /// [`Self::project`] at `z = 0`), or `None` when the work plane is edge-on ([`Self::is_edge_on`]).
    pub fn unproject_xy(&self, p: Vec2) -> Option<Vec2> {
        self.unproject_z(p, 0.0)
    }

    /// Map a 2D view-plane point `p` back onto the horizontal plane at world elevation `z` (the
    /// inverse of [`Self::project`] at `z`), or `None` when the plane is edge-on ([`Self::is_edge_on`]).
    pub fn unproject_z(&self, p: Vec2, z: f64) -> Option<Vec2> {
        if self.is_top() {
            return p.is_finite().then_some(p);
        }
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        if cp.abs() < 1e-4 {
            return None;
        }
        let x1 = p.x - self.pivot.x;
        let y2 = p.y - self.pivot.y;
        let y1 = (y2 - (z - self.pivot.z) * sp) / cp;
        let dx = x1 * cy + y1 * sy;
        let dy = -x1 * sy + y1 * cy;
        let out = Vec2::new(self.pivot.x + dx, self.pivot.y + dy);
        out.is_finite().then_some(out)
    }

    /// Conjugate a world-XY 2D affine transform `m` into projected 2D view-plane coordinates
    /// (`P * m * P^-1`), matching the result of applying `m` in world XY before projecting.
    pub fn project_mat3(&self, m: cadcraft_geom::Mat3) -> cadcraft_geom::Mat3 {
        if self.is_top() {
            return m;
        }
        let p = self.xy_mat3();
        match p.inverse() {
            Some(inv) => p.then_before(m).then_before(inv),
            None => m,
        }
    }

    /// Stable hash key for display-list caching (`0` in 2D Top view).
    pub fn hash_key(&self) -> u64 {
        if self.is_top() {
            return 0;
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            self.yaw.to_bits(),
            self.pitch.to_bits(),
            self.pivot.x.to_bits(),
            self.pivot.y.to_bits(),
            self.pivot.z.to_bits(),
        )
            .hash(&mut h);
        h.finish()
    }

    /// Short label for the ViewCube face (`"TOP"`, `"FRONT"`, `"RIGHT"`, `"ISO"`, `"3D"`).
    pub fn label(&self) -> &'static str {
        if self.is_top() {
            return "TOP";
        }
        let deg_y = self.yaw.to_degrees().rem_euclid(360.0);
        let deg_p = self.pitch.to_degrees();
        if (deg_p - 90.0).abs() < 2.0 {
            if !(2.0..=358.0).contains(&deg_y) {
                return "FRONT";
            }
            if (deg_y - 90.0).abs() < 2.0 {
                return "LEFT";
            }
            if (deg_y - 180.0).abs() < 2.0 {
                return "BACK";
            }
            if (deg_y - 270.0).abs() < 2.0 {
                return "RIGHT";
            }
        }
        if (deg_p - 54.7356).abs() < 3.0 {
            return "ISO";
        }
        "3D"
    }
}

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
    /// Accumulated Z elevation for 3D orbit projection.
    z_offset: f64,
    /// Accumulated Z scale for 3D orbit projection inside block references.
    z_scale: f64,
    /// Viewport model-to-paper transform when rendering inside a layout viewport.
    vp_xf: Mat3,
    /// Inverse of `vp_xf` (paper-to-model) when rendering inside a layout viewport.
    vp_xf_inv: Mat3,
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
    /// True when building a paper-space layout directly (ordinary paper-space entities stay in
    /// 2D paper coordinates; only model-space viewport contents use [`Options::orbit`]).
    paper: bool,
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
    /// The 3D orbit active for `ctx`: 2D Top for ordinary paper-space geometry, and
    /// [`Options::orbit`] for model space (including model space seen through a layout viewport).
    fn active_orbit(&self, ctx: &Ctx) -> Orbit3d {
        if self.paper && ctx.top != VIEWPORT_CONTENT {
            Orbit3d::default()
        } else {
            self.opts.orbit
        }
    }
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
    /// Project a 2D point `p` with additional local Z offset `extra_z` through `ctx` and the active 3D orbit.
    fn project_pt(&self, ctx: &Ctx, p: Vec2, extra_z: f64) -> Vec2 {
        let orbit = self.active_orbit(ctx);
        if orbit.is_top() {
            return ctx.xf.apply(p);
        }
        let z = ctx.z_offset + extra_z * ctx.z_scale;
        let q = ctx.xf.apply(p);
        if ctx.top == VIEWPORT_CONTENT {
            let m = ctx.vp_xf_inv.apply(q);
            let m_proj = orbit.project(cadcraft_geom::Vec3::new(m.x, m.y, z));
            ctx.vp_xf.apply(m_proj)
        } else {
            orbit.project(cadcraft_geom::Vec3::new(q.x, q.y, z))
        }
    }
    /// Append a 3D polyline primitive projected through the current context and 3D orbit.
    fn polyline_3d(&mut self, ctx: &Ctx, color: Ink, lw: f32, pts: &[cadcraft_geom::Vec3]) {
        if pts.len() < 2 {
            return;
        }
        self.note_pen(color);
        let (color, lw) = self.styled(color, lw);
        let start = self.list.verts.len() as u32;
        for p in pts {
            let q = self.project_pt(ctx, p.xy(), p.z);
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
    /// Push a styled 2D polyline primitive projected through `ctx`.
    fn push_polyline(&mut self, ctx: &Ctx, color: Ink, lw: f32, pts: &[Vec2]) {
        let start = self.list.verts.len() as u32;
        for p in pts {
            let q = self.project_pt(ctx, *p, 0.0);
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
    /// Append filled triangles transformed by `ctx.xf`.
    fn tris(&mut self, ctx: &Ctx, color: Ink, tris: &[Vec2]) {
        self.tris_at(ctx, color, tris, ctx.xf);
    }
    /// Append filled triangles transformed by `xf` and projected through the active 3D orbit.
    fn tris_at(&mut self, ctx: &Ctx, color: Ink, tris: &[Vec2], xf: Mat3) {
        if tris.len() < 3 {
            return;
        }
        self.note_pen(color);
        let (color, _) = self.styled(color, 0.0);
        let start = self.list.tris.len() as u32;
        let orbit = self.active_orbit(ctx);
        for p in tris {
            let q = if orbit.is_top() {
                xf.apply(*p)
            } else {
                let q0 = xf.apply(*p);
                if ctx.top == VIEWPORT_CONTENT {
                    let m = ctx.vp_xf_inv.apply(q0);
                    let m_proj = orbit.project(cadcraft_geom::Vec3::new(m.x, m.y, ctx.z_offset));
                    ctx.vp_xf.apply(m_proj)
                } else {
                    orbit.project(cadcraft_geom::Vec3::new(q0.x, q0.y, ctx.z_offset))
                }
            };
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
        let on_screen = !self.plotting && self.opts.min_dash > 0.0;
        let stroke_lw = if on_screen && lw * 3.78 <= 1.35 {
            -1.0
        } else {
            lw
        };
        for s in &sh.strokes {
            self.polyline(ctx, color, stroke_lw, s);
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
                    if on_screen && xf.a.hypot(xf.b).hypot(xf.c.hypot(xf.d)) * 1000.0 <= pixel * 9.0 {
                        for c in g {
                            self.polyline(ctx, color, stroke_lw, c);
                        }
                    }
                    mesh.triangles.len()
                } else {
                    let tris = crate::text::triangulate_placed(g);
                    self.tris(ctx, color, &tris);
                    if on_screen {
                        for c in g {
                            self.polyline(ctx, color, stroke_lw, c);
                        }
                    }
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
    /// Append a point primitive at `p` projected through `ctx`.
    fn point(&mut self, ctx: &Ctx, color: Ink, p: Vec2) {
        let (color, _) = self.styled(color, 0.0);
        let q = self.project_pt(ctx, p, 0.0);
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
    /// Append an infinite line (`XLINE`) or ray primitive projected through `ctx`.
    fn infinite(&mut self, ctx: &Ctx, color: Ink, base: Vec2, dir: Vec2, ray: bool) {
        let (color, _) = self.styled(color, 0.0);
        let start = self.list.verts.len() as u32;
        self.list.verts.push(self.project_pt(ctx, base, 0.0));
        let d2 = ctx.xf.apply_vec(dir);
        let orbit = self.active_orbit(ctx);
        let d_proj = if orbit.is_top() {
            d2.normalized()
        } else {
            orbit.project_vec(cadcraft_geom::Vec3::new(d2.x, d2.y, 0.0)).normalized()
        };
        self.list.verts.push(d_proj);
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
pub use display::{DPrim, DisplayList, Kind, MAX_TRANSPARENCY, NO_LAYER, PlotImage, PlotText, VIEWPORT_CONTENT, alpha};
pub use draw::entity_elevation;
pub use pipeline::{Options, build, build_entities, build_plot};
use props::{default_lineweight, resolve_common};
use text_entity::text_style;
pub use text_entity::{layout_mtext_entity, place_text_entity, style_font, text_align};
pub use viewports::{viewport_clip, viewport_content};

#[cfg(test)]
mod tests;
