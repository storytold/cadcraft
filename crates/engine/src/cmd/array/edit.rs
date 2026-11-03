//! ARRAYEDIT and ARRAYCLOSE: change an associative array's parameters, move its base point,
//! replace items with other objects (REPlace) and undo that (RESet), and edit the source objects
//! (Source places them in the drawing; ARRAYCLOSE saves or discards the changes).
//!
//! Every change builds a new array block for the edited block reference, so copies of the array
//! that shared the old block keep it; blocks nothing refers to any more are removed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{
    ArrayKind, ArrayParams, Block, Drawing, Entity, EntityKind, Handle, Insert, MAX_BLOCK_DEPTH, ReplacedItem, SourceEdit, entity_bounds,
};
use cadcraft_geom::{Bounds2, Mat3, Vec2};
use serde_json::{Value, json};

use super::super::helpers::v3;
use super::super::machines::{SelOutcome, SelectPhase, number};
use super::super::{CommandSpec, bad, bool_or, point_param, targets};

use super::{KEYWORDS, angle, check_size, count, fmt, nesting, other, yes, yes_no};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("arrayedit", "Edit Array", run_arrayedit)
            .menu(&["Modify", "Object", "Array"])
            .params(
                "{handle, cols?, rows?, colSpacing?, rowSpacing?, axisAngle? (degrees), rowElevation?, levels?, levelSpacing?, items?, angle? (fill, degrees), angleBetween?, rotate?: bool, center? ([x, y]), base? ([x, y], the new base point), reset?: bool, replace?: {handles, base? ([x, y], default the objects' centre), items: [[i, j, level]…] | \"source\"}, source?: {item? ([i, j, level], default [0, 0, 0])} (places the source objects for editing; ARRAYCLOSE ends)}",
            )
            .interactive(|_| Ok(Box::new(EditM::default()))),
        CommandSpec::new("arrayclose", "Close Array Editing", run_arrayclose)
            .params("{save?: bool (default true)}")
            .interactive(|_| Ok(Box::new(CloseM))),
    ]
}

// ---------------- the array ----------------

/// The array block reference `h`, its array block and parameters (with the block names its items
/// use).
fn array_of(d: &Drawing, h: Handle) -> Result<(Insert, Arc<Block>, ArrayParams)> {
    let e = d.entity(h).ok_or_else(|| other("No such object."))?;
    let p = d.array_params(e).ok_or_else(|| other("The object is not an associative array."))?;
    let EntityKind::Insert(ins) = &e.kind else { return Err(other("The object is not an associative array.")) };
    let blk = d.block(&ins.block).cloned().ok_or_else(|| other("The array's block is missing."))?;
    Ok((ins.clone(), blk, p))
}

/// The open ARRAYEDIT Source, if any: the array block holding it and the state.
fn open_edit(d: &Drawing) -> Option<(String, SourceEdit)> {
    d.blocks.iter().find_map(|(n, b)| b.array.as_ref().and_then(|a| a.editing.clone()).map(|e| (n.clone(), e)))
}

fn refuse_while_editing(s: &Session) -> Result<()> {
    if open_edit(s.doc()?).is_some() {
        return Err(other("An array's source objects are being edited; enter ARRAYCLOSE first."));
    }
    Ok(())
}

/// Where item (i, j, level) goes in array block coordinates: the image of the block's origin
/// and the item's rotation (a block's own base point is applied on top).
fn item_place(p: &ArrayParams, item: (u32, u32, u32)) -> Result<Mat3> {
    let it =
        p.items().unwrap_or_default().into_iter().find(|it| (it.i, it.j, it.level) == item).ok_or_else(|| other("The array has no such item."))?;
    Ok(Mat3::translate(it.insert.xy()).then_before(Mat3::rotate(it.rotation)))
}

/// Whether `e` refers to one of `names`, through nested block references.
fn refers_to(d: &Drawing, e: &Entity, names: &[&str]) -> bool {
    let mut todo: Vec<(String, usize)> = match &e.kind {
        EntityKind::Insert(i) => vec![(i.block.clone(), 0)],
        _ => return false,
    };
    let mut seen = HashSet::new();
    while let Some((b, depth)) = todo.pop() {
        if names.iter().any(|n| n.eq_ignore_ascii_case(&b)) {
            return true;
        }
        if depth > MAX_BLOCK_DEPTH || !seen.insert(b.to_ascii_uppercase()) {
            continue;
        }
        if let Some(blk) = d.block(&b) {
            todo.extend(blk.entities.iter().filter_map(|e| match &e.kind {
                EntityKind::Insert(i) => Some((i.block.clone(), depth + 1)),
                _ => None,
            }));
        }
    }
    false
}

/// Remove the anonymous blocks among `names` that nothing refers to any more (in order, so a
/// source block goes after the array block that used it).
fn drop_unused(d: &mut Drawing, names: &[String]) {
    for n in names {
        if !n.starts_with('*') || d.block(n).is_none() {
            continue;
        }
        let used = |e: &Arc<Entity>| match &e.kind {
            EntityKind::Insert(i) => i.block.eq_ignore_ascii_case(n),
            EntityKind::MLeader(m) => m.block.as_ref().is_some_and(|b| b.block.eq_ignore_ascii_case(n)),
            _ => false,
        };
        let in_spaces = d.model.iter().any(used) || d.layouts.iter().any(|l| l.entities.iter().any(used));
        let in_blocks = d.blocks.values().any(|b| b.entities.iter().any(used) || b.array.as_ref().is_some_and(|a| a.source.eq_ignore_ascii_case(n)));
        if !in_spaces && !in_blocks {
            d.blocks.remove(n);
        }
    }
}

/// Rebuild array `h` with parameters `p` (and its block reference moved to `insert` when
/// given): a new array block for this reference, old blocks removed when unused.
fn rebuild(s: &mut Session, h: Handle, p: ArrayParams, insert: Option<Vec2>) -> Result<()> {
    if !p.is_valid() {
        return Err(other("The array parameters are not valid."));
    }
    let d = s.doc()?;
    let (old_ins, _, old) = array_of(d, h)?;
    let objs = d.block(&p.source).map(|b| b.entities.len()).ok_or_else(|| other("The array's source block is missing."))?;
    check_size(s, p.item_count(), objs)?;
    let d = s.doc_mut()?;
    let items = p.item_entities(d).ok_or_else(|| other("The array parameters are not valid."))?;
    let name = d.anonymous_block_name("*U");
    let mut blk = Block::new(&name);
    for e in items {
        blk.entities.push(e);
    }
    blk.array = Some(p);
    d.blocks.insert(name.clone(), Arc::new(blk));
    d.modify_entity(h, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block.clone_from(&name);
            if let Some(at) = insert {
                i.insert = at.to3(i.insert.z);
            }
        }
    })
    .map_err(|e| other(e.to_string()))?;
    let mut gone = vec![old_ins.block, old.source];
    gone.extend(old.replaced.into_iter().map(|r| r.block));
    drop_unused(d, &gone);
    Ok(())
}

/// A new anonymous block from objects `hs` in array `h`'s block coordinates, with its base point
/// at world point `base`. The objects stay in the drawing.
fn block_from(s: &mut Session, h: Handle, hs: &[Handle], base: Vec2) -> Result<String> {
    let d = s.doc()?;
    let (ins, blk, p) = array_of(d, h)?;
    let inv = ins.transform(blk.base.xy()).inverse().ok_or_else(|| other("The array's scale is zero."))?;
    let ents: Vec<Entity> = hs.iter().filter(|x| **x != h).filter_map(|x| d.entity(*x).map(|e| (**e).clone())).collect();
    if ents.is_empty() {
        return Err(other("Select objects other than the array."));
    }
    if ents.iter().any(|e| refers_to(d, e, &[&ins.block, &p.source])) {
        return Err(other("An array's items can't contain the array itself."));
    }
    let refs: Vec<&Entity> = ents.iter().collect();
    if nesting(d, &refs, &mut Default::default(), 0) + 2 > MAX_BLOCK_DEPTH {
        return Err(other(format!("The objects nest block references too deeply (more than {MAX_BLOCK_DEPTH} levels).")));
    }
    let d = s.doc_mut()?;
    let name = d.anonymous_block_name("*U");
    let mut b = Block::new(&name);
    b.base = v3(inv.apply(base));
    for mut e in ents {
        e.handle = d.new_handle();
        e.kind.transform(&inv);
        b.entities.push(e);
    }
    d.blocks.insert(name.clone(), Arc::new(b));
    Ok(name)
}

/// The centre of objects `hs` (world).
fn centroid(d: &Drawing, hs: &[Handle]) -> Vec2 {
    let b = hs.iter().filter_map(|h| d.entity(*h)).fold(Bounds2::EMPTY, |a, e| a.union(&entity_bounds(d, e, 0)));
    Some(b.center()).filter(|c| c.is_finite()).unwrap_or_default()
}

/// The item of array `h` under world point `at` (the lowest level when levels overlap).
fn item_at(d: &Drawing, h: Handle, at: Vec2, tol: f64) -> Option<(u32, u32, u32)> {
    let (ins, blk, p) = array_of(d, h).ok()?;
    let w = ins.transform(blk.base.xy());
    let mut best: Option<((u32, u32, u32), f64)> = None;
    // Each block's own extents, measured once.
    let mut extents: HashMap<String, (Vec2, Bounds2)> = HashMap::new();
    for it in p.items()? {
        let name = p.block_for(it.i, it.j, it.level);
        let (base, local) = match extents.get(name) {
            Some(x) => *x,
            None => {
                let b = d.block(name)?;
                let x = (b.base.xy(), b.entities.iter().fold(Bounds2::EMPTY, |a, e| a.union(&entity_bounds(d, e, 1))));
                extents.insert(name.to_string(), x);
                x
            }
        };
        let m = w.then_before(Mat3::translate(it.insert.xy())).then_before(Mat3::rotate(it.rotation)).then_before(Mat3::translate(-base));
        if local.is_empty() {
            continue;
        }
        let corners = [local.min, Vec2::new(local.max.x, local.min.y), local.max, Vec2::new(local.min.x, local.max.y)];
        let wb = Bounds2::from_points(corners.iter().map(|c| m.apply(*c)));
        let inside = at.x >= wb.min.x - tol && at.x <= wb.max.x + tol && at.y >= wb.min.y - tol && at.y <= wb.max.y + tol;
        let dist = wb.center().dist(at);
        if inside && best.is_none_or(|(_, bd)| dist < bd - 1e-12) {
            best = Some(((it.i, it.j, it.level), dist));
        }
    }
    best.map(|b| b.0)
}

/// Move the base point of array `h` to world point `base` without moving the items.
fn rebase(s: &mut Session, h: Handle, base: Vec2) -> Result<()> {
    let d = s.doc()?;
    let (ins, blk, mut p) = array_of(d, h)?;
    let w = ins.transform(blk.base.xy());
    let b = w.inverse().ok_or_else(|| other("The array's scale is zero."))?.apply(base);
    if !b.is_finite() {
        return Err(other("Requires a valid base point."));
    }
    // The source block keeps its objects; its base point moves with the array's.
    let src = d.block(&p.source).ok_or_else(|| other("The array's source block is missing."))?;
    let mut ns = (**src).clone();
    ns.base = (ns.base.xy() + b).to3(ns.base.z);
    let d = s.doc_mut()?;
    ns.name = d.anonymous_block_name("*U");
    p.source.clone_from(&ns.name);
    d.blocks.insert(ns.name.clone(), Arc::new(ns));
    if let ArrayKind::Polar(k) = &mut p.kind {
        k.center -= b;
    }
    rebuild(s, h, p, Some(w.apply(b)))
}

/// ARRAYEDIT Source: place the source objects of array `h` at `item` for editing. Returns them.
fn open_source(s: &mut Session, h: Handle, item: (u32, u32, u32)) -> Result<Vec<Handle>> {
    refuse_while_editing(s)?;
    let d = s.doc()?;
    let (ins, blk, p) = array_of(d, h)?;
    let src = d.block(&p.source).cloned().ok_or_else(|| other("The array's source block is missing."))?;
    let m = ins.transform(blk.base.xy()).then_before(item_place(&p, item)?).then_before(Mat3::translate(-src.base.xy()));
    let space = d.space_of(h).unwrap_or_default();
    let d = s.doc_mut()?;
    let mut out = Vec::new();
    for e in src.entities.iter() {
        let mut ne = (**e).clone();
        ne.handle = d.new_handle();
        ne.kind.transform(&m);
        out.push(ne.handle);
        d.ensure_layer(&ne.common.layer);
        if let Some(st) = d.space_mut(&space) {
            st.push(ne);
        }
    }
    let first_new = d.handseed;
    let mut b = (*blk).clone();
    if let Some(a) = &mut b.array {
        a.editing = Some(SourceEdit { array: h, item, objects: out.clone(), first_new });
    }
    d.blocks.insert(b.name.clone(), Arc::new(b));
    s.set_selection(out.clone());
    s.echo("Edit the source objects, then enter ARRAYCLOSE to save or discard the changes.");
    Ok(out)
}

/// ARRAYCLOSE: end the open Source editing, saving the objects as the array's new source or
/// discarding them.
fn close_source(s: &mut Session, save: bool) -> Result<Value> {
    let d = s.doc()?;
    let (blk_name, ed) = open_edit(d).ok_or_else(|| other("No array is being edited."))?;
    let space = d.space_of(ed.array).unwrap_or_default();
    let names: Vec<String> = {
        let b = d.block(&blk_name);
        [Some(blk_name.clone()), b.and_then(|b| b.array.as_ref()).map(|a| a.source.clone())].into_iter().flatten().collect()
    };
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut objs: Vec<Handle> = ed.objects.iter().copied().filter(|x| d.entity(*x).is_some()).collect();
    if let Some(st) = d.space(&space) {
        let fresh: Vec<Handle> = st
            .iter()
            .filter(|e| e.handle.0 >= ed.first_new && e.handle != ed.array && !objs.contains(&e.handle) && !refers_to(d, e, &refs))
            .map(|e| e.handle)
            .collect();
        objs.extend(fresh);
    }
    // The array still refers to the block being edited (else it was erased or changed).
    let live = d.entity(ed.array).is_some_and(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(&blk_name)));
    if save && live && objs.is_empty() {
        return Err(other("The source would have no objects; enter ARRAYCLOSE No to discard the changes, or erase the array."));
    }
    let d = s.doc_mut()?;
    if let Some(b) = d.blocks.get_mut(&blk_name) {
        let b = Arc::make_mut(b);
        if let Some(a) = &mut b.array {
            a.editing = None;
        }
    }
    if !save {
        for h in &objs {
            d.remove_entity(*h);
        }
        s.set_selection(Vec::new());
        s.echo("The changes to the array were discarded.");
        return Ok(json!({ "saved": false }));
    }
    if !live {
        s.echo("The array no longer exists; the edited objects stay in the drawing.");
        return Ok(json!({ "saved": false }));
    }
    // The new source block (base point at its origin) holds the objects as seen from the item
    // they were placed at.
    let d = s.doc()?;
    let (ins, blk, mut p) = array_of(d, ed.array)?;
    let inv = ins.transform(blk.base.xy()).then_before(item_place(&p, ed.item)?).inverse().ok_or_else(|| other("The array's scale is zero."))?;
    let ents: Vec<Entity> = objs.iter().filter_map(|x| d.entity(*x).map(|e| (**e).clone())).collect();
    let er: Vec<&Entity> = ents.iter().collect();
    if nesting(d, &er, &mut Default::default(), 0) + 2 > MAX_BLOCK_DEPTH {
        return Err(other(format!("The objects nest block references too deeply (more than {MAX_BLOCK_DEPTH} levels).")));
    }
    let d = s.doc_mut()?;
    let name = d.anonymous_block_name("*U");
    let mut nb = Block::new(&name);
    for mut e in ents {
        e.handle = d.new_handle();
        e.kind.transform(&inv);
        nb.entities.push(e);
    }
    d.blocks.insert(name.clone(), Arc::new(nb));
    for h in &objs {
        d.remove_entity(*h);
    }
    p.source = name;
    rebuild(s, ed.array, p, None)?;
    s.set_selection(Vec::new());
    Ok(json!({ "saved": true }))
}

// ---------------- JSON ----------------

fn item_param(v: &Value) -> Option<(u32, u32, u32)> {
    let a = v.as_array()?;
    let n = |k: usize| a.get(k).and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok());
    Some((n(0)?, n(1).unwrap_or(0), n(2).unwrap_or(0)))
}

fn count_param(p: &Value, key: &str) -> Result<Option<u32>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| *n >= 1)
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| bad("arrayedit", format!("`{key}` must be a whole number of at least 1"))),
    }
}

fn num_param(p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| bad("arrayedit", format!("`{key}` must be a number"))),
    }
}

/// The parameter changes of the JSON form applied to `a`.
fn apply_params(a: &mut ArrayParams, p: &Value, to_local: &Mat3) -> Result<()> {
    if let Some(n) = count_param(p, "levels")? {
        a.levels = n;
    }
    if let Some(x) = num_param(p, "levelSpacing")? {
        a.level_spacing = x;
    }
    match &mut a.kind {
        ArrayKind::Rect(r) => {
            if let Some(n) = count_param(p, "cols")? {
                r.cols = n;
            }
            if let Some(n) = count_param(p, "rows")? {
                r.rows = n;
            }
            if let Some(x) = num_param(p, "colSpacing")? {
                r.col_spacing = x;
            }
            if let Some(x) = num_param(p, "rowSpacing")? {
                r.row_spacing = x;
            }
            if let Some(x) = num_param(p, "axisAngle")? {
                r.axis_angle = x.to_radians();
            }
            if let Some(x) = num_param(p, "rowElevation")? {
                r.row_elevation = x;
            }
        }
        ArrayKind::Polar(k) => {
            if let Some(n) = count_param(p, "items")? {
                k.items = n;
            }
            if let Some(n) = count_param(p, "rows")? {
                k.rows = n;
            }
            if let Some(x) = num_param(p, "rowSpacing")? {
                k.row_spacing = x;
            }
            if let Some(x) = num_param(p, "rowElevation")? {
                k.row_elevation = x;
            }
            if let Some(f) = num_param(p, "angle")? {
                k.fill = fill_angle(f)?;
            }
            if let Some(b) = num_param(p, "angleBetween")? {
                k.fill = fill_from_between(b, k.items)?;
            }
            if let Some(r) = p.get("rotate").and_then(Value::as_bool) {
                k.rotate = r;
            }
            if let Some(c) = point_param(p, "center") {
                k.center = to_local.apply(c);
            }
        }
    }
    Ok(())
}

fn fill_angle(f: f64) -> Result<f64> {
    if f.abs() < 1e-9 || f.abs() > 360.0 + 1e-9 {
        return Err(other("Requires a fill angle between -360 and 360 degrees (not 0)."));
    }
    Ok(f.clamp(-360.0, 360.0))
}

fn fill_from_between(b: f64, items: u32) -> Result<f64> {
    if b.abs() < 1e-9 {
        return Err(other("Requires a nonzero angle."));
    }
    let n = f64::from(items);
    let fill = if (b.abs() * n - 360.0).abs() < 1e-9 { 360.0 * b.signum() } else { b * (n - 1.0).max(1.0) };
    if fill.abs() > 360.0 + 1e-9 {
        return Err(other("The items would span more than 360 degrees; enter a smaller angle or fewer items."));
    }
    Ok(fill)
}

fn run_arrayedit(s: &mut Session, p: &Value) -> Result<Value> {
    let h = super::super::curves::handle_param(p, "handle").ok_or_else(|| bad("arrayedit", "`handle` (the array) is required"))?;
    refuse_while_editing(s)?;
    let (ins, blk, _) = array_of(s.doc()?, h)?;
    let w = ins.transform(blk.base.xy());
    let to_local = w.inverse().ok_or_else(|| other("The array's scale is zero."))?;
    if bool_or(p, "reset", false) {
        let (_, _, mut a) = array_of(s.doc()?, h)?;
        a.replaced.clear();
        rebuild(s, h, a, None)?;
    }
    if let Some(r) = p.get("replace").filter(|v| !v.is_null()) {
        let hs = targets(s, r)?;
        let base = point_param(r, "base").unwrap_or_else(|| s.doc().map(|d| centroid(d, &hs)).unwrap_or_default());
        let name = block_from(s, h, &hs, base)?;
        let (_, _, mut a) = array_of(s.doc()?, h)?;
        match r.get("items") {
            Some(Value::String(t)) if t.eq_ignore_ascii_case("source") => a.source = name,
            Some(Value::Array(list)) => {
                for v in list.iter().take(cadcraft_doc::MAX_ARRAY_ITEMS as usize) {
                    let (i, j, level) = item_param(v).ok_or_else(|| bad("arrayedit", "`replace.items` holds [i, j, level] item numbers"))?;
                    a.replaced.push(ReplacedItem { i, j, level, block: name.clone() });
                }
            }
            _ => return Err(bad("arrayedit", "`replace.items` must be a list of [i, j, level] or \"source\"")),
        }
        rebuild(s, h, a, None)?;
    }
    if let Some(b) = point_param(p, "base") {
        rebase(s, h, b)?;
    }
    let (_, _, mut a) = array_of(s.doc()?, h)?;
    let before = a.clone();
    apply_params(&mut a, p, &to_local)?;
    if a != before {
        rebuild(s, h, a, None)?;
    }
    if let Some(src) = p.get("source").filter(|v| !v.is_null()) {
        let item = src
            .get("item")
            .map(|v| item_param(v).ok_or_else(|| bad("arrayedit", "`source.item` is [i, j, level]")))
            .transpose()?
            .unwrap_or((0, 0, 0));
        let objs = open_source(s, h, item)?;
        return Ok(json!({ "handle": h.hex(), "source": objs.iter().map(|x| x.hex()).collect::<Vec<_>>() }));
    }
    Ok(json!({ "handle": h.hex() }))
}

fn run_arrayclose(s: &mut Session, p: &Value) -> Result<Value> {
    close_source(s, bool_or(p, "save", true))
}

// ---------------- interactive ----------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Stage {
    #[default]
    Select,
    Main,
    Base,
    ColsCount,
    ColsSpace,
    ColsTotal,
    RowsCount,
    RowsSpace,
    RowsTotal,
    RowsElev,
    LevelsCount,
    LevelsSpace,
    LevelsTotal,
    Items,
    Between,
    Fill,
    Rotate,
    /// REPlace: objects, their base point, then the items to replace.
    RepSelect,
    RepBase,
    RepItems,
    /// Source: the item whose objects are placed for editing.
    SourceItem,
}

/// ARRAYEDIT: select an array, then change it option by option until eXit.
#[derive(Default)]
struct EditM {
    stage: Stage,
    array: Option<Handle>,
    sel: SelectPhase,
    rep_objs: Vec<Handle>,
    rep_block: Option<String>,
}

impl EditM {
    /// The array's parameters as stored (cheap: the prompt asks for them on every redraw).
    fn params(&self, s: &Session) -> Option<ArrayParams> {
        let d = s.doc().ok()?;
        match &d.entity(self.array?)?.kind {
            EntityKind::Insert(i) => d.block(&i.block)?.array.clone(),
            _ => None,
        }
    }

    /// Apply `f` to the array's parameters and rebuild it.
    fn change(&self, s: &mut Session, f: impl FnOnce(&mut ArrayParams) -> Result<()>) -> Result<()> {
        let h = self.array.ok_or_else(|| other("No array selected."))?;
        let (_, _, mut p) = array_of(s.doc()?, h)?;
        f(&mut p)?;
        rebuild(s, h, p, None)
    }

    fn objs(&self, s: &Session) -> usize {
        self.params(s).and_then(|p| s.doc().ok().and_then(|d| d.block(&p.source).map(|b| b.entities.len()))).unwrap_or(1)
    }

    /// The largest count for one direction when the others hold `others` items.
    fn max_for(&self, s: &Session, others: u64) -> u64 {
        super::max_count(s, others, self.objs(s))
    }

    /// A distance in the array's own units: typed, or picked as the distance from the base point.
    fn distance(&self, s: &Session, i: &Input, cur: f64) -> Result<Option<f64>> {
        let d = match i {
            Input::Enter => return Ok(Some(cur)),
            Input::Text(t) => number(t),
            Input::Point(p) => {
                let h = self.array.ok_or_else(|| other("No array selected."))?;
                let (ins, blk, _) = array_of(s.doc()?, h)?;
                ins.transform(blk.base.xy()).inverse().map(|inv| inv.apply(*p).len())
            }
            _ => return Ok(None),
        };
        d.filter(|d| d.is_finite() && d.abs() > 1e-12).map(Some).ok_or_else(|| other("Requires a nonzero distance."))
    }

    fn main(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let k = match i {
            Input::Enter => return Ok(Step::Done),
            Input::Keyword(k) => k,
            _ => return Err(other("Invalid option keyword.")),
        };
        self.stage = match k.as_str() {
            "eXit" => return Ok(Step::Done),
            "Source" => Stage::SourceItem,
            "REPlace" => {
                self.sel = SelectPhase::default();
                Stage::RepSelect
            }
            "Base point" => Stage::Base,
            "Rows" => Stage::RowsCount,
            "Columns" => Stage::ColsCount,
            "Levels" => Stage::LevelsCount,
            "Items" => Stage::Items,
            "Angle between" => Stage::Between,
            "Fill angle" => Stage::Fill,
            "ROTate items" => Stage::Rotate,
            "RESet" => {
                self.change(s, |p| {
                    p.replaced.clear();
                    Ok(())
                })?;
                s.echo("Replaced items were restored.");
                Stage::Main
            }
            _ => return Err(other("Invalid option keyword.")),
        };
        Ok(Step::Continue)
    }
}

impl Interactive for EditM {
    fn name(&self) -> &'static str {
        "ARRAYEDIT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        refuse_while_editing(s)?;
        let pre = s.selection();
        if let [h] = pre.as_slice()
            && s.doc().ok().and_then(|d| d.entity(*h).and_then(|e| d.array_params(e))).is_some()
        {
            self.array = Some(*h);
            self.stage = Stage::Main;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        use Stage::*;
        let p = self.params(s);
        let rect = p.as_ref().and_then(|p| match &p.kind {
            ArrayKind::Rect(r) => Some(r.clone()),
            ArrayKind::Polar(_) => None,
        });
        let polar = p.as_ref().and_then(|p| match &p.kind {
            ArrayKind::Polar(k) => Some(k.clone()),
            ArrayKind::Rect(_) => None,
        });
        let (levels, ls) = p.as_ref().map(|p| (p.levels, p.level_spacing)).unwrap_or((1, 0.0));
        let (cols, rows, cs, rs, elev) = match (&rect, &polar) {
            (Some(r), _) => (r.cols, r.rows, r.col_spacing, r.row_spacing, r.row_elevation),
            (_, Some(k)) => (k.items, k.rows, 0.0, k.row_spacing, k.row_elevation),
            _ => (1, 1, 0.0, 0.0, 0.0),
        };
        let dist = |m: &str, d: f64| Prompt::new(m, Accept::POINT_OR_NUMBER).default(fmt(d));
        let total = |n: u32, d: f64| d * f64::from(n.saturating_sub(1));
        match self.stage {
            Select => Prompt::new("Select array", Accept::SELECT),
            Main if polar.is_some() => Prompt::new("Enter an option", KEYWORDS)
                .kw(&["Source", "REPlace", "Base point", "Items", "Angle between", "Fill angle", "Rows", "Levels", "ROTate items", "RESet", "eXit"])
                .default("eXit"),
            Main => Prompt::new("Enter an option", KEYWORDS)
                .kw(&["Source", "REPlace", "Base point", "Rows", "Columns", "Levels", "RESet", "eXit"])
                .default("eXit"),
            Base => Prompt::new("Specify base point", Accept::POINT).kw(&["Key point"]).default("centroid"),
            ColsCount => Prompt::new("Enter the number of columns", Accept::NUMBER).default(cols.to_string()),
            ColsSpace => dist("Specify the distance between columns", cs).kw(&["Total"]),
            ColsTotal => dist("Specify the distance between the first and last column", total(cols, cs)),
            RowsCount => Prompt::new("Enter the number of rows", Accept::NUMBER).default(rows.to_string()),
            RowsSpace => dist("Specify the distance between rows", rs).kw(&["Total"]),
            RowsTotal => dist("Specify the distance between the first and last row", total(rows, rs)),
            RowsElev => Prompt::new("Specify the incrementing elevation between rows", Accept::NUMBER).default(fmt(elev)),
            LevelsCount => Prompt::new("Enter the number of levels", Accept::NUMBER).default(levels.to_string()),
            LevelsSpace => dist("Specify the distance between levels", ls).kw(&["Total"]),
            LevelsTotal => dist("Specify the distance between the first and last level", total(levels, ls)),
            Items => Prompt::new("Enter number of items in array", Accept::NUMBER).default(cols.to_string()),
            Between => {
                let b = polar.as_ref().map_or(0.0, |k| {
                    let full = (k.fill.abs() - 360.0).abs() < 1e-9;
                    if k.items <= 1 {
                        k.fill
                    } else if full {
                        k.fill / f64::from(k.items)
                    } else {
                        k.fill / f64::from(k.items - 1)
                    }
                });
                Prompt::new("Specify the angle between items", Accept::NUMBER).default(fmt(b))
            }
            Fill => Prompt::new("Specify the angle to fill (+=ccw, -=cw)", Accept::NUMBER).default(fmt(polar.as_ref().map_or(360.0, |k| k.fill))),
            Rotate => Prompt::new("Rotate arrayed items?", KEYWORDS).kw(&["Yes", "No"]).default(yes(polar.as_ref().is_none_or(|k| k.rotate))),
            RepSelect => Prompt::new("Select replacement objects", Accept::SELECT),
            RepBase => Prompt::new("Select base point of replacement objects", Accept::POINT).kw(&["Key point"]).default("centroid"),
            RepItems => Prompt::new("Select an item in array to replace", Accept::POINT).kw(&["Source objects"]),
            SourceItem => Prompt::new("Select item in array", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        use Stage::*;
        let stage = self.stage;
        if stage == Select {
            return match i {
                Input::Pick(hs) => {
                    let d = s.doc()?;
                    match hs.iter().copied().find(|h| d.entity(*h).and_then(|e| d.array_params(e)).is_some()) {
                        Some(h) => {
                            self.array = Some(h);
                            self.stage = Main;
                            Ok(Step::Continue)
                        }
                        None => Err(other("The object is not an associative array.")),
                    }
                }
                Input::Enter | Input::Cancel => Ok(Step::Done),
                _ => Ok(Step::Continue),
            };
        }
        if stage == RepSelect {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => {
                    self.stage = Main;
                    Ok(Step::Continue)
                }
                SelOutcome::Done(hs) => {
                    self.rep_objs = hs;
                    self.stage = RepBase;
                    s.set_selection(Vec::new());
                    Ok(Step::Continue)
                }
            };
        }
        let h = self.array.ok_or_else(|| other("No array selected."))?;
        let (cols, rows, levels) = self.params(s).map(|p| (p.grid().0, p.grid().1, p.levels)).unwrap_or((1, 1, 1));
        let (c64, r64, l64) = (u64::from(cols), u64::from(rows), u64::from(levels));
        let next = match (stage, &i) {
            (Main, _) => return self.main(s, i),
            (Base, Input::Point(p)) => {
                rebase(s, h, *p)?;
                Main
            }
            (Base, Input::Enter) => {
                let d = s.doc()?;
                let (ins, blk, p) = array_of(d, h)?;
                let src = d.block(&p.source).ok_or_else(|| other("The array's source block is missing."))?;
                let local = src.entities.iter().fold(Bounds2::EMPTY, |a, e| a.union(&entity_bounds(d, e, 1)));
                let at = ins.transform(blk.base.xy()).then_before(Mat3::translate(-src.base.xy())).apply(local.center());
                if at.is_finite() {
                    rebase(s, h, at)?;
                }
                Main
            }
            (Base | RepBase, Input::Keyword(_)) => {
                s.echo("Key points are not available yet; specify a point.");
                stage
            }
            (ColsCount | Items, _) => match count(&i, c64, self.max_for(s, r64.saturating_mul(l64)))? {
                Some(n) => {
                    let n = super::count32(n);
                    self.change(s, |p| {
                        match &mut p.kind {
                            ArrayKind::Rect(r) => r.cols = n,
                            ArrayKind::Polar(k) => k.items = n,
                        }
                        Ok(())
                    })?;
                    if stage == ColsCount { ColsSpace } else { Main }
                }
                None => stage,
            },
            (RowsCount, _) => match count(&i, r64, self.max_for(s, c64.saturating_mul(l64)))? {
                Some(n) => {
                    let n = super::count32(n);
                    self.change(s, |p| {
                        match &mut p.kind {
                            ArrayKind::Rect(r) => r.rows = n,
                            ArrayKind::Polar(k) => k.rows = n,
                        }
                        Ok(())
                    })?;
                    RowsSpace
                }
                None => stage,
            },
            (LevelsCount, _) => match count(&i, l64, self.max_for(s, c64.saturating_mul(r64)))? {
                Some(n) => {
                    let n = super::count32(n);
                    self.change(s, |p| {
                        p.levels = n;
                        Ok(())
                    })?;
                    LevelsSpace
                }
                None => stage,
            },
            (ColsSpace, Input::Keyword(_)) => ColsTotal,
            (RowsSpace, Input::Keyword(_)) => RowsTotal,
            (LevelsSpace, Input::Keyword(_)) => LevelsTotal,
            (ColsSpace | ColsTotal | RowsSpace | RowsTotal | LevelsSpace | LevelsTotal, _) => {
                let p = self.params(s).ok_or_else(|| other("No array selected."))?;
                let (cur, n) = match (stage, &p.kind) {
                    (ColsSpace | ColsTotal, ArrayKind::Rect(r)) => (r.col_spacing, r.cols),
                    (RowsSpace | RowsTotal, ArrayKind::Rect(r)) => (r.row_spacing, r.rows),
                    (RowsSpace | RowsTotal, ArrayKind::Polar(k)) => (k.row_spacing, k.rows),
                    (LevelsSpace | LevelsTotal, _) => (p.level_spacing, p.levels),
                    _ => (0.0, 1),
                };
                let is_total = matches!(stage, ColsTotal | RowsTotal | LevelsTotal);
                let gaps = f64::from(n.saturating_sub(1).max(1));
                let shown = if is_total { cur * f64::from(n.saturating_sub(1)) } else { cur };
                match self.distance(s, &i, shown)? {
                    Some(d) => {
                        let v = if is_total { d / gaps } else { d };
                        self.change(s, |p| {
                            match (stage, &mut p.kind) {
                                (ColsSpace | ColsTotal, ArrayKind::Rect(r)) => r.col_spacing = v,
                                (RowsSpace | RowsTotal, ArrayKind::Rect(r)) => r.row_spacing = v,
                                (RowsSpace | RowsTotal, ArrayKind::Polar(k)) => k.row_spacing = v,
                                _ => p.level_spacing = v,
                            }
                            Ok(())
                        })?;
                        if matches!(stage, RowsSpace | RowsTotal) { RowsElev } else { Main }
                    }
                    None => stage,
                }
            }
            (RowsElev, Input::Enter) => Main,
            (RowsElev, Input::Text(t)) => match number(t) {
                Some(z) if z.is_finite() => {
                    self.change(s, |p| {
                        match &mut p.kind {
                            ArrayKind::Rect(r) => r.row_elevation = z,
                            ArrayKind::Polar(k) => k.row_elevation = z,
                        }
                        Ok(())
                    })?;
                    Main
                }
                _ => return Err(other("Requires a number.")),
            },
            (Between | Fill, Input::Enter) => Main,
            (Between | Fill, _) => match angle(&i, 0.0, &s.angle_settings())? {
                Some(a) => {
                    self.change(s, |p| {
                        if let ArrayKind::Polar(k) = &mut p.kind {
                            k.fill = if stage == Fill { fill_angle(a)? } else { fill_from_between(a, k.items)? };
                        }
                        Ok(())
                    })?;
                    Main
                }
                None => stage,
            },
            (Rotate, _) => {
                let cur = self.params(s).is_none_or(|p| !matches!(p.kind, ArrayKind::Polar(ref k) if !k.rotate));
                match yes_no(&i, cur) {
                    Some(r) => {
                        self.change(s, |p| {
                            if let ArrayKind::Polar(k) = &mut p.kind {
                                k.rotate = r;
                            }
                            Ok(())
                        })?;
                        Main
                    }
                    None => return Err(other("Requires Yes or No.")),
                }
            }
            (RepBase, Input::Point(_) | Input::Enter) => {
                let base = match &i {
                    Input::Point(p) => *p,
                    _ => centroid(s.doc()?, &self.rep_objs),
                };
                self.rep_block = Some(block_from(s, h, &self.rep_objs.clone(), base)?);
                RepItems
            }
            (RepItems, Input::Point(at)) => {
                let tol = s.pixel_size() * s.settings.pickbox;
                let item = item_at(s.doc()?, h, *at, tol).ok_or_else(|| other("No array item there; select an item of the array."))?;
                let name = self.rep_block.clone().ok_or_else(|| other("No replacement objects."))?;
                self.change(s, |p| {
                    p.replaced.push(ReplacedItem { i: item.0, j: item.1, level: item.2, block: name });
                    Ok(())
                })?;
                RepItems
            }
            (RepItems, Input::Keyword(_)) => {
                let name = self.rep_block.clone().ok_or_else(|| other("No replacement objects."))?;
                self.change(s, |p| {
                    p.source = name;
                    Ok(())
                })?;
                Main
            }
            (RepItems, Input::Enter) => Main,
            (SourceItem, Input::Point(at)) => {
                let tol = s.pixel_size() * s.settings.pickbox;
                let item = item_at(s.doc()?, h, *at, tol).ok_or_else(|| other("No array item there; select an item of the array."))?;
                open_source(s, h, item)?;
                return Ok(Step::Done);
            }
            (SourceItem, Input::Enter) => Main,
            _ => stage,
        };
        self.stage = next;
        Ok(Step::Continue)
    }
}

/// ARRAYCLOSE: save or discard the source objects being edited.
struct CloseM;

impl Interactive for CloseM {
    fn name(&self) -> &'static str {
        "ARRAYCLOSE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if open_edit(s.doc()?).is_none() {
            return Err(other("No array is being edited."));
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Save changes to array?", KEYWORDS).kw(&["Yes", "No"]).default("Yes")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match yes_no(&i, true) {
            Some(save) => {
                close_source(s, save)?;
                Ok(Step::Done)
            }
            None => Err(other("Requires Yes or No.")),
        }
    }
}
