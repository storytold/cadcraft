//! What the Block Editor shows of a dynamic block's parameters and actions: each one's
//! geometry and label, in block coordinates, for the editor to draw.

use cadcraft_geom::Vec3;
use serde::Serialize;

use super::{Cursor, ParameterKind, dynamic_block, section, sections};
use crate::Drawing;

/// One parameter or action as the Block Editor draws it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Mark {
    /// A linear parameter: a dimension from `base` to `end`, its line `offset` to the left of
    /// base → end, its label in the middle; arrows at the ends that have grips.
    Linear { node: i32, base: Vec3, end: Vec3, offset: f64, label: String, base_grip: bool, end_grip: bool },
    /// A point parameter: a marker at `at` and its label at `label_at`.
    Point { node: i32, at: Vec3, label_at: Vec3, label: String, grip: bool },
    /// Another parameter: its name at `at`.
    Parameter { node: i32, at: Vec3, label: String },
    /// An action: a badge with its name at `at`.
    Action { node: i32, at: Vec3, name: String },
}

/// Block `name`'s parameters and actions as the Block Editor draws them (empty when it has none).
pub fn marks(d: &Drawing, name: &str) -> Vec<Mark> {
    let Some(m) = dynamic_block(d, name) else { return Vec::new() };
    let mut out = Vec::new();
    for p in &m.parameters {
        let tags = d.foreign.objects.iter().find(|o| o.handle == p.handle).map(|o| o.tags.as_slice()).unwrap_or_default();
        let secs = sections(tags);
        let label = p.kind.label().unwrap_or(&p.element).to_string();
        let grip = |i: usize| p.grips.get(i).is_some_and(|g| *g != 0);
        out.push(match &p.kind {
            ParameterKind::Linear { base, end, .. } => {
                // The label's offset (140) after its name and description (305, 306).
                let mut c = Cursor::new(section(&secs, "AcDbBlockLinearParameter"));
                c.next(306);
                Mark::Linear { node: p.node, base: *base, end: *end, offset: c.real(140), label, base_grip: grip(0), end_grip: grip(1) }
            }
            ParameterKind::Point { at, .. } => {
                let label_at = Cursor::new(section(&secs, "AcDbBlockPointParameter")).point(1011);
                Mark::Point { node: p.node, at: *at, label_at, label, grip: grip(0) }
            }
            ParameterKind::Polar { base, .. }
            | ParameterKind::Xy { base, .. }
            | ParameterKind::Rotation { base, .. }
            | ParameterKind::Alignment { base, .. }
            | ParameterKind::Flip { base, .. } => Mark::Parameter { node: p.node, at: *base, label },
            ParameterKind::Visibility { at, .. } | ParameterKind::Lookup { at, .. } | ParameterKind::BasePoint { at } => {
                Mark::Parameter { node: p.node, at: *at, label }
            }
            ParameterKind::Other { .. } => continue,
        });
    }
    out.extend(m.actions.iter().map(|a| Mark::Action { node: a.node, at: a.at, name: a.name.clone() }));
    out
}
