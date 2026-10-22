//! Leaving out what lies wholly outside the area a build covers. This is the one place that
//! decides it for every build, screen or plot: the window a build culls against, whether an
//! object reaches it, and which entities of a block reference reach it once the window is
//! brought into the block's coordinates. Nested references are culled the same way at each
//! level. An object without measurable bounds (an infinite line, a ray) always reaches.
//!
//! A model-space plot with a view culls (its view grown by [`MARGIN`]); other builds cull
//! nothing. A builder holds its window in `Builder::cull`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use cadcraft_doc::{Block, Drawing, Entity, EntityKind, Space, entity_bounds};
use cadcraft_geom::{Bounds2, Mat3};

use crate::Options;

/// Margin around a plot window, as a fraction of its larger side, within which objects are
/// still built (lineweights and rounding reach a little past an object's bounds).
pub(crate) const MARGIN: f64 = 0.02;

/// The window a build of `space` culls against (world units of that space), or `None` to build
/// everything: a model-space plot with a view culls against the view grown by [`MARGIN`].
pub(crate) fn window(opts: &Options, space: &Space, plotting: bool) -> Option<Bounds2> {
    opts.view.filter(|w| plotting && matches!(space, Space::Model) && !w.is_empty()).map(|w| w.expand(MARGIN * w.width().max(w.height())))
}

/// Whether an object with bounds `b` can reach `window`: it has no measurable bounds, or they
/// meet the window.
pub(crate) fn reaches(b: &Bounds2, window: &Bounds2) -> bool {
    b.is_empty() || b.intersects(window)
}

/// Whether the top-level object `e` can reach `window`.
pub(crate) fn entity_reaches(d: &Drawing, e: &Entity, window: &Bounds2) -> bool {
    reaches(&entity_bounds(d, e, 0), window)
}

/// The block entities of `blk`, drawn with `xf` (block coordinates to the space being built),
/// that can reach `window`: `None` builds them all (`xf` cannot be inverted); otherwise one flag
/// per entity, in draw order. The window is brought into block coordinates as the box around
/// its corners, so a rotated or skewed reference keeps everything that may show.
pub(crate) fn block_entities(d: &Drawing, blk: &Arc<Block>, xf: &Mat3, window: &Bounds2) -> Option<Vec<bool>> {
    let inv = xf.inverse()?;
    let local = Bounds2::from_points(window.corners().map(|p| inv.apply(p)));
    Some(block_bounds(d, blk).iter().map(|b| reaches(b, &local)).collect())
}

/// Block definitions whose entity bounds are kept (the oldest is dropped past it).
const MAX_BLOCKS: usize = 512;

/// The definitions a block draws through references, by the name it refers to them by (`None`:
/// missing when measured).
type Nested = Vec<(String, Option<Arc<Block>>)>;

/// (definition, layer-visibility key, the definitions it reaches, bounds of each entity in draw
/// order).
type Entry = (Arc<Block>, u64, Nested, Arc<Vec<Bounds2>>);

static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// Which layers are on and thawed: nested block contents on hidden layers take no room, so the
/// bounds depend on it.
fn layer_key(d: &Drawing) -> u64 {
    let mut h = DefaultHasher::new();
    for l in &d.layers {
        (l.name.as_str(), l.on, l.frozen).hash(&mut h);
    }
    h.finish()
}

/// Every definition `blk` reaches through block references, dimension blocks and multileader
/// blocks, at any depth, each once.
fn nested(d: &Drawing, blk: &Block) -> Nested {
    let mut out: Nested = Vec::new();
    let mut todo: Vec<Arc<Block>> = Vec::new();
    let visit = |name: &str, out: &mut Nested, todo: &mut Vec<Arc<Block>>| {
        if out.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
            return;
        }
        let def = d.block(name).cloned();
        if let Some(def) = &def {
            todo.push(def.clone());
        }
        out.push((name.to_string(), def));
    };
    let scan = |b: &Block, out: &mut Nested, todo: &mut Vec<Arc<Block>>| {
        for e in b.entities.iter() {
            match &e.kind {
                EntityKind::Insert(ins) => visit(&ins.block, out, todo),
                EntityKind::Dimension(dm) => {
                    if let Some(n) = &dm.block {
                        visit(n, out, todo);
                    }
                }
                EntityKind::MLeader(m) => {
                    if let Some(ins) = &m.block {
                        visit(&ins.block, out, todo);
                    }
                }
                _ => {}
            }
        }
    };
    scan(blk, &mut out, &mut todo);
    while let Some(b) = todo.pop() {
        scan(&b, &mut out, &mut todo);
    }
    out
}

/// Whether the definitions `nested` were measured with are still the drawing's.
fn unchanged(d: &Drawing, nested: &Nested) -> bool {
    nested.iter().all(|(name, def)| match (d.block(name), def) {
        (Some(now), Some(then)) => Arc::ptr_eq(now, then),
        (None, None) => true,
        _ => false,
    })
}

/// The bounds of every entity of `blk` (block coordinates, draw order), measured once per block
/// definition, layer state and set of nested definitions. The cache holds the definitions
/// themselves, so an edited block, or an edited block nested in it, is measured again.
fn block_bounds(d: &Drawing, blk: &Arc<Block>) -> Arc<Vec<Bounds2>> {
    let key = layer_key(d);
    if let Ok(cache) = CACHE.lock()
        && let Some((_, _, _, b)) = cache.iter().find(|(blk2, k, n, _)| *k == key && Arc::ptr_eq(blk2, blk) && unchanged(d, n))
    {
        return b.clone();
    }
    let bounds: Arc<Vec<Bounds2>> = Arc::new(blk.entities.iter().map(|e| entity_bounds(d, e, 1)).collect());
    let deps = nested(d, blk);
    if let Ok(mut cache) = CACHE.lock() {
        cache.retain(|(blk2, k, _, _)| !(*k == key && Arc::ptr_eq(blk2, blk)));
        if cache.len() >= MAX_BLOCKS {
            cache.remove(0);
        }
        cache.push((blk.clone(), key, deps, bounds.clone()));
    }
    bounds
}
