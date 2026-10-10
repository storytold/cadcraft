//! R-tree spatial index over entity bounds, used by picking, window/crossing/fence selection and
//! object snaps so they look only at entities near the query instead of scanning the space.
//!
//! The index is cached per drawing space and keyed by *content identity*, not by a revision
//! number: the cache holds a clone of the space's [`EntityStore`] (a cheap copy of chunk
//! pointers) and of the block table, and an entry is reused only while the live store still
//! shares every chunk with it ([`EntityStore::same_as`]). Because the cache holds a reference,
//! any edit copies the touched chunk, so a stale index can never be returned — whether the edit
//! came from a command, an interactive prompt mid-command, undo/redo or a script. Undoing back to
//! a cached state reuses its index.
//!
//! Queries return candidates in draw order; callers then run exactly the same tests as the
//! brute-force scan, so results are identical (see the parity tests).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use cadcraft_doc::{Block, DimStyle, Drawing, Entity, EntityKind, EntityStore, Space, entity_bounds};
use cadcraft_geom::Bounds2;
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};

/// Spaces with fewer entities than this are scanned directly (building an index costs more).
pub const MIN_INDEXED: usize = 256;
/// Indexes kept alive (open drawings × spaces, plus a few undo states).
const CACHE_SIZE: usize = 4;

/// Tree items carry (chunk serial, index within the chunk); draw indices are derived at query
/// time so inserting or removing entities never invalidates other chunks' items.
type Item = GeomWithData<Rectangle<[f64; 2]>, (u32, u32)>;

static NEXT_SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// One store chunk's entities and bounds, shared between successive indexes while unchanged.
struct ChunkIx {
    serial: u32,
    key: usize,
    ents: Vec<Arc<Entity>>,
    bounds: Vec<Bounds2>,
    /// Local indices of xlines and rays (unbounded; always candidates for pick and crossing).
    infinite: Vec<u32>,
}

impl ChunkIx {
    fn build(d: &Drawing, key: usize, items: &[Arc<Entity>]) -> ChunkIx {
        let mut infinite = Vec::new();
        let bounds = items
            .iter()
            .enumerate()
            .map(|(i, e)| {
                if matches!(e.kind, EntityKind::XLine(_) | EntityKind::Ray(_)) {
                    infinite.push(u32::try_from(i).unwrap_or(u32::MAX));
                }
                entity_bounds(d, e, 0)
            })
            .collect();
        let serial = NEXT_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ChunkIx { serial, key, ents: items.to_vec(), bounds, infinite }
    }
    fn items(&self) -> impl Iterator<Item = Item> + '_ {
        self.bounds.iter().enumerate().filter(|(_, b)| !b.is_empty()).map(|(i, b)| {
            GeomWithData::new(Rectangle::from_corners([b.min.x, b.min.y], [b.max.x, b.max.y]), (self.serial, u32::try_from(i).unwrap_or(u32::MAX)))
        })
    }
}

/// An R-tree over one space's entity bounds. Built incrementally from the previous index: only
/// store chunks that changed are re-measured and patched into the tree.
pub struct SpatialIndex {
    store: EntityStore,
    blocks: BTreeMap<String, Arc<Block>>,
    dim_styles: Vec<DimStyle>,
    dimscale: f64,
    /// (draw index of the chunk's first entity, chunk), in draw order.
    chunks: Vec<(u32, Arc<ChunkIx>)>,
    /// Chunk serial → draw offset.
    offsets: std::collections::HashMap<u32, u32>,
    tree: RTree<Item>,
    /// Draw indices of xlines and rays.
    infinite: Vec<u32>,
    len: usize,
}

impl SpatialIndex {
    /// Build an index over a store (no caching).
    pub fn build(d: &Drawing, store: &EntityStore) -> SpatialIndex {
        Self::build_from(d, store, None)
    }

    /// Build, reusing `prev` for chunks the two stores share (only when the bounds context —
    /// blocks, dimension styles, DIMSCALE — is the same).
    fn build_from(d: &Drawing, store: &EntityStore, prev: Option<&SpatialIndex>) -> SpatialIndex {
        let prev = prev.filter(|p| p.same_context(d));
        let old: std::collections::HashMap<usize, Arc<ChunkIx>> =
            prev.map(|p| p.chunks.iter().map(|(_, c)| (c.key, c.clone())).collect()).unwrap_or_default();
        let mut chunks = Vec::new();
        let mut fresh = Vec::new();
        let mut kept = std::collections::HashSet::new();
        let mut offset = 0u32;
        for (key, items) in store.chunk_slices() {
            let c = match old.get(&key) {
                Some(c) if c.ents.len() == items.len() => {
                    kept.insert(c.serial);
                    c.clone()
                }
                _ => {
                    let c = Arc::new(ChunkIx::build(d, key, items));
                    fresh.push(c.clone());
                    c
                }
            };
            let n = u32::try_from(c.ents.len()).unwrap_or(u32::MAX);
            chunks.push((offset, c));
            offset = offset.saturating_add(n);
        }
        // Patch the previous tree when most of it survives; otherwise bulk-load.
        let tree = match prev {
            Some(p) if kept.len() * 2 >= chunks.len().max(1) => {
                let mut tree = p.tree.clone();
                for (_, c) in p.chunks.iter().filter(|(_, c)| !kept.contains(&c.serial)) {
                    for it in c.items() {
                        tree.remove(&it);
                    }
                }
                for c in &fresh {
                    for it in c.items() {
                        tree.insert(it);
                    }
                }
                tree
            }
            _ => RTree::bulk_load(chunks.iter().flat_map(|(_, c)| c.items()).collect()),
        };
        let offsets = chunks.iter().map(|(off, c)| (c.serial, *off)).collect();
        let infinite = chunks.iter().flat_map(|(off, c)| c.infinite.iter().map(move |i| off + i)).collect();
        SpatialIndex {
            store: store.clone(),
            blocks: d.blocks.clone(),
            dim_styles: d.dim_styles.clone(),
            dimscale: d.header.f64("DIMSCALE", 1.0),
            chunks,
            offsets,
            tree,
            infinite,
            len: offset as usize,
        }
    }

    fn same_context(&self, d: &Drawing) -> bool {
        self.blocks.len() == d.blocks.len()
            && self.blocks.iter().zip(&d.blocks).all(|((ka, a), (kb, b))| ka == kb && Arc::ptr_eq(a, b))
            && self.dimscale.to_bits() == d.header.f64("DIMSCALE", 1.0).to_bits()
            && self.dim_styles == d.dim_styles
    }

    fn matches(&self, d: &Drawing, store: &EntityStore) -> bool {
        self.store.same_as(store) && self.same_context(d)
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    fn locate(&self, i: u32) -> Option<(&ChunkIx, usize)> {
        let pos = self.chunks.partition_point(|(off, _)| *off <= i).checked_sub(1)?;
        let (off, c) = self.chunks.get(pos)?;
        Some((c, (i - off) as usize))
    }
    /// The entity at draw index `i`.
    pub fn entity(&self, i: u32) -> Option<&Arc<Entity>> {
        let (c, l) = self.locate(i)?;
        c.ents.get(l)
    }
    /// The bounds of the entity at draw index `i` (as [`entity_bounds`] computes them).
    pub fn bounds(&self, i: u32) -> Bounds2 {
        self.locate(i).and_then(|(c, l)| c.bounds.get(l).copied()).unwrap_or(Bounds2::EMPTY)
    }

    /// Draw indices (ascending) of entities whose bounds touch `b`, optionally with the infinite
    /// lines. Returns `None` for a non-finite query box (callers fall back to a full scan).
    pub fn query(&self, b: &Bounds2, with_infinite: bool) -> Option<Vec<u32>> {
        if b.is_empty() {
            return Some(if with_infinite { self.infinite.clone() } else { Vec::new() });
        }
        if !(b.min.is_finite() && b.max.is_finite()) {
            return None;
        }
        // Pad a little so rounding in the callers' own predicates never loses a candidate.
        let pad = (b.width() + b.height()) * 1e-9 + (b.min.x.abs() + b.min.y.abs() + b.max.x.abs() + b.max.y.abs()) * 1e-12 + 1e-300;
        let env = AABB::from_corners([b.min.x - pad, b.min.y - pad], [b.max.x + pad, b.max.y + pad]);
        let mut out: Vec<u32> =
            self.tree.locate_in_envelope_intersecting(&env).filter_map(|it| self.offsets.get(&it.data.0).map(|off| off + it.data.1)).collect();
        if with_infinite {
            out.extend_from_slice(&self.infinite);
        }
        out.sort_unstable();
        out.dedup();
        Some(out)
    }
}

static CACHE: Mutex<Vec<Arc<SpatialIndex>>> = Mutex::new(Vec::new());

/// The cached index for a space, building it when the space changed. `None` when the space
/// doesn't exist or is small enough to scan directly.
pub fn index(d: &Drawing, space: &Space) -> Option<Arc<SpatialIndex>> {
    let store = d.space(space)?;
    if store.len() < MIN_INDEXED {
        return None;
    }
    Some(index_store(d, store))
}

/// The cached index for a store regardless of its size.
pub fn index_store(d: &Drawing, store: &EntityStore) -> Arc<SpatialIndex> {
    {
        let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(pos) = cache.iter().position(|ix| ix.matches(d, store)) {
            let ix = cache.remove(pos);
            cache.push(ix.clone());
            return ix;
        }
    }
    // The most recent index is the likeliest ancestor of this store (one edit ago): reuse its
    // unchanged chunks. Build outside the lock so other threads aren't blocked.
    let prev = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).last().cloned();
    let ix = Arc::new(SpatialIndex::build_from(d, store, prev.as_deref()));
    let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Drop entries whose store this one supersedes cheaply: keep the most recent ones.
    while cache.len() >= CACHE_SIZE {
        cache.remove(0);
    }
    cache.push(ix.clone());
    ix
}

/// Drop every cached index (frees memory held by closed drawings).
pub fn clear_cache() {
    CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{select, snap};
    use cadcraft_doc::{Common, Insert, Text};
    use cadcraft_geom::{PolyVertex, Vec2, Vec3};

    struct Rng(u64);
    impl Rng {
        fn f(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
        fn r(&mut self, lo: f64, hi: f64) -> f64 {
            lo + (hi - lo) * self.f()
        }
    }

    fn v3(x: f64, y: f64) -> Vec3 {
        Vec3::new(x, y, 0.0)
    }

    /// A random drawing mixing every kind the selection code special-cases, overlapping heavily.
    fn drawing(seed: u64, n: usize) -> Drawing {
        let mut d = Drawing::new_imperial();
        let mut blk = Block {
            name: "B".into(),
            base: Vec3::default(),
            entities: EntityStore::new(),
            description: String::new(),
            anonymous: false,
            xref_path: None,
            explodable: true,
            units: 0,
        };
        blk.entities
            .push(Entity::new(cadcraft_doc::Handle(1_000_000), EntityKind::Circle(cadcraft_doc::Circle { center: v3(0.0, 0.0), radius: 1.0 })));
        d.blocks.insert("B".into(), Arc::new(blk));
        d.ensure_layer("OFF");
        if let Some(l) = d.layer_mut("OFF") {
            l.on = false;
        }
        let mut r = Rng(seed.max(1));
        for i in 0..n {
            let (x, y) = (r.r(0.0, 100.0), r.r(0.0, 100.0));
            let s = r.r(0.2, 6.0);
            let kind = match i % 10 {
                0 | 1 => EntityKind::Line(cadcraft_doc::Line { a: v3(x, y), b: v3(x + r.r(-s, s), y + r.r(-s, s)) }),
                2 => EntityKind::Circle(cadcraft_doc::Circle { center: v3(x, y), radius: s * 0.5 }),
                3 => EntityKind::Arc(cadcraft_doc::Arc { center: v3(x, y), radius: s * 0.5, start: r.r(0.0, 3.0), end: r.r(3.2, 6.2) }),
                4 => EntityKind::LwPolyline(cadcraft_doc::LwPolyline {
                    vertices: (0..5)
                        .map(|k| PolyVertex {
                            p: Vec2::new(x + f64::from(k) * s * 0.3, y + r.r(-s, s) * 0.3),
                            bulge: if k == 1 { 0.5 } else { 0.0 },
                            start_width: 0.0,
                            end_width: 0.0,
                        })
                        .collect(),
                    closed: i % 3 == 0,
                    const_width: 0.0,
                    elevation: 0.0,
                    plinegen: false,
                }),
                5 => EntityKind::Text(Text {
                    insert: v3(x, y),
                    align_pt: None,
                    height: s * 0.3,
                    value: format!("T{i}"),
                    rotation: r.r(0.0, 1.0),
                    width_factor: 1.0,
                    oblique: 0.0,
                    style: "Standard".into(),
                    halign: Default::default(),
                    valign: Default::default(),
                }),
                6 => EntityKind::Point(cadcraft_doc::Point { p: v3(x, y), angle: 0.0 }),
                7 => EntityKind::Insert(Insert {
                    block: "B".into(),
                    insert: v3(x, y),
                    scale: Vec3::new(s * 0.3, s * 0.3, 1.0),
                    rotation: 0.0,
                    attribs: Vec::new(),
                    cols: 1,
                    rows: 1,
                    col_spacing: 0.0,
                    row_spacing: 0.0,
                }),
                8 if i % 40 == 8 => EntityKind::XLine(cadcraft_doc::RayLine { base: v3(x, y), dir: v3(r.r(-1.0, 1.0), 1.0) }),
                8 if i % 40 == 18 => EntityKind::Ray(cadcraft_doc::RayLine { base: v3(x, y), dir: v3(1.0, r.r(-1.0, 1.0)) }),
                _ => EntityKind::Circle(cadcraft_doc::Circle { center: v3(x, y), radius: s * 0.1 }),
            };
            let common = if i % 17 == 0 { Common { layer: "OFF".into(), ..Common::default() } } else { Common::default() };
            d.add(&Space::Model, common, kind).unwrap();
        }
        d
    }

    fn forced(d: &Drawing) -> Option<Arc<SpatialIndex>> {
        Some(Arc::new(SpatialIndex::build(d, &d.model)))
    }

    #[test]
    fn pick_parity_with_scan() {
        for seed in [3, 11, 97] {
            let d = drawing(seed, 600);
            let ix = forced(&d);
            let mut r = Rng(seed * 7 + 1);
            let mut hits = 0;
            for k in 0..400 {
                let p = Vec2::new(r.r(-5.0, 105.0), r.r(-5.0, 105.0));
                let ap = [0.05, 0.3, 1.5][k % 3];
                let a = select::pick_with(&d, &Space::Model, &ix, p, ap);
                let b = select::pick_with(&d, &Space::Model, &None, p, ap);
                assert_eq!(a, b, "seed {seed} p {p:?} ap {ap}");
                hits += usize::from(a.is_some());
            }
            assert!(hits > 20, "test should exercise hits ({hits})");
        }
    }

    #[test]
    fn window_and_fence_parity_with_scan() {
        for seed in [5, 23] {
            let d = drawing(seed, 500);
            let ix = forced(&d);
            let mut r = Rng(seed + 100);
            for k in 0..60 {
                let a = Vec2::new(r.r(-10.0, 110.0), r.r(-10.0, 110.0));
                let b = a + Vec2::new(r.r(-40.0, 40.0), r.r(-40.0, 40.0));
                let bx = Bounds2::new(a, b);
                for crossing in [false, true] {
                    let x = select::select_window_with(&d, &Space::Model, &ix, bx, crossing);
                    let y = select::select_window_with(&d, &Space::Model, &None, bx, crossing);
                    assert_eq!(x, y, "seed {seed} box {bx:?} crossing {crossing}");
                }
                let fence: Vec<Vec2> = (0..(2 + k % 3)).map(|_| Vec2::new(r.r(0.0, 100.0), r.r(0.0, 100.0))).collect();
                assert_eq!(select::select_fence_with(&d, &Space::Model, &ix, &fence), select::select_fence_with(&d, &Space::Model, &None, &fence));
            }
        }
    }

    #[test]
    fn osnap_parity_with_scan() {
        let d = drawing(41, 500);
        let ix = forced(&d);
        let mut r = Rng(9);
        let modes = [
            snap::mode::END | snap::mode::MID | snap::mode::CEN | snap::mode::INT,
            snap::mode::QUA | snap::mode::NEA | snap::mode::INS | snap::mode::NOD,
            0x3fff,
        ];
        let mut hits = 0;
        for k in 0..300 {
            let p = Vec2::new(r.r(0.0, 100.0), r.r(0.0, 100.0));
            let base = if k % 2 == 0 { Some(Vec2::new(r.r(0.0, 100.0), r.r(0.0, 100.0))) } else { None };
            let m = modes[k % 3];
            let a = snap::osnap_with(&d, &Space::Model, &ix, p, 1.0, m, base, false);
            let b = snap::osnap_with(&d, &Space::Model, &None, p, 1.0, m, base, false);
            assert_eq!(a, b, "p {p:?} mode {m}");
            hits += usize::from(a.is_some());
        }
        assert!(hits > 20);
    }

    #[test]
    fn hostile_queries_match_scan() {
        let d = drawing(77, 300);
        let ix = forced(&d);
        for (p, ap) in
            [(Vec2::new(f64::NAN, 1.0), 1.0), (Vec2::new(1.0, 1.0), f64::NAN), (Vec2::new(f64::INFINITY, 0.0), 1.0), (Vec2::new(50.0, 50.0), 1e300)]
        {
            assert_eq!(select::pick_with(&d, &Space::Model, &ix, p, ap), select::pick_with(&d, &Space::Model, &None, p, ap));
        }
        let bx = Bounds2 { min: Vec2::new(f64::NEG_INFINITY, 0.0), max: Vec2::new(f64::INFINITY, 50.0) };
        assert_eq!(select::select_window_with(&d, &Space::Model, &ix, bx, true), select::select_window_with(&d, &Space::Model, &None, bx, true));
    }

    #[test]
    fn cached_index_follows_edits() {
        let mut d = drawing(13, 400);
        let first = index(&d, &Space::Model).unwrap();
        assert!(Arc::ptr_eq(&first, &index(&d, &Space::Model).unwrap()), "unchanged drawing reuses its index");
        // Move a line far away: the cached index must not be reused.
        let h = d.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_)) && d.is_visible(e)).unwrap().handle;
        d.modify_entity(h, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.a = v3(500.0, 500.0);
                l.b = v3(501.0, 500.0);
            }
        })
        .unwrap();
        let second = index(&d, &Space::Model).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(select::pick(&d, &Space::Model, Vec2::new(500.5, 500.0), 0.1), Some(h));
        // A new entity is found too.
        let nh = d.add(&Space::Model, Common::default(), EntityKind::Circle(cadcraft_doc::Circle { center: v3(-300.0, 0.0), radius: 1.0 })).unwrap();
        assert_eq!(select::pick(&d, &Space::Model, Vec2::new(-299.0, 0.0), 0.1), Some(nh));
        // Redefining the block changes insert bounds: the index is rebuilt.
        let before = index(&d, &Space::Model).unwrap();
        let mut blk = (**d.blocks.get("B").unwrap()).clone();
        blk.entities
            .push(Entity::new(cadcraft_doc::Handle(2_000_000), EntityKind::Circle(cadcraft_doc::Circle { center: v3(0.0, 0.0), radius: 50.0 })));
        d.blocks.insert("B".into(), Arc::new(blk));
        assert!(!Arc::ptr_eq(&before, &index(&d, &Space::Model).unwrap()));
    }

    #[test]
    fn small_spaces_are_scanned() {
        let d = drawing(1, MIN_INDEXED - 1);
        assert!(index(&d, &Space::Model).is_none());
        assert!(index(&d, &Space::Paper("nope".into())).is_none());
    }

    #[test]
    fn incremental_index_matches_fresh_build() {
        let mut d = drawing(7, 3000);
        let mut r = Rng(99);
        let mut prev = SpatialIndex::build(&d, &d.model);
        for step in 0..60 {
            let hs = d.model.handles();
            let h = hs[(r.f() * hs.len() as f64) as usize % hs.len()];
            match step % 4 {
                0 => {
                    d.model.remove(h);
                }
                1 => {
                    let c = v3(r.r(-50.0, 150.0), r.r(-50.0, 150.0));
                    d.add(&Space::Model, Common::default(), EntityKind::Circle(cadcraft_doc::Circle { center: c, radius: r.r(0.1, 5.0) })).unwrap();
                }
                2 => {
                    d.model.send_to_back(h);
                }
                _ => {
                    let (dx, dy) = (r.r(-20.0, 20.0), r.r(-20.0, 20.0));
                    d.modify_entity(h, |e| e.kind.transform(&cadcraft_geom::Mat3::translate(Vec2::new(dx, dy)))).unwrap();
                }
            }
            let inc = SpatialIndex::build_from(&d, &d.model, Some(&prev));
            let fresh = SpatialIndex::build(&d, &d.model);
            assert_eq!(inc.len(), fresh.len());
            for _ in 0..20 {
                let c = Vec2::new(r.r(-50.0, 150.0), r.r(-50.0, 150.0));
                let w = r.r(0.1, 30.0);
                let b = Bounds2::new(c, c + Vec2::new(w, w));
                let (a, f) = (inc.query(&b, true).unwrap(), fresh.query(&b, true).unwrap());
                assert_eq!(a, f, "step {step}");
                for i in a {
                    assert_eq!(inc.entity(i).map(|e| e.handle), fresh.entity(i).map(|e| e.handle));
                }
            }
            prev = inc;
        }
    }
}
