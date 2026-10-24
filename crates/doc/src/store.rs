//! A copy-on-write entity store with draw order.
//!
//! Entities live in chunks (`Arc<Vec<Arc<Entity>>>`) in draw order, and a sharded handle index
//! maps each handle to the stable id of its chunk. Cloning a store (an undo snapshot) copies
//! only the chunk and shard pointers; an edit copies one chunk and one shard.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{Entity, Handle};

const CHUNK_MAX: usize = 1024;
const SHARDS: usize = 64;

#[derive(Clone, Debug, Default)]
struct Chunk {
    id: u32,
    items: Vec<Arc<Entity>>,
    slots: Arc<HashMap<Handle, usize>>,
    associated: Vec<Handle>,
}

impl Chunk {
    fn reindex(&mut self) {
        self.slots = Arc::new(self.items.iter().enumerate().map(|(i, e)| (e.handle, i)).collect());
        self.associated = self.items.iter().filter(|e| associated(e)).map(|e| e.handle).collect();
    }
    fn note_association(&mut self, handle: Handle) {
        let active = self.slots.get(&handle).and_then(|i| self.items.get(*i)).is_some_and(|e| associated(e));
        let present = self.associated.contains(&handle);
        if active && !present {
            self.associated.push(handle);
        } else if !active && present {
            self.associated.retain(|h| *h != handle);
        }
    }
}

fn associated(e: &Entity) -> bool {
    matches!(&e.kind, crate::EntityKind::Dimension(d) if !d.assoc.is_empty())
}

#[derive(Clone, Debug)]
pub struct EntityStore {
    chunks: Vec<Arc<Chunk>>,
    shards: Vec<Arc<HashMap<Handle, u32>>>,
    len: usize,
    next_chunk: u32,
    positions: Arc<HashMap<u32, usize>>,
}

impl Default for EntityStore {
    fn default() -> Self {
        EntityStore {
            chunks: Vec::new(),
            shards: (0..SHARDS).map(|_| Arc::new(HashMap::new())).collect(),
            len: 0,
            next_chunk: 0,
            positions: Arc::default(),
        }
    }
}

impl PartialEq for EntityStore {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

fn shard_of(h: Handle) -> usize {
    // Fibonacci hashing spreads sequential handles across shards.
    ((h.0.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize) % SHARDS
}

impl EntityStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Iterate in draw order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<Entity>> + '_ {
        self.chunks.iter().flat_map(|c| c.items.iter())
    }
    pub fn handles(&self) -> Vec<Handle> {
        self.iter().map(|e| e.handle).collect()
    }
    pub fn contains(&self, h: Handle) -> bool {
        self.chunk_id(h).is_some()
    }
    fn chunk_id(&self, h: Handle) -> Option<u32> {
        self.shards.get(shard_of(h)).and_then(|s| s.get(&h).copied())
    }
    fn chunk_pos(&self, id: u32) -> Option<usize> {
        self.positions.get(&id).copied()
    }
    pub fn get(&self, h: Handle) -> Option<&Arc<Entity>> {
        let pos = self.chunk_pos(self.chunk_id(h)?)?;
        let c = self.chunks.get(pos)?;
        c.items.get(*c.slots.get(&h)?).filter(|e| e.handle == h)
    }
    fn set_index(&mut self, h: Handle, chunk: Option<u32>) {
        if let Some(shard) = self.shards.get_mut(shard_of(h)) {
            let m = Arc::make_mut(shard);
            match chunk {
                Some(c) => {
                    m.insert(h, c);
                }
                None => {
                    m.remove(&h);
                }
            }
        }
    }
    fn new_chunk(&mut self) -> Arc<Chunk> {
        let id = self.next_chunk;
        self.next_chunk = self.next_chunk.wrapping_add(1);
        Arc::new(Chunk { id, items: Vec::with_capacity(64), slots: Arc::default(), associated: Vec::new() })
    }
    fn reindex_positions(&mut self) {
        self.positions = Arc::new(self.chunks.iter().enumerate().map(|(i, c)| (c.id, i)).collect());
    }
    /// Append on top of the draw order. Replaces an entity with the same handle in place.
    pub fn push(&mut self, e: Entity) {
        if self.contains(e.handle) {
            self.replace(e);
            return;
        }
        let needs_new = self.chunks.last().is_none_or(|c| c.items.len() >= CHUNK_MAX);
        if needs_new {
            let c = self.new_chunk();
            Arc::make_mut(&mut self.positions).insert(c.id, self.chunks.len());
            self.chunks.push(c);
        }
        let h = e.handle;
        let Some(last) = self.chunks.last_mut() else { return };
        let chunk = Arc::make_mut(last);
        Arc::make_mut(&mut chunk.slots).insert(h, chunk.items.len());
        if associated(&e) {
            chunk.associated.push(h);
        }
        chunk.items.push(Arc::new(e));
        let id = chunk.id;
        self.set_index(h, Some(id));
        self.len += 1;
    }
    /// Insert at the bottom of the draw order.
    pub fn push_front(&mut self, e: Entity) {
        if self.contains(e.handle) {
            self.replace(e);
            return;
        }
        let needs_new = self.chunks.first().is_none_or(|c| c.items.len() >= CHUNK_MAX);
        if needs_new {
            let c = self.new_chunk();
            self.chunks.insert(0, c);
            self.reindex_positions();
        }
        let h = e.handle;
        let Some(first) = self.chunks.first_mut() else { return };
        let chunk = Arc::make_mut(first);
        chunk.items.insert(0, Arc::new(e));
        chunk.reindex();
        let id = chunk.id;
        self.set_index(h, Some(id));
        self.len += 1;
    }
    /// Replace the entity with the same handle, keeping its draw position. Returns false if absent.
    pub fn replace(&mut self, e: Entity) -> bool {
        let h = e.handle;
        let Some(id) = self.chunk_id(e.handle) else { return false };
        let Some(pos) = self.chunk_pos(id) else { return false };
        let Some(c) = self.chunks.get_mut(pos) else { return false };
        let chunk = Arc::make_mut(c);
        if let Some(slot) = chunk.slots.get(&e.handle).and_then(|i| chunk.items.get_mut(*i)).filter(|x| x.handle == e.handle) {
            *slot = Arc::new(e);
            chunk.note_association(h);
            true
        } else {
            false
        }
    }
    pub fn remove(&mut self, h: Handle) -> Option<Arc<Entity>> {
        let id = self.chunk_id(h)?;
        let pos = self.chunk_pos(id)?;
        let c = self.chunks.get_mut(pos)?;
        let chunk = Arc::make_mut(c);
        let i = *chunk.slots.get(&h)?;
        let e = chunk.items.remove(i);
        let empty = chunk.items.is_empty();
        if empty {
            self.chunks.remove(pos);
            self.reindex_positions();
        } else {
            chunk.reindex();
        }
        self.set_index(h, None);
        self.len = self.len.saturating_sub(1);
        Some(e)
    }
    /// Remove a set of handles, copying and reindexing each affected chunk only once.
    /// Results follow the requested order; duplicate and absent handles are ignored.
    pub fn remove_many(&mut self, handles: &[Handle]) -> Vec<Arc<Entity>> {
        let ids: std::collections::HashSet<u32> = handles.iter().filter_map(|h| self.chunk_id(*h)).collect();
        if ids.is_empty() {
            return Vec::new();
        }
        let wanted: std::collections::HashSet<Handle> = handles.iter().copied().collect();
        let mut removed = HashMap::new();
        for c in &mut self.chunks {
            if !ids.contains(&c.id) {
                continue;
            }
            let chunk = Arc::make_mut(c);
            chunk.items.retain(|e| {
                if wanted.contains(&e.handle) {
                    removed.insert(e.handle, e.clone());
                    false
                } else {
                    true
                }
            });
            chunk.reindex();
        }
        self.chunks.retain(|c| !c.items.is_empty());
        self.reindex_positions();
        for h in removed.keys() {
            self.set_index(*h, None);
        }
        self.len = self.len.saturating_sub(removed.len());
        handles.iter().filter_map(|h| removed.remove(h)).collect()
    }
    /// Modify an entity in place through a closure (copy-on-write).
    pub fn modify<F: FnOnce(&mut Entity)>(&mut self, h: Handle, f: F) -> bool {
        let Some(id) = self.chunk_id(h) else { return false };
        let Some(pos) = self.chunk_pos(id) else { return false };
        let Some(c) = self.chunks.get_mut(pos) else { return false };
        let chunk = Arc::make_mut(c);
        match chunk.slots.get(&h).and_then(|i| chunk.items.get_mut(*i)).filter(|x| x.handle == h) {
            Some(slot) => {
                f(Arc::make_mut(slot));
                chunk.note_association(h);
                true
            }
            None => false,
        }
    }
    pub fn bring_to_front(&mut self, h: Handle) -> bool {
        match self.remove(h) {
            Some(e) => {
                self.push(Arc::unwrap_or_clone(e));
                true
            }
            None => false,
        }
    }
    pub fn send_to_back(&mut self, h: Handle) -> bool {
        match self.remove(h) {
            Some(e) => {
                self.push_front(Arc::unwrap_or_clone(e));
                true
            }
            None => false,
        }
    }
    /// Move `h` directly above (`above`) or under `anchor` in the draw order. False (and nothing
    /// moves) when either is absent or they are the same object.
    pub fn move_next_to(&mut self, h: Handle, anchor: Handle, above: bool) -> bool {
        if h == anchor || !self.contains(anchor) {
            return false;
        }
        let Some(e) = self.remove(h) else { return false };
        let at = self.chunk_id(anchor).and_then(|id| Some((id, self.chunk_pos(id)?)));
        let Some((id, pos)) = at else {
            // Unreachable in practice (the anchor was present); keep the object on top.
            self.push(Arc::unwrap_or_clone(e));
            return false;
        };
        let Some(c) = self.chunks.get_mut(pos) else {
            // Unreachable in practice too; never lose the object.
            self.push(Arc::unwrap_or_clone(e));
            return false;
        };
        let chunk = Arc::make_mut(c);
        let i = chunk.items.iter().position(|x| x.handle == anchor).unwrap_or(chunk.items.len());
        let i = if above { (i + 1).min(chunk.items.len()) } else { i };
        chunk.items.insert(i, e);
        chunk.reindex();
        let split = chunk.items.len() > 2 * CHUNK_MAX;
        self.set_index(h, Some(id));
        self.len += 1;
        if split {
            self.split_chunk(pos);
        }
        true
    }
    /// Split an oversized chunk in two, keeping the order.
    fn split_chunk(&mut self, pos: usize) {
        let mut fresh = self.new_chunk();
        let Some(c) = self.chunks.get_mut(pos) else { return };
        let chunk = Arc::make_mut(c);
        let tail = chunk.items.split_off(chunk.items.len() / 2);
        let new = Arc::make_mut(&mut fresh);
        new.items = tail;
        new.reindex();
        chunk.reindex();
        let (id, moved): (u32, Vec<Handle>) = (new.id, new.items.iter().map(|e| e.handle).collect());
        self.chunks.insert(pos + 1, fresh);
        self.reindex_positions();
        for h in moved {
            self.set_index(h, Some(id));
        }
    }
    /// Reorder the whole store by a key (stable: equal keys keep their order).
    pub fn sort_by_key<K: Ord>(&mut self, mut key: impl FnMut(&Entity) -> K) {
        let mut items: Vec<Arc<Entity>> = self.iter().cloned().collect();
        items.sort_by_cached_key(|e| key(e));
        *self = EntityStore::default();
        for e in items {
            let needs_new = self.chunks.last().is_none_or(|c| c.items.len() >= CHUNK_MAX);
            if needs_new {
                let c = self.new_chunk();
                Arc::make_mut(&mut self.positions).insert(c.id, self.chunks.len());
                self.chunks.push(c);
            }
            let h = e.handle;
            let Some(last) = self.chunks.last_mut() else { return };
            let chunk = Arc::make_mut(last);
            Arc::make_mut(&mut chunk.slots).insert(h, chunk.items.len());
            if associated(&e) {
                chunk.associated.push(h);
            }
            chunk.items.push(e);
            let id = chunk.id;
            self.set_index(h, Some(id));
            self.len += 1;
        }
    }
    /// True when `other` shares every chunk with `self` (same draw order, same entities). Holding
    /// a clone of a store makes this exact: any edit to the original copies the touched chunk.
    pub fn same_as(&self, other: &EntityStore) -> bool {
        self.len == other.len && self.chunks.len() == other.chunks.len() && self.chunks.iter().zip(&other.chunks).all(|(a, b)| Arc::ptr_eq(a, b))
    }
    /// The chunks in draw order as (identity key, entities). A key is stable for as long as any
    /// clone of this store is alive and changes whenever the chunk is edited (copy-on-write), so
    /// caches holding a clone can reuse per-chunk work.
    pub fn chunk_slices(&self) -> impl Iterator<Item = (usize, &[Arc<Entity>])> + '_ {
        self.chunks.iter().map(|c| (Arc::as_ptr(c) as usize, c.items.as_slice()))
    }
    /// Associated dimensions, without walking unrelated entities after each edit. The per-chunk
    /// index follows copy-on-write snapshots, so undo and redo restore it with the geometry.
    pub fn associated_dimensions(&self) -> impl Iterator<Item = &Arc<Entity>> + '_ {
        self.chunks.iter().flat_map(|c| c.associated.iter().filter_map(|h| c.slots.get(h).and_then(|i| c.items.get(*i))))
    }
    /// The last entity in draw order (AutoCAD's "Last" selection).
    pub fn last(&self) -> Option<&Arc<Entity>> {
        self.chunks.last().and_then(|c| c.items.last())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityKind, Point};
    use cadcraft_geom::Vec2;

    fn ent(h: u64) -> Entity {
        Entity::new(Handle(h), EntityKind::Point(Point { p: Vec2::new(h as f64, 0.0).to3(0.0), angle: 0.0 }))
    }

    #[test]
    fn push_get_remove() {
        let mut s = EntityStore::new();
        for h in 1..=3000 {
            s.push(ent(h));
        }
        assert_eq!(s.len(), 3000);
        assert!(s.get(Handle(1500)).is_some());
        assert!(s.remove(Handle(1500)).is_some());
        assert!(s.get(Handle(1500)).is_none());
        assert_eq!(s.len(), 2999);
        assert_eq!(s.iter().count(), 2999);
    }

    #[test]
    fn snapshots_are_independent() {
        let mut s = EntityStore::new();
        for h in 1..=10 {
            s.push(ent(h));
        }
        let snap = s.clone();
        s.remove(Handle(3));
        s.modify(Handle(4), |e| e.common.layer = "X".into());
        assert!(snap.get(Handle(3)).is_some());
        assert_eq!(snap.get(Handle(4)).map(|e| e.common.layer.as_str()), Some("0"));
        assert_eq!(s.get(Handle(4)).map(|e| e.common.layer.as_str()), Some("X"));
    }

    #[test]
    fn batch_remove_preserves_order_and_undo_snapshot_across_chunks() {
        let mut s = EntityStore::new();
        for h in 1..=3072 {
            s.push(ent(h));
        }
        let snapshot = s.clone();
        let requested = [Handle(2050), Handle(2), Handle(1025), Handle(2), Handle(9999)];
        let removed = s.remove_many(&requested);
        assert_eq!(removed.iter().map(|e| e.handle.0).collect::<Vec<_>>(), [2050, 2, 1025]);
        let expected: Vec<_> = (1..=3072).filter(|h| ![2050, 2, 1025].contains(h)).collect();
        assert_eq!(order(&s), expected);
        assert_eq!(s.len(), expected.len());
        for h in &expected {
            assert_eq!(s.get(Handle(*h)).map(|e| e.handle.0), Some(*h));
        }
        assert_eq!(snapshot.len(), 3072);
        assert!(requested.iter().take(3).all(|h| snapshot.get(*h).is_some()));
        assert!(s.remove_many(&requested).is_empty());
        // Dropping an entire middle chunk must update the positions of later chunks too.
        let middle: Vec<_> = (1025..=2048).map(Handle).collect();
        s.remove_many(&middle);
        assert_eq!(s.get(Handle(3072)).map(|e| e.handle.0), Some(3072));
    }

    #[test]
    fn association_index_tracks_replacement_edit_reorder_and_snapshots() {
        let mut s = EntityStore::new();
        s.push(ent(1));
        s.push(ent(2));
        let Ok(mut dim) = serde_json::from_value::<crate::Dimension>(serde_json::json!({
            "kind": "aligned", "defpt": {"x": 0.0, "y": 0.0, "z": 0.0},
            "textMid": {"x": 0.0, "y": 0.0, "z": 0.0}
        })) else {
            panic!("dimension fixture")
        };
        dim.assoc.push(crate::DimAssoc { point: "p13".into(), handle: Handle(1), snap: crate::AssocSnap::Start });
        s.replace(Entity::new(Handle(2), EntityKind::Dimension(dim)));
        let snapshot = s.clone();
        assert_eq!(s.associated_dimensions().map(|e| e.handle.0).collect::<Vec<_>>(), [2]);
        s.send_to_back(Handle(2));
        s.sort_by_key(|e| e.handle.0);
        assert_eq!(s.associated_dimensions().count(), 1);
        s.modify(Handle(2), |e| {
            if let EntityKind::Dimension(d) = &mut e.kind {
                d.assoc.clear();
            }
        });
        assert_eq!(s.associated_dimensions().count(), 0);
        assert_eq!(snapshot.associated_dimensions().count(), 1);
        s = snapshot.clone();
        s.remove_many(&[Handle(2)]);
        assert_eq!(s.associated_dimensions().count(), 0);
        assert_eq!(snapshot.associated_dimensions().count(), 1);
    }

    #[test]
    fn draw_order_moves() {
        let mut s = EntityStore::new();
        for h in 1..=5 {
            s.push(ent(h));
        }
        s.bring_to_front(Handle(1));
        assert_eq!(s.last().map(|e| e.handle), Some(Handle(1)));
        s.send_to_back(Handle(5));
        assert_eq!(s.iter().next().map(|e| e.handle), Some(Handle(5)));
        assert_eq!(s.len(), 5);
    }

    fn order(s: &EntityStore) -> Vec<u64> {
        s.iter().map(|e| e.handle.0).collect()
    }

    #[test]
    fn move_next_to_and_sort() {
        let mut s = EntityStore::new();
        for h in 1..=5 {
            s.push(ent(h));
        }
        assert!(s.move_next_to(Handle(1), Handle(3), true));
        assert_eq!(order(&s), [2, 3, 1, 4, 5]);
        assert!(s.move_next_to(Handle(5), Handle(2), false));
        assert_eq!(order(&s), [5, 2, 3, 1, 4]);
        assert!(!s.move_next_to(Handle(5), Handle(5), true));
        assert!(!s.move_next_to(Handle(5), Handle(99), true));
        assert!(!s.move_next_to(Handle(99), Handle(5), true));
        assert_eq!(order(&s), [5, 2, 3, 1, 4]);
        assert_eq!(s.len(), 5);
        s.sort_by_key(|e| e.handle.0 % 2);
        assert_eq!(order(&s), [2, 4, 5, 3, 1]);
        assert!(s.get(Handle(3)).is_some());
    }

    #[test]
    fn move_next_to_splits_big_chunks() {
        let mut s = EntityStore::new();
        for h in 1..=3000 {
            s.push(ent(h));
        }
        // Pile everything into the first chunk, under handle 1.
        for h in 1500..=3000 {
            assert!(s.move_next_to(Handle(h), Handle(1), true));
        }
        assert_eq!(s.len(), 3000);
        assert!(s.chunks.iter().all(|c| c.items.len() <= 2 * CHUNK_MAX));
        let o = order(&s);
        assert_eq!(o.first(), Some(&1));
        assert_eq!(o.get(1), Some(&3000));
        for h in 1..=3000 {
            assert_eq!(s.get(Handle(h)).map(|e| e.handle.0), Some(h));
        }
    }
}
