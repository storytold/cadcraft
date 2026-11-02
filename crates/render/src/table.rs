//! TABLE geometry from the table's cell styles: cell fills, borders and texts. Shared by the
//! display list and the DXF writer (which stores the same geometry as the table's block).

use std::collections::HashMap;

use cadcraft_color::Color;
use cadcraft_doc::{CellBorder, CellStyle, Drawing, Lineweight, Table, TableStyle};
use cadcraft_geom::Vec2;

use crate::{Builder, Ctx, Ink, ink};

/// The most cells laid out (rows × columns); larger tables draw only their outline.
pub const MAX_LAYOUT_CELLS: usize = 1_000_000;

/// A border segment.
#[derive(Clone, Debug, PartialEq)]
pub struct TableLine {
    pub a: Vec2,
    pub b: Vec2,
    pub color: Color,
    pub lineweight: Lineweight,
}

/// A cell background.
#[derive(Clone, Debug, PartialEq)]
pub struct TableFill {
    /// Corners, counter-clockwise from the bottom left.
    pub corners: [Vec2; 4],
    pub color: Color,
}

/// A cell's text, placed at its alignment point.
#[derive(Clone, Debug, PartialEq)]
pub struct TableText {
    pub row: usize,
    pub col: usize,
    pub at: Vec2,
    /// MTEXT attachment (1 top left … 9 bottom right).
    pub attach: u8,
    pub height: f64,
    pub color: Color,
    pub style: String,
    pub text: String,
}

/// Everything a table draws, in world coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableGeom {
    pub fills: Vec<TableFill>,
    pub lines: Vec<TableLine>,
    pub texts: Vec<TableText>,
}

/// Which cells are covered by a merge (and so not drawn on their own).
pub fn table_covered(t: &Table) -> Vec<Vec<bool>> {
    coverage(t).unwrap_or_else(|partial| partial)
}

// No partial coverage may reach owner assignment: it would revisit unbounded merge rectangles.
/// Mark cells covered by merges under grid-size and marking-work limits.
/// Return partial coverage as an error so layout can use its outline fallback instead of assigning owners.
fn coverage(t: &Table) -> Result<Vec<Vec<bool>>, Vec<Vec<bool>>> {
    let rows = t.row_heights.len().min(10_000);
    let cols = t.col_widths.len().min(10_000);
    if rows.saturating_mul(cols) > MAX_LAYOUT_CELLS {
        return Err(vec![Vec::new(); rows]);
    }
    let mut cov = vec![vec![false; cols]; rows];
    // Coverage marking visits each merge's whole rectangle. Overlapping merges re-mark cells that
    // are already covered, so bound the total marking steps to a small multiple of the cell count
    // rather than letting overlapping declarations cause quadratic work (CWE-407).
    let mut budget: u64 = (rows as u64).saturating_mul(cols as u64).saturating_mul(4).saturating_add(1024);
    for (r, row) in t.cells.iter().enumerate().take(rows) {
        for (c, cell) in row.iter().enumerate().take(cols) {
            if let Some((rs, cs)) = cell.merged {
                for rr in r..(r + rs.max(1) as usize).min(rows) {
                    for cc in c..(c + cs.max(1) as usize).min(cols) {
                        budget = budget.saturating_sub(1);
                        if budget == 0 {
                            return Err(cov);
                        }
                        if (rr, cc) != (r, c)
                            && let Some(x) = cov.get_mut(rr).and_then(|v| v.get_mut(cc))
                        {
                            *x = true;
                        }
                    }
                }
            }
        }
    }
    Ok(cov)
}

/// The table style a table uses (Standard defaults when missing).
pub fn table_style_of(d: &Drawing, t: &Table) -> TableStyle {
    d.table_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&t.style)).cloned().unwrap_or_default()
}

/// The scale of a table relative to its style: SCALE changes the table's text height, and every
/// cell style height and margin follows.
fn style_scale(st: &TableStyle, t: &Table) -> f64 {
    // A table from another writer is resolved to its style's height on reading (scale 1); a
    // non-positive height here is the same "no scale of its own".
    let k = t.text_height / st.text_height;
    if k.is_finite() && k > 0.0 { k } else { 1.0 }
}

/// Column and row edge offsets (x right, y down from the insertion point).
fn edges(sizes: &[f64], n: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(n + 1);
    out.push(0.0);
    let mut acc = 0.0;
    for s in sizes.iter().take(n) {
        acc += if s.is_finite() { *s } else { 0.0 };
        out.push(acc);
    }
    out
}

/// A merged cell (or a plain one): its anchor, span and cell style.
struct Anchor {
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    style: usize,
}

/// Lay out table `t` of drawing `d`, every part of a broken table.
pub fn table_geometry(d: &Drawing, t: &Table) -> TableGeom {
    let parts = t.parts();
    if parts.len() <= 1 {
        return layout(d, t);
    }
    let mut g = TableGeom::default();
    for p in parts {
        let mut pg = layout(d, &part_table(t, &p.rows, p.origin));
        for tx in &mut pg.texts {
            tx.row = p.rows.get(tx.row).copied().unwrap_or(tx.row);
        }
        g.fills.extend(pg.fills);
        g.lines.extend(pg.lines);
        g.texts.extend(pg.texts);
    }
    g
}

/// One part of a broken table as a table of its own: its rows (with their cell styles) at
/// `origin`; merges are cut at the part's edge.
fn part_table(t: &Table, rows: &[usize], origin: Vec2) -> Table {
    let cols = t.col_widths.len().min(10_000);
    let mut cells: Vec<Vec<cadcraft_doc::TableCell>> =
        rows.iter().take(10_000).map(|r| t.cells.get(*r).map(|row| row.iter().take(cols).cloned().collect()).unwrap_or_default()).collect();
    // Only expand sparse rows to the full column count when detailed layout is actually permitted.
    // Otherwise `layout` renders an outline, so materialising millions of empty cells here would be
    // wasted work an attacker can trigger with a compact dimension array (CWE-789).
    if rows.len().min(10_000).saturating_mul(cols) <= MAX_LAYOUT_CELLS {
        for (i, row) in cells.iter_mut().enumerate() {
            row.resize_with(cols, Default::default);
            for cell in row.iter_mut() {
                if let Some((rs, cs)) = cell.merged {
                    // Rows of the merge that follow in this part.
                    let start = rows.get(i).copied().unwrap_or(0);
                    let mut n = 1u32;
                    while n < rs && rows.get(i + n as usize).is_some_and(|r| *r == start + n as usize) {
                        n += 1;
                    }
                    cell.merged = (n > 1 || cs > 1).then_some((n, cs));
                }
            }
        }
    }
    Table {
        insert: origin.to3(t.insert.z),
        col_widths: t.col_widths.clone(),
        row_heights: rows.iter().map(|r| t.row_heights.get(*r).copied().unwrap_or(0.0)).collect(),
        row_styles: rows.iter().map(|r| t.row_style(*r).to_string()).collect(),
        cells,
        style: t.style.clone(),
        text_height: t.text_height,
        title: false,
        header: false,
        breaks: None,
        data_link: None,
    }
}

/// Lay out one part.
fn layout(d: &Drawing, t: &Table) -> TableGeom {
    let st = table_style_of(d, t);
    let rows = t.row_heights.len().min(10_000);
    let cols = t.col_widths.len().min(10_000);
    let xs = edges(&t.col_widths, cols);
    let ys = edges(&t.row_heights, rows);
    let o = t.insert.xy();
    let p = |x: f64, y: f64| o + Vec2::new(x, -y);
    let x_at = |c: usize| xs.get(c).copied().unwrap_or(0.0);
    let y_at = |r: usize| ys.get(r).copied().unwrap_or(0.0);
    let mut g = TableGeom::default();
    if rows == 0 || cols == 0 {
        return g;
    }
    let cov = coverage(t);
    if rows.saturating_mul(cols) > MAX_LAYOUT_CELLS || cov.is_err() {
        let (w, h) = (x_at(cols), y_at(rows));
        let corners = [p(0.0, 0.0), p(w, 0.0), p(w, h), p(0.0, h), p(0.0, 0.0)];
        for s in corners.windows(2) {
            if let [a, b] = s {
                g.lines.push(TableLine { a: *a, b: *b, color: Color::ByBlock, lineweight: Lineweight::ByBlock });
            }
        }
        return g;
    }
    let k = style_scale(&st, t);
    // Cell styles by name, scaled to the table.
    let mut styles: Vec<CellStyle> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut style_index = |name: &str| -> usize {
        let key = name.to_ascii_lowercase();
        if let Some(i) = by_name.get(&key) {
            return *i;
        }
        let mut cs = st.cell_style(name);
        cs.text_height *= k;
        cs.margin_h *= k;
        cs.margin_v *= k;
        styles.push(cs);
        let i = styles.len() - 1;
        by_name.insert(key, i);
        i
    };
    // Anchors and the anchor owning each grid cell.
    let cov = cov.unwrap_or_default();
    let mut anchors: Vec<Anchor> = Vec::new();
    let mut owner = vec![usize::MAX; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            if cov.get(r).and_then(|v| v.get(c)).copied().unwrap_or(false) {
                continue;
            }
            let cell = t.cells.get(r).and_then(|row| row.get(c));
            let (rs, cs) = cell.and_then(|x| x.merged).map(|(a, b)| (a.max(1) as usize, b.max(1) as usize)).unwrap_or((1, 1));
            let (rs, cs) = (rs.min(rows - r), cs.min(cols - c));
            let style = style_index(t.cell_style_name(r, c));
            let id = anchors.len();
            for rr in r..r + rs {
                for cc in c..c + cs {
                    if let Some(x) = owner.get_mut(rr * cols + cc)
                        && *x == usize::MAX
                    {
                        *x = id;
                    }
                }
            }
            anchors.push(Anchor { row: r, col: c, rows: rs, cols: cs, style });
        }
    }
    let own = |r: usize, c: usize| owner.get(r * cols + c).copied().filter(|x| *x != usize::MAX);
    let style_of = |a: Option<usize>| a.and_then(|a| anchors.get(a)).and_then(|a| styles.get(a.style));
    let same_style = |a: Option<usize>, b: Option<usize>| matches!((a.and_then(|a| anchors.get(a)), b.and_then(|b| anchors.get(b))), (Some(x), Some(y)) if x.style == y.style);
    // Fills first, under the borders and texts.
    for a in &anchors {
        let Some(cs) = styles.get(a.style) else { continue };
        if let Some(fill) = cs.fill {
            let (x0, x1, y0, y1) = (x_at(a.col), x_at(a.col + a.cols), y_at(a.row), y_at(a.row + a.rows));
            g.fills.push(TableFill { corners: [p(x0, y1), p(x1, y1), p(x1, y0), p(x0, y0)], color: fill });
        }
    }
    // Borders: one segment per grid edge between different cells, joined while unchanged.
    let push = |g: &mut TableGeom, a: Vec2, b: Vec2, br: Option<CellBorder>| {
        let Some(br) = br.filter(|b| b.visible) else { return };
        if let Some(last) = g.lines.last_mut()
            && last.b == a
            && last.color == br.color
            && last.lineweight == br.lineweight
            && (last.b - last.a).cross(b - a).abs() <= 1e-12 * (1.0 + (last.b - last.a).len() * (b - a).len())
        {
            last.b = b;
            return;
        }
        g.lines.push(TableLine { a, b, color: br.color, lineweight: br.lineweight });
    };
    for i in 0..=rows {
        for c in 0..cols {
            let above = if i > 0 { own(i - 1, c) } else { None };
            let below = if i < rows { own(i, c) } else { None };
            if above.is_some() && above == below {
                continue;
            }
            let br = match (style_of(above), style_of(below)) {
                (None, Some(b)) => Some(b.borders.top),
                (Some(a), None) => Some(a.borders.bottom),
                (Some(a), Some(_)) if same_style(above, below) => Some(a.borders.inside_h),
                (Some(a), Some(b)) => Some(CellBorder { visible: a.borders.bottom.visible && b.borders.top.visible, ..b.borders.top }),
                (None, None) => None,
            };
            push(&mut g, p(x_at(c), y_at(i)), p(x_at(c + 1), y_at(i)), br);
        }
    }
    for j in 0..=cols {
        for r in 0..rows {
            let left = if j > 0 { own(r, j - 1) } else { None };
            let right = if j < cols { own(r, j) } else { None };
            if left.is_some() && left == right {
                continue;
            }
            let br = match (style_of(left), style_of(right)) {
                (None, Some(b)) => Some(b.borders.left),
                (Some(a), None) => Some(a.borders.right),
                (Some(a), Some(_)) if same_style(left, right) => Some(a.borders.inside_v),
                (Some(a), Some(b)) => Some(CellBorder { visible: a.borders.right.visible && b.borders.left.visible, ..b.borders.left }),
                (None, None) => None,
            };
            push(&mut g, p(x_at(j), y_at(r)), p(x_at(j), y_at(r + 1)), br);
        }
    }
    // Texts at their alignment point inside the margins.
    for a in &anchors {
        let Some(cell) = t.cells.get(a.row).and_then(|row| row.get(a.col)).filter(|c| !c.text.is_empty()) else { continue };
        let Some(cs) = styles.get(a.style) else { continue };
        let (x0, x1, y0, y1) = (x_at(a.col), x_at(a.col + a.cols), y_at(a.row), y_at(a.row + a.rows));
        let attach = cs.align.clamp(1, 9);
        let x = match (attach - 1) % 3 {
            0 => x0 + cs.margin_h,
            1 => (x0 + x1) / 2.0,
            _ => x1 - cs.margin_h,
        };
        let y = match (attach - 1) / 3 {
            0 => y0 + cs.margin_v,
            1 => (y0 + y1) / 2.0,
            _ => y1 - cs.margin_v,
        };
        g.texts.push(TableText {
            row: a.row,
            col: a.col,
            at: p(x, y),
            attach,
            height: cell.text_height.filter(|h| h.is_finite() && *h > 0.0).map_or(cs.text_height, |h| h * k),
            color: cs.text_color,
            style: cs.text_style.clone(),
            text: cell.text.clone(),
        });
    }
    g
}

/// A cell style colour as drawn: ByBlock / ByLayer take the table's colour.
fn style_ink(c: Color, table: Ink) -> Ink {
    match c {
        Color::ByBlock | Color::ByLayer => table,
        c => ink(c, Color::Index(7), Color::Index(7)),
    }
}

/// Draw a table into the display list.
pub(crate) fn draw(b: &mut Builder, ctx: &Ctx, t: &Table, rgb: Ink, lw: f32) {
    let g = table_geometry(ctx.d, t);
    if b.opts.fill {
        for f in &g.fills {
            let [a, bb, c, dd] = f.corners;
            b.tris(ctx, style_ink(f.color, rgb), &[a, bb, c, a, c, dd]);
        }
    }
    for l in &g.lines {
        let w = match l.lineweight {
            Lineweight::Mm100(v) => f32::from(v) / 100.0,
            Lineweight::Default => crate::default_lineweight(ctx.d),
            Lineweight::ByBlock | Lineweight::ByLayer => lw,
        };
        b.polyline(ctx, style_ink(l.color, rgb), w, &[l.a, l.b]);
    }
    if !b.opts.text {
        return;
    }
    for tx in &g.texts {
        let ts = crate::text_style(ctx.d, &tx.style);
        let font = cadcraft_fonts::TextFont::resolve(&ts.font);
        // Cell content is MTEXT (`\P`, `{\Q15;…}`, `\F…;`): laid out as such, at the cell's
        // alignment point (its numbering is MTEXT's attachment).
        let mp = cadcraft_fonts::MTextParams {
            attach: tx.attach,
            font,
            width_factor: ts.width_factor,
            oblique: ts.oblique,
            ..cadcraft_fonts::MTextParams::new(tx.at, tx.height)
        };
        let layout = cadcraft_fonts::layout_mtext_with(&tx.text, &mp);
        for piece in &layout.pieces {
            b.shaped(ctx, style_ink(tx.color, rgb), lw, &piece.shaped);
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use cadcraft_doc::TableCell;

    /// Excessive overlapping merges draw only the table outline after coverage work is exhausted.
    #[test]
    fn overlapping_merges_use_outline_fallback() {
        let t = Table {
            row_heights: vec![1.0; 40],
            col_widths: vec![1.0; 40],
            cells: vec![vec![TableCell { merged: Some((40, 40)), ..Default::default() }; 40]; 40],
            ..Default::default()
        };
        assert!(coverage(&t).is_err());
        let g = layout(&Drawing::default(), &t);
        assert_eq!(g.lines.len(), 4);
        assert!(g.fills.is_empty() && g.texts.is_empty());
    }

    /// A split table within the displayed-cell budget clips a merge to contiguous selected rows.
    #[test]
    fn part_clips_merges_using_displayed_column_count() {
        let t = Table {
            row_heights: vec![1.0; 102],
            col_widths: vec![1.0; 10_001],
            cells: vec![vec![TableCell { merged: Some((100, 1)), ..Default::default() }]],
            ..Default::default()
        };
        let mut rows: Vec<_> = (0..99).collect();
        rows.push(101);
        let part = part_table(&t, &rows, Vec2::ZERO);
        assert_eq!(part.cells[0][0].merged, Some((99, 1)));
    }
}
