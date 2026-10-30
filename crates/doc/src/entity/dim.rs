use super::block::Insert;
use super::common::Handle;
use super::text::MText;
use super::{Deserialize, GSpline, Serialize, Vec2, Vec3, one, standard, yes};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DimKind {
    /// Rotated linear dimension (horizontal = 0, vertical = 90°).
    Linear {
        rotation: f64,
    },
    Aligned,
    Angular,
    Angular3P,
    Diameter,
    Radius,
    Ordinate {
        x_type: bool,
    },
    ArcLength,
}

/// A dimension. Definition points follow DXF: `defpt` (10: dimension line point),
/// `p13`/`p14` (extension origins), `p15`/`p16` (arc/angle points), `text_mid` (11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimension {
    pub kind: DimKind,
    pub defpt: Vec3,
    pub text_mid: Vec3,
    #[serde(default)]
    pub p13: Vec3,
    #[serde(default)]
    pub p14: Vec3,
    #[serde(default)]
    pub p15: Vec3,
    #[serde(default)]
    pub p16: Vec3,
    /// "" = measured value, "<>" inside = measurement placeholder, " " = suppressed.
    #[serde(default)]
    pub text: String,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default)]
    pub measurement: f64,
    #[serde(default)]
    pub text_rotation: f64,
    /// User moved the text (text_mid is authoritative).
    #[serde(default)]
    pub user_text_pos: bool,
    /// Anonymous block with the rendered geometry (from files); regenerated when edited.
    #[serde(default)]
    pub block: Option<String>,
    /// Per-dimension style overrides (DIMOVERRIDE): DimStyle field names (camelCase) → values,
    /// merged over the named style.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub overrides: serde_json::Map<String, serde_json::Value>,
    /// Associativity: which definition points follow which objects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assoc: Vec<DimAssoc>,
    /// Oblique extension lines, jogs, breaks and inspection frames.
    #[serde(default, skip_serializing_if = "DimMarks::is_empty")]
    pub marks: DimMarks,
}

/// What dimension edits add to a dimension's drawing: DIMEDIT Oblique, DIMJOGLINE, DIMJOGGED,
/// DIMBREAK and DIMINSPECT.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DimMarks {
    /// Angle (radians) of the extension lines of a linear or aligned dimension; None =
    /// perpendicular to the dimension line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oblique: Option<f64>,
    /// A jog on the dimension line of a linear or aligned dimension, at this point (DIMJOGLINE).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jog_line: Option<Vec3>,
    /// A jogged radius dimension (DIMJOGGED).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jogged: Option<JoggedRadius>,
    /// Gaps in the dimension and extension lines (DIMBREAK).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<DimBreak>,
    /// Inspection frame, label and rate (DIMINSPECT).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspect: Option<Inspection>,
}

impl DimMarks {
    pub fn is_empty(&self) -> bool {
        *self == DimMarks::default()
    }

    /// Transform the points and angles (part of [`super::EntityKind::transform`]).
    pub(super) fn transform(&mut self, t3: &dyn Fn(&mut Vec3), rot: f64, s: f64, mirror: bool) {
        if let Some(a) = &mut self.oblique {
            *a = cadcraft_geom::norm_angle(if mirror { rot - (*a - rot) } else { *a + rot });
        }
        if let Some(j) = &mut self.jog_line {
            t3(j);
        }
        if let Some(j) = &mut self.jogged {
            t3(&mut j.center);
            t3(&mut j.jog);
        }
        for b in &mut self.breaks {
            t3(&mut b.at);
            b.size *= s;
        }
    }
}

/// The definition of a jogged radius dimension: the dimension line starts at an overriding
/// centre and jogs on its way to the arc.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct JoggedRadius {
    /// Where the dimension line starts instead of the true centre.
    pub center: Vec3,
    /// The middle of the jog.
    pub jog: Vec3,
}

/// A gap in a dimension's lines: everything within `size / 2` of `at` is left out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DimBreak {
    pub at: Vec3,
    pub size: f64,
    /// Made by DIMBREAK Auto (recomputed by the next Auto) rather than picked by hand.
    pub auto: bool,
}

/// An inspection dimension (DIMINSPECT).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Inspection {
    /// Frame shape: 0 round ends, 1 angular ends, 2 none.
    pub shape: u8,
    /// Text left of the value ("" = none).
    pub label: String,
    /// Inspection rate right of the value ("" = none).
    pub rate: String,
}

/// A dimension definition point that follows an object (associative dimensions).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimAssoc {
    /// Definition point: "defpt", "p13", "p14", "p15" or "p16".
    pub point: String,
    /// The object it is attached to.
    pub handle: Handle,
    pub snap: AssocSnap,
}

/// Where on the object an associative definition point sits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssocSnap {
    /// Start point of a line/arc (or polyline vertex 0).
    Start,
    /// End point of a line/arc.
    End,
    Mid,
    /// Centre of a circle/arc.
    Center,
    /// On a circle/arc at a fixed angle (radians) from its centre.
    OnCircle {
        angle: f64,
    },
    /// Intersection of this (line) object with another line.
    Intersection {
        other: Handle,
    },
    /// Vertex `index` of a polyline.
    Vertex {
        index: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leader {
    pub vertices: Vec<Vec3>,
    #[serde(default = "yes")]
    pub arrow: bool,
    #[serde(default)]
    pub spline: bool,
    #[serde(default = "standard")]
    pub style: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MLeader {
    pub leaders: Vec<Vec<Vec3>>,
    pub landing: Vec3,
    pub dogleg: f64,
    pub text: Option<MText>,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default = "one")]
    pub arrow_size: f64,
    /// Leader lines are smooth curves through their points (leader type Spline). An empty
    /// `leaders` is a multileader without a leader line (leader type None).
    #[serde(default)]
    pub spline: bool,
    /// Block content in place of the text: a block reference placed at the landing.
    #[serde(default)]
    pub block: Option<Insert>,
}

impl MLeader {
    /// Each leader line as drawn: through its points to the landing, smoothed for spline leaders.
    pub fn leader_paths(&self) -> Vec<Vec<Vec2>> {
        self.leaders
            .iter()
            .map(|l| {
                let pts: Vec<Vec2> = l.iter().map(|v| v.xy()).chain(std::iter::once(self.landing.xy())).collect();
                if self.spline && pts.len() > 2 { GSpline::from_fit_points(&pts).tessellate(1e-3) } else { pts }
            })
            .collect()
    }
}
