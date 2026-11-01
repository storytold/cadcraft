//! OOPS: bring back the objects the last ERASE (or the Delete key) removed, or BLOCK deleting the
//! objects it makes a block of, with their handles and each where it was in the draw order, as
//! AutoCAD 2024 does (accoreconsole, the drawing's `entnext` order read back afterwards).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, EntityKind, EntityStore, Handle, Space};
use serde_json::{Value, json};

use crate::{Result, Session};

/// One erased object, with the objects that were directly under and above it.
#[derive(Clone, Debug)]
struct Taken {
    space: Space,
    entity: Arc<Entity>,
    below: Option<Handle>,
    above: Option<Handle>,
}

/// What the drawing's last erase removed, for OOPS.
#[derive(Clone, Debug, Default)]
pub(crate) struct Erased(Vec<Taken>);

impl Erased {
    /// Never bring back the objects `drop` picks.
    pub(crate) fn forget(&mut self, drop: impl Fn(Handle) -> bool) {
        self.0.retain(|t| !drop(t.entity.handle));
    }
}

/// Remove the objects `hs` and keep them for OOPS in place of what an earlier erase kept.
/// Returns how many went; when none did, OOPS keeps what it had.
pub(crate) fn erase_kept(s: &mut Session, hs: &[Handle]) -> Result<usize> {
    let d = s.doc_mut()?;
    // Every neighbour is read before anything goes: objects erased together may be neighbours.
    let at: Vec<(Handle, Space, Option<Handle>, Option<Handle>)> = hs
        .iter()
        .filter_map(|&h| {
            let space = d.space_of(h)?;
            let (below, above) = d.space(&space)?.neighbours(h)?;
            Some((h, space, below, above))
        })
        .collect();
    let mut taken = Vec::with_capacity(at.len());
    for (h, space, below, above) in at {
        if let Some(entity) = d.space_mut(&space).and_then(|st| st.remove(h)) {
            taken.push(Taken { space, entity, below, above });
        }
    }
    let n = taken.len();
    if n > 0 {
        s.state_mut()?.erased = Erased(taken);
    }
    Ok(n)
}

pub(super) fn run_oops(s: &mut Session, _p: &Value) -> Result<Value> {
    let taken = s.state()?.erased.0.clone();
    let back: Vec<bool> = {
        let d = s.doc()?;
        let in_blocks = in_blocks(d, &taken);
        taken.iter().map(|t| !in_blocks.contains(&t.entity.handle) && can_return(d, t)).collect()
    };
    // Nothing to bring back leaves the drawing as it is: no undo step.
    if !back.contains(&true) {
        return Ok(json!({ "restored": 0 }));
    }
    let n = restore(s.doc_mut()?, &taken, &back);
    Ok(json!({ "restored": n }))
}

/// The handles of `taken` that a block's object has now (an undo can take the drawing back to
/// before an erased object was made, and a block's new object take its handle).
fn in_blocks(d: &Drawing, taken: &[Taken]) -> HashSet<Handle> {
    let hs: HashSet<Handle> = taken.iter().map(|t| t.entity.handle).collect();
    let mut out = HashSet::new();
    for b in d.blocks.values() {
        // Through whichever is smaller.
        if b.entities.len() < hs.len() {
            out.extend(b.entities.iter().map(|e| e.handle).filter(|h| hs.contains(h)));
        } else {
            out.extend(hs.iter().copied().filter(|h| b.entities.contains(*h)));
        }
    }
    out
}

/// Whether `t` can come back: it is not in the drawing (OOPS again, or an undo brought it back),
/// its handle is one the drawing has given out, a reference being edited in place (REFEDIT)
/// doesn't keep it out, and its layout, layer and block are still there.
fn can_return(d: &Drawing, t: &Taken) -> bool {
    let h = t.entity.handle;
    h.0 < d.handseed
        && d.entity(h).is_none()
        && !crate::cmd::blocks::refedit::locked(d, h)
        && d.space(&t.space).is_some()
        && d.layer(&t.entity.common.layer).is_some()
        && match &t.entity.kind {
            EntityKind::Insert(i) => d.block(&i.block).is_some(),
            _ => true,
        }
}

/// Put back the objects `back` marks. Each run of objects that were neighbours comes back from
/// its bottom one up, so each goes directly above the one that was under it.
fn restore(d: &mut Drawing, taken: &[Taken], back: &[bool]) -> usize {
    let index: HashMap<Handle, usize> = taken.iter().enumerate().map(|(i, t)| (t.entity.handle, i)).collect();
    let next: HashMap<Handle, usize> = taken.iter().enumerate().filter_map(|(i, t)| Some((t.below.filter(|b| index.contains_key(b))?, i))).collect();
    let mut done = vec![false; taken.len()];
    let mut n = 0;
    for first in 0..taken.len() {
        if taken[first].below.is_some_and(|b| index.contains_key(&b)) {
            continue;
        }
        let mut at = Some(first);
        while let Some(i) = at {
            if std::mem::replace(&mut done[i], true) {
                break;
            }
            if back[i] && put_back(d, taken, &index, i) {
                n += 1;
            }
            at = next.get(&taken[i].entity.handle).copied();
        }
    }
    n
}

/// Put `taken[i]` back directly above the nearest object that was under it and is here; when
/// those are gone, directly under the nearest one that was above it; else on top.
fn put_back(d: &mut Drawing, taken: &[Taken], index: &HashMap<Handle, usize>, i: usize) -> bool {
    let t = &taken[i];
    let Some(store) = d.space_mut(&t.space) else { return false };
    let h = t.entity.handle;
    store.push(Entity::clone(&t.entity));
    match nearest(store, taken, index, t.below, |t| t.below) {
        Some(Some(b)) => {
            store.move_next_to(h, b, true);
        }
        Some(None) => {
            store.send_to_back(h);
        }
        None => {
            if let Some(Some(a)) = nearest(store, taken, index, t.above, |t| t.above) {
                store.move_next_to(h, a, false);
            }
        }
    }
    true
}

/// From `h`, past objects erased with it that are not here, to the first object that is
/// (`Some(Some)`), the end of the draw order (`Some(None)`), or an object that is gone (`None`).
fn nearest(
    store: &EntityStore,
    taken: &[Taken],
    index: &HashMap<Handle, usize>,
    mut h: Option<Handle>,
    step: fn(&Taken) -> Option<Handle>,
) -> Option<Option<Handle>> {
    for _ in 0..=taken.len() {
        let Some(x) = h else { return Some(None) };
        if store.contains(x) {
            return Some(Some(x));
        }
        h = step(taken.get(*index.get(&x)?)?);
    }
    None
}
