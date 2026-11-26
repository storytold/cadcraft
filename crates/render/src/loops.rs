//! Loops in the block graph, for dimensions inside blocks (#1923).
//!
//! A block leads to the blocks that drawing its contents draws in turn: those of its block
//! references, dimensions and multileaders, also inside proxy graphics. A dimension inside block
//! `C` whose own block leads back to `C` is part of a loop: drawn from its block, it would draw `C`
//! again, and every such dimension again at each level down to `MAX_BLOCK_DEPTH`. With k of them
//! in the loop that is up to k^16 copies, or one per ordering of the distinct blocks on the loop,
//! which only the expansion budget cuts. Such a dimension is drawn from its definition points
//! instead, as one whose block is missing is. The loops are those of the drawing's data: an
//! object on the way that is hidden or on a layer that is off still closes one.

use std::collections::HashMap;
use std::sync::OnceLock;

use cadcraft_doc::{Block, Drawing, Entity, EntityKind};

/// The most blocks, and objects in them, that the search from a dimension's block looks at before
/// the whole block graph is searched instead: room for a dimension's block and the blocks of its
/// arrowheads, which is what such a search usually meets.
const NEARBY_BLOCKS: usize = 16;
const NEARBY_OBJECTS: usize = 256;

/// Whether block `to` leads back to block `from` in `d`: it is `from`, or drawing `to` draws
/// `from` again. A short search from `to` answers for ordinary blocks. Past it, the loops of the
/// whole block graph are found, once per build (`loops`), and answer from then on.
pub(crate) fn leads_back(d: &Drawing, loops: &OnceLock<BlockLoops>, from: &Block, to: &Block) -> bool {
    if std::ptr::eq(from, to) {
        return true;
    }
    if let Some(l) = loops.get() {
        return l.on_one_loop(from, to);
    }
    match reaches_nearby(d, to, from) {
        Some(found) => found,
        None => loops.get_or_init(|| BlockLoops::new(d)).on_one_loop(from, to),
    }
}

/// Whether `start` leads to `target`, when looking at no more than [`NEARBY_BLOCKS`] blocks and
/// [`NEARBY_OBJECTS`] objects in them settles it.
fn reaches_nearby(d: &Drawing, start: &Block, target: &Block) -> Option<bool> {
    let mut found: Vec<&Block> = vec![start];
    let mut objects = NEARBY_OBJECTS;
    let mut drawn = Vec::new();
    let mut next = 0;
    while let Some(b) = found.get(next).copied() {
        next += 1;
        for e in b.entities.iter() {
            if !drawn_blocks(d, e, &mut drawn, &mut objects) {
                return None;
            }
        }
        for t in drawn.drain(..) {
            if std::ptr::eq(t, target) {
                return Some(true);
            }
            if !found.iter().any(|f| std::ptr::eq(*f, t)) {
                if found.len() >= NEARBY_BLOCKS {
                    return None;
                }
                found.push(t);
            }
        }
    }
    Some(false)
}

/// The blocks that lie on a loop, numbered by their strongly connected component: two blocks
/// lead to each other exactly when they have the same number.
pub(crate) struct BlockLoops {
    /// The component of each block that shares one with another block, by the block's address.
    component: HashMap<usize, usize>,
}

impl BlockLoops {
    fn new(d: &Drawing) -> Self {
        let blocks: Vec<&Block> = d.blocks.values().map(|b| &**b).collect();
        let index: HashMap<usize, usize> = blocks.iter().enumerate().map(|(i, b)| (address(b), i)).collect();
        let edges: Vec<Vec<usize>> = blocks
            .iter()
            .map(|b| {
                let (mut drawn, mut unlimited) = (Vec::new(), usize::MAX);
                for e in b.entities.iter() {
                    drawn_blocks(d, e, &mut drawn, &mut unlimited);
                }
                drawn.into_iter().filter_map(|t| index.get(&address(t)).copied()).collect()
            })
            .collect();
        let mut component = HashMap::new();
        for (n, members) in components(&edges).into_iter().filter(|c| c.len() > 1).enumerate() {
            for i in members {
                if let Some(b) = blocks.get(i) {
                    component.insert(address(b), n);
                }
            }
        }
        BlockLoops { component }
    }

    /// Whether blocks `a` and `b` lie on one loop (so each leads to the other).
    fn on_one_loop(&self, a: &Block, b: &Block) -> bool {
        self.component.get(&address(a)).is_some_and(|c| self.component.get(&address(b)) == Some(c))
    }
}

fn address(b: &Block) -> usize {
    std::ptr::from_ref(b) as usize
}

/// Push each block that `e` draws itself, as the renderer finds it by name, counting the objects
/// looked at (`e` and its proxy graphics) against `objects`: false when they run out.
fn drawn_blocks<'d>(d: &'d Drawing, e: &Entity, drawn: &mut Vec<&'d Block>, objects: &mut usize) -> bool {
    // Proxy graphics wait here (no allocation for other objects).
    let mut pending: Vec<&Entity> = Vec::new();
    let mut next = Some(e);
    while let Some(e) = next.take().or_else(|| pending.pop()) {
        let Some(left) = objects.checked_sub(1) else { return false };
        *objects = left;
        let name = match &e.kind {
            EntityKind::Insert(i) => Some(i.block.as_str()),
            EntityKind::Dimension(dm) => dm.block.as_deref(),
            EntityKind::MLeader(m) => m.block.as_ref().map(|i| i.block.as_str()),
            EntityKind::Unknown(u) => {
                pending.extend(u.graphics.iter());
                None
            }
            _ => None,
        };
        if let Some(b) = name.and_then(|n| d.block(n)) {
            drawn.push(&**b);
        }
    }
    true
}

/// The strongly connected components of the graph with `edges[i]` leaving node `i` (Tarjan's
/// algorithm, with an explicit stack so a long chain of blocks can't overflow the call stack).
fn components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNSEEN: usize = usize::MAX;
    let n = edges.len();
    let (mut order, mut low, mut on_stack) = (vec![UNSEEN; n], vec![0; n], vec![false; n]);
    let (mut next, mut stack, mut out) = (0, Vec::new(), Vec::new());
    let mut visit = |v: usize, order: &mut [usize], low: &mut [usize], on_stack: &mut [bool], stack: &mut Vec<usize>| {
        if let (Some(o), Some(l), Some(s)) = (order.get_mut(v), low.get_mut(v), on_stack.get_mut(v)) {
            (*o, *l, *s) = (next, next, true);
            next += 1;
            stack.push(v);
        }
    };
    for root in 0..n {
        if order.get(root) != Some(&UNSEEN) {
            continue;
        }
        visit(root, &mut order, &mut low, &mut on_stack, &mut stack);
        // Nodes being visited, each with the position of its next edge.
        let mut path: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some((v, at)) = path.last_mut() {
            let v = *v;
            if let Some(&w) = edges.get(v).and_then(|es| es.get(*at)) {
                *at += 1;
                match order.get(w).copied() {
                    Some(UNSEEN) => {
                        visit(w, &mut order, &mut low, &mut on_stack, &mut stack);
                        path.push((w, 0));
                    }
                    Some(ow) if on_stack.get(w) == Some(&true) => {
                        if let Some(l) = low.get_mut(v) {
                            *l = (*l).min(ow);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            path.pop();
            let low_v = low.get(v).copied().unwrap_or(0);
            if let Some(l) = path.last().and_then(|(u, _)| low.get_mut(*u)) {
                *l = (*l).min(low_v);
            }
            if order.get(v) == Some(&low_v) {
                let mut members = Vec::new();
                while let Some(w) = stack.pop() {
                    if let Some(s) = on_stack.get_mut(w) {
                        *s = false;
                    }
                    members.push(w);
                    if w == v {
                        break;
                    }
                }
                out.push(members);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::components;

    fn sorted(mut c: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
        c.iter_mut().for_each(|m| m.sort_unstable());
        c.sort();
        c
    }

    #[test]
    fn components_group_exactly_the_nodes_that_reach_each_other() {
        // 0 -> 1 -> 2 -> 0 is a loop; 2 -> 3 -> 4 -> 3 is another, reached from the first; 5
        // reaches 0 but nothing reaches 5; 6 holds only a loop to itself.
        let edges = vec![vec![1], vec![2], vec![0, 3], vec![4], vec![3], vec![0], vec![6]];
        assert_eq!(sorted(components(&edges)), vec![vec![0, 1, 2], vec![3, 4], vec![5], vec![6]]);
        // A long chain closing on its start is one component, without deep recursion.
        let n = 200_000;
        let chain: Vec<Vec<usize>> = (0..n).map(|i| vec![(i + 1) % n]).collect();
        assert_eq!(components(&chain).iter().map(Vec::len).collect::<Vec<_>>(), vec![n]);
    }
}
