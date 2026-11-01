//! Parametric drawing data: geometric and dimensional constraints, user parameters and the
//! parametric settings (GEOMCONSTRAINT, DIMCONSTRAINT, PARAMETERS, CONSTRAINTSETTINGS).
//!
//! Only the data lives here; the solver is `cadcraft-constraints`.

use serde::{Deserialize, Serialize};

use crate::Handle;

/// Which part of an entity a constraint refers to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "index")]
pub enum Sub {
    /// The object itself (a line, circle, arc, point).
    #[default]
    Whole,
    Start,
    End,
    Mid,
    Center,
    /// Polyline vertex `i`.
    Vertex(u32),
    /// Polyline segment `i` (from vertex `i` to the next one), treated as a line.
    Segment(u32),
}

impl Sub {
    /// Parse `start`, `end`, `mid`, `center`, `whole`, `v3` / `vertex3`, `s2` / `seg2` / `segment2`.
    pub fn parse(t: &str) -> Option<Sub> {
        let l = t.trim().to_ascii_lowercase();
        Some(match l.as_str() {
            "" | "whole" | "object" | "o" => Sub::Whole,
            "start" | "s" => Sub::Start,
            "end" | "e" => Sub::End,
            "mid" | "midpoint" | "m" => Sub::Mid,
            "center" | "centre" | "c" => Sub::Center,
            _ => {
                for (p, vertex) in [("vertex", true), ("segment", false), ("seg", false), ("s", false), ("v", true)] {
                    if let Some(n) = l.strip_prefix(p) {
                        let i: u32 = n.trim().parse().ok()?;
                        return Some(if vertex { Sub::Vertex(i) } else { Sub::Segment(i) });
                    }
                }
                return None;
            }
        })
    }
    pub fn label(self) -> String {
        match self {
            Sub::Whole => "whole".into(),
            Sub::Start => "start".into(),
            Sub::End => "end".into(),
            Sub::Mid => "mid".into(),
            Sub::Center => "center".into(),
            Sub::Vertex(i) => format!("v{i}"),
            Sub::Segment(i) => format!("seg{i}"),
        }
    }
}

/// A reference to (part of) an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GeomRef {
    pub handle: Handle,
    #[serde(default)]
    pub sub: Sub,
}

impl GeomRef {
    pub fn new(handle: Handle, sub: Sub) -> Self {
        GeomRef { handle, sub }
    }
    pub fn whole(handle: Handle) -> Self {
        GeomRef { handle, sub: Sub::Whole }
    }
}

/// Axis of a linear dimensional constraint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DistAxis {
    #[default]
    Aligned,
    Horizontal,
    Vertical,
}

/// Constraint types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConstraintKind {
    // geometric
    Coincident,
    Collinear,
    Concentric,
    Fix,
    Parallel,
    Perpendicular,
    Horizontal,
    Vertical,
    Tangent,
    /// G2 continuity (common point, tangent and curvature) with a spline; tangency between
    /// other curves.
    Smooth,
    Symmetric,
    Equal,
    // dimensional
    Distance(DistAxis),
    /// Degrees.
    Angular,
    Radius,
    Diameter,
}

impl ConstraintKind {
    pub fn is_dimensional(self) -> bool {
        matches!(self, ConstraintKind::Distance(_) | ConstraintKind::Angular | ConstraintKind::Radius | ConstraintKind::Diameter)
    }
    /// The AutoCAD-style name (`Coincident`, `Horizontal`, `Aligned`, …).
    pub fn name(self) -> &'static str {
        match self {
            ConstraintKind::Coincident => "Coincident",
            ConstraintKind::Collinear => "Collinear",
            ConstraintKind::Concentric => "Concentric",
            ConstraintKind::Fix => "Fix",
            ConstraintKind::Parallel => "Parallel",
            ConstraintKind::Perpendicular => "Perpendicular",
            ConstraintKind::Horizontal => "Horizontal",
            ConstraintKind::Vertical => "Vertical",
            ConstraintKind::Tangent => "Tangent",
            ConstraintKind::Smooth => "Smooth",
            ConstraintKind::Symmetric => "Symmetric",
            ConstraintKind::Equal => "Equal",
            ConstraintKind::Distance(DistAxis::Aligned) => "Aligned",
            ConstraintKind::Distance(DistAxis::Horizontal) => "HorizontalDistance",
            ConstraintKind::Distance(DistAxis::Vertical) => "VerticalDistance",
            ConstraintKind::Angular => "Angular",
            ConstraintKind::Radius => "Radius",
            ConstraintKind::Diameter => "Diameter",
        }
    }
    /// Default parameter name prefix for dimensional constraints (`d`, `ang`, `rad`, `dia`).
    pub fn name_prefix(self) -> &'static str {
        match self {
            ConstraintKind::Angular => "ang",
            ConstraintKind::Radius => "rad",
            ConstraintKind::Diameter => "dia",
            _ => "d",
        }
    }
}

/// A constraint between entity parts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Constraint {
    /// Unique per drawing.
    pub id: u32,
    pub kind: ConstraintKind,
    pub refs: Vec<GeomRef>,
    /// Dimensional constraints: the parameter name (`d1`, `rad1`, …).
    #[serde(default)]
    pub name: String,
    /// Dimensional constraints: the expression (`10`, `d1*2`, `width/2+1`). Lengths are in drawing
    /// units, angles in degrees.
    #[serde(default)]
    pub expr: String,
}

/// A user parameter (Parameters Manager).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub description: String,
}

/// Constraint inference and display options (CONSTRAINTSETTINGS, CONSTRAINTBAR, DCDISPLAY).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ParametricSettings {
    /// CONSTRAINTINFER: infer constraints while drawing.
    pub infer: bool,
    /// AUTOCONSTRAIN distance tolerance (drawing units).
    pub distance_tolerance: f64,
    /// AUTOCONSTRAIN angle tolerance (degrees).
    pub angle_tolerance: f64,
    /// Constraint types AUTOCONSTRAIN may apply (names as in [`ConstraintKind::name`]).
    pub auto_types: Vec<String>,
    /// Constraint bars shown for all objects.
    pub bars_visible: bool,
    /// Objects whose bars are individually hidden (when `bars_visible`) or shown (otherwise).
    pub bar_exceptions: Vec<Handle>,
    /// Dynamic dimensional constraints shown.
    pub dims_visible: bool,
    pub dim_exceptions: Vec<Handle>,
    /// Constraint bar transparency, percent.
    pub bar_transparency: u8,
}

impl Default for ParametricSettings {
    fn default() -> Self {
        ParametricSettings {
            infer: false,
            distance_tolerance: 0.05,
            angle_tolerance: 1.0,
            auto_types: ["Coincident", "Collinear", "Parallel", "Perpendicular", "Horizontal", "Vertical", "Tangent", "Concentric", "Equal"]
                .iter()
                .map(ToString::to_string)
                .collect(),
            bars_visible: true,
            bar_exceptions: Vec::new(),
            dims_visible: true,
            dim_exceptions: Vec::new(),
            bar_transparency: 50,
        }
    }
}

/// User parameters plus parametric settings, stored on the drawing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Parametric {
    pub parameters: Vec<Parameter>,
    pub settings: ParametricSettings,
}
