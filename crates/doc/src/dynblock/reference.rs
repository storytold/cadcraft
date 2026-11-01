//! Dynamic block references: which dynamic block a block reference shows, the values it was given
//! and the custom properties those make.
//!
//! A reference whose values differ from the block's own refers to an anonymous block (`*U12`)
//! holding the result, and its extension dictionary says what it is: `AcDbBlockRepresentation`
//! has `AcDbRepData` (pointing at the dynamic block's BLOCK_RECORD) and an `AppDataCache` whose
//! `ACAD_ENHANCEDBLOCKDATA` dictionary holds one XRECORD per parameter, keyed by its graph id, with
//! the value in block coordinates. A reference to the dynamic block itself has the block's values.

use std::collections::{BTreeMap, HashMap};

use cadcraft_geom::Vec3;
use serde::Serialize;

use super::{ActionKind, DynamicBlock, ParameterKind, dynamic_block};
use crate::foreign::ForeignObject;
use crate::{Drawing, EntityKind, Handle};

/// Most values one reference is read with.
const MAX_VALUES: usize = 10_000;

/// A parameter's value on one reference, in block coordinates.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Value {
    /// A visibility state's name or a lookup value, and where the parameter is when the record
    /// says (an action may have carried it from where the dynamic block has it).
    Text {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<Vec3>,
    },
    /// A point parameter's position.
    Point { at: Vec3 },
    /// A two-point parameter's points.
    Points { base: Vec3, end: Vec3 },
    /// A flip parameter's reflection line and whether it is flipped.
    Flip { base: Vec3, end: Vec3, flipped: bool },
}

/// A dynamic block reference.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    /// The dynamic block it shows (its name in the drawing).
    pub definition: String,
    /// The values it was given, by parameter graph id; parameters missing here have the block's
    /// own values.
    pub values: BTreeMap<i32, Value>,
}

/// One custom property of a reference, as the Properties palette shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProperty {
    pub name: String,
    pub value: String,
    /// The values it may be set to, when it is a choice (visibility states, lookup values, a
    /// distance's list).
    pub choices: Vec<String>,
    /// CADCraft can change it ([`super::set_property`]): a visibility state, a distance, a
    /// position, or a lookup whose tables set only those.
    pub editable: bool,
}

fn objects(d: &Drawing) -> HashMap<Handle, &ForeignObject> {
    d.foreign.objects.iter().map(|o| (o.handle, o)).collect()
}

/// The object entry `key` of dictionary `dict` refers to.
fn entry(objects: &HashMap<Handle, &ForeignObject>, dict: Handle, key: &str) -> Option<Handle> {
    let o = objects.get(&dict)?;
    let mut tags = o.tags.iter();
    while let Some(t) = tags.next() {
        if t.code == 3 && t.value.eq_ignore_ascii_case(key) {
            return tags.next().filter(|t| t.code == 350 || t.code == 360).and_then(|t| Handle::parse_hex(&t.value));
        }
    }
    None
}

/// Every entry of dictionary `dict`: (key, object).
fn entries(objects: &HashMap<Handle, &ForeignObject>, dict: Handle) -> Vec<(String, Handle)> {
    let Some(o) = objects.get(&dict) else { return Vec::new() };
    let mut out = Vec::new();
    let mut key: Option<&str> = None;
    for t in &o.tags {
        match t.code {
            3 => key = Some(t.value.as_str()),
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

/// A value XRECORD: its text (group 1) when it has one, with the parameter's position (a
/// visibility or lookup grip's, 10/20/30) when it has that too; else its points; a flip
/// parameter's also has its state, a group 70 after its points (the header's `70 25`, `70 104`
/// come before them).
fn value(o: &ForeignObject) -> Option<Value> {
    let num = |code: i32, i: usize| {
        o.tags.iter().filter(|t| t.code == code).nth(i).and_then(|t| t.value.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    };
    let point = |i: usize| Some(Vec3::new(num(10, i)?, num(20, i)?, num(30, i).unwrap_or(0.0)));
    if let Some(t) = o.tags.iter().find(|t| t.code == 1) {
        return Some(Value::Text { value: t.value.clone(), at: point(0) });
    }
    let first_point = o.tags.iter().position(|t| t.code == 10).unwrap_or(o.tags.len());
    let state = o.tags.get(first_point..).unwrap_or_default().iter().rev().find(|t| t.code == 70).map(|t| t.value.trim() == "1");
    match (point(0), point(1)) {
        (Some(base), Some(end)) if state.is_some() => Some(Value::Flip { base, end, flipped: state == Some(true) }),
        (Some(base), Some(end)) => Some(Value::Points { base, end }),
        (Some(at), None) => Some(Value::Point { at }),
        _ => None,
    }
}

/// The dynamic block reference `insert` is, or `None` when it isn't one.
pub fn reference(d: &Drawing, insert: Handle) -> Option<Reference> {
    let EntityKind::Insert(ins) = &d.entity(insert)?.kind else { return None };
    let objects = objects(d);
    let rep = d.foreign.entities.get(&insert).and_then(|x| x.xdict).and_then(|x| entry(&objects, x, "AcDbBlockRepresentation"));
    let Some(rep) = rep else {
        // No representation: a reference to a dynamic block with its own values.
        return dynamic_block(d, &ins.block)
            .is_some()
            .then(|| Reference { definition: d.block(&ins.block).map_or_else(|| ins.block.clone(), |b| b.name.clone()), values: BTreeMap::new() });
    };
    let data = entry(&objects, rep, "AcDbRepData").and_then(|h| objects.get(&h).copied())?;
    let target = data.tags.iter().find(|t| t.code == 340).and_then(|t| Handle::parse_hex(&t.value))?;
    let (kind, name) = d.foreign.targets.get(&target)?;
    let definition = d.block(name).filter(|_| kind == "BLOCK_RECORD")?.name.clone();
    let m = dynamic_block(d, &definition)?;
    let mut values = BTreeMap::new();
    let cache = entry(&objects, rep, "AppDataCache").and_then(|c| entry(&objects, c, "ACAD_ENHANCEDBLOCKDATA"));
    for (key, h) in cache.map(|c| entries(&objects, c)).unwrap_or_default().into_iter().take(MAX_VALUES) {
        if let (Ok(node), Some(v)) = (key.trim().parse::<i32>(), objects.get(&h).and_then(|o| value(o))) {
            // The record's groups alone can't tell a flip from another two-point parameter (a stray
            // 70, or a flip without its state): the parameter it belongs to decides.
            let flip = m.parameters.iter().any(|p| p.node == node && matches!(p.kind, ParameterKind::Flip { .. }));
            let v = match (flip, v) {
                (false, Value::Flip { base, end, .. }) => Value::Points { base, end },
                (true, Value::Points { base, end }) => Value::Flip { base, end, flipped: false },
                (_, v) => v,
            };
            values.insert(node, v);
        }
    }
    Some(Reference { definition, values })
}

/// An angle (radians) as Properties shows it: degrees with AUPREC decimals.
/// An angle parameter's value `a` (radians, counterclockwise) as the drawing shows angles: from
/// ANGBASE, clockwise when ANGDIR is 1 (AutoCAD shows a 0 angle as 90 in a drawing measured
/// clockwise from north, and 0.5 rad as 61.35).
pub(super) fn angle_shown(d: &Drawing, a: f64) -> f64 {
    let base = d.header.f64("ANGBASE", 0.0);
    let shown = if d.header.i64("ANGDIR", 0) == 1 { base - a } else { a - base };
    shown.rem_euclid(std::f64::consts::TAU)
}

/// The value of an angle parameter shown (or typed) as `v` (radians): the inverse of
/// [`angle_shown`].
pub(super) fn angle_from_shown(d: &Drawing, v: f64) -> f64 {
    let base = d.header.f64("ANGBASE", 0.0);
    let a = if d.header.i64("ANGDIR", 0) == 1 { base - v } else { v + base };
    a.rem_euclid(std::f64::consts::TAU)
}

fn angle_text(d: &Drawing, a: f64) -> String {
    let prec = usize::try_from(d.header.i64("AUPREC", 0).clamp(0, 8)).unwrap_or(0);
    format!("{:.prec$}", angle_shown(d, a).to_degrees())
}

/// A number as the drawing shows lengths (LUPREC decimals).
fn number(d: &Drawing, v: f64) -> String {
    let prec = usize::try_from(d.header.i64("LUPREC", 4).clamp(0, 8)).unwrap_or(4);
    format!("{v:.prec$}")
}

/// Whether table cell `cell` reads as `value` (numbers compare as numbers).
fn same(cell: &str, value: &str) -> bool {
    match (cell.trim().parse::<f64>(), value.trim().parse::<f64>()) {
        (Ok(a), Ok(b)) => (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0),
        _ => cell.trim().eq_ignore_ascii_case(value.trim()),
    }
}

/// The current values of a reference's parameters, for evaluating its lookup tables.
struct Current<'a> {
    m: &'a DynamicBlock,
    values: &'a BTreeMap<i32, Value>,
}

impl Current<'_> {
    /// Parameter `node`'s value of property `property`, as table cells write it.
    fn property(&self, node: i32, property: &str) -> Option<String> {
        let p = self.m.parameters.iter().find(|p| p.node == node)?;
        let v = self.values.get(&node);
        match (&p.kind, property.to_ascii_lowercase().as_str()) {
            (ParameterKind::Visibility { states, .. }, "visibilitystate") => match v {
                Some(Value::Text { value, .. }) => Some(value.clone()),
                _ => states.first().map(|s| s.name.clone()),
            },
            (ParameterKind::Linear { base, end, .. }, "updateddistance" | "distance") => {
                let (b, e) = match v {
                    Some(Value::Points { base, end }) => (*base, *end),
                    _ => (*base, *end),
                };
                Some(format!("{}", (e - b).len()))
            }
            (ParameterKind::Lookup { .. }, "lookupstring") => match v {
                Some(Value::Text { value, .. }) => Some(value.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Lookup parameter `node`'s value: the value every lookup table that sets it gives for the
    /// current inputs, or the tables' "no match" name ("Custom") when they don't agree or a table
    /// has no matching row. Also the values it may be set to.
    fn lookup(&self, node: i32) -> (String, Vec<String>) {
        let mut found: Option<Option<String>> = None;
        let mut unmatched = String::from("Custom");
        let mut choices: Vec<String> = Vec::new();
        for a in &self.m.actions {
            let ActionKind::Lookup { columns, rows } = &a.kind else { continue };
            let Some(out) = columns.iter().position(|c| c.lookup && c.node == node) else { continue };
            if rows.is_empty() {
                continue;
            }
            if let Some(c) = columns.get(out) {
                unmatched = c.unmatched.clone();
            }
            for r in rows {
                if let Some(v) = r.get(out).filter(|v| !choices.contains(v)) {
                    choices.push(v.clone());
                }
            }
            let hit = rows.iter().find(|r| {
                columns.iter().enumerate().filter(|(_, c)| !c.lookup).all(|(i, c)| match (r.get(i), self.property(c.node, &c.property)) {
                    (Some(cell), Some(v)) => same(cell, &v),
                    _ => false,
                })
            });
            let v = hit.and_then(|r| r.get(out).cloned());
            found = Some(match found {
                None => v,
                Some(prev) if prev == v => prev,
                Some(_) => None,
            });
        }
        (found.flatten().unwrap_or(unmatched), choices)
    }
}

/// The custom properties block reference `insert` shows (the Properties palette's Custom
/// section): its dynamic block's name and, in parameter order, each parameter shown in Properties
/// with its value. Lengths and positions are in drawing units (block units × the reference's
/// scale); positions are relative to the block's base point.
pub fn custom_properties(d: &Drawing, insert: Handle) -> Option<(String, Vec<CustomProperty>)> {
    let r = reference(d, insert)?;
    let EntityKind::Insert(ins) = &d.entity(insert)?.kind else { return None };
    let m = dynamic_block(d, &r.definition)?;
    let base = d.block(&r.definition).map(|b| b.base).unwrap_or_default();
    let (sx, sy) = (ins.scale.x.abs(), ins.scale.y.abs());
    let cur = Current { m: &m, values: &r.values };
    let mut out = Vec::new();
    for p in m.parameters.iter().filter(|p| p.show_properties) {
        let v = r.values.get(&p.node);
        match &p.kind {
            ParameterKind::Linear { base: b, end: e, label, values, .. } => {
                let (b, e) = match v {
                    Some(Value::Points { base, end }) => (*base, *end),
                    _ => (*b, *e),
                };
                let choices = if values.is_list() { values.list.iter().map(|x| number(d, x * sx)).collect() } else { Vec::new() };
                out.push(CustomProperty { name: label.clone(), value: number(d, (e - b).len() * sx), choices, editable: true });
            }
            ParameterKind::Point { at, label, .. } => {
                let at = match v {
                    Some(Value::Point { at }) => *at,
                    _ => *at,
                };
                out.push(CustomProperty { name: format!("{label} X"), value: number(d, (at.x - base.x) * sx), choices: Vec::new(), editable: true });
                out.push(CustomProperty { name: format!("{label} Y"), value: number(d, (at.y - base.y) * sy), choices: Vec::new(), editable: true });
            }
            ParameterKind::Rotation { base: hb, end: he, label, .. } => {
                let (b, e) = match v {
                    Some(Value::Points { base, end }) => (*base, *end),
                    _ => (*hb, *he),
                };
                let now = (e - b).xy();
                let zero = (*he - *hb).xy();
                let a = (now.y.atan2(now.x) - zero.y.atan2(zero.x)).rem_euclid(std::f64::consts::TAU);
                out.push(CustomProperty { name: label.clone(), value: angle_text(d, a), choices: Vec::new(), editable: true });
            }
            ParameterKind::Flip { label, states, .. } => {
                let flipped = matches!(v, Some(Value::Flip { flipped: true, .. }));
                let value = states[usize::from(flipped)].clone();
                out.push(CustomProperty { name: label.clone(), value, choices: states.to_vec(), editable: true });
            }
            ParameterKind::Visibility { label, states, .. } => {
                let value = cur.property(p.node, "VisibilityState").unwrap_or_default();
                out.push(CustomProperty { name: label.clone(), value, choices: states.iter().map(|s| s.name.clone()).collect(), editable: true });
            }
            ParameterKind::Lookup { label, .. } => {
                let (value, choices) = cur.lookup(p.node);
                let editable = lookup_sets_what_changes(&m, p.node);
                out.push(CustomProperty { name: label.clone(), value, choices, editable });
            }
            _ => {}
        }
    }
    Some((r.definition, out))
}

/// Whether every lookup table that sets lookup parameter `node` sets only what
/// [`super::set_property`] can apply with it: visibility states and distances.
fn lookup_sets_what_changes(m: &DynamicBlock, node: i32) -> bool {
    let mut any = false;
    for a in &m.actions {
        let ActionKind::Lookup { columns, rows } = &a.kind else { continue };
        if rows.is_empty() || !columns.iter().any(|c| c.lookup && c.node == node) {
            continue;
        }
        any = true;
        if !columns
            .iter()
            .all(|c| matches!(c.property.to_ascii_lowercase().as_str(), "lookupstring" | "visibilitystate" | "updateddistance" | "distance"))
        {
            return false;
        }
    }
    any
}
