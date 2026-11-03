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
    /// Draw lines with their linetypes. Off, every line is drawn whole and continuous, as picking
    /// sees it: a dashed line is hit across its gaps, and a small pattern on a long line makes
    /// millions of dashes.
    pub linetypes: bool,
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
            linetypes: true,
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
    let mut b = Builder {
        list: DisplayList::default(),
        opts,
        plotting,
        styles: styler.as_ref(),
        named: None,
        shared_budget: None,
        transparency: 0,
        bounds_context: None,
    };
    if let Space::Paper(name) = space {
        b.list.sheet = paper::sheet(d, name);
    }
    if let Some(store) = d.space(space) {
        let paper = matches!(space, Space::Paper(_));
        // Worked out once per windowed screen build (model space, or a layout and the
        // viewports in it) for the large blocks culled against the window.
        if !plotting && opts.view.is_some() {
            b.bounds_context = Some(bounds_context(d));
        }
        let shown = on_screen(d, store, opts, paper || plotting);
        let shown_at = |i: usize| shown.as_ref().is_none_or(|s| s.get(i).copied().unwrap_or(true));
        #[cfg(not(target_arch = "wasm32"))]
        if !paper && !plotting && b.styles.is_none() && store.len() >= 2 * PARALLEL_CHUNK {
            build_parallel(&mut b, d, store, &shown_at);
            return b.list;
        }
        let mut viewports = 0usize;
        for (i, e) in store.iter().enumerate() {
            if !shown_at(i) {
                continue;
            }
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

/// Which top-level entities of a model-space screen build lie near enough to the
/// build window (`opts.view`, the visible area with its pan margin) to be drawn; `None` draws
/// everything (no window, a layout, a plot). The canvas rebuilds when the view leaves the window,
/// so what is left out comes back as soon as it can be seen. Bounds are approximate (text boxes are
/// estimated), so each is widened by a tenth of its size; an entity with no bounds is kept, and so
/// is one whose bounds can fall far short of what is drawn (see [`super::blocks::estimated`]).
/// On a 300 MB site plan the list no longer holds every primitive of the drawing at every zoom.
fn on_screen(d: &Drawing, store: &cadcraft_doc::EntityStore, opts: &Options, everything: bool) -> Option<Vec<bool>> {
    let window = opts.view.filter(|w| !everything && !w.is_empty() && w.width().is_finite() && w.height().is_finite())?;
    let bounds = model_bounds(d, store);
    Some(
        bounds
            .iter()
            .map(|(b, estimated)| {
                let size = b.width().max(b.height());
                *estimated || b.is_empty() || !size.is_finite() || b.expand(size * 0.1).intersects(&window)
            })
            .collect(),
    )
}

/// The bounds measured for the last drawing built, kept between builds: the canvas rebuilds its
/// list at every zoom step and after every edit, and measuring a site plan's every insert anew
/// costs more than the build it saves. Each entry holds its entity, so an edited entity's new
/// allocation can never take the address of one still listed here.
struct BoundsMemo {
    /// What the bounds of an unchanged entity also depend on: the block definitions (held, so
    /// compared by address), and a hash of layer visibility and annotation scale.
    blocks: Vec<std::sync::Arc<cadcraft_doc::Block>>,
    context: u64,
    /// Each entity's bounds, and whether they are only an estimate that can fall short of what
    /// is drawn (see [`super::blocks::estimated`]).
    by_entity: std::collections::HashMap<usize, (std::sync::Arc<Entity>, Bounds2, bool)>,
}

/// Shared and never taken out: screen builds run in the background while the pointer picks, and
/// a pick that found the memo missing would measure a site plan's huge block reference from
/// scratch (about 0.9 s per mouse move on a 330 MB drawing).
static BOUNDS_MEMO: std::sync::Mutex<Option<std::sync::Arc<BoundsMemo>>> = std::sync::Mutex::new(None);

pub(crate) fn bounds_context(d: &Drawing) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in &d.layers {
        (&l.name, l.on, l.frozen).hash(&mut h);
    }
    d.current_anno_scale().factor().to_bits().hash(&mut h);
    d.anno_all_visible().hash(&mut h);
    d.header.f64("DIMSCALE", 1.0).to_bits().hash(&mut h);
    // Whether points draw as a sized figure decides whether they may be left out.
    d.header.i64("PDMODE", 0).hash(&mut h);
    d.header.f64("PDSIZE", 0.0).to_bits().hash(&mut h);
    // Dimensions and text are measured with their styles, so an edited style changes the bounds
    // of entities that did not change themselves: hash every style's content, not their count.
    d.dim_styles.len().hash(&mut h);
    d.text_styles.len().hash(&mut h);
    {
        use std::fmt::Write;
        let mut w = HashWrite(&mut h);
        for s in &d.dim_styles {
            let _ = write!(w, "{s:?}");
        }
        for s in &d.text_styles {
            let _ = write!(w, "{s:?}");
        }
    }
    // The block definitions by address: a block's bounds include those of the blocks it
    // references, so an edited nested block (a new allocation) must not reuse them. Whoever keeps
    // bounds made in this state also holds these blocks, so no new block can take their addresses.
    d.blocks.len().hash(&mut h);
    for b in d.blocks.values() {
        (std::sync::Arc::as_ptr(b) as usize).hash(&mut h);
    }
    h.finish()
}

/// Feeds formatted text to a hasher without building a string.
struct HashWrite<'a, H: std::hash::Hasher>(&'a mut H);

impl<H: std::hash::Hasher> std::fmt::Write for HashWrite<'_, H> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// The bounds the last screen build measured for `e` (a model-space entity of this
/// drawing state), if it measured them: picking an insert of a site-plan block then needs no new
/// pass over the block. `None` when unknown (another drawing, or `e` not built yet).
pub fn known_bounds(d: &Drawing, e: &Entity) -> Option<Bounds2> {
    known_bounds_of(d)?.get(e)
}

/// The bounds the last screen build measured, checked against the drawing state
/// once, for looking up many entities (the pick index measures every entity of model space on
/// its first use: 1.2 s on a 330 MB drawing whose screen build had just measured them all).
pub struct KnownBounds(std::sync::Arc<BoundsMemo>);

impl KnownBounds {
    /// The bounds measured for `e`, if the last screen build measured them.
    pub fn get(&self, e: &Entity) -> Option<Bounds2> {
        self.0.by_entity.get(&(e as *const Entity as usize)).map(|(_, b, _)| *b)
    }
}

/// [`KnownBounds`] for this drawing state, `None` when the last screen build was of another.
pub fn known_bounds_of(d: &Drawing) -> Option<KnownBounds> {
    let context = bounds_context(d);
    let m = BOUNDS_MEMO.lock().ok()?.clone()?;
    let same_blocks = m.blocks.len() == d.blocks.len() && m.blocks.iter().zip(d.blocks.values()).all(|(a, b)| std::sync::Arc::ptr_eq(a, b));
    (m.context == context && same_blocks).then_some(KnownBounds(m))
}

/// Bounds of model space's entities in store order, with whether each is only an estimate,
/// measuring only those not measured for the previous build of the same drawing state
/// ([`BoundsMemo`]).
fn model_bounds(d: &Drawing, store: &cadcraft_doc::EntityStore) -> Vec<(Bounds2, bool)> {
    let context = bounds_context(d);
    let previous = BOUNDS_MEMO.lock().ok().and_then(|m| m.clone());
    let same_blocks =
        |m: &BoundsMemo| m.blocks.len() == d.blocks.len() && m.blocks.iter().zip(d.blocks.values()).all(|(a, b)| std::sync::Arc::ptr_eq(a, b));
    let empty = std::collections::HashMap::new();
    let old = previous.as_deref().filter(|m| m.context == context && same_blocks(m)).map_or(&empty, |m| &m.by_entity);
    let mut cache = cadcraft_doc::BlockBoundsCache::default();
    let mut estimated = std::collections::HashMap::new();
    let mut by_entity = std::collections::HashMap::with_capacity(store.len());
    let mut out = Vec::with_capacity(store.len());
    for e in store.iter() {
        let key = std::sync::Arc::as_ptr(e) as usize;
        let (b, est) = match old.get(&key) {
            Some((_, b, est)) => (*b, *est),
            None => (cadcraft_doc::entity_bounds_cached(d, e, 0, &mut cache), super::blocks::estimated(d, e, &mut estimated, 0)),
        };
        by_entity.insert(key, (std::sync::Arc::clone(e), b, est));
        out.push((b, est));
    }
    drop(previous);
    if let Ok(mut m) = BOUNDS_MEMO.lock() {
        *m = Some(std::sync::Arc::new(BoundsMemo { blocks: d.blocks.values().cloned().collect(), context, by_entity }));
    }
    out
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
fn build_parallel(b: &mut Builder, d: &Drawing, store: &cadcraft_doc::EntityStore, shown: &dyn Fn(usize) -> bool) {
    use rayon::prelude::*;
    let ents: Vec<&std::sync::Arc<Entity>> = store.iter().enumerate().filter(|(i, _)| shown(*i)).map(|(_, e)| e).collect();
    let budget = std::sync::atomic::AtomicUsize::new(0);
    let (opts, plotting, bounds_context) = (b.opts, b.plotting, b.bounds_context);
    // Left to the entities near the view, a site plan's heavy block references sit
    // side by side and a few 256-entity chunks would hold nearly all the work; chunks of about a
    // sixteenth of each core's share let the cores even it out between them.
    let chunk = (ents.len() / rayon::current_num_threads().max(1).saturating_mul(16)).clamp(1, PARALLEL_CHUNK);
    let parts: Vec<DisplayList> = ents
        .par_chunks(chunk)
        .map(|chunk| {
            let mut part = Builder {
                list: DisplayList::default(),
                opts,
                plotting,
                styles: None,
                named: None,
                shared_budget: Some(&budget),
                transparency: 0,
                bounds_context,
            };
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
    let mut b = Builder {
        list: DisplayList::default(),
        opts,
        plotting: false,
        styles: None,
        named: None,
        shared_budget: None,
        transparency: 0,
        bounds_context: None,
    };
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
