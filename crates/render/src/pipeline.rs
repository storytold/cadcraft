#[cfg(not(target_arch = "wasm32"))]
use super::display::append;
use super::display::{DisplayList, Kind, NO_LAYER, PlotText, VIEWPORT_CONTENT};
use super::draw::entity;
use super::viewports::{MAX_VIEWPORTS, viewport};
use super::{Bounds2, Builder, Color, Ctx, Drawing, Entity, EntityKind, Handle, Lineweight, Mat3, Space, Vec2, paper, pen, plotstyle, point};

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

/// Build the display list for a space. For a layout (paper space) this includes the model-space
/// geometry seen through each viewport, clipped to the viewport, and [`DisplayList::sheet`].
pub fn build(d: &Drawing, space: &Space, opts: &Options) -> DisplayList {
    build_space(d, space, opts, false)
}

/// Like [`build`] but for plotting: layers marked "do not plot" (such as `Defpoints`) are left out.
pub fn build_plot(d: &Drawing, space: &Space, opts: &Options) -> DisplayList {
    build_space(d, space, opts, true)
}

pub(crate) fn top_ctx<'a>(d: &'a Drawing, xf: Mat3, top: Handle, frozen: &'a [String], vp_colors: &'a [(String, Color)]) -> Ctx<'a> {
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
pub(crate) const PARALLEL_CHUNK: usize = 256;

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

/// The plot style table a build draws with: the one in `opts`, else (on screen) the layout's
/// when its page setup has "Display plot styles" on.
pub(crate) fn style_table(d: &Drawing, space: &Space, opts: &Options, plotting: bool) -> Option<std::sync::Arc<cadcraft_doc::PlotStyleTable>> {
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
pub(crate) fn with_point_view(d: &Drawing, space: &Space, opts: &Options) -> Options {
    let mut o = opts.clone();
    let relative = d.header.f64("PDSIZE", 0.0) <= 0.0 && point::sized(d.header.i64("PDMODE", 0));
    if !(o.view_height.is_finite() && o.view_height > 0.0) && relative {
        let ext = d.extents(space);
        o.view_height = if ext.is_empty() { 0.0 } else { ext.height().max(ext.width() * 1e-3) };
    }
    o
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
