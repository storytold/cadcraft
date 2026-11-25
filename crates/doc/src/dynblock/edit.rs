//! Editing a dynamic block's parameters and actions in the Block Editor: moving them, dragging
//! one of their points, erasing them. As AutoCAD does it (observed in files it saves):
//!
//! - Moving a parameter moves its points, its grips and its label; moving an action moves only
//!   where it shows (its frame and selection stay).
//! - Erasing a parameter erases its grips, their location components and the actions that follow
//!   it; erasing an action erases only it. The graph keeps every other node's id; node and edge
//!   indices close up and the edges are linked again in their order.

use cadcraft_geom::{Vec2, Vec3};

use super::write::{Graph, dict_entry, finite, real};
use super::{ParameterKind, dynamic_block};
use crate::foreign::ForeignObject;
use crate::{Drawing, Handle, RawTag};

/// A point of a parameter or action that can be dragged on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementPoint {
    /// A linear parameter's start point.
    Base,
    /// A linear parameter's endpoint.
    End,
    /// Where a one-point parameter is (a point, visibility or lookup parameter), or where an
    /// action shows.
    At,
    /// A point parameter's label, or the line of a linear parameter's label (its offset).
    Label,
}

/// The block's graph object and its graph.
fn graph(d: &Drawing, block: &str) -> Result<Graph, String> {
    let key = ("BLOCK_RECORD".to_string(), block.trim().to_ascii_uppercase());
    let xdict = d.foreign.records.get(&key).and_then(|x| x.xdict).ok_or("The block has no parameters or actions.")?;
    let g = dict_entry(d, xdict, "ACAD_ENHANCEDBLOCK").ok_or("The block has no parameters or actions.")?;
    let o = d.foreign.objects.iter().find(|o| o.handle == g).ok_or("The block's evaluation graph is missing.")?;
    Graph::read(o).ok_or_else(|| "The block's evaluation graph can't be changed.".to_string())
}

fn object_mut(d: &mut Drawing, h: Handle) -> Option<&mut ForeignObject> {
    std::sync::Arc::make_mut(&mut d.foreign).objects.iter_mut().find(|o| o.handle == h)
}

/// The groups of section `marker` (from its `100` marker to the next one) of `tags`.
fn section_range(tags: &[RawTag], marker: &str) -> Option<std::ops::Range<usize>> {
    let start = tags.iter().position(|t| t.code == 100 && t.value.eq_ignore_ascii_case(marker))? + 1;
    let end = tags.iter().skip(start).position(|t| t.code == 100).map_or(tags.len(), |n| start + n);
    Some(start..end)
}

/// The index of the first group `code` in section `marker`.
fn find(tags: &[RawTag], marker: &str, code: i32) -> Option<usize> {
    let r = section_range(tags, marker)?;
    let start = r.start;
    tags.get(r)?.iter().position(|t| t.code == code).map(|i| start + i)
}

/// The point at group `code` (x; y and z follow) in section `marker`.
fn point_at(tags: &[RawTag], marker: &str, code: i32) -> Option<Vec3> {
    let i = find(tags, marker, code)?;
    let v = |k: usize| tags.get(i + k).filter(|t| t.code == code + 10 * k as i32).and_then(|t| t.value.trim().parse::<f64>().ok());
    Some(Vec3::new(v(0)?, v(1)?, v(2).unwrap_or(0.0)))
}

/// Set the point at group `code` in section `marker`; whether it was there.
fn set_point(tags: &mut [RawTag], marker: &str, code: i32, p: Vec3) -> bool {
    let Some(i) = find(tags, marker, code) else { return false };
    for (k, v) in [p.x, p.y, p.z].into_iter().enumerate() {
        let c = code + 10 * k as i32;
        if let Some(t) = tags.get_mut(i + k).filter(|t| t.code == c) {
            *t = real(c, v);
        }
    }
    true
}

/// Set the real at group `code` in section `marker`.
fn set_real(tags: &mut [RawTag], marker: &str, code: i32, v: f64) {
    if let Some(i) = find(tags, marker, code)
        && let Some(t) = tags.get_mut(i)
    {
        *t = real(code, v);
    }
}

/// Element graph id `node` of `block`: its object's kind and handle, and its grips' handles by
/// position (a linear parameter's base grip first; `None` where it has no grip there).
struct Element {
    kind: String,
    handle: Handle,
    param: Option<ParameterKind>,
    grips: Vec<Option<Handle>>,
}

fn element(d: &Drawing, block: &str, node: i32) -> Result<Element, String> {
    let g = graph(d, block)?;
    let n = g.nodes.iter().find(|n| n.id == node).ok_or_else(|| format!("No parameter or action {node} in the block."))?;
    let o = d.foreign.objects.iter().find(|o| o.handle == n.handle).ok_or("The block's parameter is missing.")?;
    let m = dynamic_block(d, block);
    let p = m.as_ref().and_then(|m| m.parameters.iter().find(|p| p.node == node));
    let grips = match (&m, p) {
        (Some(m), Some(p)) => p.grips.iter().map(|id| m.grip_objects.iter().find(|(g, _)| g == id).map(|(_, h)| *h)).collect(),
        _ => Vec::new(),
    };
    Ok(Element { kind: o.kind.clone(), handle: o.handle, param: p.map(|p| p.kind.clone()), grips })
}

fn is_action(kind: &str) -> bool {
    kind.starts_with("BLOCK") && kind.ends_with("ACTION")
}

/// Move a grip object to `at`, and point a linear grip along `dir`.
fn move_grip(d: &mut Drawing, grip: Handle, at: Vec3, dir: Option<Vec3>) {
    if let Some(o) = object_mut(d, grip) {
        set_point(&mut o.tags, "AcDbBlockGrip", 1010, at);
        if let Some(dir) = dir {
            for (code, v) in [(140, dir.x), (141, dir.y), (142, dir.z)] {
                set_real(&mut o.tags, "AcDbBlockLinearGrip", code, v);
            }
        }
    }
}

/// Move parameter or action `node` of block `block` (any case) by `delta`.
pub fn move_element(d: &mut Drawing, block: &str, node: i32, delta: Vec3) -> Result<(), String> {
    if !finite(delta) {
        return Err("Invalid displacement.".into());
    }
    let e = element(d, block, node)?;
    let shifted = |tags: &[RawTag], marker: &str, code: i32| point_at(tags, marker, code).map(|p| p + delta).filter(|p| finite(*p));
    let tags = d.foreign.objects.iter().find(|o| o.handle == e.handle).map(|o| o.tags.clone()).unwrap_or_default();
    match (&e.param, e.kind.as_str()) {
        (_, k) if is_action(k) => {
            set_element_point(d, block, node, ElementPoint::At, shifted(&tags, "AcDbBlockAction", 1010).ok_or("Invalid point.")?)
        }
        (Some(ParameterKind::Linear { base, end, .. }), _) => {
            let (b, n) = (*base + delta, *end + delta);
            if !finite(b) || !finite(n) {
                return Err("Invalid point.".into());
            }
            if let Some(o) = object_mut(d, e.handle) {
                set_point(&mut o.tags, "AcDbBlock2PtParameter", 1010, b);
                set_point(&mut o.tags, "AcDbBlock2PtParameter", 1011, n);
            }
            for (grip, at) in e.grips.iter().zip([b, n]) {
                if let Some(g) = grip {
                    move_grip(d, *g, at, None);
                }
            }
            Ok(())
        }
        (Some(ParameterKind::Point { .. }), _) => {
            let label = shifted(&tags, "AcDbBlockPointParameter", 1011).ok_or("Invalid point.")?;
            set_element_point(d, block, node, ElementPoint::At, shifted(&tags, "AcDbBlock1PtParameter", 1010).ok_or("Invalid point.")?)?;
            set_element_point(d, block, node, ElementPoint::Label, label)
        }
        (Some(ParameterKind::Visibility { .. } | ParameterKind::Lookup { .. }), _) => {
            set_element_point(d, block, node, ElementPoint::At, shifted(&tags, "AcDbBlock1PtParameter", 1010).ok_or("Invalid point.")?)
        }
        _ => Err("This parameter can't be moved yet.".into()),
    }
}

/// Put point `which` of parameter or action `node` of block `block` (any case) at `to` (block
/// coordinates). A linear parameter's grips turn with it; its label stays as far from its line.
pub fn set_element_point(d: &mut Drawing, block: &str, node: i32, which: ElementPoint, to: Vec3) -> Result<(), String> {
    if !finite(to) {
        return Err("Invalid point.".into());
    }
    let e = element(d, block, node)?;
    let none = || format!("That point of the {} can't be moved.", e.kind.to_ascii_lowercase());
    match (&e.param, which) {
        (None, ElementPoint::At) if is_action(&e.kind) => {
            let o = object_mut(d, e.handle).ok_or("The block's action is missing.")?;
            set_point(&mut o.tags, "AcDbBlockAction", 1010, to);
            Ok(())
        }
        (Some(ParameterKind::Linear { base, end, .. }), ElementPoint::Base | ElementPoint::End) => {
            let (b, n) = if which == ElementPoint::Base { (to, *end) } else { (*base, to) };
            let dir = n - b;
            if !finite(dir) || dir.len() <= 1e-9 {
                return Err("The endpoint must differ from the start point.".into());
            }
            if let Some(o) = object_mut(d, e.handle) {
                set_point(&mut o.tags, "AcDbBlock2PtParameter", if which == ElementPoint::Base { 1010 } else { 1011 }, to);
            }
            for (grip, (at, dir)) in e.grips.iter().zip([(b, -dir), (n, dir)]) {
                if let Some(g) = grip {
                    move_grip(d, *g, at, Some(dir));
                }
            }
            Ok(())
        }
        (Some(ParameterKind::Linear { base, end, .. }), ElementPoint::Label) => {
            let (a, b, p) = (base.xy(), end.xy(), to.xy());
            let u = b - a;
            let len = u.len();
            let offset = if len <= 1e-12 { 0.0 } else { (u.x * (p.y - a.y) - u.y * (p.x - a.x)) / len };
            let o = object_mut(d, e.handle).ok_or("The block's parameter is missing.")?;
            set_real(&mut o.tags, "AcDbBlockLinearParameter", 140, offset);
            Ok(())
        }
        (Some(ParameterKind::Point { .. } | ParameterKind::Visibility { .. } | ParameterKind::Lookup { .. }), ElementPoint::At) => {
            if let Some(o) = object_mut(d, e.handle) {
                set_point(&mut o.tags, "AcDbBlock1PtParameter", 1010, to);
            }
            if let Some(Some(g)) = e.grips.first() {
                move_grip(d, *g, to, None);
            }
            Ok(())
        }
        (Some(ParameterKind::Point { .. }), ElementPoint::Label) => {
            let o = object_mut(d, e.handle).ok_or("The block's parameter is missing.")?;
            set_point(&mut o.tags, "AcDbBlockPointParameter", 1011, to);
            Ok(())
        }
        _ => Err(none()),
    }
}

/// Erase parameters and actions `nodes` of block `block` (any case), as AutoCAD erases them: a
/// parameter takes its grips, their location components and its actions along. Returns the
/// graph ids erased (the parameters' and actions'; grips and components aren't listed).
pub fn erase_elements(d: &mut Drawing, block: &str, nodes: &[i32]) -> Result<Vec<i32>, String> {
    let mut g = graph(d, block)?;
    let kind_of = |h: Handle| d.foreign.objects.iter().find(|o| o.handle == h).map(|o| o.kind.clone()).unwrap_or_default();
    let index_of = |g: &Graph, id: i32| g.nodes.iter().find(|n| n.id == id).map(|n| n.index);
    let mut gone: Vec<i32> = Vec::new(); // node indices
    let mut erased: Vec<i32> = Vec::new(); // graph ids
    for id in nodes {
        let Some(i) = index_of(&g, *id) else { return Err(format!("No parameter or action {id} in the block.")) };
        let kind = g.nodes.iter().find(|n| n.index == i).map(|n| kind_of(n.handle)).unwrap_or_default();
        if kind.contains("GRIP") {
            return Err(format!("{id} is a grip; erase its parameter."));
        }
        let take = |i: i32, gone: &mut Vec<i32>| {
            if !gone.contains(&i) {
                gone.push(i);
            }
        };
        take(i, &mut gone);
        if !erased.contains(id) {
            erased.push(*id);
        }
        if is_action(&kind) {
            continue;
        }
        // A parameter: what it feeds (components, actions) and the grips that feed it.
        for e in &g.edges {
            let (other, outgoing) = if e.from == i {
                (e.to, true)
            } else if e.to == i {
                (e.from, false)
            } else {
                continue;
            };
            let Some(n) = g.nodes.iter().find(|n| n.index == other) else { continue };
            let k = kind_of(n.handle);
            let follows = outgoing && (k == "BLOCKGRIPLOCATIONCOMPONENT" || is_action(&k));
            let grip = !outgoing && k.contains("GRIP") && k != "BLOCKGRIPLOCATIONCOMPONENT";
            if follows || grip {
                take(other, &mut gone);
                if is_action(&k) && !erased.contains(&n.id) {
                    erased.push(n.id);
                }
            }
            // A grip's components hang off the parameter, so they are caught above.
        }
    }
    let handles: Vec<Handle> = g.nodes.iter().filter(|n| gone.contains(&n.index)).map(|n| n.handle).collect();
    // What stays, its indices closed up and its edges linked again in their order.
    let kept: Vec<_> = g.nodes.iter().filter(|n| !gone.contains(&n.index)).cloned().collect();
    let renumber: Vec<(i32, i32)> = kept.iter().zip(0..).map(|(n, i)| (n.index, i)).collect();
    let new_index = |old: i32| renumber.iter().find(|(o, _)| *o == old).map(|(_, n)| *n);
    let edges: Vec<(i32, i32, i32, i32)> = g.edges.iter().filter_map(|e| Some((new_index(e.from)?, new_index(e.to)?, e.kind, e.flags))).collect();
    g.nodes = kept;
    for (n, i) in g.nodes.iter_mut().zip(0..) {
        n.index = i;
        n.links = [-1; 4];
    }
    g.edges.clear();
    for (from, to, kind, flags) in edges {
        g.link(from, to, kind);
        // An edge keeps its flags (a graph read from a file may have set some).
        if let Some(e) = g.edges.last_mut() {
            e.flags = flags;
        }
    }
    let erased_handles: Vec<Handle> = handles.clone();
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    let tags = g.tags();
    if let Some(o) = f.objects.iter_mut().find(|o| o.handle == g.handle) {
        o.tags = tags;
    }
    f.objects.retain(|o| !handles.contains(&o.handle));
    // Visibility states no longer list what was erased.
    drop_from_states(d, block, &erased_handles)?;
    Ok(erased)
}

/// Remove `handles` from the visibility parameter's state lists (parameters and grips shown).
fn drop_from_states(d: &mut Drawing, block: &str, handles: &[Handle]) -> Result<(), String> {
    let Some(m) = dynamic_block(d, block) else { return Ok(()) };
    let Some(ParameterKind::Visibility { controlled, states, .. }) =
        m.parameters.into_iter().map(|p| p.kind).find(|k| matches!(k, ParameterKind::Visibility { .. }))
    else {
        return Ok(());
    };
    if !states.iter().any(|s| s.active.iter().any(|h| handles.contains(h))) {
        return Ok(());
    }
    let mut states = states;
    for s in &mut states {
        s.active.retain(|h| !handles.contains(h));
    }
    super::set_visibility(d, block, &controlled, &states)
}

/// A parameter or action's points the Block Editor can drag, in block coordinates.
pub fn element_points(d: &Drawing, block: &str, node: i32) -> Vec<(ElementPoint, Vec3)> {
    let Ok(e) = element(d, block, node) else { return Vec::new() };
    let Some(tags) = d.foreign.objects.iter().find(|o| o.handle == e.handle).map(|o| &o.tags) else { return Vec::new() };
    match &e.param {
        None if is_action(&e.kind) => point_at(tags, "AcDbBlockAction", 1010).map(|p| vec![(ElementPoint::At, p)]).unwrap_or_default(),
        Some(ParameterKind::Linear { base, end, .. }) => {
            // The label's grip: the middle of its line, `offset` to the left of base → end.
            let offset =
                find(tags, "AcDbBlockLinearParameter", 140).and_then(|i| tags.get(i)).and_then(|t| t.value.trim().parse::<f64>().ok()).unwrap_or(0.0);
            let (a, b) = (base.xy(), end.xy());
            let u = b - a;
            let n = if u.len() > 1e-12 { Vec2::new(-u.y, u.x) * (1.0 / u.len()) } else { Vec2::ZERO };
            let mid = (a + b) * 0.5 + n * offset;
            vec![(ElementPoint::Base, *base), (ElementPoint::End, *end), (ElementPoint::Label, mid.to3(base.z))]
        }
        Some(ParameterKind::Point { at, .. }) => {
            let mut v = vec![(ElementPoint::At, *at)];
            v.extend(point_at(tags, "AcDbBlockPointParameter", 1011).map(|p| (ElementPoint::Label, p)));
            v
        }
        Some(ParameterKind::Visibility { at, .. } | ParameterKind::Lookup { at, .. }) => vec![(ElementPoint::At, *at)],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynblock::{ActionSpec, ParamPoint, ParameterSpec, add_action, add_parameter};

    fn v(x: f64, y: f64) -> Vec3 {
        Vec3::new(x, y, 0.0)
    }

    /// Erasing keeps the flags of the edges that stay (a graph from a file may set them).
    #[test]
    fn erasing_keeps_the_remaining_edges_flags() {
        let mut d = Drawing::default();
        let p = add_parameter(&mut d, "B", &ParameterSpec::Linear { base: v(0.0, 0.0), end: v(10.0, 0.0), label: None, label_offset: 0.0, grips: 2 })
            .unwrap();
        let a =
            add_action(&mut d, "B", &ActionSpec::Move { parameter: p, point: ParamPoint::End, selection: vec![Handle(1)], at: v(1.0, 1.0) }).unwrap();
        let mut g = graph(&d, "B").unwrap();
        for e in &mut g.edges {
            e.flags = 4;
        }
        let tags = g.tags();
        if let Some(o) = object_mut(&mut d, g.handle) {
            o.tags = tags;
        }
        assert_eq!(erase_elements(&mut d, "B", &[a]).unwrap(), [a]);
        let g = graph(&d, "B").unwrap();
        assert_eq!(g.edges.len(), 6, "the parameter's components and grips");
        assert!(g.edges.iter().all(|e| e.flags == 4), "{:?}", g.edges);
    }

    /// A linear parameter with only its end grip: moving it, or its endpoint, takes that grip along.
    #[test]
    fn a_lone_end_grip_follows_its_linear_parameter() {
        let mut d = Drawing::default();
        let p = add_parameter(&mut d, "B", &ParameterSpec::Linear { base: v(0.0, 0.0), end: v(10.0, 0.0), label: None, label_offset: 0.0, grips: 1 })
            .unwrap();
        let grip = |d: &Drawing| {
            let m = dynamic_block(d, "B").unwrap();
            let id = m.parameters.iter().find(|x| x.node == p).unwrap().grips.iter().copied().find(|g| *g != 0).unwrap();
            let h = m.grip_objects.iter().find(|(g, _)| *g == id).unwrap().1;
            let tags = &d.foreign.objects.iter().find(|o| o.handle == h).unwrap().tags;
            let dir_x = find(tags, "AcDbBlockLinearGrip", 140).and_then(|i| tags[i].value.trim().parse::<f64>().ok());
            (point_at(tags, "AcDbBlockGrip", 1010).unwrap(), dir_x)
        };
        move_element(&mut d, "B", p, v(5.0, 1.0)).unwrap();
        assert_eq!(grip(&d).0, v(15.0, 1.0));
        set_element_point(&mut d, "B", p, ElementPoint::End, v(5.0, 11.0)).unwrap();
        let (at, dir_x) = grip(&d);
        assert_eq!(at, v(5.0, 11.0));
        assert!(dir_x.is_some_and(|x| x.abs() < 1e-9), "the grip points along the new direction (0, 10): {dir_x:?}");
    }
}
