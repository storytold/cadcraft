//! Changing a dynamic block reference's custom properties.
//!
//! A changed reference shows an anonymous block (`*U12`) made from its current one with the
//! change applied, and says so the way AutoCAD's own references do, so AutoCAD still reads it as
//! a reference to the dynamic block: the anonymous block's BLOCK_RECORD carries
//! `AcDbBlockRepBTag` xdata naming the dynamic block's record, each copied object keeps the
//! `AcDbBlockRepETag` xdata that pairs it with its object in the dynamic block, and the reference's
//! `AcDbBlockRepresentation` data holds its values (created when it had none) and its history
//! says what was done to it (AutoCAD replays the history when the dynamic block is redefined).
//!
//! What can change so far: a visibility state, a lookup value, a distance and a position, with
//! the stretch and move actions they drive and the lookups that follow. The other parameters'
//! actions come later.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadcraft_geom::{Mat3, Vec2, Vec3};

use super::{ActionKind, DynamicBlock, ParameterKind, Value, dynamic_block, reference};
use crate::foreign::{ForeignExtra, ForeignObject};
use crate::{Block, Drawing, Entity, EntityKind, Handle, RawTag};

/// Xdata application naming a copied object's object in the dynamic block (its element tag).
const ETAG: &str = "AcDbBlockRepETag";
/// Xdata application naming an anonymous block's dynamic block.
const BTAG: &str = "AcDbBlockRepBTag";

fn tag(code: i32, value: impl Into<String>) -> RawTag {
    RawTag { code, value: value.into() }
}

/// The element tag (`AcDbBlockRepETag` 1071) in `x`, if it has one.
fn etag_in(x: &[RawTag]) -> Option<i64> {
    let at = x.iter().position(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(ETAG))?;
    x.iter().skip(at + 1).take_while(|t| t.code != 1001).find(|t| t.code == 1071).and_then(|t| t.value.trim().parse().ok())
}

/// The element tag of object `e`: its own xdata (where the importer keeps it), else any kept in the
/// foreign data.
fn etag(d: &Drawing, e: &Entity) -> Option<i64> {
    etag_in(&e.common.xdata).or_else(|| etag_in(&d.foreign.entities.get(&e.handle)?.xdata))
}

/// Each object of `b` keyed by its element tag, or by its position when it has none.
fn by_tag(d: &Drawing, b: &Block) -> Vec<(i64, Handle)> {
    b.entities.iter().enumerate().map(|(i, e)| (etag(d, e).unwrap_or(i as i64 + 1_000_000), e.handle)).collect()
}

/// The file handle of block `name`'s BLOCK_RECORD, as the kept data refers to it.
fn record_handle(d: &Drawing, name: &str) -> Option<Handle> {
    let key = name.to_ascii_uppercase();
    d.foreign.targets.iter().find(|(_, (k, n))| k == "BLOCK_RECORD" && *n == key).map(|(h, _)| *h)
}

/// The object entry `key` of dictionary `dict` refers to.
fn entry(d: &Drawing, dict: Handle, key: &str) -> Option<Handle> {
    let o = d.foreign.objects.iter().find(|o| o.handle == dict)?;
    let mut tags = o.tags.iter();
    while let Some(t) = tags.next() {
        if t.code == 3 && t.value.eq_ignore_ascii_case(key) {
            return tags.next().filter(|t| t.code == 350 || t.code == 360).and_then(|t| Handle::parse_hex(&t.value));
        }
    }
    None
}

/// Every entry of dictionary `dict`: (key, object).
fn entries(d: &Drawing, dict: Handle) -> Vec<(String, Handle)> {
    let Some(o) = d.foreign.objects.iter().find(|o| o.handle == dict) else { return Vec::new() };
    let mut out = Vec::new();
    let mut key: Option<&str> = None;
    for t in &o.tags {
        match t.code {
            3 => key = Some(&t.value),
            350 | 360 => {
                if let (Some(k), Some(h)) = (key.take(), Handle::parse_hex(&t.value)) {
                    out.push((k.to_string(), h));
                }
            }
            _ => {}
        }
    }
    out
}

/// A parameter's value as a change sets it (block coordinates).
#[derive(Clone, Debug, PartialEq)]
enum Val {
    /// A visibility state or lookup value.
    Text(String),
    /// A point parameter's position.
    Point(Vec3),
    /// A two-point parameter's base and end points.
    Points(Vec3, Vec3),
}

/// A parameter's current value on the reference: the value it was given, else the block's own.
fn current(m: &DynamicBlock, values: &BTreeMap<i32, Value>, node: i32) -> Option<Val> {
    let p = m.parameters.iter().find(|p| p.node == node)?;
    let given = values.get(&node);
    Some(match (&p.kind, given) {
        (_, Some(Value::Text { value })) => Val::Text(value.clone()),
        (_, Some(Value::Point { at })) => Val::Point(*at),
        (_, Some(Value::Points { base, end })) => Val::Points(*base, *end),
        (ParameterKind::Visibility { states, .. }, None) => Val::Text(states.first().map(|s| s.name.clone()).unwrap_or_default()),
        (ParameterKind::Point { at, .. }, None) => Val::Point(*at),
        (
            ParameterKind::Linear { base, end, .. }
            | ParameterKind::Polar { base, end }
            | ParameterKind::Xy { base, end }
            | ParameterKind::Rotation { base, end }
            | ParameterKind::Alignment { base, end }
            | ParameterKind::Flip { base, end },
            None,
        ) => Val::Points(*base, *end),
        _ => return None,
    })
}

/// `v` as a value set allows it: the nearest listed value, a multiple of the increment from the
/// minimum, within the minimum and maximum.
fn allowed(set: &super::ValueSet, v: f64) -> f64 {
    let mut v = v;
    if set.is_list() && !set.list.is_empty() {
        v = set.list.iter().copied().fold(f64::NAN, |best, x| if best.is_nan() || (x - v).abs() < (best - v).abs() { x } else { best });
    }
    if set.has_increment() && set.increment > 0.0 {
        let from = if set.has_minimum() { set.minimum } else { 0.0 };
        v = from + ((v - from) / set.increment).round() * set.increment;
    }
    if set.has_minimum() {
        v = v.max(set.minimum);
    }
    if set.has_maximum() && set.maximum > 0.0 {
        v = v.min(set.maximum);
    }
    v
}

/// A linear parameter's points for distance `dist`: the end point moves along the parameter, or
/// both points move apart from the midpoint.
fn linear_points(base: Vec3, end: Vec3, dist: f64, from_midpoint: bool) -> (Vec3, Vec3) {
    let dir = end - base;
    let len = dir.len();
    let u = if len > 1e-12 { dir * (1.0 / len) } else { Vec3::new(1.0, 0.0, 0.0) };
    if from_midpoint {
        let mid = (base + end) * 0.5;
        (mid - u * (dist / 2.0), mid + u * (dist / 2.0))
    } else {
        (base, base + u * dist)
    }
}

/// The changes one property change makes, before actions run: parameter values by graph id.
#[derive(Default)]
struct Plan {
    set: BTreeMap<i32, Val>,
}

impl Plan {
    /// Set the value a lookup table cell gives property `property` of parameter `node`.
    fn cell(&mut self, m: &DynamicBlock, values: &BTreeMap<i32, Value>, node: i32, property: &str, cell: &str) -> Result<(), String> {
        let p = m.parameters.iter().find(|p| p.node == node).ok_or("A lookup table refers to a missing parameter.")?;
        match (&p.kind, property.to_ascii_lowercase().as_str()) {
            (_, "lookupstring") | (ParameterKind::Visibility { .. }, "visibilitystate") => {
                self.set.insert(node, Val::Text(cell.to_string()));
            }
            (ParameterKind::Linear { from_midpoint, .. }, "updateddistance" | "distance") => {
                let dist = cell
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| format!("A lookup table gives \"{cell}\" for a distance."))?;
                let Some(Val::Points(b, e)) = self.set.get(&node).cloned().or_else(|| current(m, values, node)) else { return Ok(()) };
                let (b2, e2) = linear_points(b, e, dist, *from_midpoint);
                self.set.insert(node, Val::Points(b2, e2));
            }
            (_, other) => return Err(format!("A lookup table sets {other}, which CADCraft can't change yet.")),
        }
        Ok(())
    }

    /// Lookup parameter `node` set to `value`: every lookup table that sets it gives its row's
    /// inputs. A table without that value is left alone; none having it is an error.
    fn lookup(&mut self, m: &DynamicBlock, values: &BTreeMap<i32, Value>, node: i32, value: &str, skip: Option<usize>) -> Result<bool, String> {
        let mut found = false;
        for (i, a) in m.actions.iter().enumerate() {
            let ActionKind::Lookup { columns, rows } = &a.kind else { continue };
            if Some(i) == skip {
                continue;
            }
            let Some(out) = columns.iter().position(|c| c.lookup && c.node == node) else { continue };
            let Some(row) = rows.iter().find(|r| r.get(out).is_some_and(|v| v.trim().eq_ignore_ascii_case(value.trim()))) else { continue };
            found = true;
            for (c, cell) in columns.iter().zip(row) {
                self.cell(m, values, c.node, &c.property, cell)?;
            }
        }
        Ok(found)
    }

    /// After input property `property` of parameter `node` changed: a lookup table with that input
    /// whose inputs (with the new value) match a row gives its lookup value, and the lookup's
    /// other tables then set their inputs for that value (as AutoCAD does).
    fn chain(&mut self, m: &DynamicBlock, values: &BTreeMap<i32, Value>, node: i32, property: &str) -> Result<(), String> {
        let read = |plan: &Plan, n: i32, prop: &str| -> Option<String> {
            let v = plan.set.get(&n).cloned().or_else(|| current(m, values, n))?;
            match (v, prop.to_ascii_lowercase().as_str()) {
                (Val::Text(t), "visibilitystate" | "lookupstring") => Some(t),
                (Val::Points(b, e), "updateddistance" | "distance") => Some(format!("{}", (e - b).len())),
                _ => None,
            }
        };
        for (i, a) in m.actions.iter().enumerate() {
            let ActionKind::Lookup { columns, rows } = &a.kind else { continue };
            if !columns.iter().any(|c| !c.lookup && c.node == node && c.property.eq_ignore_ascii_case(property)) {
                continue;
            }
            let Some(out) = columns.iter().position(|c| c.lookup) else { continue };
            let lookup_node = columns.get(out).map_or(0, |c| c.node);
            let hit = rows.iter().find(|r| {
                columns.iter().enumerate().filter(|(_, c)| !c.lookup).all(|(j, c)| match (r.get(j), read(self, c.node, &c.property)) {
                    (Some(cell), Some(v)) => same(cell, &v),
                    _ => false,
                })
            });
            if let Some(v) = hit.and_then(|r| r.get(out)).cloned() {
                self.set.insert(lookup_node, Val::Text(v.clone()));
                self.lookup(m, values, lookup_node, &v, Some(i))?;
            }
        }
        Ok(())
    }
}

/// Whether table cell `cell` reads as `value` (numbers compare as numbers).
fn same(cell: &str, value: &str) -> bool {
    match (cell.trim().parse::<f64>(), value.trim().parse::<f64>()) {
        (Ok(a), Ok(b)) => (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0),
        _ => cell.trim().eq_ignore_ascii_case(value.trim()),
    }
}

/// A number typed for a property, in drawing units.
fn number(value: &str) -> Result<f64, String> {
    value.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("\"{value}\" is not a number."))
}

/// Set custom property `name` (as the Properties palette names it, any case) of dynamic block
/// reference `insert` to `value` (lengths and positions in drawing units, as Properties shows them).
pub fn set_property(d: &mut Drawing, insert: Handle, name: &str, value: &str) -> Result<(), String> {
    let r = reference(d, insert).ok_or("Not a dynamic block reference.")?;
    let m = dynamic_block(d, &r.definition).ok_or("Not a dynamic block reference.")?;
    let EntityKind::Insert(ins) = &d.entity(insert).ok_or("No such object.")?.kind else { return Err("Not a block reference.".into()) };
    let (sx, sy) = (ins.scale.x.abs().max(1e-12), ins.scale.y.abs().max(1e-12));
    let block_base = d.block(&r.definition).map(|b| b.base).unwrap_or_default();
    let want = name.trim();
    // A point parameter's position is two properties, "Position1 X" and "Position1 Y".
    let (label, axis) = match want.rsplit_once(' ') {
        Some((l, a)) if a.eq_ignore_ascii_case("X") || a.eq_ignore_ascii_case("Y") => (l, Some(a.eq_ignore_ascii_case("X"))),
        _ => (want, None),
    };
    let p = m
        .parameters
        .iter()
        .find(|p| {
            p.show_properties && p.kind.label().is_some_and(|l| l.eq_ignore_ascii_case(want) || (axis.is_some() && l.eq_ignore_ascii_case(label)))
        })
        .ok_or_else(|| format!("The reference has no property \"{name}\"."))?;
    let mut plan = Plan::default();
    match (&p.kind, axis) {
        (ParameterKind::Visibility { states, label, .. }, _) => {
            let s = states.iter().find(|s| s.name.eq_ignore_ascii_case(value.trim())).ok_or_else(|| {
                format!("\"{value}\" is not a state of {label}: {}.", states.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "))
            })?;
            plan.set.insert(p.node, Val::Text(s.name.clone()));
            plan.chain(&m, &r.values, p.node, "VisibilityState")?;
        }
        (ParameterKind::Lookup { label, .. }, _) => {
            plan.set.insert(p.node, Val::Text(value.trim().to_string()));
            if !plan.lookup(&m, &r.values, p.node, value, None)? {
                return Err(format!("\"{value}\" is not a value of {label}."));
            }
        }
        (ParameterKind::Linear { values, from_midpoint, .. }, None) => {
            let typed = number(value)? / sx;
            if typed <= 0.0 {
                return Err(format!("{name} must be more than 0."));
            }
            let dist = allowed(values, typed);
            if dist <= 0.0 {
                return Err(format!("{name} must be more than 0."));
            }
            let Some(Val::Points(b, e)) = current(&m, &r.values, p.node) else { return Err(format!("{name} has no points.")) };
            let (b2, e2) = linear_points(b, e, dist, *from_midpoint);
            plan.set.insert(p.node, Val::Points(b2, e2));
            plan.chain(&m, &r.values, p.node, "UpdatedDistance")?;
        }
        (ParameterKind::Point { .. }, Some(is_x)) => {
            let Some(Val::Point(at)) = current(&m, &r.values, p.node) else { return Err(format!("{name} has no position.")) };
            let v = number(value)?;
            let at = if is_x { Vec3::new(block_base.x + v / sx, at.y, at.z) } else { Vec3::new(at.x, block_base.y + v / sy, at.z) };
            plan.set.insert(p.node, Val::Point(at));
        }
        _ => return Err(format!("CADCraft can't change {name} yet.")),
    }
    apply(d, insert, &r.definition, &r.values, &plan.set, p.node)
}

/// Drag grip `part` of parameter `node` of dynamic block reference `insert` to `to` (drawing
/// coordinates), as AutoCAD's dynamic grips do: a distance's end grip (or base grip) changes the
/// distance by the drag along the parameter, snapped to its value set (both ends move when the
/// distance is measured from its midpoint); a position grip moves the position. The actions and
/// lookups follow as for [`set_property`].
pub fn drag_grip(d: &mut Drawing, insert: Handle, node: i32, part: super::GripPart, to: Vec2) -> Result<(), String> {
    use super::GripPart;
    if !to.is_finite() {
        return Err("Invalid point.".into());
    }
    let r = reference(d, insert).ok_or("Not a dynamic block reference.")?;
    let m = dynamic_block(d, &r.definition).ok_or("Not a dynamic block reference.")?;
    let EntityKind::Insert(ins) = &d.entity(insert).ok_or("No such object.")?.kind else { return Err("Not a block reference.".into()) };
    let base = d.block(&r.definition).map(|b| b.base.xy()).unwrap_or_default();
    let q = ins.transform(base).inverse().ok_or("The reference has a zero scale.")?.apply(to);
    let p = m.parameters.iter().find(|p| p.node == node).ok_or("The reference has no such parameter.")?;
    let mut plan = Plan::default();
    match (&p.kind, part) {
        (ParameterKind::Linear { values, from_midpoint, label, .. }, GripPart::Base | GripPart::End) => {
            let Some(Val::Points(b, e)) = current(&m, &r.values, node) else { return Err(format!("{label} has no points.")) };
            let len = (e - b).len();
            if len < 1e-12 {
                return Err(format!("{label} has no direction."));
            }
            let u = (e - b) * (1.0 / len);
            let q = Vec3::new(q.x, q.y, b.z);
            let raw = if *from_midpoint {
                2.0 * (q - (b + e) * 0.5).dot(u).abs()
            } else if part == GripPart::End {
                (q - b).dot(u)
            } else {
                (e - q).dot(u)
            };
            let dist = allowed(values, raw);
            if !(raw > 0.0 && dist > 0.0 && dist.is_finite()) {
                return Err(format!("{label} must be more than 0."));
            }
            let (b2, e2) = match (from_midpoint, part) {
                (true, _) => linear_points(b, e, dist, true),
                (false, GripPart::Base) => (e - u * dist, e),
                (false, _) => (b, b + u * dist),
            };
            plan.set.insert(node, Val::Points(b2, e2));
            plan.chain(&m, &r.values, node, "UpdatedDistance")?;
        }
        (ParameterKind::Point { label, .. }, GripPart::At) => {
            let Some(Val::Point(at)) = current(&m, &r.values, node) else { return Err(format!("{label} has no position.")) };
            plan.set.insert(node, Val::Point(Vec3::new(q.x, q.y, at.z)));
        }
        _ => return Err("That grip can't be dragged.".into()),
    }
    apply(d, insert, &r.definition, &r.values, &plan.set, node)
}

/// The change of each parameter value an action can follow, by parameter graph id and the
/// connection's name ("EndXDelta", "XDelta"…), from `old` and `new` values.
fn delta(old: &Val, new: &Val, connection: &str) -> f64 {
    let c = connection.to_ascii_lowercase();
    let pick = |v: Vec3| if c.contains('x') { v.x } else { v.y };
    match (old, new) {
        (Val::Point(a), Val::Point(b)) => pick(*b - *a),
        (Val::Points(ab, ae), Val::Points(bb, be)) => {
            if c.starts_with("base") || c.starts_with("updatedbase") {
                pick(*bb - *ab)
            } else {
                pick(*be - *ae)
            }
        }
        _ => 0.0,
    }
}

/// Move entity `e` by `v`: the listed vertices of a line or polyline, else the whole object.
fn move_entity(e: &mut Entity, vertices: Option<&[i32]>, v: Vec2) {
    match (&mut e.kind, vertices) {
        (EntityKind::LwPolyline(pl), Some(idx)) => {
            for i in idx {
                if let Some(p) = usize::try_from(*i).ok().and_then(|i| pl.vertices.get_mut(i)) {
                    p.p += v;
                }
            }
        }
        (EntityKind::Line(l), Some(idx)) => {
            for i in idx {
                match i {
                    0 => l.a = l.a + v.to3(0.0),
                    1 => l.b = l.b + v.to3(0.0),
                    _ => {}
                }
            }
        }
        _ => e.kind.transform(&Mat3::translate(v)),
    }
}

/// Whether point `p` lies inside a stretch frame (two corners, or a polygon).
fn in_frame(frame: &[Vec2], p: Vec2) -> bool {
    match frame {
        [a, b] => p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y),
        f if f.len() >= 3 => {
            let mut inside = false;
            let mut j = f.len() - 1;
            for i in 0..f.len() {
                let (Some(pi), Some(pj)) = (f.get(i), f.get(j)) else { break };
                if (pi.y > p.y) != (pj.y > p.y) && p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x {
                    inside = !inside;
                }
                j = i;
            }
            inside
        }
        _ => true,
    }
}

/// Give `insert` a new anonymous block with the plan's values applied (visibility states, and
/// the stretch and move actions of the changed parameters, with the parameters in their selections
/// carried along), and record the new values on it.
fn apply(
    d: &mut Drawing,
    insert: Handle,
    definition: &str,
    values: &BTreeMap<i32, Value>,
    set: &BTreeMap<i32, Val>,
    direct: i32,
) -> Result<(), String> {
    let m = dynamic_block(d, definition).ok_or("Not a dynamic block reference.")?;
    let EntityKind::Insert(ins) = &d.entity(insert).ok_or("No such object.")?.kind else { return Err("Not a block reference.".into()) };
    let ins = ins.clone();
    let def = d.block(definition).cloned().ok_or("The dynamic block is missing.")?;
    let src = d.block(&ins.block).cloned().ok_or("The reference's block is missing.")?;
    // The dynamic block's objects and the reference's copies of them, by element tag.
    let def_tags: HashMap<Handle, i64> = by_tag(d, &def).into_iter().map(|(t, h)| (h, t)).collect();
    let src_by_tag: HashMap<i64, Handle> = by_tag(d, &src).into_iter().collect();
    let copy_of = |dh: &Handle| def_tags.get(dh).and_then(|t| src_by_tag.get(t)).copied();
    // New values, starting from the change; actions add the parameters they carry along.
    let mut new: BTreeMap<i32, Val> = set.clone();
    let mut moves: HashMap<Handle, Vec<(Option<Vec<i32>>, Vec2)>> = HashMap::new();
    let param_of: HashMap<Handle, i32> = m.parameters.iter().map(|p| (p.handle, p.node)).collect();
    for a in &m.actions {
        let changed: Vec<&(i32, String)> = a.connections.iter().filter(|(n, _)| set.contains_key(n)).collect();
        if changed.is_empty() {
            continue;
        }
        let (mult, angle, frame, vertices): (f64, f64, &[Vec2], &[(Handle, Vec<i32>)]) = match &a.kind {
            ActionKind::Stretch { frame, vertices, multiplier, angle_offset } => (*multiplier, *angle_offset, frame, vertices),
            ActionKind::Move { multiplier, angle_offset } => (*multiplier, *angle_offset, &[], &[]),
            _ => continue,
        };
        // The two connections give the change in x and y.
        let comp = |i: usize| {
            a.connections.get(i).map_or(0.0, |(n, conn)| match (set.get(n), current(&m, values, *n)) {
                (Some(nv), Some(ov)) => delta(&ov, nv, conn),
                _ => 0.0,
            })
        };
        let mult = if mult == 0.0 { 1.0 } else { mult };
        let v = Vec2::new(comp(0), comp(1)).rotate(angle) * mult;
        if !(v.x.is_finite() && v.y.is_finite()) || (v.x == 0.0 && v.y == 0.0) {
            continue;
        }
        // Index this action's vertex lists by handle once, and skip handles already handled in
        // this action's selection, so a selection that repeats a handle does not re-scan the
        // vertex list and re-clone its index vector per occurrence (CWE-400).
        let vtx_by_handle: HashMap<Handle, &Vec<i32>> = vertices.iter().rev().map(|(vh, i)| (*vh, i)).collect();
        let mut handled: HashSet<Handle> = HashSet::new();
        for h in &a.selection {
            if !handled.insert(*h) {
                continue;
            }
            if let Some(node) = param_of.get(h) {
                // A parameter in the selection is carried along: its points in the frame. Which
                // points are in it is decided where the dynamic block has them (AutoCAD evaluates
                // the actions from the block itself, so a point another action has moved out of
                // the frame still follows).
                if set.contains_key(node) {
                    continue;
                }
                let Some(cur) = new.get(node).cloned().or_else(|| current(&m, values, *node)) else { continue };
                let home = current(&m, &BTreeMap::new(), *node);
                let shift = |p: Vec3, at: Option<Vec3>| {
                    if frame.is_empty() || in_frame(frame, at.unwrap_or(p).xy()) { p + v.to3(0.0) } else { p }
                };
                let moved = match (cur, home) {
                    (Val::Point(p), Some(Val::Point(h))) => Val::Point(shift(p, Some(h))),
                    (Val::Point(p), _) => Val::Point(shift(p, None)),
                    (Val::Points(b, e), Some(Val::Points(hb, he))) => Val::Points(shift(b, Some(hb)), shift(e, Some(he))),
                    (Val::Points(b, e), _) => Val::Points(shift(b, None), shift(e, None)),
                    (t, _) => t,
                };
                new.insert(*node, moved);
                continue;
            }
            let Some(c) = copy_of(h) else { continue };
            let idx = vtx_by_handle.get(h).map(|i| (*i).clone());
            moves.entry(c).or_default().push((idx, v));
        }
    }
    // Visibility states.
    let mut shown: HashMap<Handle, bool> = HashMap::new();
    for (node, v) in &new {
        let (Some(ParameterKind::Visibility { controlled, states, .. }), Val::Text(state)) =
            (m.parameters.iter().find(|p| p.node == *node).map(|p| &p.kind), v)
        else {
            continue;
        };
        let visible: HashSet<Handle> =
            states.iter().find(|s| s.name.eq_ignore_ascii_case(state)).map(|s| s.visible.iter().copied().collect()).unwrap_or_default();
        for h in controlled {
            shown.insert(*h, visible.contains(h));
        }
    }
    let shown_copy: HashMap<Handle, bool> = shown.iter().filter_map(|(dh, v)| copy_of(dh).map(|c| (c, *v))).collect();
    // The new anonymous block.
    let name = d.anonymous_block_name("*U");
    // AutoCAD's anonymous copies have no units of their own.
    let mut b =
        Block { name: name.clone(), anonymous: true, xref_path: None, array: None, units: 0, entities: crate::EntityStore::new(), ..(*src).clone() };
    let mut copies: HashMap<Handle, Handle> = HashMap::new();
    for e in src.entities.iter() {
        copies.insert(e.handle, d.new_handle());
    }
    // How far each attribute definition moves (block coordinates): the reference's attribute of
    // that tag moves with it, as AutoCAD moves it.
    let attrib_moves: HashMap<String, Vec2> = src
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(ad) => {
                let v = moves.get(&e.handle).into_iter().flatten().fold(Vec2::ZERO, |acc, (_, v)| acc + *v);
                (v != Vec2::ZERO).then(|| (ad.tag.to_ascii_uppercase(), v))
            }
            _ => None,
        })
        .collect();
    for e in src.entities.iter() {
        let Some(nh) = copies.get(&e.handle).copied() else { continue };
        let mut c: Entity = (**e).clone();
        c.handle = nh;
        // The copy's own xdata names the copy, not its original (as the kept data below does).
        for t in &mut c.common.xdata {
            if t.code == 1005 && Handle::parse_hex(&t.value) == Some(e.handle) {
                t.value = nh.hex();
            }
        }
        if let Some(v) = shown_copy.get(&e.handle) {
            c.common.visible = *v;
        }
        for (idx, v) in moves.get(&e.handle).into_iter().flatten() {
            move_entity(&mut c, idx.as_deref(), *v);
        }
        if let EntityKind::Hatch(h) = &mut c.kind {
            for l in &mut h.loops {
                l.sources = l.sources.iter().map(|s| copies.get(s).copied().unwrap_or(*s)).collect();
            }
        }
        b.entities.push(c);
    }
    refill_hatches(&mut b);
    let def_record = Some(record_handle(d, definition).unwrap_or_else(|| {
        let h = d.new_handle();
        let f = std::sync::Arc::make_mut(&mut d.foreign);
        f.targets.insert(h, ("BLOCK_RECORD".to_string(), definition.to_ascii_uppercase()));
        h
    }));
    let old = ins.block.clone();
    d.blocks.insert(name.clone(), std::sync::Arc::new(b));
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    for (old_h, new_h) in &copies {
        if let Some(mut x) = f.entities.get(old_h).cloned() {
            for t in &mut x.xdata {
                if t.code == 1005 && Handle::parse_hex(&t.value) == Some(*old_h) {
                    t.value = new_h.hex();
                }
            }
            x.xdict = None;
            // Reactors within the block (an associative hatch's outline) follow to the copies;
            // ones outside it are not ours.
            x.reactors = x.reactors.iter().filter_map(|r| copies.get(r).copied()).collect();
            f.entities.insert(*new_h, x);
        }
    }
    if let Some(rec) = def_record {
        let xdata = vec![tag(1001, BTAG), tag(1070, "1"), tag(1005, rec.hex())];
        f.records.insert(("BLOCK_RECORD".into(), name.to_ascii_uppercase()), ForeignExtra { xdata, ..Default::default() });
    }
    // The block-to-drawing transform's linear part turns a block-coordinate move into the move of
    // the reference's attribute.
    let to_world = ins.transform(src.base.xy());
    let origin = to_world.apply(Vec2::ZERO);
    d.modify_entity(insert, |e| {
        if let EntityKind::Insert(i) = &mut e.kind {
            i.block = name.clone();
            for a in &mut i.attribs {
                if let Some(v) = attrib_moves.get(&a.tag.to_ascii_uppercase()) {
                    let w = to_world.apply(*v) - origin;
                    let mut k = EntityKind::Text(a.text.clone());
                    k.transform(&Mat3::translate(w));
                    if let EntityKind::Text(t) = k {
                        a.text = t;
                    }
                }
            }
        }
    })
    .map_err(|e| e.to_string())?;
    let was = current(&m, values, direct);
    record_values(d, insert, def_record, &m, &new, (direct, was.as_ref()));
    drop_if_unused(d, &old);
    Ok(())
}

/// Associative hatches of block `b` whose loop is one closed polyline of the block take its
/// outline again (it may have been stretched).
fn refill_hatches(b: &mut Block) {
    let outlines: HashMap<Handle, Vec<cadcraft_geom::PolyVertex>> = b
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::LwPolyline(pl) if pl.closed => Some((e.handle, pl.vertices.clone())),
            _ => None,
        })
        .collect();
    let hatches: Vec<Handle> = b.entities.iter().filter(|e| matches!(&e.kind, EntityKind::Hatch(h) if h.associative)).map(|e| e.handle).collect();
    for h in hatches {
        b.entities.modify(h, |e| {
            if let EntityKind::Hatch(hh) = &mut e.kind {
                for l in &mut hh.loops {
                    if let [s] = l.sources.as_slice()
                        && let Some(vs) = outlines.get(s)
                    {
                        l.vertices.clone_from(vs);
                    }
                }
            }
        });
    }
}

/// The value records of the references to the dynamic block whose record is `def_record` (from
/// any one of them, by graph id): AutoCAD's layout of each record, class ids included.
fn templates(d: &Drawing, def_record: Handle) -> HashMap<String, Vec<RawTag>> {
    let objects: HashMap<Handle, &ForeignObject> = d.foreign.objects.iter().map(|o| (o.handle, o)).collect();
    let mut out: HashMap<String, Vec<RawTag>> = HashMap::new();
    for rep in d.foreign.objects.iter().filter(|o| o.kind.eq_ignore_ascii_case("DICTIONARY")) {
        // A representation dictionary whose data names this dynamic block.
        let Some(rd) = entry(d, rep.handle, "AcDbRepData").and_then(|h| objects.get(&h)) else { continue };
        if !rd.tags.iter().any(|t| t.code == 340 && Handle::parse_hex(&t.value) == Some(def_record)) {
            continue;
        }
        let Some(values) = entry(d, rep.handle, "AppDataCache").and_then(|c| entry(d, c, "ACAD_ENHANCEDBLOCKDATA")) else { continue };
        for (k, h) in entries(d, values) {
            if let Some(x) = objects.get(&h).filter(|x| x.kind.eq_ignore_ascii_case("XRECORD")) {
                out.entry(k).or_insert_with(|| x.tags.clone());
            }
        }
    }
    out
}

/// A dictionary object `h` owned by `owner` with `entries`.
fn dictionary(h: Handle, owner: Handle, entries: &[(&str, Handle)]) -> ForeignObject {
    let mut tags = vec![tag(5, h.hex()), tag(330, owner.hex()), tag(100, "AcDbDictionary"), tag(280, "1"), tag(281, "1")];
    for (k, v) in entries {
        tags.push(tag(3, *k));
        tags.push(tag(360, v.hex()));
    }
    ForeignObject { handle: h, kind: "DICTIONARY".into(), tags }
}

/// `tags` (a template's groups) as new record `h` owned by `owner`: the template's handle, owner
/// and reactors are not ours (inside reactor brackets, 102, everything goes).
fn own(mut tags: Vec<RawTag>, h: Handle, owner: Handle) -> Vec<RawTag> {
    let mut inside = false;
    tags.retain(|t| {
        if t.code == 102 {
            inside = t.value.starts_with('{');
            return false;
        }
        !(inside || t.code == 5 || t.code == 330)
    });
    let at = tags.iter().position(|t| t.code == 100).unwrap_or(0);
    tags.insert(at, tag(330, owner.hex()));
    tags.insert(0, tag(5, h.hex()));
    tags
}

/// Record values `set` (by parameter graph id) in `insert`'s representation data, and the direct
/// change (of parameter `direct`, from value `old`) in its history, creating them when the reference has none. A
/// new reference's data gets a record for everything other references of the dynamic block have
/// (AutoCAD keeps one for each parameter and stretch action), with its own values; with no other
/// reference to copy, a changed value gets a minimal record.
fn record_values(
    d: &mut Drawing,
    insert: Handle,
    def_record: Option<Handle>,
    m: &DynamicBlock,
    set: &BTreeMap<i32, Val>,
    (direct, old): (i32, Option<&Val>),
) {
    let xdict = d.foreign.entities.get(&insert).and_then(|x| x.xdict);
    let rep = xdict.and_then(|x| entry(d, x, "AcDbBlockRepresentation"));
    let cache = rep.and_then(|r| entry(d, r, "AppDataCache"));
    let data = cache.and_then(|c| entry(d, c, "ACAD_ENHANCEDBLOCKDATA"));
    let templates = def_record.map(|r| templates(d, r)).unwrap_or_default();
    let param = |node: i32| m.parameters.iter().find(|p| p.node == node).map(|p| &p.kind);
    let at = |node: i32| match param(node) {
        Some(
            ParameterKind::Visibility { at, .. }
            | ParameterKind::Lookup { at, .. }
            | ParameterKind::Point { at, .. }
            | ParameterKind::BasePoint { at },
        ) => *at,
        _ => Vec3::ZERO,
    };
    let mut objects: Vec<ForeignObject> = Vec::new();
    let mut fresh: Vec<(String, Vec<RawTag>)> = Vec::new();
    let (cache, data) = match (cache, data) {
        (Some(c), Some(h)) => (c, h),
        _ => {
            // Create the representation: reference xdict ▸ AcDbBlockRepresentation ▸ AcDbRepData
            // (the dynamic block's record) and AppDataCache ▸ ACAD_ENHANCEDBLOCKDATA.
            let Some(rec) = def_record else { return };
            let xd = xdict.unwrap_or_else(|| d.new_handle());
            let (rep, rd, cache, data) = (d.new_handle(), d.new_handle(), d.new_handle(), d.new_handle());
            objects.push(dictionary(rep, xd, &[("AcDbRepData", rd), ("AppDataCache", cache)]));
            objects.push(ForeignObject {
                handle: rd,
                kind: "ACDB_BLOCKREPRESENTATION_DATA".into(),
                tags: vec![tag(5, rd.hex()), tag(330, rep.hex()), tag(100, "AcDbBlockRepresentationData"), tag(70, "1"), tag(340, rec.hex())],
            });
            objects.push(dictionary(cache, rep, &[("ACAD_ENHANCEDBLOCKDATA", data)]));
            objects.push(dictionary(data, cache, &[]));
            // Every record the other references have, with this reference's values: its
            // parameters' positions and defaults (lookups as Properties shows them); action
            // records as they are.
            let lookups: HashMap<String, String> = super::custom_properties(d, insert)
                .map(|(_, ps)| ps.into_iter().map(|p| (p.name.to_ascii_uppercase(), p.value)).collect())
                .unwrap_or_default();
            let mut keys: Vec<&String> = templates.keys().collect();
            keys.sort();
            for k in keys {
                let (Ok(node), Some(t)) = (k.parse::<i32>(), templates.get(k)) else { continue };
                let mut tags = t.clone();
                match param(node) {
                    Some(ParameterKind::Visibility { .. } | ParameterKind::Point { .. } | ParameterKind::BasePoint { .. }) => {
                        write_value(&mut tags, &Val::Point(at(node)));
                    }
                    Some(ParameterKind::Lookup { label, .. }) => {
                        write_value(&mut tags, &Val::Point(at(node)));
                        if let Some(v) = lookups.get(&label.to_ascii_uppercase()) {
                            write_value(&mut tags, &Val::Text(v.clone()));
                        }
                    }
                    _ => {}
                }
                if let Some(v) = current(m, &BTreeMap::new(), node) {
                    write_value(&mut tags, &v);
                }
                fresh.push((k.clone(), tags));
            }
            let f = std::sync::Arc::make_mut(&mut d.foreign);
            match f.objects.iter_mut().find(|o| Some(o.handle) == xdict) {
                Some(x) => {
                    x.tags.push(tag(3, "AcDbBlockRepresentation"));
                    x.tags.push(tag(360, rep.hex()));
                }
                None => {
                    objects.push(dictionary(xd, insert, &[("AcDbBlockRepresentation", rep)]));
                    f.entities.entry(insert).or_default().xdict = Some(xd);
                }
            }
            (cache, data)
        }
    };
    let existing: HashMap<String, Handle> = match objects.iter().find(|o| o.handle == data) {
        Some(_) => HashMap::new(),
        None => entries(d, data).into_iter().collect(),
    };
    let mut add: Vec<(String, Handle)> = Vec::new();
    for (key, mut tags) in fresh {
        if let Some(v) = key.parse::<i32>().ok().and_then(|n| set.get(&n)) {
            write_value(&mut tags, v);
        }
        let h = d.new_handle();
        objects.push(ForeignObject { handle: h, kind: "XRECORD".into(), tags: own(tags, h, data) });
        add.push((key, h));
    }
    for (node, v) in set {
        let key = node.to_string();
        if add.iter().any(|(k, _)| *k == key) {
            continue;
        }
        match existing.get(&key) {
            Some(h) => {
                let f = std::sync::Arc::make_mut(&mut d.foreign);
                if let Some(x) = f.objects.iter_mut().find(|o| o.handle == *h) {
                    write_value(&mut x.tags, v);
                }
            }
            None => {
                let h = d.new_handle();
                let mut tags = templates.get(&key).cloned().unwrap_or_else(|| {
                    let p = at(*node);
                    vec![tag(100, "AcDbXrecord"), tag(280, "1"), tag(10, p.x.to_string()), tag(20, p.y.to_string()), tag(30, p.z.to_string())]
                });
                write_value(&mut tags, v);
                objects.push(ForeignObject { handle: h, kind: "XRECORD".into(), tags: own(tags, h, data) });
                add.push((key, h));
            }
        }
    }
    // The history: AutoCAD replays it over the dynamic block (when the block is redefined), so
    // it must say what was done to this reference.
    let history = entry(d, cache, "ACAD_ENHANCEDBLOCKHISTORY").filter(|h| d.foreign.objects.iter().any(|o| o.handle == *h));
    let mut new_history = None;
    if let (Some(v), Some(p)) = (set.get(&direct), m.parameters.iter().find(|p| p.node == direct)) {
        match history {
            Some(h) => {
                let f = std::sync::Arc::make_mut(&mut d.foreign);
                if let Some(x) = f.objects.iter_mut().find(|o| o.handle == h) {
                    write_history(&mut x.tags, p, old, v);
                }
            }
            None => {
                let h = d.new_handle();
                let mut tags = vec![tag(5, h.hex()), tag(330, cache.hex()), tag(100, "AcDbXrecord"), tag(280, "1"), tag(1070, "3")];
                write_history(&mut tags, p, old, v);
                objects.push(ForeignObject { handle: h, kind: "XRECORD".into(), tags });
                new_history = Some(h);
            }
        }
    }
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    f.objects.extend(objects);
    if let Some(x) = f.objects.iter_mut().find(|o| o.handle == data) {
        for (k, h) in add {
            x.tags.push(tag(3, k));
            x.tags.push(tag(360, h.hex()));
        }
    }
    if let (Some(h), Some(x)) = (new_history, f.objects.iter_mut().find(|o| o.handle == cache)) {
        x.tags.push(tag(3, "ACAD_ENHANCEDBLOCKHISTORY"));
        x.tags.push(tag(360, h.hex()));
    }
}

/// Record the direct change of parameter `p` to `v` in a reference's history (groups `tags`):
/// after its header (`280 1`, `1070 3`), an entry per change: `1071` the id, `300` what, then
/// the value. A visibility state or lookup value goes by the property's name (`1` the text), as
/// does a distance (`40`, in block units: typed in Properties, it replaces its grips' drags); a
/// position as the drag of its grip, `GRIPLOC` with how far it is from where the dynamic block
/// has it (`11/21/31`), when the change moves it (from `old`). An entry for the same id is
/// replaced; a grip back in its place has none.
fn write_history(tags: &mut Vec<RawTag>, p: &super::Parameter, old: Option<&Val>, v: &Val) {
    let grip = |id: i32, moved: Vec3| {
        let home = moved.x.abs() < 1e-9 && moved.y.abs() < 1e-9 && moved.z.abs() < 1e-9;
        let e = vec![
            tag(1071, id.to_string()),
            tag(300, "GRIPLOC"),
            tag(11, moved.x.to_string()),
            tag(21, moved.y.to_string()),
            tag(31, moved.z.to_string()),
        ];
        (id, if home { Vec::new() } else { e })
    };
    // Each grip with where it was, where it is and where the dynamic block has it.
    let grips = |places: &[(Vec3, Vec3, Vec3)]| -> Vec<(i32, Vec<RawTag>)> {
        p.grips
            .iter()
            .zip(places)
            .filter(|(g, (was, is, _))| **g != 0 && (*was - *is).len() > 1e-9)
            .map(|(g, (_, is, home))| grip(*g, *is - *home))
            .collect()
    };
    let was = match old {
        Some(Val::Point(q)) => Some(*q),
        _ => None,
    };
    let changes: Vec<(i32, Vec<RawTag>)> = match (&p.kind, v) {
        (ParameterKind::Visibility { label, .. } | ParameterKind::Lookup { label, .. }, Val::Text(t)) => {
            vec![(p.node, vec![tag(1071, p.node.to_string()), tag(300, label.clone()), tag(1, t.clone())])]
        }
        (ParameterKind::Linear { label, .. }, Val::Points(b, e)) => {
            let mut c: Vec<(i32, Vec<RawTag>)> = p.grips.iter().filter(|g| **g != 0).map(|g| (*g, Vec::new())).collect();
            c.push((p.node, vec![tag(1071, p.node.to_string()), tag(300, label.clone()), tag(40, (*e - *b).len().to_string())]));
            c
        }
        (ParameterKind::Point { at, .. }, Val::Point(q)) => grips(&[(was.unwrap_or(*at), *q, *at)]),
        _ => return,
    };
    // The header, then the entries (each from its 1071).
    let start = tags.iter().position(|t| t.code == 1071).unwrap_or(tags.len());
    let mut entries: Vec<Vec<RawTag>> = Vec::new();
    for t in tags.drain(start..) {
        if t.code == 1071 || entries.is_empty() {
            entries.push(Vec::new());
        }
        if let Some(e) = entries.last_mut() {
            e.push(t);
        }
    }
    let id = |e: &[RawTag]| e.first().filter(|t| t.code == 1071).and_then(|t| t.value.trim().parse::<i32>().ok());
    entries.retain(|e| !changes.iter().any(|(c, _)| id(e) == Some(*c)));
    entries.extend(changes.into_iter().map(|(_, e)| e).filter(|e| !e.is_empty()));
    tags.extend(entries.into_iter().flatten());
}

/// Write value `v` into a value record's groups: its text (group 1), or its points in order over
/// the record's points (10/20/30), adding them when it has fewer.
fn write_value(tags: &mut Vec<RawTag>, v: &Val) {
    let points: Vec<Vec3> = match v {
        Val::Text(t) => {
            match tags.iter_mut().find(|x| x.code == 1) {
                Some(x) => x.value.clone_from(t),
                None => tags.push(tag(1, t.clone())),
            }
            return;
        }
        Val::Point(p) => vec![*p],
        Val::Points(b, e) => vec![*b, *e],
    };
    for (i, p) in points.iter().enumerate() {
        // The i-th point's groups, else new ones before any text.
        let find = |tags: &[RawTag], code: i32, n: usize| tags.iter().enumerate().filter(|(_, t)| t.code == code).nth(n).map(|(j, _)| j);
        match (find(tags, 10, i), find(tags, 20, i)) {
            (Some(jx), Some(jy)) => {
                if let Some(t) = tags.get_mut(jx) {
                    t.value = p.x.to_string();
                }
                if let Some(t) = tags.get_mut(jy) {
                    t.value = p.y.to_string();
                }
                if let Some(t) = find(tags, 30, i).and_then(|jz| tags.get_mut(jz)) {
                    t.value = p.z.to_string();
                }
            }
            _ => {
                let at = tags.iter().position(|t| t.code == 1).unwrap_or(tags.len());
                tags.splice(at..at, [tag(10, p.x.to_string()), tag(20, p.y.to_string()), tag(30, p.z.to_string())]);
            }
        }
    }
}

/// Remove anonymous block `name` when nothing refers to it any more (the block a changed
/// reference used to show), with the data kept for its record.
fn drop_if_unused(d: &mut Drawing, name: &str) {
    if !name.starts_with('*') {
        return;
    }
    let refers = |e: &Entity| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(name));
    let used = d.model.iter().any(|e| refers(e))
        || d.layouts.iter().any(|l| l.entities.iter().any(|e| refers(e)))
        || d.blocks.values().any(|b| b.entities.iter().any(|e| refers(e)));
    if used {
        return;
    }
    if let Some(b) = d.blocks.remove(name) {
        let f = std::sync::Arc::make_mut(&mut d.foreign);
        f.records.remove(&("BLOCK_RECORD".to_string(), name.to_ascii_uppercase()));
        f.records.remove(&("BLOCK".to_string(), name.to_ascii_uppercase()));
        for e in b.entities.iter() {
            f.entities.remove(&e.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(h: u64, tag_value: Option<&str>) -> Entity {
        let mut e = Entity::new(Handle(h), EntityKind::Line(crate::Line { a: Vec3::ZERO, b: Vec3::ZERO }));
        if let Some(v) = tag_value {
            e.common.xdata = vec![tag(1001, ETAG), tag(1071, v)];
        }
        e
    }

    #[test]
    fn element_tags_are_read_from_the_entitys_own_xdata() {
        let d = Drawing::new_imperial();
        let mut def = Block::new("DEF");
        def.entities.push(tagged(1, Some("10")));
        def.entities.push(tagged(2, Some("20")));
        let mut anon = Block::new("*U1");
        // Reordered, and the first object left out.
        anon.entities.push(tagged(5, Some("20")));
        anon.entities.push(tagged(6, None));
        assert_eq!(by_tag(&d, &def), vec![(10, Handle(1)), (20, Handle(2))]);
        assert_eq!(by_tag(&d, &anon), vec![(20, Handle(5)), (1_000_001, Handle(6))]);
    }
}
