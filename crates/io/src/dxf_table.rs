//! Table data in DXF beyond the plain table: cell styles and per-cell styles and formulas.
//!
//! TABLESTYLE: the Data, Header and Title cell types are written in the groups the public DXF
//! Reference documents (text style 7, height 140, alignment 170, text colour 62, fill colour 63
//! with flag 283, border lineweights 274–279, visibility 284–289 and colours 64–69). Everything
//! else — custom cell styles, per-style margins, data format and precision, true colours — goes
//! into CADCraft xdata (`CELLSTYLES`, JSON). ACAD_TABLE: the cell texts are the evaluated values;
//! row and cell styles and the formulas go into CADCraft xdata (`TABLEX`, JSON).

use cadcraft_color::Color;
use cadcraft_doc::{
    CellBorder, CellStyle, DataLink, Lineweight, MAX_CELL_STYLES, MAX_TABLE_PARTS, Table, TableBreak, TableCellData, TableStyle, TableStyleProps,
};
use cadcraft_dxf::Tag;
use serde::{Deserialize, Serialize};

use crate::dxf_ext::{APP, xdata};

/// Marker of the cell styles in a TABLESTYLE's xdata.
pub(crate) const CELLSTYLES: &str = "CELLSTYLES";
/// Marker of the extra table data in an ACAD_TABLE's xdata.
pub(crate) const TABLEX: &str = "TABLEX";
/// Largest JSON payload read back.
const MAX_PAYLOAD: usize = 8 << 20;
/// The documented cell types of a TABLESTYLE, in file order.
pub(crate) const CELL_TYPES: [&str; 3] = [CellStyle::DATA, CellStyle::HEADER, CellStyle::TITLE];

/// `text` cut into xdata strings of at most 250 bytes (at character boundaries).
pub(crate) fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if cur.len() + ch.len_utf8() > 250 {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The `1000` strings after `marker` in our xdata, joined; `None` without the marker.
fn payload(tags: &[Tag], marker: &str) -> Option<String> {
    let x = xdata(tags, APP);
    let at = x.iter().position(|t| t.code == 1000 && t.str().trim() == marker)?;
    let mut out = String::new();
    for t in x.get(at + 1..).unwrap_or(&[]) {
        if t.code != 1000 {
            continue;
        }
        let s = t.str();
        if out.len() + s.len() > MAX_PAYLOAD {
            return None;
        }
        out.push_str(&s);
    }
    Some(out)
}

#[derive(Serialize, Deserialize)]
struct CellStylesPayload {
    v: u32,
    styles: Vec<CellStyle>,
}

/// The xdata strings of a table style's cell styles (marker first), or nothing for a plain style.
pub(crate) fn cell_styles_xdata(s: &TableStyle) -> Vec<String> {
    if s.cell_styles.is_empty() {
        return Vec::new();
    }
    let Ok(json) = serde_json::to_string(&CellStylesPayload { v: 1, styles: s.cell_styles.clone() }) else { return Vec::new() };
    std::iter::once(CELLSTYLES.to_string()).chain(chunks(&json)).collect()
}

/// A colour from an ACI group value.
fn aci(v: i64) -> Color {
    Color::from_aci(i16::try_from(v).unwrap_or(256))
}

/// A table style from a TABLESTYLE object's groups.
pub(crate) fn read_table_style(name: &str, tags: &[Tag]) -> TableStyle {
    let first = |code: i32, set: &[Tag]| set.iter().find(|t| t.code == code).map(|t| t.f64()).filter(|v| v.is_finite());
    let def = TableStyle::default();
    // Groups up to the first cell type (code 7) are the style's own.
    let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbTableStyle").unwrap_or(0);
    let body = tags.get(start..).unwrap_or(&[]);
    let body = body.get(..body.iter().position(|t| t.code >= 1000).unwrap_or(body.len())).unwrap_or(&[]);
    let head_end = body.iter().position(|t| t.code == 7).unwrap_or(body.len());
    let head = body.get(..head_end).unwrap_or(&[]);
    // The version group 280 comes before the description (3); the title and header switches after.
    let desc_at = head.iter().position(|t| t.code == 3).unwrap_or(0);
    let own = head.get(desc_at..).unwrap_or(&[]);
    // Each cell type starts at a code 7 group.
    let mut blocks: Vec<&[Tag]> = Vec::new();
    let mut i = head_end;
    while i < body.len() && blocks.len() < CELL_TYPES.len() {
        let next = body.get(i + 1..).and_then(|r| r.iter().position(|t| t.code == 7)).map(|p| i + 1 + p).unwrap_or(body.len());
        blocks.push(body.get(i..next).unwrap_or(&[]));
        i = next;
    }
    let data_h = blocks.first().and_then(|b| first(140, b)).filter(|h| *h > 0.0);
    let mut st = TableStyle {
        props: Default::default(),
        handle: Default::default(),
        name: name.to_string(),
        text_height: data_h.unwrap_or(def.text_height),
        margin: first(40, head).filter(|m| *m >= 0.0).unwrap_or(def.margin),
        title: own.iter().find(|t| t.code == 280).map(Tag::i64).unwrap_or(0) == 0,
        header: own.iter().find(|t| t.code == 281).map(Tag::i64).unwrap_or(0) == 0,
        cell_styles: Vec::new(),
    };
    st.props = TableStyleProps {
        version: head.get(..desc_at).and_then(|h| h.iter().find(|t| t.code == 280)).and_then(|t| i16::try_from(t.i64()).ok()),
        description: head.iter().find(|t| t.code == 3).map(Tag::str).unwrap_or_default(),
        flow_up: head.iter().find(|t| t.code == 70).is_some_and(|t| t.i64() != 0),
        flags: head.iter().find(|t| t.code == 71).and_then(|t| i16::try_from(t.i64()).ok()).unwrap_or(0),
        cells: std::array::from_fn(|i| {
            let b = blocks.get(i).copied().unwrap_or(&[]);
            let n = |c: i32, d: i32| b.iter().find(|t| t.code == c).and_then(|t| i32::try_from(t.i64()).ok()).unwrap_or(d);
            let def = TableCellData::default();
            TableCellData {
                data_type: n(90, def.data_type),
                unit_type: n(91, def.unit_type),
                format: b.iter().find(|t| t.code == 1).map(Tag::str).unwrap_or_default(),
            }
        }),
    };
    let margin_v = first(41, head).filter(|m| *m >= 0.0);
    for (kind, block) in CELL_TYPES.iter().zip(&blocks) {
        let Some(base) = st.base_cell_style(kind) else { continue };
        let mut cs = base.clone();
        if let Some(v) = margin_v {
            cs.margin_v = v;
        }
        if let Some(n) = block.iter().find(|t| t.code == 7).map(Tag::str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            cs.text_style = n;
        }
        // Heights equal to the implied ones up to rounding keep the implied value.
        if let Some(h) = first(140, block).filter(|h| *h > 0.0 && (h - base.text_height).abs() > 1e-9 * base.text_height.max(1.0)) {
            cs.text_height = h;
        }
        if let Some(a) = block.iter().find(|t| t.code == 170).map(Tag::i64).and_then(|a| u8::try_from(a).ok()).filter(|a| (1..=9).contains(a)) {
            cs.align = a;
        }
        if let Some(c) = block.iter().find(|t| t.code == 62) {
            cs.text_color = aci(c.i64());
        }
        let filled = block.iter().find(|t| t.code == 283).is_some_and(|t| t.i64() != 0);
        cs.fill = if filled { Some(block.iter().find(|t| t.code == 63).map(|t| aci(t.i64())).unwrap_or(Color::Index(7))) } else { None };
        for (k, b) in cs.borders.as_array_mut().into_iter().enumerate() {
            let k = k as i32;
            if let Some(w) = block.iter().find(|t| t.code == 274 + k) {
                b.lineweight = Lineweight::from_dxf(i16::try_from(w.i64()).unwrap_or(-2));
            }
            if let Some(v) = block.iter().find(|t| t.code == 284 + k) {
                b.visible = v.i64() != 0;
            }
            if let Some(c) = block.iter().find(|t| t.code == 64 + k) {
                b.color = aci(c.i64());
            }
        }
        st.set_cell_style(cs);
    }
    // Our own complete cell styles replace what the groups could say.
    if let Some(p) = payload(tags, CELLSTYLES).and_then(|j| serde_json::from_str::<CellStylesPayload>(&j).ok()).filter(|p| p.v == 1) {
        st.cell_styles.clear();
        for cs in p.styles.into_iter().take(MAX_CELL_STYLES) {
            if !cs.name.trim().is_empty() {
                st.set_cell_style(cs);
            }
        }
    }
    st
}

/// The groups of one documented cell type of a TABLESTYLE, in file order.
pub(crate) fn cell_type_tags(cs: &CellStyle, text_style: &str, data: &TableCellData) -> Vec<Tag> {
    let mut out = vec![
        Tag::s(7, text_style),
        Tag::f(140, cs.text_height),
        Tag::i(170, i64::from(cs.align.clamp(1, 9))),
        Tag::i(62, i64::from(cs.text_color.to_aci())),
        Tag::i(63, i64::from(cs.fill.unwrap_or(Color::Index(7)).to_aci())),
        Tag::i(283, i64::from(cs.fill.is_some())),
        Tag::i(90, i64::from(data.data_type)),
        Tag::i(91, i64::from(data.unit_type)),
        Tag::s(1, data.format.clone()),
    ];
    let b: [CellBorder; 6] = cs.borders.as_array();
    out.extend(b.iter().enumerate().map(|(k, x)| Tag::i(274 + k as i32, i64::from(x.lineweight.to_dxf()))));
    out.extend(b.iter().enumerate().map(|(k, x)| Tag::i(284 + k as i32, i64::from(x.visible))));
    out.extend(b.iter().enumerate().map(|(k, x)| Tag::i(64 + k as i32, i64::from(x.color.to_aci()))));
    out
}

/// Row styles and per-cell styles and formulas of a table.
#[derive(Default, Serialize, Deserialize)]
struct TableExtra {
    v: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rows: Vec<String>,
    /// (row, col, cell style, formula).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cells: Vec<(usize, usize, String, Option<String>)>,
    /// Table breaking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    breaks: Option<TableBreak>,
    /// The data link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link: Option<DataLink>,
}

/// The xdata strings of a table's extra data (marker first), or nothing for a plain table.
pub(crate) fn table_xdata(t: &Table) -> Vec<String> {
    let mut x = TableExtra { v: 1, rows: t.row_styles.clone(), cells: Vec::new(), breaks: t.breaks.clone(), link: t.data_link.clone() };
    for (r, row) in t.cells.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if !cell.style.is_empty() || cell.formula.is_some() {
                x.cells.push((r, c, cell.style.clone(), cell.formula.clone()));
            }
        }
    }
    if x.rows.is_empty() && x.cells.is_empty() && x.breaks.is_none() && x.link.is_none() {
        return Vec::new();
    }
    let Ok(json) = serde_json::to_string(&x) else { return Vec::new() };
    std::iter::once(TABLEX.to_string()).chain(chunks(&json)).collect()
}

/// Apply a table's extra data from its xdata.
pub(crate) fn read_table_xdata(tags: &[Tag], t: &mut Table) {
    let Some(x) = payload(tags, TABLEX).and_then(|j| serde_json::from_str::<TableExtra>(&j).ok()).filter(|x| x.v == 1) else { return };
    let rows = t.cells.len();
    t.row_styles = x.rows.into_iter().take(rows).map(|s| s.chars().take(255).collect()).collect();
    for (r, c, style, formula) in x.cells {
        if let Some(cell) = t.cells.get_mut(r).and_then(|row| row.get_mut(c)) {
            cell.style = style.chars().take(255).collect();
            cell.formula = formula.filter(|f| f.chars().count() <= 1024);
        }
    }
    t.breaks = x.breaks.map(|mut b| {
        b.heights.truncate(MAX_TABLE_PARTS);
        b.offsets.truncate(MAX_TABLE_PARTS);
        b
    });
    t.data_link = x.link.filter(|l| !l.path.trim().is_empty() && l.path.len() <= 4096);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_respect_byte_limit_and_characters() {
        let s = "é".repeat(300);
        let c = chunks(&s);
        assert!(c.iter().all(|x| x.len() <= 250));
        assert_eq!(c.concat(), s);
    }
}
