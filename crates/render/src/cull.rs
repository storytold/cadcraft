//! Leaving out what lies wholly outside the area a build covers. This is the one place that
//! decides it for every build, screen or plot: the window a build culls against, whether an
//! object reaches it, and which entities of a block reference reach it once the window is
//! brought into the block's coordinates. Nested references are culled the same way at each
//! level. An object without measurable bounds (an infinite line, a ray) always reaches, and so
//! does a POINT, or a reference to a block that draws one: a point's marker (PDMODE, PDSIZE) can
//! be any size, and its bounds are only the point.
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
    reaches(&cull_bounds(d, e, 0), window)
}

/// The bounds a top-level object is culled by: its own, or none (it always reaches) for a point
/// or a reference to a block that draws points.
fn cull_bounds(d: &Drawing, e: &Entity, depth: usize) -> Bounds2 {
    let draws_points = |name: &str| d.block(name).is_some_and(|blk| block_bounds(d, blk).points);
    let points = match &e.kind {
        EntityKind::Point(_) => true,
        EntityKind::Insert(ins) => draws_points(&ins.block),
        EntityKind::Dimension(dm) => dm.block.as_deref().is_some_and(draws_points),
        EntityKind::MLeader(m) => m.block.as_ref().is_some_and(|ins| draws_points(&ins.block)),
        _ => false,
    };
    if points { Bounds2::EMPTY } else { entity_bounds(d, e, depth) }
}

/// Whether `blk`, or a definition it draws through references (`nested`), has a POINT.
fn has_points(blk: &Block, nested: &Nested) -> bool {
    let any = |b: &Block| b.entities.iter().any(|e| matches!(e.kind, EntityKind::Point(_)));
    any(blk) || nested.iter().any(|(_, def)| def.as_deref().is_some_and(any))
}

/// The measured entity bounds of one block definition, looked up once per block reference and
/// asked about each of its copies.
pub(crate) struct BlockReach(Measured);

/// Which entities of one copy of a block can reach the window.
pub(crate) enum CopyReach {
    /// Build them all (the copy's transform cannot be inverted).
    All,
    /// None of them: the copy lies wholly outside the window.
    Nothing,
    /// One flag per entity, in draw order.
    Some(Vec<bool>),
}

/// The entity bounds of `blk`, to cull its copies with [`BlockReach::copy`].
pub(crate) fn block_reach(d: &Drawing, blk: &Arc<Block>) -> BlockReach {
    BlockReach(block_bounds(d, blk))
}

impl BlockReach {
    /// The entities of the copy drawn with `xf` (block coordinates to the space being built) that
    /// can reach `window`. The window is brought into block coordinates as the box around its
    /// corners, so a rotated or skewed reference keeps everything that may show. A copy whose
    /// definition has finite bounds wholly outside the window is ruled out without looking at
    /// each entity.
    pub(crate) fn copy(&self, xf: &Mat3, window: &Bounds2) -> CopyReach {
        let Some(inv) = xf.inverse() else { return CopyReach::All };
        let local = Bounds2::from_points(window.corners().map(|p| inv.apply(p)));
        if self.0.extent.is_some_and(|x| !x.intersects(&local)) {
            return CopyReach::Nothing;
        }
        CopyReach::Some(self.0.bounds.iter().map(|b| reaches(b, &local)).collect())
    }
}

/// A block definition's entity bounds (block coordinates, draw order) and the box around them
/// all: `None` when one has no measurable bounds (an infinite line, a ray), or the block is
/// empty, so that the box alone never rules a copy out.
#[derive(Clone)]
struct Measured {
    bounds: Arc<Vec<Bounds2>>,
    extent: Option<Bounds2>,
    /// The block, or one drawn through it, has a POINT (see [`cull_bounds`]).
    points: bool,
}

impl Measured {
    fn new(bounds: Vec<Bounds2>, points: bool) -> Self {
        let extent = if bounds.is_empty() || bounds.iter().any(Bounds2::is_empty) {
            None
        } else {
            Some(bounds.iter().fold(Bounds2::EMPTY, |a, b| a.union(b)))
        };
        Measured { bounds: Arc::new(bounds), extent, points }
    }
}

/// Block definitions whose entity bounds are kept (the oldest is dropped past it).
const MAX_BLOCKS: usize = 512;

/// The definitions a block draws through references, by the name it refers to them by (`None`:
/// missing when measured).
type Nested = Vec<(String, Option<Arc<Block>>)>;

/// (definition, drawing key, the definitions it reaches, its measured bounds).
type Entry = (Arc<Block>, u64, Nested, Measured);

static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// The drawing-wide inputs block entity bounds read besides the definitions: which layers are on
/// and thawed (nested block contents on hidden layers take no room), and the dimension styles, text
/// style heights and width factors (MTEXT bounds) and DIMSCALE that size a dimension's room for
/// text and arrows and a tolerance frame.
fn drawing_key(d: &Drawing) -> u64 {
    let mut h = DefaultHasher::new();
    for l in &d.layers {
        (l.name.as_str(), l.on, l.frozen).hash(&mut h);
    }
    for st in &d.dim_styles {
        (st.name.as_str(), st.scale.to_bits(), st.text_height.to_bits(), st.text_gap.to_bits(), st.text_style.as_str()).hash(&mut h);
    }
    for t in &d.text_styles {
        (t.name.as_str(), t.height.to_bits(), t.width_factor.to_bits()).hash(&mut h);
    }
    d.header.f64("DIMSCALE", 1.0).to_bits().hash(&mut h);
    h.finish()
}

/// Every definition `blk` reaches through block references, dimension blocks and multileader
/// blocks, at any depth, each once.
fn nested(d: &Drawing, blk: &Block) -> Nested {
    let mut out: Nested = Vec::new();
    let mut todo: Vec<Arc<Block>> = Vec::new();
    let visit = |name: &str, out: &mut Nested, todo: &mut Vec<Arc<Block>>| {
        // Exact names: `d.block` prefers an exact match, so `A` and `a` can be two definitions.
        if out.iter().any(|(n, _)| n == name) {
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
/// definition, drawing key ([`drawing_key`]) and set of nested definitions. The cache holds the definitions
/// themselves, so an edited block, or an edited block nested in it, is measured again.
fn block_bounds(d: &Drawing, blk: &Arc<Block>) -> Measured {
    let key = drawing_key(d);
    if let Ok(cache) = CACHE.lock()
        && let Some((_, _, _, b)) = cache.iter().find(|(blk2, k, n, _)| *k == key && Arc::ptr_eq(blk2, blk) && unchanged(d, n))
    {
        return b.clone();
    }
    let deps = nested(d, blk);
    // A point, or (when this block draws points anywhere) a reference, has no bounds to cull by.
    let points = has_points(blk, &deps);
    let measure = |e: &Entity| match &e.kind {
        EntityKind::Point(_) => Bounds2::EMPTY,
        EntityKind::Insert(_) if points => Bounds2::EMPTY,
        EntityKind::Dimension(dm) if points && dm.block.is_some() => Bounds2::EMPTY,
        EntityKind::MLeader(m) if points && m.block.is_some() => Bounds2::EMPTY,
        _ => entity_bounds(d, e, 1),
    };
    let bounds = Measured::new(blk.entities.iter().map(|e| measure(e)).collect(), points);
    if let Ok(mut cache) = CACHE.lock() {
        cache.retain(|(blk2, k, _, _)| !(*k == key && Arc::ptr_eq(blk2, blk)));
        if cache.len() >= MAX_BLOCKS {
            cache.remove(0);
        }
        cache.push((blk.clone(), key, deps, bounds.clone()));
    }
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Handle, Line, RayLine};
    use cadcraft_geom::{Vec2, Vec3};

    fn block(name: &str, kinds: Vec<EntityKind>) -> Arc<Block> {
        let mut blk = Block::new(name);
        for (i, k) in kinds.into_iter().enumerate() {
            blk.entities.push(Entity::new(Handle(i as u64 + 1), k));
        }
        Arc::new(blk)
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> EntityKind {
        EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })
    }

    #[test]
    fn a_copy_outside_the_window_is_ruled_out_whole() {
        let d = Drawing::new_imperial();
        let window = Bounds2::from_points([Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)]);
        let blk = block("CullAggregate", vec![line((0.0, 0.0), (1.0, 0.0)), line((0.0, 5.0), (1.0, 5.0))]);
        let reach = block_reach(&d, &blk);
        assert!(matches!(reach.copy(&Mat3::translate(Vec2::new(100.0, 0.0)), &window), CopyReach::Nothing));
        // A copy straddling the window edge is looked at entity by entity.
        match reach.copy(&Mat3::translate(Vec2::new(9.5, 6.0)), &window) {
            CopyReach::Some(s) => assert_eq!(s, vec![true, false]),
            _ => panic!("a copy meeting the window keeps its per-entity flags"),
        }
    }

    #[test]
    fn points_and_blocks_drawing_points_always_reach() {
        let mut d = Drawing::new_imperial();
        let window = Bounds2::from_points([Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)]);
        let point = EntityKind::Point(cadcraft_doc::Point { p: Vec3::new(10.5, 5.0, 0.0), angle: 0.0 });
        // A point just outside the window: its marker may still reach it.
        let e = Entity::new(Handle(1), point.clone());
        assert!(entity_reaches(&d, &e, &window));
        // A block drawing one, directly or through a nested reference, is never ruled out.
        let inner = block("CullPointInner", vec![point]);
        d.blocks.insert("CullPointInner".into(), inner.clone());
        let EntityKind::Insert(mut ins) = insert_kind("CullPointInner") else { panic!("an insert") };
        ins.insert = Vec3::new(100.0, 0.0, 0.0);
        let outer = block("CullPointOuter", vec![EntityKind::Insert(ins.clone())]);
        d.blocks.insert("CullPointOuter".into(), outer.clone());
        assert!(entity_reaches(&d, &Entity::new(Handle(2), EntityKind::Insert(ins)), &window));
        assert!(matches!(block_reach(&d, &outer).copy(&Mat3::translate(Vec2::new(100.0, 0.0)), &window), CopyReach::Some(_)));
        assert!(matches!(block_reach(&d, &inner).copy(&Mat3::translate(Vec2::new(100.0, 0.0)), &window), CopyReach::Some(_)));
    }

    #[test]
    fn a_text_style_width_factor_change_measures_blocks_again() {
        let mut d = Drawing::new_imperial();
        let before = drawing_key(&d);
        if let Some(s) = d.text_styles.first_mut() {
            s.width_factor = 2.0;
        }
        assert_ne!(drawing_key(&d), before, "MTEXT bounds in blocks follow the style's width factor");
    }

    fn insert_kind(name: &str) -> EntityKind {
        EntityKind::Insert(cadcraft_doc::Insert {
            block: name.into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        })
    }

    #[test]
    fn a_block_with_an_unbounded_entity_is_never_ruled_out_whole() {
        let d = Drawing::new_imperial();
        let window = Bounds2::from_points([Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)]);
        let xline = EntityKind::XLine(RayLine { base: Vec3::new(0.0, 0.0, 0.0), dir: Vec3::new(0.0, 1.0, 0.0) });
        let blk = block("CullUnbounded", vec![line((0.0, 0.0), (1.0, 0.0)), xline]);
        match block_reach(&d, &blk).copy(&Mat3::translate(Vec2::new(100.0, 0.0)), &window) {
            CopyReach::Some(s) => assert_eq!(s, vec![false, true]),
            _ => panic!("the xline may cross the window"),
        }
    }
}
