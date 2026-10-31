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
    /// A formula for the dimension text (`=<>/2`, `=TEXT(<>,"0.0")&" mm"`; `<>` is the
    /// measurement); `Dimension::text` holds the last result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
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
    /// Arrowhead of the leader lines: a DIMBLK-style name (`_DotSmall`, `_Open30`, `_None`…) or a
    /// user arrowhead block of the drawing; empty for the style's (closed filled by default).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arrow: String,
    /// Leader lines are smooth curves through their points (leader type Spline). An empty
    /// `leaders` is a multileader without a leader line (leader type None).
    #[serde(default)]
    pub spline: bool,
    /// Leader type None set on a multileader that has leader lines: they are kept (to come back
    /// with Straight or Spline) but not drawn, nor the landing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_leader: bool,
    /// Block content in place of the text: a block reference placed at the landing.
    #[serde(default)]
    pub block: Option<Insert>,
}

impl MLeader {
    /// The grip index of the text (after the leader points and the landing), when it has text.
    pub fn text_grip(&self) -> Option<usize> {
        self.text.as_ref().map(|_| self.leaders.iter().map(Vec::len).sum::<usize>() + 1)
    }

    /// The gap between the end of the landing and the text, as placed now (never negative).
    fn text_gap(&self) -> f64 {
        self.text.as_ref().map_or(0.0, |t| ((t.insert.x - self.landing.x).abs() - self.dogleg).max(0.0))
    }

    /// The side of the landing the text is on: 1 to the right, -1 to the left.
    fn text_side(&self) -> f64 {
        match &self.text {
            Some(t) if t.insert.x < self.landing.x => -1.0,
            _ => 1.0,
        }
    }

    /// Move with Leader: the text goes to `to` and the landing follows it (next to the text,
    /// at its height), so the leader runs from its last point to the new landing.
    pub fn move_with_leader(&mut self, to: Vec2) {
        let gap = self.text_gap();
        let last = self.leaders.iter().filter_map(|l| l.last()).map(|v| v.xy()).next().unwrap_or(self.landing.xy());
        let side = if to.x >= last.x { 1.0 } else { -1.0 };
        let old = self.landing.xy();
        self.landing.x = to.x - side * (self.dogleg + gap);
        self.landing.y = to.y;
        if let Some(t) = &mut self.text {
            t.insert.x = to.x;
            t.insert.y = to.y;
            // Attached at its left edge on the right of the landing, its right edge on the left.
            let row = (t.attach.clamp(1, 9) - 1) / 3;
            t.attach = row * 3 + if side > 0.0 { 1 } else { 3 };
        }
        if let Some(b) = &mut self.block {
            let d = self.landing.xy() - old;
            b.insert.x += d.x;
            b.insert.y += d.y;
        }
    }

    /// Reset Text Position: the text back beside the landing, on the side it is on.
    pub fn reset_text(&mut self) {
        let side = self.text_side();
        let (lx, ly) = (self.landing.x, self.landing.y);
        let dogleg = self.dogleg;
        if let Some(t) = &mut self.text {
            // The usual landing gap: half the text height.
            let gap = t.height.abs() * 0.5;
            t.insert.x = lx + side * (dogleg + gap);
            t.insert.y = ly;
            let row = (t.attach.clamp(1, 9) - 1) / 3;
            t.attach = row * 3 + if side > 0.0 { 1 } else { 3 };
        }
    }

    /// It draws no leader line (leader type None).
    pub fn leaderless(&self) -> bool {
        self.no_leader || self.leaders.is_empty()
    }

    /// The arrowhead it draws: its own, else its style's (in `d`).
    pub fn arrow_in<'a>(&'a self, d: &'a crate::Drawing) -> &'a str {
        if self.arrow.trim().is_empty() {
            d.mleader_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&self.style)).map_or("", |s| s.arrow_block.as_str())
        } else {
            &self.arrow
        }
    }

    /// Each leader line as drawn: through its points to the landing, smoothed for spline leaders.
    pub fn leader_paths(&self) -> Vec<Vec<Vec2>> {
        if self.no_leader {
            return Vec::new();
        }
        self.leaders
            .iter()
            .map(|l| {
                let pts: Vec<Vec2> = l.iter().map(|v| v.xy()).chain(std::iter::once(self.landing.xy())).collect();
                if self.spline && pts.len() > 2 { GSpline::from_fit_points(&pts).tessellate(1e-3) } else { pts }
            })
            .collect()
    }
}
