use super::draw::entity;
use super::props::{resolve, resolve_common, transparency};
use super::text_entity::{mtext, place_text_entity};
use super::{Builder, Color, Ctx, Entity, EntityKind, Ink, Lineweight, Mat3, Vec2};

pub(crate) fn sub_ctx<'a>(ctx: &Ctx<'a>, e: &Entity, m: Mat3) -> Ctx<'a> {
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

/// Blocks with fewer entities than this are drawn whole (measuring is not worth it).
const CULL_MIN_ENTITIES: usize = 64;

/// The entities of a large block with their bounds (block coordinates, entity order) and a grid
/// over them, so the entities near an area are found without visiting every one: hovering over
/// a site plan held in one block searched its millions of entities at every mouse move.
pub struct BlockIndex {
    /// The block's entities, in its order (held: indexed access, and kept alive with the memo).
    pub entities: Vec<std::sync::Arc<Entity>>,
    /// Each entity's bounds, widened by a tenth of its size (approximate text boxes).
    pub bounds: Vec<super::Bounds2>,
    grid: Grid,
}

struct Grid {
    area: super::Bounds2,
    cols: usize,
    rows: usize,
    cells: Vec<Vec<u32>>,
    /// Entities with no usable bounds, or spanning many cells: always candidates.
    always: Vec<u32>,
}

/// Cells per grid side at most, and the most cells one entity is listed in.
const GRID_MAX_SIDE: usize = 1024;
const GRID_MAX_SPAN: usize = 256;

impl Grid {
    fn build(bounds: &[super::Bounds2]) -> Grid {
        let mut area = super::Bounds2::EMPTY;
        for b in bounds.iter().filter(|b| usable(b)) {
            area = area.union(b);
        }
        // About four entities per cell, the cells as square as the area allows.
        let target = (bounds.len() as f64 / 4.0).clamp(1.0, (GRID_MAX_SIDE * GRID_MAX_SIDE) as f64);
        let (w, h) = (area.width(), area.height());
        let (cols, rows) = if area.is_empty() || !(w > 0.0 || h > 0.0) || !w.is_finite() || !h.is_finite() {
            (1, 1)
        } else if h <= 0.0 {
            (target.min(GRID_MAX_SIDE as f64).max(1.0) as usize, 1)
        } else if w <= 0.0 {
            (1, target.min(GRID_MAX_SIDE as f64).max(1.0) as usize)
        } else {
            let cols = (target * w / h).sqrt().round().clamp(1.0, GRID_MAX_SIDE as f64);
            let rows = (target / cols).round().clamp(1.0, GRID_MAX_SIDE as f64);
            (cols as usize, rows as usize)
        };
        let mut g = Grid { area, cols, rows, cells: vec![Vec::new(); cols * rows], always: Vec::new() };
        for (i, b) in bounds.iter().enumerate() {
            let i = u32::try_from(i).unwrap_or(u32::MAX);
            match g.span(b).filter(|(c0, r0, c1, r1)| (c1 - c0 + 1).saturating_mul(r1 - r0 + 1) <= GRID_MAX_SPAN) {
                Some((c0, r0, c1, r1)) => {
                    for r in r0..=r1 {
                        for c in c0..=c1 {
                            if let Some(cell) = g.cells.get_mut(r * cols + c) {
                                cell.push(i);
                            }
                        }
                    }
                }
                None => g.always.push(i),
            }
        }
        g
    }

    /// The cell range (col0, row0, col1, row1) an area covers, clamped to the grid; `None` for an
    /// unusable area.
    fn span(&self, b: &super::Bounds2) -> Option<(usize, usize, usize, usize)> {
        if !usable(b) || self.area.is_empty() {
            return None;
        }
        let w = self.area.width().max(1e-300);
        let h = self.area.height().max(1e-300);
        let cell = |v: f64, lo: f64, size: f64, n: usize| -> usize {
            let f = ((v - lo) / size * n as f64).floor();
            if f.is_finite() { f.clamp(0.0, (n - 1) as f64) as usize } else { 0 }
        };
        Some((
            cell(b.min.x, self.area.min.x, w, self.cols),
            cell(b.min.y, self.area.min.y, h, self.rows),
            cell(b.max.x, self.area.min.x, w, self.cols),
            cell(b.max.y, self.area.min.y, h, self.rows),
        ))
    }
}

fn usable(b: &super::Bounds2) -> bool {
    !b.is_empty() && b.min.x.is_finite() && b.min.y.is_finite() && b.max.x.is_finite() && b.max.y.is_finite()
}

impl BlockIndex {
    /// Indices of the entities whose bounds meet `local` (block coordinates), in block order;
    /// entities without usable bounds are always included.
    pub fn near(&self, local: &super::Bounds2) -> Vec<usize> {
        // Entities listed apart (long or without bounds): those whose bounds meet the area.
        let mut out: Vec<u32> =
            self.grid.always.iter().copied().filter(|&i| self.bounds.get(i as usize).is_none_or(|b| !usable(b) || b.intersects(local))).collect();
        if usable(local)
            && local.intersects(&self.grid.area.expand(self.grid.area.width().max(self.grid.area.height()) * 0.01))
            && let Some((c0, r0, c1, r1)) = self.grid.span(local)
        {
            for r in r0..=r1 {
                for c in c0..=c1 {
                    if let Some(cell) = self.grid.cells.get(r * self.grid.cols + c) {
                        out.extend(cell.iter().copied().filter(|&i| self.bounds.get(i as usize).is_some_and(|b| b.intersects(local))));
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out.into_iter().map(|i| i as usize).collect()
    }
}

/// Block indexes kept between builds for the drawing state they were made in (see
/// `pipeline::bounds_context`, which covers the block definitions by address). The memo holds
/// every block definition of that state and each indexed block, so a changed block's new
/// allocation never takes an old address. Each block has one slot: threads that meet the same block together wait for one build
/// instead of each building its index.
struct BlockMemo {
    context: u64,
    _blocks: Vec<std::sync::Arc<cadcraft_doc::Block>>,
    by_block: std::collections::HashMap<usize, (std::sync::Arc<cadcraft_doc::Block>, IndexSlot)>,
}

type IndexSlot = std::sync::Arc<std::sync::OnceLock<std::sync::Arc<BlockIndex>>>;

static BLOCK_BOUNDS: std::sync::Mutex<Option<BlockMemo>> = std::sync::Mutex::new(None);

/// The [`BlockIndex`] of `blk`, made once per drawing state and kept: the renderer, picking and
/// object snaps cull a large block with it.
pub fn block_index(d: &cadcraft_doc::Drawing, blk: &std::sync::Arc<cadcraft_doc::Block>) -> std::sync::Arc<BlockIndex> {
    block_index_in(d, blk, super::pipeline::bounds_context(d))
}

/// [`block_index`] with the drawing state's `pipeline::bounds_context` already worked out (once
/// per build rather than once per reference).
pub(crate) fn block_index_in(d: &cadcraft_doc::Drawing, blk: &std::sync::Arc<cadcraft_doc::Block>, context: u64) -> std::sync::Arc<BlockIndex> {
    let key = std::sync::Arc::as_ptr(blk) as usize;
    let slot = BLOCK_BOUNDS.lock().ok().and_then(|mut m| {
        if m.as_ref().is_none_or(|m| m.context != context) {
            *m = Some(BlockMemo { context, _blocks: d.blocks.values().cloned().collect(), by_block: std::collections::HashMap::new() });
        }
        m.as_mut().map(|m| std::sync::Arc::clone(&m.by_block.entry(key).or_insert_with(|| (std::sync::Arc::clone(blk), IndexSlot::default())).1))
    });
    match slot {
        Some(slot) => std::sync::Arc::clone(slot.get_or_init(|| build_block_index(d, blk))),
        None => build_block_index(d, blk),
    }
}

fn build_block_index(d: &cadcraft_doc::Drawing, blk: &cadcraft_doc::Block) -> std::sync::Arc<BlockIndex> {
    let mut cache = cadcraft_doc::BlockBoundsCache::default();
    let entities: Vec<std::sync::Arc<Entity>> = blk.entities.iter().cloned().collect();
    let bounds: Vec<super::Bounds2> = entities
        .iter()
        .map(|be| {
            // Depth 1, as the renderer draws them: entities inside a block are not scaled for
            // the current annotation scale the way top-level annotative ones are.
            let b = cadcraft_doc::entity_bounds_cached(d, be, 1, &mut cache);
            let size = b.width().max(b.height());
            if usable(&b) && size.is_finite() { b.expand(size * 0.1) } else { b }
        })
        .collect();
    let grid = Grid::build(&bounds);
    std::sync::Arc::new(BlockIndex { entities, bounds, grid })
}

pub(crate) fn insert(b: &mut Builder, ctx: &Ctx, e: &Entity, ins: &cadcraft_doc::Insert, rgb: Ink) {
    if ctx.depth >= cadcraft_doc::MAX_BLOCK_DEPTH {
        return;
    }
    if let Some(blk) = ctx.d.block(&ins.block) {
        // On screen, a large block's entities away from the build window are left
        // out, so a site plan held in one block draws only the part in view, and they no longer
        // spend the block-expansion budget that would cut the visible part short.
        let window = b.opts.view.filter(|w| !b.plotting && blk.entities.len() >= CULL_MIN_ENTITIES && !w.is_empty());
        let index = window.map(|_| block_index_in(ctx.d, blk, b.bounds_context.unwrap_or_else(|| super::pipeline::bounds_context(ctx.d))));
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
                // The window in this copy's block coordinates.
                let local = window.and_then(|w| {
                    let inv = sub.xf.inverse()?;
                    Some(super::Bounds2::from_points(w.corners().iter().map(|p| inv.apply(*p))))
                });
                match (index.as_ref(), local.as_ref()) {
                    // Only the entities near the window, from the grid, in block order.
                    (Some(ix), Some(local)) => {
                        for i in ix.near(local) {
                            let Some(be) = ix.entities.get(i) else { continue };
                            if matches!(be.kind, EntityKind::AttDef(_)) {
                                continue;
                            }
                            if !b.expand() {
                                break 'copies;
                            }
                            entity(b, &sub, be);
                        }
                    }
                    _ => {
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
