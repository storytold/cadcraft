//! Bounded model-space tessellation cache for an interactive canvas.
//!
//! Copy-on-write entity chunks identify unchanged geometry. Plots and layouts continue to use
//! the full builder; a screen list preserves entity order, including wipeout barriers.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use cadcraft_doc::{Drawing, Entity, Space};
use cadcraft_geom::Mat3;

use crate::{Builder, DisplayList, MAX_BLOCK_EXPANSION, Options, entity, top_ctx};

// The joined list needs the same space again. Oversized scenes use the existing full builder.
const MAX_BYTES: usize = 32 << 20;
const MAX_PARTS: usize = 4096;
static NEXT_PART: AtomicU64 = AtomicU64::new(1);

/// Font preferences are part of the canvas and text geometry cache keys.
pub fn font_revision() -> u64 {
    cadcraft_fonts::ttf::revision()
}

#[derive(Clone)]
pub struct Part {
    /// Identity of the tessellation, independent of its position in drawing order.
    pub id: u64,
    pub list: Arc<DisplayList>,
}

pub struct Built {
    pub list: DisplayList,
    pub parts: Vec<Part>,
    pub rebuilt: usize,
    pub reused: usize,
}

#[derive(Clone, Default)]
pub struct Cache {
    // Retaining the snapshot prevents a freed chunk address from being reused as a cache key.
    drawing: Option<Arc<Drawing>>,
    options: Option<Options>,
    font_revision: u64,
    chunks: HashMap<usize, Part>,
}

impl Cache {
    pub fn bytes(&self) -> usize {
        self.chunks.values().map(|p| bytes(&p.list)).sum()
    }

    pub fn build(&mut self, drawing: &Arc<Drawing>, space: &Space, options: &Options) -> Built {
        let fallback = |this: &mut Self| {
            *this = Self::default();
            Built { list: crate::build(drawing, space, options), parts: Vec::new(), rebuilt: 0, reused: 0 }
        };
        if *space != Space::Model || options.plot_style_table.is_some() {
            return fallback(self);
        }
        let options = crate::with_point_view(drawing, space, options);
        let font_revision = cadcraft_fonts::ttf::revision();
        if self.font_revision != font_revision
            || self.options.as_ref().is_none_or(|o| !same_options(o, &options))
            || self.drawing.as_ref().is_none_or(|d| !d.same_render_context(drawing))
        {
            self.chunks.clear();
        }
        let chunks: Vec<_> = drawing.model.chunk_slices().collect();
        if chunks.len() > MAX_PARTS {
            return fallback(self);
        }
        let used: usize = chunks.iter().filter_map(|(k, _)| self.chunks.get(k)).map(|p| p.list.expanded).sum();
        // A cached part must not give a new part a fresh block-expansion allowance.
        if used >= MAX_BLOCK_EXPANSION {
            return fallback(self);
        }
        let budget = AtomicUsize::new(used);
        let missing: Vec<_> = chunks.iter().filter(|(k, _)| !self.chunks.contains_key(k)).collect();
        let make = |&&(key, ents): &&(usize, &[Arc<Entity>])| {
            let mut b = Builder {
                list: DisplayList::default(),
                opts: &options,
                plotting: false,
                styles: None,
                named: None,
                shared_budget: Some(&budget),
                transparency: 0,
            };
            for e in ents {
                entity(&mut b, &top_ctx(drawing, Mat3::IDENTITY, e.handle, &[], &[]), e);
            }
            (key, Part { id: NEXT_PART.fetch_add(1, Ordering::Relaxed), list: Arc::new(b.list) })
        };
        #[cfg(not(target_arch = "wasm32"))]
        let made: Vec<_> = {
            use rayon::prelude::*;
            missing.par_iter().map(make).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let made: Vec<_> = missing.iter().map(make).collect();
        if budget.load(Ordering::Relaxed) >= MAX_BLOCK_EXPANSION {
            drop(made);
            return fallback(self);
        }
        if options.cancelled() {
            return fallback(self);
        }
        let rebuilt = made.len();
        let mut next: HashMap<_, _> = made.into_iter().collect();
        let mut parts = Vec::with_capacity(chunks.len());
        let mut size = 0usize;
        for (key, _) in chunks {
            let Some(part) = next.get(&key).or_else(|| self.chunks.get(&key)).cloned() else { return fallback(self) };
            size = size.saturating_add(bytes(&part.list));
            if size > MAX_BYTES {
                drop(part);
                drop(parts);
                drop(next);
                return fallback(self);
            }
            next.insert(key, part.clone());
            parts.push(part);
        }
        let mut list = DisplayList::default();
        list.prims.reserve(parts.iter().map(|p| p.list.prims.len()).sum());
        list.verts.reserve(parts.iter().map(|p| p.list.verts.len()).sum());
        list.tris.reserve(parts.iter().map(|p| p.list.tris.len()).sum());
        for part in &parts {
            append(&mut list, &part.list);
        }
        self.chunks = next;
        let mut options = options;
        options.cancel = None;
        self.options = Some(options);
        self.font_revision = font_revision;
        self.drawing = Some(drawing.clone());
        Built { reused: parts.len().saturating_sub(rebuilt), rebuilt, list, parts }
    }
}

fn same_options(a: &Options, b: &Options) -> bool {
    a.tolerance.to_bits() == b.tolerance.to_bits()
        && a.min_dash.to_bits() == b.min_dash.to_bits()
        && a.text == b.text
        && a.fill == b.fill
        && a.lineweights == b.lineweights
        && a.view_height.to_bits() == b.view_height.to_bits()
        && a.view == b.view
        && a.transparency == b.transparency
        && a.skip_viewport == b.skip_viewport
}

pub fn bytes(list: &DisplayList) -> usize {
    list.prims
        .capacity()
        .saturating_mul(std::mem::size_of::<crate::DPrim>())
        .saturating_add(list.verts.capacity().saturating_mul(std::mem::size_of::<cadcraft_geom::Vec2>()))
        .saturating_add(list.tris.capacity().saturating_mul(std::mem::size_of::<cadcraft_geom::Vec2>()))
}

fn append(dst: &mut DisplayList, src: &DisplayList) {
    let (verts, tris) = (dst.verts.len() as u32, dst.tris.len() as u32);
    dst.prims.extend(src.prims.iter().copied().map(|mut p| {
        p.start += if matches!(p.kind, crate::Kind::Tris | crate::Kind::Mask) { tris } else { verts };
        p
    }));
    dst.verts.extend_from_slice(&src.verts);
    dst.tris.extend_from_slice(&src.tris);
    dst.bounds = dst.bounds.union(&src.bounds);
    dst.expanded = dst.expanded.saturating_add(src.expanded);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{EntityKind, Handle, Line};
    use cadcraft_geom::{Bounds2, Vec2, Vec3};

    fn drawing() -> Arc<Drawing> {
        let mut d = Drawing::new_imperial();
        for i in 0..2050 {
            let x = f64::from(i);
            d.model.push(Entity::new(Handle(i as u64 + 1), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x, 5.0, 0.0) })));
        }
        Arc::new(d)
    }

    fn matches_full(d: &Drawing, opts: &Options, list: &DisplayList) {
        let full = crate::build(d, &Space::Model, opts);
        assert_eq!(list.verts, full.verts);
        assert_eq!(list.tris, full.tris);
        assert_eq!(list.bounds, full.bounds);
        assert_eq!(format!("{:?}", list.prims), format!("{:?}", full.prims));
    }

    #[test]
    fn a_small_edit_reuses_other_chunks_and_preserves_offscreen_bounds() {
        let mut d = drawing();
        let original = d.clone();
        let opts = Options { view_height: 100.0, view: Some(Bounds2::new(Vec2::ZERO, Vec2::new(10.0, 10.0))), ..Default::default() };
        let mut cache = Cache::default();
        let first = cache.build(&d, &Space::Model, &opts);
        matches_full(&d, &opts, &first.list);
        assert_eq!(first.rebuilt, 3);
        Arc::make_mut(&mut d).model.modify(Handle(1), |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.a.x = -100.0;
            }
        });
        let edited = cache.build(&d, &Space::Model, &opts);
        assert_eq!((edited.rebuilt, edited.reused), (1, 2));
        matches_full(&d, &opts, &edited.list);
        assert!(edited.list.bounds.max.x > 2000.0, "the visible window must not crop drawing bounds");
        // Undo restores the previous chunk identities, not the edited geometry.
        let undone = cache.build(&original, &Space::Model, &opts);
        matches_full(&original, &opts, &undone.list);
    }

    #[test]
    fn symbol_and_display_changes_invalidate_reused_geometry() {
        let mut d = drawing();
        let opts = Options { view_height: 100.0, ..Default::default() };
        let mut cache = Cache::default();
        cache.build(&d, &Space::Model, &opts);
        Arc::make_mut(&mut d).header.set_f64("LTSCALE", 2.0);
        let changed = cache.build(&d, &Space::Model, &opts);
        assert_eq!(changed.reused, 0);
        matches_full(&d, &opts, &changed.list);
        let alpha = Options { transparency: true, ..opts.clone() };
        let changed = cache.build(&d, &Space::Model, &alpha);
        assert_eq!(changed.reused, 0);
        matches_full(&d, &alpha, &changed.list);
    }

    #[test]
    fn a_cancelled_build_cannot_poison_the_next_complete_build() {
        let d = drawing();
        let opts = Options { view_height: 100.0, ..Default::default() };
        let cancelled = Options { cancel: Some(Arc::new(std::sync::atomic::AtomicBool::new(true))), ..opts.clone() };
        let mut cache = Cache::default();
        cache.build(&d, &Space::Model, &cancelled);
        let complete = cache.build(&d, &Space::Model, &opts);
        matches_full(&d, &opts, &complete.list);
        assert_eq!(complete.reused, 0);
        let space = Space::Paper("Layout1".into());
        let paper = cache.build(&d, &space, &opts);
        let full = crate::build(&d, &space, &opts);
        assert!(paper.parts.is_empty());
        assert_eq!(paper.list.verts, full.verts);
        assert_eq!(paper.list.bounds, full.bounds);
    }
}
