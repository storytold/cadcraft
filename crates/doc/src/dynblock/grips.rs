//! The grips of a dynamic block reference, as AutoCAD shows them on a selected reference: an
//! arrow at each end of a distance (linear parameter), a square at a position (point parameter),
//! a circle at the end of an angle (rotation parameter), a flip arrow across a reflection line
//! (flip parameter) and a down-pointing triangle for a list (visibility states, lookup values).
//! Dragging an arrow, square or circle is [`super::drag_grip`]; a flip grip flips at a click; a
//! list grip offers its property's choices.
//!
//! With a visibility parameter, a state lists the grips shown in it; the others are hidden.

use std::collections::{HashMap, HashSet};

use cadcraft_geom::{Vec2, Vec3};
use serde::Serialize;

use super::{DynamicBlock, ParameterKind, Value, custom_properties, dynamic_block, reference};
use crate::{Drawing, EntityKind, Handle};

/// Most grips listed for one reference.
const MAX_GRIPS: usize = 200;

/// Which of a parameter's points a grip is at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GripPart {
    /// A distance's base point.
    Base,
    /// A distance's end point.
    End,
    /// A one-point parameter's point (a position, a visibility or lookup grip).
    At,
}

impl GripPart {
    pub fn name(self) -> &'static str {
        match self {
            GripPart::Base => "base",
            GripPart::End => "end",
            GripPart::At => "at",
        }
    }
    pub fn from_name(s: &str) -> Option<GripPart> {
        match s.trim().to_ascii_lowercase().as_str() {
            "base" => Some(GripPart::Base),
            "end" => Some(GripPart::End),
            "at" => Some(GripPart::At),
            _ => None,
        }
    }
}

/// How a grip looks and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GripShape {
    /// An arrow pointing away from the distance: drag it to change the distance.
    Linear,
    /// A square: drag it to move the position.
    Point,
    /// A down-pointing triangle: click it for the property's choices.
    List,
    /// A circle at the end of an angle: drag it to turn the angle.
    Rotation,
    /// An arrow across a reflection line: click it to flip (its choices are the two states).
    Flip,
}

/// One grip of a dynamic block reference.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynGrip {
    /// The parameter's graph id.
    pub node: i32,
    pub part: GripPart,
    pub shape: GripShape,
    /// Where it is, in drawing coordinates.
    #[serde(serialize_with = "point")]
    pub at: Vec2,
    /// The way an arrow points (a unit vector in drawing coordinates; zero for the others).
    #[serde(serialize_with = "point")]
    pub dir: Vec2,
    /// The property it changes, as Properties names it.
    pub property: String,
    /// A list grip's choices.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

/// Which ends of two-point parameter `node` the block's actions follow, as (base, end): a
/// connection to one point's change (BaseXDelta, EndYDelta, UpdatedEnd…) follows that end, one to
/// the value itself (a distance, an angle) follows both. None when no action is attached to the
/// parameter: its grips are all kept, as the block has them.
fn ends_used(m: &DynamicBlock, node: i32) -> Option<(bool, bool)> {
    let mut used: Option<(bool, bool)> = None;
    for (n, name) in m.actions.iter().flat_map(|a| a.connections.iter()) {
        if *n != node {
            continue;
        }
        let c = name.to_ascii_lowercase();
        let (base, end) = used.unwrap_or((false, false));
        used = Some(if c.starts_with("base") || c.starts_with("updatedbase") {
            (true, end)
        } else if c.starts_with("end") || c.starts_with("updatedend") {
            (base, true)
        } else {
            (true, true)
        });
    }
    used
}

/// Where a grip object at `g` (from its parameter's base, in the dynamic block) is when the
/// parameter's line `home` (base to end, in the block) is now `now` from `base`: kept along and
/// across the line as it turns, moves or stretches. `home` and `now` aren't zero.
fn carried(g: Vec2, home: Vec2, base: Vec2, now: Vec2) -> Vec2 {
    let (u0, u) = (home * (1.0 / home.len()), now * (1.0 / now.len()));
    let (along, across) = (g.dot(u0) / home.len(), u0.x * g.y - u0.y * g.x);
    base + now * along + u.perp() * across
}

fn point<S: serde::Serializer>(p: &Vec2, s: S) -> Result<S::Ok, S::Error> {
    [p.x, p.y].serialize(s)
}

/// The grips of dynamic block reference `insert` (none when it is not one).
pub fn grips(d: &Drawing, insert: Handle) -> Vec<DynGrip> {
    let Some(r) = reference(d, insert) else { return Vec::new() };
    let Some(m) = dynamic_block(d, &r.definition) else { return Vec::new() };
    let Some(EntityKind::Insert(ins)) = d.entity(insert).map(|e| &e.kind) else { return Vec::new() };
    let base = d.block(&r.definition).map(|b| b.base.xy()).unwrap_or_default();
    let xf = ins.transform(base);
    let world = |p: Vec3| xf.apply(p.xy());
    let toward = |p: Vec3, u: Vec3| {
        let v = xf.apply(p.xy() + u.xy()) - xf.apply(p.xy());
        if v.len() > 1e-12 { v * (1.0 / v.len()) } else { Vec2::ZERO }
    };
    // The grips the current visibility state shows (all of them without visibility states).
    let objects: HashMap<i32, Handle> = m.grip_objects.iter().copied().collect();
    let points: HashMap<i32, Vec3> = m.grip_points.iter().copied().collect();
    let mut shown_in_state: Option<HashSet<Handle>> = None;
    let mut visibility_node = None;
    for p in &m.parameters {
        if let ParameterKind::Visibility { states, .. } = &p.kind {
            let current = match r.values.get(&p.node) {
                Some(Value::Text { value, .. }) => states.iter().find(|s| s.name.eq_ignore_ascii_case(value)),
                _ => states.first(),
            };
            if let Some(s) = current {
                shown_in_state = Some(s.active.iter().copied().collect());
            }
            visibility_node = Some(p.node);
            break;
        }
    }
    // A grip shows in the current visibility state when the state lists it, or lists its
    // parameter (AutoCAD shows a listed parameter's grips though the grips aren't listed).
    let param_handle: HashMap<i32, Handle> = m.parameters.iter().map(|p| (p.node, p.handle)).collect();
    let shown = |node: i32, id: i32| {
        id != 0
            && (Some(node) == visibility_node
                || shown_in_state
                    .as_ref()
                    .is_none_or(|a| objects.get(&id).is_some_and(|h| a.contains(h)) || param_handle.get(&node).is_some_and(|h| a.contains(h))))
    };
    let choices: HashMap<String, Vec<String>> =
        custom_properties(d, insert).map(|(_, ps)| ps.into_iter().map(|p| (p.name.to_ascii_uppercase(), p.choices)).collect()).unwrap_or_default();
    let mut out = Vec::new();
    for p in &m.parameters {
        let grip = |i: usize| p.grips.get(i).copied().unwrap_or(0);
        match &p.kind {
            ParameterKind::Linear { base, end, label, from_midpoint, .. } => {
                let (b, e) = match r.values.get(&p.node) {
                    Some(Value::Points { base, end }) => (*base, *end),
                    _ => (*base, *end),
                };
                let u = e - b;
                let u = if u.len() > 1e-12 { u * (1.0 / u.len()) } else { Vec3::new(1.0, 0.0, 0.0) };
                // An end no action follows has no grip (dragging it would change nothing); a
                // distance from its midpoint moves both ends.
                let used = match ends_used(&m, p.node) {
                    Some((base, end)) if !*from_midpoint => [base, end],
                    _ => [true, true],
                };
                for (i, part, at, way) in [(0, GripPart::Base, b, u * -1.0), (1, GripPart::End, e, u)] {
                    if used[i] && shown(p.node, grip(i)) {
                        let (at, dir) = (world(at), toward(at, way));
                        out.push(DynGrip { node: p.node, part, shape: GripShape::Linear, at, dir, property: label.clone(), choices: Vec::new() });
                    }
                }
            }
            ParameterKind::Rotation { base: hb, end: he, label, .. } => {
                let (b, e) = match r.values.get(&p.node) {
                    Some(Value::Points { base, end }) => (*base, *end),
                    _ => (*hb, *he),
                };
                let id = grip(1);
                let (home, now) = ((*he - *hb).xy(), (e - b).xy());
                if shown(p.node, id) && home.len() > 1e-12 && now.len() > 1e-12 {
                    // The grip object's place (AutoCAD draws the circle there, not at the radius'
                    // end), turned and moved with the radius.
                    let at = carried(points.get(&id).copied().unwrap_or(*he).xy() - hb.xy(), home, b.xy(), now);
                    let at = Vec3::new(at.x, at.y, b.z);
                    let u = at - b;
                    let u = if u.len() > 1e-12 { u * (1.0 / u.len()) } else { Vec3::new(1.0, 0.0, 0.0) };
                    let (at, dir) = (world(at), toward(at, u));
                    out.push(DynGrip {
                        node: p.node,
                        part: GripPart::End,
                        shape: GripShape::Rotation,
                        at,
                        dir,
                        property: label.clone(),
                        choices: Vec::new(),
                    });
                }
            }
            ParameterKind::Flip { base: hb, end: he, label, states, .. } => {
                let (b, e, flipped) = match r.values.get(&p.node) {
                    Some(Value::Flip { base, end, flipped }) => (*base, *end, *flipped),
                    _ => (*hb, *he, false),
                };
                let id = grip(0);
                let (home, now) = ((*he - *hb).xy(), (e - b).xy());
                if shown(p.node, id) && home.len() > 1e-12 && now.len() > 1e-12 {
                    // The grip object's place, kept along and across the reflection line as the
                    // line turns or moves with other actions.
                    let g = points.get(&id).copied().unwrap_or(*hb).xy() - hb.xy();
                    let at = carried(g, home, b.xy(), now);
                    let u = now * (1.0 / now.len());
                    let across_dir = if flipped { -u.perp() } else { u.perp() };
                    let (at3, dir) = (Vec3::new(at.x, at.y, b.z), toward(Vec3::new(at.x, at.y, b.z), across_dir.to3(0.0)));
                    out.push(DynGrip {
                        node: p.node,
                        part: GripPart::Base,
                        shape: GripShape::Flip,
                        at: world(at3),
                        dir,
                        property: label.clone(),
                        choices: states.to_vec(),
                    });
                }
            }
            ParameterKind::Point { at, label, .. } => {
                let at = match r.values.get(&p.node) {
                    Some(Value::Point { at }) => *at,
                    _ => *at,
                };
                if shown(p.node, grip(0)) {
                    let at = world(at);
                    out.push(DynGrip {
                        node: p.node,
                        part: GripPart::At,
                        shape: GripShape::Point,
                        at,
                        dir: Vec2::ZERO,
                        property: label.clone(),
                        choices: Vec::new(),
                    });
                }
            }
            ParameterKind::Visibility { at, label, .. } | ParameterKind::Lookup { at, label, .. } => {
                let Some(list) = choices.get(&label.to_ascii_uppercase()).filter(|c| !c.is_empty()) else { continue };
                if shown(p.node, grip(0)) {
                    // Where the reference's record has it (an action may have carried it).
                    let at = match r.values.get(&p.node) {
                        Some(Value::Text { at: Some(q), .. }) => world(*q),
                        _ => world(*at),
                    };
                    out.push(DynGrip {
                        node: p.node,
                        part: GripPart::At,
                        shape: GripShape::List,
                        at,
                        dir: Vec2::ZERO,
                        property: label.clone(),
                        choices: list.clone(),
                    });
                }
            }
            _ => {}
        }
        if out.len() >= MAX_GRIPS {
            break;
        }
    }
    out.retain(|g| g.at.is_finite());
    out
}
