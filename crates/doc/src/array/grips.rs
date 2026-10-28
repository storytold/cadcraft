//! The grips of an associative array, as a selected array shows them: arrows that change the
//! number of items or the distance between them by dragging (or typing a value), besides the
//! ordinary base-point grip of its block reference.
//!
//! Rectangular arrays have a column count arrow at the last column, a column spacing arrow at
//! the second column, the same two for rows, and a square at the far corner that changes both
//! counts at once. Polar arrays have an "angle between items" arrow at the second item, an item
//! count arrow at the last item of an array that doesn't fill a whole circle, and the row count
//! and spacing arrows outwards. A grip is only shown where it has a direction: a count of one, or
//! a spacing of zero, has none (ARRAYEDIT changes those).
//!
//! Positions here are in the array block's coordinates (see [`super`]); [`Drawing::array_grips`]
//! places them in the drawing.

use cadcraft_geom::{Vec2, norm_angle};
use serde::Serialize;

use super::{ArrayKind, ArrayParams, MAX_ARRAY_ITEMS};
use crate::{Drawing, EntityKind, Handle};

/// What dragging a grip changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArrayGripKind {
    /// Rectangular: the number of columns (an arrow at the last column).
    ColumnCount,
    /// Rectangular: the distance between columns (an arrow at the second column).
    ColumnSpacing,
    /// The number of rows (an arrow at the last row; polar rows are rings further out).
    RowCount,
    /// The distance between rows (an arrow at the second row).
    RowSpacing,
    /// Rectangular: the numbers of columns and rows at once (a square at the far corner).
    RowColumnCount,
    /// Polar, not a whole circle: the number of items, keeping the angle between them (an arrow
    /// at the last item).
    ItemCount,
    /// Polar: the angle between items (an arrow at the second item). A whole circle stays whole:
    /// the angle sets the number of items instead.
    ItemAngle,
}

impl ArrayGripKind {
    pub const ALL: [ArrayGripKind; 7] = [
        ArrayGripKind::ColumnCount,
        ArrayGripKind::ColumnSpacing,
        ArrayGripKind::RowCount,
        ArrayGripKind::RowSpacing,
        ArrayGripKind::RowColumnCount,
        ArrayGripKind::ItemCount,
        ArrayGripKind::ItemAngle,
    ];

    /// The name the JSON form uses.
    pub fn name(self) -> &'static str {
        match self {
            ArrayGripKind::ColumnCount => "columnCount",
            ArrayGripKind::ColumnSpacing => "columnSpacing",
            ArrayGripKind::RowCount => "rowCount",
            ArrayGripKind::RowSpacing => "rowSpacing",
            ArrayGripKind::RowColumnCount => "rowColumnCount",
            ArrayGripKind::ItemCount => "itemCount",
            ArrayGripKind::ItemAngle => "itemAngle",
        }
    }

    /// The grip named `s` (case-insensitive).
    pub fn from_name(s: &str) -> Option<ArrayGripKind> {
        let s = s.trim();
        Self::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(s))
    }

    /// The grip's name as the grip edit announces it.
    pub fn label(self) -> &'static str {
        match self {
            ArrayGripKind::ColumnCount => "Column Count",
            ArrayGripKind::ColumnSpacing => "Column Spacing",
            ArrayGripKind::RowCount => "Row Count",
            ArrayGripKind::RowSpacing => "Row Spacing",
            ArrayGripKind::RowColumnCount => "Row and Column Count",
            ArrayGripKind::ItemCount => "Item Count",
            ArrayGripKind::ItemAngle => "Angle Between Items",
        }
    }

    /// Whether the grip sets a number of items (a typed value is a count).
    pub fn is_count(self) -> bool {
        matches!(self, ArrayGripKind::ColumnCount | ArrayGripKind::RowCount | ArrayGripKind::RowColumnCount | ArrayGripKind::ItemCount)
    }

    /// Whether a typed value can set the grip (the corner changes two counts: only a point).
    pub fn takes_value(self) -> bool {
        self != ArrayGripKind::RowColumnCount
    }
}

/// One grip of an associative array.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrayGrip {
    #[serde(rename = "grip")]
    pub kind: ArrayGripKind,
    /// Where it is (the item it sits on).
    #[serde(serialize_with = "point")]
    pub at: Vec2,
    /// The way the arrow points: where the count or the distance grows (a unit vector; zero for
    /// the corner square).
    #[serde(serialize_with = "point")]
    pub dir: Vec2,
}

fn point<S: serde::Serializer>(p: &Vec2, s: S) -> Result<S::Ok, S::Error> {
    [p.x, p.y].serialize(s)
}

/// What a grip edit is given: the point it was dragged to (array block coordinates) or a typed
/// value (a count, a distance, or an angle in degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArrayGripInput {
    Point(Vec2),
    Value(f64),
}

/// A count `t / spacing + 1` rounded, at least 1 and at most `max`; `None` when not a number.
fn count_from(t: f64, spacing: f64, max: u32) -> Option<u32> {
    let n = (t / spacing).round() + 1.0;
    if !n.is_finite() {
        return None;
    }
    Some(n.clamp(1.0, f64::from(max.max(1))) as u32)
}

/// A typed count: a whole number from 1 to `max`.
fn typed_count(v: f64, max: u32) -> Option<u32> {
    (v.is_finite() && v.fract() == 0.0 && v >= 1.0 && v <= f64::from(max)).then_some(v as u32)
}

/// A spacing: nonzero and finite.
fn spacing(v: f64) -> Option<f64> {
    (v.is_finite() && v.abs() > 1e-9).then_some(v)
}

/// Whether a polar fill (degrees) is a whole circle.
fn full_circle(fill: f64) -> bool {
    (fill.abs() - 360.0).abs() < 1e-9
}

impl ArrayParams {
    /// The grips of the array in its block's coordinates (see the module documentation).
    pub fn grips(&self) -> Vec<ArrayGrip> {
        let mut out = Vec::new();
        if !self.is_valid() {
            return out;
        }
        let mut push = |kind, at: Vec2, dir: Vec2| {
            if at.is_finite() && dir.is_finite() {
                out.push(ArrayGrip { kind, at, dir });
            }
        };
        match &self.kind {
            ArrayKind::Rect(r) => {
                let (ux, uy) = (Vec2::from_angle(r.axis_angle), Vec2::from_angle(r.axis_angle).perp());
                let at = |i: u32, j: u32| ux * (r.col_spacing * f64::from(i)) + uy * (r.row_spacing * f64::from(j));
                let cols = r.cols >= 2 && r.col_spacing.abs() > 1e-9;
                let rows = r.rows >= 2 && r.row_spacing.abs() > 1e-9;
                if cols {
                    let dir = ux * r.col_spacing.signum();
                    push(ArrayGripKind::ColumnSpacing, at(1, 0), dir);
                    push(ArrayGripKind::ColumnCount, at(r.cols - 1, 0), dir);
                }
                if rows {
                    let dir = uy * r.row_spacing.signum();
                    push(ArrayGripKind::RowSpacing, at(0, 1), dir);
                    push(ArrayGripKind::RowCount, at(0, r.rows - 1), dir);
                }
                if cols && rows {
                    push(ArrayGripKind::RowColumnCount, at(r.cols - 1, r.rows - 1), Vec2::ZERO);
                }
            }
            ArrayKind::Polar(p) => {
                let total = p.fill.to_radians();
                let full = full_circle(p.fill);
                let step = if full { total / f64::from(p.items.max(1)) } else { total / f64::from(p.items.saturating_sub(1).max(1)) };
                let tangent = |at: Vec2| (at - p.center).normalized().perp() * total.signum();
                if p.items >= 2 {
                    let second = Vec2::ZERO.rotate_about(p.center, step);
                    push(ArrayGripKind::ItemAngle, second, tangent(second));
                    if !full {
                        let last = Vec2::ZERO.rotate_about(p.center, step * f64::from(p.items - 1));
                        push(ArrayGripKind::ItemCount, last, tangent(last));
                    }
                }
                if p.rows >= 2 && p.row_spacing.abs() > 1e-9 {
                    let out_dir = polar_out(p.center);
                    let dir = out_dir * p.row_spacing.signum();
                    push(ArrayGripKind::RowSpacing, out_dir * p.row_spacing, dir);
                    push(ArrayGripKind::RowCount, out_dir * (p.row_spacing * f64::from(p.rows - 1)), dir);
                }
            }
        }
        out
    }

    /// The parameters after grip `kind` is dragged to a point or given a value; counts are kept
    /// to at most `max_count` (and [`MAX_ARRAY_ITEMS`]). `None` when the array has no such grip or
    /// the input doesn't make a valid array (a zero distance, a fill past 360 degrees, a count
    /// typed out of range).
    pub fn with_grip(&self, kind: ArrayGripKind, input: ArrayGripInput, max_count: u32) -> Option<ArrayParams> {
        if !self.grips().iter().any(|g| g.kind == kind) {
            return None;
        }
        if let ArrayGripInput::Point(c) = input
            && !c.is_finite()
        {
            return None;
        }
        if let ArrayGripInput::Value(v) = input
            && (!kind.takes_value() || !v.is_finite())
        {
            return None;
        }
        let max = max_count.clamp(1, u32::try_from(MAX_ARRAY_ITEMS).unwrap_or(u32::MAX));
        let mut a = self.clone();
        match &mut a.kind {
            ArrayKind::Rect(r) => {
                let (ux, uy) = (Vec2::from_angle(r.axis_angle), Vec2::from_angle(r.axis_angle).perp());
                match (kind, input) {
                    (ArrayGripKind::ColumnCount, ArrayGripInput::Point(c)) => r.cols = count_from(c.dot(ux), r.col_spacing, max)?,
                    (ArrayGripKind::ColumnCount, ArrayGripInput::Value(v)) => r.cols = typed_count(v, max)?,
                    (ArrayGripKind::RowCount, ArrayGripInput::Point(c)) => r.rows = count_from(c.dot(uy), r.row_spacing, max)?,
                    (ArrayGripKind::RowCount, ArrayGripInput::Value(v)) => r.rows = typed_count(v, max)?,
                    (ArrayGripKind::ColumnSpacing, ArrayGripInput::Point(c)) => r.col_spacing = spacing(c.dot(ux))?,
                    (ArrayGripKind::ColumnSpacing, ArrayGripInput::Value(v)) => r.col_spacing = spacing(v)?,
                    (ArrayGripKind::RowSpacing, ArrayGripInput::Point(c)) => r.row_spacing = spacing(c.dot(uy))?,
                    (ArrayGripKind::RowSpacing, ArrayGripInput::Value(v)) => r.row_spacing = spacing(v)?,
                    (ArrayGripKind::RowColumnCount, ArrayGripInput::Point(c)) => {
                        r.cols = count_from(c.dot(ux), r.col_spacing, max)?;
                        r.rows = count_from(c.dot(uy), r.row_spacing, max)?;
                    }
                    _ => return None,
                }
            }
            ArrayKind::Polar(p) => {
                let full = full_circle(p.fill);
                let n = p.items.max(1);
                // The angle from the first item to `c` around the centre, in the fill's direction
                // (counterclockwise in [0, 360), clockwise in (-360, 0]), degrees.
                let swept = |c: Vec2| {
                    let a = norm_angle((c - p.center).angle() - (Vec2::ZERO - p.center).angle()).to_degrees();
                    if p.fill < 0.0 && a > 0.0 { a - 360.0 } else { a }
                };
                match (kind, input) {
                    (ArrayGripKind::ItemAngle, _) => {
                        let between = match input {
                            ArrayGripInput::Point(c) => swept(c),
                            ArrayGripInput::Value(v) => v,
                        };
                        if !between.is_finite() || between.abs() < 1e-9 {
                            return None;
                        }
                        if full {
                            let items = (360.0 / between.abs()).round();
                            if !items.is_finite() {
                                return None;
                            }
                            p.items = items.clamp(1.0, f64::from(max)) as u32;
                        } else {
                            let fill = between * f64::from(n - 1);
                            if fill.abs() > 360.0 + 1e-9 {
                                return None;
                            }
                            p.fill = fill.clamp(-360.0, 360.0);
                        }
                    }
                    (ArrayGripKind::ItemCount, _) => {
                        let step = p.fill / f64::from(n.saturating_sub(1).max(1));
                        if step.abs() < 1e-9 {
                            return None;
                        }
                        // The items stay short of a whole circle, as the array's angle between
                        // them can't be kept round one.
                        let most = (((360.0 - 1e-6) / step.abs()).floor() + 1.0).min(f64::from(max));
                        let items = match input {
                            ArrayGripInput::Point(c) => (swept(c) / step).round() + 1.0,
                            ArrayGripInput::Value(v) => {
                                if v.fract() != 0.0 || v < 1.0 || v > most {
                                    return None;
                                }
                                v
                            }
                        };
                        if !items.is_finite() {
                            return None;
                        }
                        let items = items.clamp(1.0, most.max(1.0)) as u32;
                        p.items = items;
                        p.fill = step * f64::from(items.saturating_sub(1).max(1));
                    }
                    (ArrayGripKind::RowCount, ArrayGripInput::Point(c)) => p.rows = count_from(c.dot(polar_out(p.center)), p.row_spacing, max)?,
                    (ArrayGripKind::RowCount, ArrayGripInput::Value(v)) => p.rows = typed_count(v, max)?,
                    (ArrayGripKind::RowSpacing, ArrayGripInput::Point(c)) => p.row_spacing = spacing(c.dot(polar_out(p.center)))?,
                    (ArrayGripKind::RowSpacing, ArrayGripInput::Value(v)) => p.row_spacing = spacing(v)?,
                    _ => return None,
                }
            }
        }
        (a.is_valid() && a.reach_is_finite()).then_some(a)
    }

    /// Whether the furthest item is at a finite distance (a dragged spacing times the count
    /// can overflow).
    fn reach_is_finite(&self) -> bool {
        let far = match &self.kind {
            ArrayKind::Rect(r) => [r.col_spacing * f64::from(r.cols), r.row_spacing * f64::from(r.rows), r.row_elevation * f64::from(r.rows)],
            ArrayKind::Polar(p) => [p.row_spacing * f64::from(p.rows), p.row_elevation * f64::from(p.rows), p.center.len()],
        };
        far.iter().chain(std::iter::once(&(self.level_spacing * f64::from(self.levels)))).all(|v| v.is_finite())
    }
}

/// The way a polar array's rows go: from the centre through the base point.
fn polar_out(center: Vec2) -> Vec2 {
    let d = (Vec2::ZERO - center).normalized();
    if d == Vec2::ZERO { Vec2::X } else { d }
}

impl Drawing {
    /// The grips of associative array `h` in the drawing (none when it is not one).
    pub fn array_grips(&self, h: Handle) -> Vec<ArrayGrip> {
        let Some(e) = self.entity(h) else { return Vec::new() };
        let EntityKind::Insert(ins) = &e.kind else { return Vec::new() };
        let Some(p) = self.array_params(e) else { return Vec::new() };
        let Some(blk) = self.block(&ins.block) else { return Vec::new() };
        let w = ins.transform(blk.base.xy());
        p.grips()
            .into_iter()
            .filter_map(|g| {
                let at = w.apply(g.at);
                let ahead = w.apply(g.at + g.dir) - at;
                let dir = if g.dir == Vec2::ZERO || ahead.len() <= 1e-12 { Vec2::ZERO } else { ahead * (1.0 / ahead.len()) };
                (at.is_finite() && dir.is_finite()).then_some(ArrayGrip { kind: g.kind, at, dir })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PolarArray, RectArray};

    fn rect(cols: u32, rows: u32) -> ArrayParams {
        ArrayParams {
            source: "*U1".into(),
            kind: ArrayKind::Rect(RectArray { cols, rows, col_spacing: 2.0, row_spacing: 3.0, axis_angle: 0.0, row_elevation: 0.0 }),
            levels: 1,
            level_spacing: 0.0,
            replaced: Vec::new(),
            editing: None,
        }
    }

    fn polar(items: u32, fill: f64, rows: u32) -> ArrayParams {
        ArrayParams {
            source: "*U1".into(),
            kind: ArrayKind::Polar(PolarArray {
                center: Vec2::new(-10.0, 0.0),
                items,
                fill,
                rotate: true,
                rows,
                row_spacing: 2.0,
                row_elevation: 0.0,
            }),
            levels: 1,
            level_spacing: 0.0,
            replaced: Vec::new(),
            editing: None,
        }
    }

    fn grip(p: &ArrayParams, k: ArrayGripKind) -> Option<ArrayGrip> {
        p.grips().into_iter().find(|g| g.kind == k)
    }

    #[test]
    fn rect_grips_sit_on_their_items() {
        let p = rect(4, 3);
        assert_eq!(grip(&p, ArrayGripKind::ColumnCount).map(|g| (g.at, g.dir)), Some((Vec2::new(6.0, 0.0), Vec2::X)));
        assert_eq!(grip(&p, ArrayGripKind::ColumnSpacing).map(|g| g.at), Some(Vec2::new(2.0, 0.0)));
        assert_eq!(grip(&p, ArrayGripKind::RowCount).map(|g| g.at), Some(Vec2::new(0.0, 6.0)));
        assert_eq!(grip(&p, ArrayGripKind::RowSpacing).map(|g| g.at), Some(Vec2::new(0.0, 3.0)));
        assert_eq!(grip(&p, ArrayGripKind::RowColumnCount).map(|g| g.at), Some(Vec2::new(6.0, 6.0)));
        // One row: no row grips and no corner.
        let p = rect(4, 1);
        assert!(grip(&p, ArrayGripKind::RowCount).is_none() && grip(&p, ArrayGripKind::RowColumnCount).is_none());
    }

    #[test]
    fn dragging_rect_grips_changes_counts_and_spacing() {
        let p = rect(4, 3);
        let cols = |p: &ArrayParams| match &p.kind {
            ArrayKind::Rect(r) => (r.cols, r.rows, r.col_spacing, r.row_spacing),
            ArrayKind::Polar(_) => (0, 0, 0.0, 0.0),
        };
        // To x = 10.9 (5.45 spacings): 6 columns.
        let q = p.with_grip(ArrayGripKind::ColumnCount, ArrayGripInput::Point(Vec2::new(10.9, 4.0)), 1000);
        assert_eq!(q.as_ref().map(cols), Some((6, 3, 2.0, 3.0)));
        // Back past the base: one column.
        let q = p.with_grip(ArrayGripKind::ColumnCount, ArrayGripInput::Point(Vec2::new(-5.0, 0.0)), 1000);
        assert_eq!(q.as_ref().map(cols), Some((1, 3, 2.0, 3.0)));
        // Capped.
        let q = p.with_grip(ArrayGripKind::ColumnCount, ArrayGripInput::Point(Vec2::new(1e6, 0.0)), 50);
        assert_eq!(q.as_ref().map(cols), Some((50, 3, 2.0, 3.0)));
        let q = p.with_grip(ArrayGripKind::RowSpacing, ArrayGripInput::Point(Vec2::new(1.0, 5.0)), 1000);
        assert_eq!(q.as_ref().map(cols), Some((4, 3, 2.0, 5.0)));
        let q = p.with_grip(ArrayGripKind::RowColumnCount, ArrayGripInput::Point(Vec2::new(2.2, 9.1)), 1000);
        assert_eq!(q.as_ref().map(cols), Some((2, 4, 2.0, 3.0)));
        let q = p.with_grip(ArrayGripKind::ColumnSpacing, ArrayGripInput::Value(-1.5), 1000);
        assert_eq!(q.as_ref().map(cols), Some((4, 3, -1.5, 3.0)));
        assert!(p.with_grip(ArrayGripKind::ColumnSpacing, ArrayGripInput::Value(0.0), 1000).is_none());
        assert!(p.with_grip(ArrayGripKind::RowColumnCount, ArrayGripInput::Value(3.0), 1000).is_none());
        assert!(p.with_grip(ArrayGripKind::ItemCount, ArrayGripInput::Value(3.0), 1000).is_none());
    }

    #[test]
    fn rotated_rect_grips_follow_the_axis() {
        let mut p = rect(3, 2);
        if let ArrayKind::Rect(r) = &mut p.kind {
            r.axis_angle = std::f64::consts::FRAC_PI_2;
        }
        let g = grip(&p, ArrayGripKind::ColumnCount);
        assert!(g.is_some_and(|g| g.at.near(Vec2::new(0.0, 4.0), 1e-9) && g.dir.near(Vec2::Y, 1e-9)));
        let q = p.with_grip(ArrayGripKind::ColumnCount, ArrayGripInput::Point(Vec2::new(0.3, 8.0)), 100);
        assert!(matches!(q.map(|q| q.kind), Some(ArrayKind::Rect(r)) if r.cols == 5));
    }

    #[test]
    fn polar_grips_change_angle_and_count() {
        let p = polar(5, 180.0, 1);
        let fill_items = |p: &ArrayParams| match &p.kind {
            ArrayKind::Polar(k) => (k.items, k.fill),
            ArrayKind::Rect(_) => (0, 0.0),
        };
        // 45 degrees between items; the second item is a quarter turn short of the base... at 45°.
        let g = grip(&p, ArrayGripKind::ItemAngle);
        let at45 = Vec2::ZERO.rotate_about(Vec2::new(-10.0, 0.0), 45f64.to_radians());
        assert!(g.is_some_and(|g| g.at.near(at45, 1e-9)));
        // Dragging it to 30 degrees: 4 gaps of 30 = 120.
        let to30 = Vec2::ZERO.rotate_about(Vec2::new(-10.0, 0.0), 30f64.to_radians());
        let q = p.with_grip(ArrayGripKind::ItemAngle, ArrayGripInput::Point(to30), 100);
        assert!(q.as_ref().map(fill_items).is_some_and(|(n, f)| n == 5 && (f - 120.0).abs() < 1e-9));
        // Too wide: 4 gaps of 100 degrees pass a whole circle.
        assert!(p.with_grip(ArrayGripKind::ItemAngle, ArrayGripInput::Value(100.0), 100).is_none());
        // The last item to 270 degrees keeps 45 between: 7 items.
        let to270 = Vec2::ZERO.rotate_about(Vec2::new(-10.0, 0.0), 270f64.to_radians());
        let q = p.with_grip(ArrayGripKind::ItemCount, ArrayGripInput::Point(to270), 100);
        assert!(q.as_ref().map(fill_items).is_some_and(|(n, f)| n == 7 && (f - 270.0).abs() < 1e-9));
        // Typed: 8 items is 315 degrees; 9 would close the circle and is refused.
        let q = p.with_grip(ArrayGripKind::ItemCount, ArrayGripInput::Value(8.0), 100);
        assert!(q.as_ref().map(fill_items).is_some_and(|(n, f)| n == 8 && (f - 315.0).abs() < 1e-9));
        assert!(p.with_grip(ArrayGripKind::ItemCount, ArrayGripInput::Value(9.0), 100).is_none());
    }

    #[test]
    fn a_whole_circle_stays_whole() {
        let p = polar(6, 360.0, 1);
        assert!(grip(&p, ArrayGripKind::ItemCount).is_none());
        let q = p.with_grip(ArrayGripKind::ItemAngle, ArrayGripInput::Value(30.0), 100);
        assert!(matches!(q.map(|q| q.kind), Some(ArrayKind::Polar(k)) if k.items == 12 && k.fill == 360.0));
        // Clockwise fills measure clockwise.
        let p = polar(4, -180.0, 1);
        let to_minus90 = Vec2::ZERO.rotate_about(Vec2::new(-10.0, 0.0), -90f64.to_radians());
        let q = p.with_grip(ArrayGripKind::ItemAngle, ArrayGripInput::Point(to_minus90), 100);
        assert!(matches!(q.map(|q| q.kind), Some(ArrayKind::Polar(k)) if (k.fill + 270.0).abs() < 1e-9));
    }

    #[test]
    fn polar_rows_go_outwards() {
        let p = polar(4, 360.0, 3);
        let g = grip(&p, ArrayGripKind::RowCount);
        assert!(g.is_some_and(|g| g.at.near(Vec2::new(4.0, 0.0), 1e-9) && g.dir.near(Vec2::X, 1e-9)));
        let q = p.with_grip(ArrayGripKind::RowCount, ArrayGripInput::Point(Vec2::new(9.0, 1.0)), 100);
        assert!(matches!(q.map(|q| q.kind), Some(ArrayKind::Polar(k)) if k.rows == 6));
    }

    #[test]
    fn hostile_grip_inputs_are_refused() {
        let p = rect(4, 3);
        for v in [f64::NAN, f64::INFINITY, -1.0, 0.0, 2.5, 1e300] {
            assert!(p.with_grip(ArrayGripKind::ColumnCount, ArrayGripInput::Value(v), 1000).is_none(), "{v}");
        }
        assert!(p.with_grip(ArrayGripKind::ColumnCount, ArrayGripInput::Point(Vec2::new(f64::NAN, 0.0)), 1000).is_none());
        // A spacing whose rows would end past the largest number is refused.
        assert!(p.with_grip(ArrayGripKind::RowSpacing, ArrayGripInput::Point(Vec2::new(f64::MAX, f64::MAX)), 1000).is_none());
        // A huge column count stays within MAX_ARRAY_ITEMS whatever the cap says.
        let q = p.with_grip(ArrayGripKind::ColumnCount, ArrayGripInput::Point(Vec2::new(1e300, 0.0)), u32::MAX);
        assert!(q.is_none_or(|q| q.item_count() <= MAX_ARRAY_ITEMS));
        // Invalid parameters have no grips.
        let mut bad = rect(4, 3);
        bad.level_spacing = f64::NAN;
        assert!(bad.grips().is_empty() && bad.with_grip(ArrayGripKind::ColumnCount, ArrayGripInput::Value(2.0), 10).is_none());
        let p = polar(3, 1e-12, 1);
        let _ = p.with_grip(ArrayGripKind::ItemCount, ArrayGripInput::Point(Vec2::new(1.0, 1.0)), 100);
        let _ = p.with_grip(ArrayGripKind::ItemAngle, ArrayGripInput::Value(1e-300), 100);
    }
}
