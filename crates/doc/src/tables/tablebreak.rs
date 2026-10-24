//! Table breaking: a long table drawn in several parts side by side or one below the other, with
//! the label rows repeated, and data links (the table's cells filled from a CSV file).

use cadcraft_geom::{Bounds2, Vec2};
use serde::{Deserialize, Serialize};

use crate::Table;

/// The most parts a table is broken into.
pub const MAX_TABLE_PARTS: usize = 1000;

/// Where the next part of a broken table goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BreakDirection {
    #[default]
    Right,
    Left,
    Down,
}

/// TABLEBREAK settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TableBreak {
    pub enabled: bool,
    pub direction: BreakDirection,
    /// Repeat the title and header rows at the top of every part.
    pub repeat_top: bool,
    /// Repeat the last row (bottom labels) at the bottom of every part.
    pub repeat_bottom: bool,
    /// Largest part height (labels included).
    pub height: f64,
    /// Manual heights of the first parts; later parts use `height`.
    pub heights: Vec<f64>,
    /// Space between parts.
    pub spacing: f64,
    /// Moves of parts 2, 3, … from their automatic place (part grips).
    pub offsets: Vec<Vec2>,
}

impl Default for TableBreak {
    fn default() -> Self {
        TableBreak {
            enabled: false,
            direction: BreakDirection::Right,
            repeat_top: true,
            repeat_bottom: false,
            height: 0.0,
            heights: Vec::new(),
            spacing: 0.5,
            offsets: Vec::new(),
        }
    }
}

/// A CSV file the table's cells come from (DATALINK).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DataLink {
    pub path: String,
    /// The first table row the file's rows fill.
    pub start_row: usize,
}

/// One part of a (broken) table: the rows it shows, top to bottom, and its insertion point.
#[derive(Clone, Debug, PartialEq)]
pub struct TablePart {
    pub rows: Vec<usize>,
    pub origin: Vec2,
}

fn size(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

impl Table {
    /// Number of label rows at the top (title and header).
    pub fn top_label_rows(&self) -> usize {
        (usize::from(self.title) + usize::from(self.header)).min(self.row_heights.len())
    }

    /// Whether the table is drawn in several parts.
    pub fn is_broken(&self) -> bool {
        self.breaks.as_ref().is_some_and(|b| b.enabled && (size(b.height) > 0.0 || b.heights.iter().any(|h| size(*h) > 0.0)))
    }

    /// The parts the table is drawn in (one part when it isn't broken).
    pub fn parts(&self) -> Vec<TablePart> {
        let rows = self.row_heights.len().min(10_000);
        let o = self.insert.xy();
        let all = || TablePart { rows: (0..rows).collect(), origin: o };
        let Some(b) = self.breaks.as_ref().filter(|_| self.is_broken()) else { return vec![all()] };
        let h = |r: usize| size(self.row_heights.get(r).copied().unwrap_or(0.0));
        let top: Vec<usize> = (0..self.top_label_rows()).collect();
        let bottom: Option<usize> = (b.repeat_bottom && rows > top.len() + 1).then(|| rows - 1);
        let data_end = bottom.unwrap_or(rows);
        let bottom_h = bottom.map(h).unwrap_or(0.0);
        let mut parts: Vec<Vec<usize>> = Vec::new();
        let mut r = top.len();
        while r < data_end && parts.len() < MAX_TABLE_PARTS {
            let i = parts.len();
            let limit = b.heights.get(i).copied().map(size).filter(|v| *v > 0.0).unwrap_or(size(b.height));
            let mut rows_here: Vec<usize> = if i == 0 || b.repeat_top { top.clone() } else { Vec::new() };
            let mut used: f64 = rows_here.iter().map(|r| h(*r)).sum::<f64>() + if b.repeat_bottom { bottom_h } else { 0.0 };
            // At least one data row per part.
            while r < data_end {
                let next = r;
                let fits = limit <= 0.0 || used + h(next) <= limit + 1e-9;
                if !fits && rows_here.len() > if i == 0 || b.repeat_top { top.len() } else { 0 } {
                    break;
                }
                used += h(next);
                rows_here.push(next);
                r += 1;
            }
            parts.push(rows_here);
        }
        if parts.is_empty() {
            return vec![all()];
        }
        // The rest when the part limit is reached; the bottom labels.
        let n = parts.len();
        if let Some(last) = parts.last_mut() {
            last.extend(r..data_end);
        }
        for p in &mut parts {
            if let Some(br) = bottom {
                p.push(br);
            }
        }
        // Place them.
        let width: f64 = self.col_widths.iter().map(|w| size(*w)).sum();
        let spacing = size(b.spacing);
        let mut out = Vec::with_capacity(n);
        let mut at = o;
        for (i, rows) in parts.into_iter().enumerate() {
            if i > 0 {
                let prev_h: f64 = out.last().map(|p: &TablePart| p.rows.iter().map(|r| h(*r)).sum()).unwrap_or(0.0);
                at = match b.direction {
                    BreakDirection::Right => at + Vec2::new(width + spacing, 0.0),
                    BreakDirection::Left => at - Vec2::new(width + spacing, 0.0),
                    BreakDirection::Down => at - Vec2::new(0.0, prev_h + spacing),
                };
            }
            let off = if i > 0 { b.offsets.get(i - 1).copied().filter(|v| v.is_finite()).unwrap_or(Vec2::ZERO) } else { Vec2::ZERO };
            // Automatic places follow the automatic chain; manual moves apply to the part only.
            out.push(TablePart { rows, origin: at + off });
        }
        out
    }

    /// The area the table covers, all parts included.
    pub fn bounds(&self) -> Bounds2 {
        let w: f64 = self.col_widths.iter().map(|w| size(*w)).sum();
        let mut b = Bounds2::EMPTY;
        for p in self.parts() {
            let h: f64 = p.rows.iter().map(|r| size(self.row_heights.get(*r).copied().unwrap_or(0.0))).sum();
            b.add(p.origin);
            b.add(p.origin + Vec2::new(w, -h));
        }
        b
    }

    /// The top-left corner, width and height of cell (`row`, `col`) — over its merged span — in
    /// the part that holds the row.
    pub fn cell_rect(&self, row: usize, col: usize) -> Option<(Vec2, f64, f64)> {
        if col >= self.col_widths.len() || row >= self.row_heights.len() {
            return None;
        }
        let (rs, cs) =
            self.cells.get(row).and_then(|r| r.get(col)).and_then(|c| c.merged).map_or((1, 1), |(a, b)| (a.max(1) as usize, b.max(1) as usize));
        let x: f64 = self.col_widths.iter().take(col).map(|w| size(*w)).sum();
        let w: f64 = self.col_widths.iter().skip(col).take(cs).map(|w| size(*w)).sum();
        for part in self.parts() {
            let Some(at) = part.rows.iter().position(|r| *r == row) else { continue };
            let y: f64 = part.rows.iter().take(at).map(|r| size(self.row_heights.get(*r).copied().unwrap_or(0.0))).sum();
            let h: f64 = part.rows.iter().skip(at).take(rs).map(|r| size(self.row_heights.get(*r).copied().unwrap_or(0.0))).sum();
            return Some((part.origin + Vec2::new(x, -y), w, h));
        }
        None
    }

    /// The cell (row, col) under `p`, in any part.
    pub fn cell_at_point(&self, p: Vec2) -> Option<(usize, usize)> {
        for part in self.parts() {
            let (x, y) = (p.x - part.origin.x, part.origin.y - p.y);
            if !(x.is_finite() && y.is_finite()) || x < 0.0 || y < 0.0 {
                continue;
            }
            let mut acc = 0.0;
            let mut row = None;
            for r in &part.rows {
                acc += size(self.row_heights.get(*r).copied().unwrap_or(0.0));
                if y <= acc {
                    row = Some(*r);
                    break;
                }
            }
            let Some(row) = row else { continue };
            let mut acc = 0.0;
            for (c, w) in self.col_widths.iter().enumerate() {
                acc += size(*w);
                if x <= acc {
                    return Some((row, c));
                }
            }
        }
        None
    }

    /// Grip points: the insertion point, then the origin of every further part.
    pub fn grip_points(&self) -> Vec<Vec2> {
        let mut g = vec![self.insert.xy()];
        if self.is_broken() {
            g.extend(self.parts().into_iter().skip(1).map(|p| p.origin));
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TableCell;

    fn table(rows: usize) -> Table {
        Table {
            col_widths: vec![2.0, 3.0],
            row_heights: vec![1.0; rows],
            cells: vec![vec![TableCell::default(); 2]; rows],
            title: true,
            header: true,
            ..Table::default()
        }
    }

    #[test]
    fn cell_rect_covers_the_merged_span_and_hostile_indices_are_none() {
        let mut t = table(3);
        assert_eq!(t.cell_rect(1, 1), Some((Vec2::new(2.0, -1.0), 3.0, 1.0)));
        t.cells[0][0].merged = Some((1, 2));
        assert_eq!(t.cell_rect(0, 0), Some((Vec2::ZERO, 5.0, 1.0)));
        assert_eq!(t.cell_rect(9, 0), None);
        assert_eq!(t.cell_rect(0, usize::MAX), None);
    }

    #[test]
    fn unbroken_is_one_part() {
        let t = table(5);
        assert_eq!(t.parts(), vec![TablePart { rows: vec![0, 1, 2, 3, 4], origin: Vec2::ZERO }]);
    }

    #[test]
    fn breaks_repeat_labels_and_go_right() {
        let mut t = table(8);
        t.breaks = Some(TableBreak { enabled: true, height: 4.0, spacing: 1.0, ..TableBreak::default() });
        let p = t.parts();
        let rows: Vec<Vec<usize>> = p.iter().map(|x| x.rows.clone()).collect();
        assert_eq!(rows, vec![vec![0, 1, 2, 3], vec![0, 1, 4, 5], vec![0, 1, 6, 7]]);
        assert_eq!(p[1].origin, Vec2::new(6.0, 0.0));
        assert_eq!(t.grip_points().len(), 3);
        assert_eq!(t.cell_at_point(Vec2::new(7.0, -2.5)), Some((4, 0)));
        let b = t.bounds();
        assert_eq!((b.min, b.max), (Vec2::new(0.0, -4.0), Vec2::new(17.0, 0.0)));
    }

    #[test]
    fn down_without_repeats_and_manual_heights() {
        let mut t = table(8);
        t.breaks = Some(TableBreak {
            enabled: true,
            direction: BreakDirection::Down,
            repeat_top: false,
            repeat_bottom: true,
            height: 3.0,
            heights: vec![5.0],
            offsets: vec![Vec2::new(1.0, 0.0)],
            ..TableBreak::default()
        });
        let p = t.parts();
        let rows: Vec<Vec<usize>> = p.iter().map(|x| x.rows.clone()).collect();
        assert_eq!(rows, vec![vec![0, 1, 2, 3, 7], vec![4, 5, 7], vec![6, 7]]);
        assert_eq!(p[1].origin, Vec2::new(1.0, -5.5));
    }

    #[test]
    fn hostile_break_settings() {
        let mut t = table(3000);
        t.breaks = Some(TableBreak { enabled: true, height: 1e-9, spacing: f64::NAN, ..TableBreak::default() });
        let p = t.parts();
        assert!(p.len() <= MAX_TABLE_PARTS);
        assert_eq!(p.iter().map(|x| x.rows.iter().filter(|r| **r >= 2).count()).sum::<usize>(), 2998);
        t.breaks = Some(TableBreak { enabled: true, height: f64::INFINITY, ..TableBreak::default() });
        assert_eq!(t.parts().len(), 1);
    }
}
