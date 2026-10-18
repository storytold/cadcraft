//! Skipping block contents outside the view. A zoomed-in screen build (`Options::only` set)
//! already leaves out top-level objects far from the view; a large block reference near it (a
//! whole site plan inserted as one block) would still be drawn entirely. Its entities' bounds,
//! in block coordinates, are measured once per block definition and layer state and kept here,
//! so each build tests them against the view brought into the block's coordinates.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use cadcraft_doc::{Block, Drawing, entity_bounds};
use cadcraft_geom::Bounds2;

/// Block definitions whose entity bounds are kept (the oldest is dropped past it).
const MAX_BLOCKS: usize = 512;

/// (definition, layer-visibility key, bounds of each entity in draw order).
type Entry = (Arc<Block>, u64, Arc<Vec<Bounds2>>);

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

/// The bounds of every entity of `blk` (block coordinates, draw order).
pub(crate) fn entity_bounds_of(d: &Drawing, blk: &Arc<Block>) -> Arc<Vec<Bounds2>> {
    let key = layer_key(d);
    if let Ok(cache) = CACHE.lock()
        && let Some((_, _, b)) = cache.iter().find(|(blk2, k, _)| *k == key && Arc::ptr_eq(blk2, blk))
    {
        return b.clone();
    }
    let bounds: Arc<Vec<Bounds2>> = Arc::new(blk.entities.iter().map(|e| entity_bounds(d, e, 1)).collect());
    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() >= MAX_BLOCKS {
            cache.remove(0);
        }
        cache.push((blk.clone(), key, bounds.clone()));
    }
    bounds
}
