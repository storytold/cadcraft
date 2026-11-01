//! Associative arrays: the parameters an array keeps, and where they place its items.
//!
//! An associative array is a block reference to an anonymous block (`*U…`, the *array block*)
//! that holds one block reference per item. Each item refers to the array's *source block*
//! (another anonymous block holding the source objects) or, for items replaced with ARRAYEDIT,
//! to another block. Other programs see an ordinary block reference; CADCraft keeps the
//! parameters below on the array block ([`Block::array`](crate::Block)) so the items can be
//! regenerated when the array is edited.
//!
//! All positions are in the array block's coordinates: its origin is the array's base point,
//! which is where the array's block reference is inserted.

use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{Common, Drawing, Entity, EntityKind, Insert, Lineweight};

/// Most items one array may have (items × rows × levels), whatever MAXARRAY says.
pub const MAX_ARRAY_ITEMS: u64 = 10_000_000;

fn one() -> u32 {
    1
}

/// The parameters of an associative array.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrayParams {
    /// The source block the items refer to.
    pub source: String,
    pub kind: ArrayKind,
    /// Levels: copies of the whole array along Z.
    #[serde(default = "one")]
    pub levels: u32,
    #[serde(default)]
    pub level_spacing: f64,
    /// Items that refer to another block than the source (ARRAYEDIT REPlace).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaced: Vec<ReplacedItem>,
    /// The source objects are out for editing (ARRAYEDIT Source, until ARRAYCLOSE). Not saved:
    /// a drawing saved meanwhile keeps the objects as separate objects.
    #[serde(skip)]
    pub editing: Option<SourceEdit>,
}

/// ARRAYEDIT Source in progress: the array's source objects placed in the drawing for editing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceEdit {
    /// The array (block reference) being edited.
    pub array: crate::Handle,
    /// The item the objects were placed at: (i, j, level).
    pub item: (u32, u32, u32),
    /// The objects placed for editing.
    pub objects: Vec<crate::Handle>,
    /// Objects with this handle or above were drawn while editing; they join the source too.
    pub first_new: u64,
}

/// Rectangular or polar placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ArrayKind {
    Rect(RectArray),
    Polar(PolarArray),
}

/// A rectangular array: columns along the axis angle, rows across it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectArray {
    pub cols: u32,
    pub rows: u32,
    pub col_spacing: f64,
    pub row_spacing: f64,
    /// Direction of the rows (the column axis), radians.
    #[serde(default)]
    pub axis_angle: f64,
    /// Elevation added per row.
    #[serde(default)]
    pub row_elevation: f64,
}

/// A polar array: items around a centre, rows as rings further out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolarArray {
    pub center: Vec2,
    pub items: u32,
    /// Angle to fill, degrees (positive = counterclockwise, ±360 = a full circle).
    pub fill: f64,
    /// Items are rotated as they go around (else they keep their orientation).
    pub rotate: bool,
    #[serde(default = "one")]
    pub rows: u32,
    /// Distance between rings, outwards from the centre.
    #[serde(default)]
    pub row_spacing: f64,
    #[serde(default)]
    pub row_elevation: f64,
}

/// An item that refers to another block. `i` is the column (rectangular) or the item around the
/// circle (polar), `j` the row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedItem {
    pub i: u32,
    pub j: u32,
    #[serde(default)]
    pub level: u32,
    pub block: String,
}

/// Where one item goes: its block reference's insertion point (the image of the array's base
/// point) and rotation (radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrayItem {
    pub i: u32,
    pub j: u32,
    pub level: u32,
    pub insert: Vec3,
    pub rotation: f64,
}

impl ArrayParams {
    /// Items along the first direction (columns or items around) and rows.
    pub fn grid(&self) -> (u32, u32) {
        match &self.kind {
            ArrayKind::Rect(r) => (r.cols, r.rows),
            ArrayKind::Polar(p) => (p.items, p.rows),
        }
    }

    /// The number of items, saturating.
    pub fn item_count(&self) -> u64 {
        let (a, b) = self.grid();
        u64::from(a).saturating_mul(u64::from(b)).saturating_mul(u64::from(self.levels))
    }

    /// Whether the parameters are usable: at least one item in every direction, at most
    /// [`MAX_ARRAY_ITEMS`] in all, and finite numbers. Parameters read from files are checked
    /// with this before they are kept.
    pub fn is_valid(&self) -> bool {
        let (a, b) = self.grid();
        let nums = match &self.kind {
            ArrayKind::Rect(r) => [r.col_spacing, r.row_spacing, r.axis_angle, r.row_elevation, 0.0, 0.0],
            ArrayKind::Polar(p) => [p.center.x, p.center.y, p.fill, p.row_spacing, p.row_elevation, 0.0],
        };
        a >= 1
            && b >= 1
            && self.levels >= 1
            && self.item_count() <= MAX_ARRAY_ITEMS
            && !self.source.trim().is_empty()
            && nums.iter().chain(std::iter::once(&self.level_spacing)).all(|v| v.is_finite())
            && self.replaced.len() as u64 <= MAX_ARRAY_ITEMS
    }

    /// Where every item goes, row by row and level by level; `None` when the parameters are not
    /// [valid](Self::is_valid).
    pub fn items(&self) -> Option<Vec<ArrayItem>> {
        if !self.is_valid() {
            return None;
        }
        let n = usize::try_from(self.item_count()).ok()?;
        let mut out = Vec::with_capacity(n);
        for level in 0..self.levels {
            let lz = self.level_spacing * f64::from(level);
            match &self.kind {
                ArrayKind::Rect(r) => {
                    let (ux, uy) = (Vec2::from_angle(r.axis_angle), Vec2::from_angle(r.axis_angle).perp());
                    for j in 0..r.rows {
                        for i in 0..r.cols {
                            let p = ux * (r.col_spacing * f64::from(i)) + uy * (r.row_spacing * f64::from(j));
                            out.push(ArrayItem { i, j, level, insert: p.to3(lz + r.row_elevation * f64::from(j)), rotation: 0.0 });
                        }
                    }
                }
                ArrayKind::Polar(p) => {
                    let total = p.fill.to_radians();
                    let full = (total.abs() - std::f64::consts::TAU).abs() < 1e-9;
                    let step = if full { total / f64::from(p.items.max(1)) } else { total / f64::from(p.items.saturating_sub(1).max(1)) };
                    // Rows move the items outwards: along the line from the centre to the base point.
                    let out_dir = {
                        let d = (Vec2::ZERO - p.center).normalized();
                        if d == Vec2::ZERO { Vec2::X } else { d }
                    };
                    for j in 0..p.rows {
                        let o = out_dir * (p.row_spacing * f64::from(j));
                        for i in 0..p.items {
                            let a = step * f64::from(i);
                            let at = o.rotate_about(p.center, a);
                            let rotation = if p.rotate { a } else { 0.0 };
                            out.push(ArrayItem { i, j, level, insert: at.to3(lz + p.row_elevation * f64::from(j)), rotation });
                        }
                    }
                }
            }
        }
        Some(out)
    }

    /// The block an item refers to: a replacement, else the source.
    pub fn block_for(&self, i: u32, j: u32, level: u32) -> &str {
        self.replaced.iter().rev().find(|r| (r.i, r.j, r.level) == (i, j, level)).map_or(self.source.as_str(), |r| r.block.as_str())
    }

    /// The block references that make up the array block: one per item, on layer 0 with ByBlock
    /// properties so they take the array's. Handles are allocated from `d`. `None` when the
    /// parameters are not [valid](Self::is_valid).
    pub fn item_entities(&self, d: &mut Drawing) -> Option<Vec<Entity>> {
        let items = self.items()?;
        let common = Common {
            layer: "0".into(),
            color: cadcraft_color::Color::ByBlock,
            linetype: "ByBlock".into(),
            lineweight: Lineweight::ByBlock,
            ..Common::default()
        };
        let mut out = Vec::with_capacity(items.len());
        for it in items {
            let ins = Insert {
                block: self.block_for(it.i, it.j, it.level).to_string(),
                insert: it.insert,
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation: it.rotation,
                attribs: Vec::new(),
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
            };
            out.push(Entity { handle: d.new_handle(), common: common.clone(), kind: EntityKind::Insert(ins) });
        }
        Some(out)
    }

    /// Take the block names from the array block's items `ents` (in the order [`items`]
    /// lists them): the source block is whatever the items that are not replaced refer to, so
    /// the parameters follow blocks renamed on paste, on INSERT of a file or by other programs
    /// (which may rename anonymous blocks). `false` when the items don't match the parameters
    /// (another program changed the block), so the array should be treated as a plain block.
    ///
    /// [`items`]: Self::items
    pub fn sync_names<'a>(&mut self, ents: impl Iterator<Item = &'a Entity>) -> bool {
        let n = self.item_count();
        let ents: Vec<&Entity> = ents.take(usize::try_from(n).unwrap_or(usize::MAX).saturating_add(1)).collect();
        if ents.len() as u64 != n {
            return false;
        }
        let Some(items) = self.items() else { return false };
        // Index the replacement entries by placement so each item is matched in O(1) instead of
        // scanning the whole `replaced` vector per item, which is quadratic when both the items
        // and the replacements are large (CWE-407). Last entry wins, as `.rev().find` did.
        let mut last_replaced: std::collections::HashMap<(u32, u32, u32), usize> = std::collections::HashMap::with_capacity(self.replaced.len());
        for (idx, r) in self.replaced.iter().enumerate() {
            last_replaced.insert((r.i, r.j, r.level), idx);
        }
        let mut source: Option<&str> = None;
        for (it, e) in items.iter().zip(&ents) {
            let EntityKind::Insert(ins) = &e.kind else { return false };
            match last_replaced.get(&(it.i, it.j, it.level)).and_then(|&idx| self.replaced.get_mut(idx)) {
                Some(r) => r.block.clone_from(&ins.block),
                None => match source {
                    None => source = Some(&ins.block),
                    Some(s) if s.eq_ignore_ascii_case(&ins.block) => {}
                    Some(_) => return false,
                },
            }
        }
        if let Some(s) = source {
            self.source = s.to_string();
        }
        true
    }
}

impl Drawing {
    /// The parameters of `e` when it is an associative array (a block reference to an array
    /// block whose items match its parameters), with the block names its items use.
    pub fn array_params(&self, e: &Entity) -> Option<ArrayParams> {
        let EntityKind::Insert(i) = &e.kind else { return None };
        let b = self.block(&i.block)?;
        let mut p = b.array.clone()?;
        p.sync_names(b.entities.iter().map(|e| e.as_ref())).then_some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(cols: u32, rows: u32) -> ArrayParams {
        ArrayParams {
            source: "*U1".into(),
            kind: ArrayKind::Rect(RectArray { cols, rows, col_spacing: 2.0, row_spacing: 3.0, axis_angle: 0.0, row_elevation: 0.0 }),
            levels: 2,
            level_spacing: 5.0,
            replaced: Vec::new(),
            editing: None,
        }
    }

    #[test]
    fn rect_items_go_column_row_level() {
        let items = rect(3, 2).items().unwrap_or_default();
        assert_eq!(items.len(), 12);
        let last = items.last().copied().map(|i| (i.i, i.j, i.level, i.insert));
        assert_eq!(last, Some((2, 1, 1, Vec3::new(4.0, 3.0, 5.0))));
    }

    #[test]
    fn polar_rows_go_outwards() {
        let p = ArrayParams {
            source: "*U1".into(),
            kind: ArrayKind::Polar(PolarArray {
                center: Vec2::new(-10.0, 0.0),
                items: 4,
                fill: 360.0,
                rotate: true,
                rows: 2,
                row_spacing: 1.0,
                row_elevation: 0.0,
            }),
            levels: 1,
            level_spacing: 0.0,
            replaced: Vec::new(),
            editing: None,
        };
        let items = p.items().unwrap_or_default();
        assert_eq!(items.len(), 8);
        // Item 1 of row 0 is a quarter turn round: base (0,0) → (-10,10).
        assert!(items[1].insert.xy().near(Vec2::new(-10.0, 10.0), 1e-9));
        // Row 1 starts one unit further out.
        assert!(items[4].insert.xy().near(Vec2::new(1.0, 0.0), 1e-9));
    }

    #[test]
    fn hostile_params_are_invalid() {
        let mut p = rect(u32::MAX, u32::MAX);
        assert!(!p.is_valid() && p.items().is_none());
        p = rect(0, 1);
        assert!(!p.is_valid());
        p = rect(2, 2);
        p.level_spacing = f64::NAN;
        assert!(!p.is_valid());
    }
}
