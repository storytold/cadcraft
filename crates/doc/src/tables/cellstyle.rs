//! Table cell styles: the Title, Header and Data styles every table style has, plus named custom
//! ones. A cell style sets the text (style, height, colour, alignment), the cell (fill, margins,
//! borders) and how numbers are shown.

use cadcraft_color::Color;
use serde::{Deserialize, Serialize};

use crate::{Lineweight, Table, TableStyle};

/// The most cell styles a table style keeps (hostile input).
pub const MAX_CELL_STYLES: usize = 64;

/// One border of a cell style.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CellBorder {
    pub visible: bool,
    pub color: Color,
    pub lineweight: Lineweight,
}

impl Default for CellBorder {
    fn default() -> Self {
        CellBorder { visible: true, color: Color::ByBlock, lineweight: Lineweight::ByBlock }
    }
}

/// The borders of a cell style, in the order of the DXF Reference's TABLESTYLE groups: top,
/// inside horizontal, bottom, left, inside vertical, right. "Inside" borders separate cells that
/// share the style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CellBorders {
    pub top: CellBorder,
    pub inside_h: CellBorder,
    pub bottom: CellBorder,
    pub left: CellBorder,
    pub inside_v: CellBorder,
    pub right: CellBorder,
}

impl CellBorders {
    /// The borders in DXF order (top, inside horizontal, bottom, left, inside vertical, right).
    pub fn as_array(&self) -> [CellBorder; 6] {
        [self.top, self.inside_h, self.bottom, self.left, self.inside_v, self.right]
    }
    /// Mutable borders in DXF order.
    pub fn as_array_mut(&mut self) -> [&mut CellBorder; 6] {
        [&mut self.top, &mut self.inside_h, &mut self.bottom, &mut self.left, &mut self.inside_v, &mut self.right]
    }
}

/// How a cell shows a number (a formula result).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CellFormat {
    /// As short as possible (up to the precision when it has decimals).
    #[default]
    General,
    /// Fixed decimals.
    Decimal,
    /// × 100 with a percent sign.
    Percent,
    /// Shown as typed: formulas keep their text.
    Text,
}

/// A cell style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CellStyle {
    pub name: String,
    /// Text style name.
    pub text_style: String,
    pub text_height: f64,
    pub text_color: Color,
    /// Background fill; `None` for no fill.
    pub fill: Option<Color>,
    /// Text alignment in the cell, as MTEXT attachment: 1 top left … 9 bottom right.
    pub align: u8,
    /// Horizontal and vertical margin between the cell edge and its text.
    pub margin_h: f64,
    pub margin_v: f64,
    pub borders: CellBorders,
    pub format: CellFormat,
    /// Decimal places for numbers (0..=8).
    pub precision: u8,
    /// New rows in this style merge across the table (titles).
    pub merge: bool,
}

impl Default for CellStyle {
    fn default() -> Self {
        CellStyle {
            name: CellStyle::DATA.into(),
            text_style: "Standard".into(),
            text_height: 0.18,
            text_color: Color::ByBlock,
            fill: None,
            align: 4,
            margin_h: 0.06,
            margin_v: 0.06,
            borders: CellBorders::default(),
            format: CellFormat::General,
            precision: 2,
            merge: false,
        }
    }
}

impl CellStyle {
    pub const TITLE: &str = "Title";
    pub const HEADER: &str = "Header";
    pub const DATA: &str = "Data";

    /// Whether `name` is one of the three styles every table style has.
    pub fn is_builtin(name: &str) -> bool {
        [Self::TITLE, Self::HEADER, Self::DATA].iter().any(|n| n.eq_ignore_ascii_case(name))
    }

    /// A copy with every value in range (hostile input from files and JSON).
    pub fn sanitized(mut self) -> CellStyle {
        let pos = |v: f64, d: f64| if v.is_finite() && v > 0.0 { v.min(1e9) } else { d };
        let nonneg = |v: f64| if v.is_finite() && v >= 0.0 { v.min(1e9) } else { 0.0 };
        self.text_height = pos(self.text_height, 0.18);
        self.margin_h = nonneg(self.margin_h);
        self.margin_v = nonneg(self.margin_v);
        if !(1..=9).contains(&self.align) {
            self.align = 4;
        }
        self.precision = self.precision.min(8);
        self.name = self.name.trim().chars().take(255).collect();
        if self.text_style.trim().is_empty() {
            self.text_style = "Standard".into();
        }
        self
    }

    /// `v` as this style shows numbers.
    pub fn format_number(&self, v: f64) -> String {
        if !v.is_finite() {
            return "#NUM".into();
        }
        let p = usize::from(self.precision.min(8));
        match self.format {
            CellFormat::Decimal => format!("{:.*}", p, v),
            CellFormat::Percent => format!("{:.*}%", p, v * 100.0),
            CellFormat::General | CellFormat::Text => {
                let s = format!("{:.*}", p, v);
                let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
                if s == "-0" { "0".into() } else { s }
            }
        }
    }
}

impl TableStyle {
    /// The Title, Header or Data style this table style implies from its text height and margin
    /// (data middle left; header and title middle centre, the title 1.4 × as high and merged).
    pub fn base_cell_style(&self, name: &str) -> Option<CellStyle> {
        let base = CellStyle { text_height: self.text_height, margin_h: self.margin, margin_v: self.margin, ..CellStyle::default() };
        if name.eq_ignore_ascii_case(CellStyle::TITLE) {
            Some(CellStyle { name: CellStyle::TITLE.into(), text_height: self.text_height * 1.4, align: 5, merge: true, ..base })
        } else if name.eq_ignore_ascii_case(CellStyle::HEADER) {
            Some(CellStyle { name: CellStyle::HEADER.into(), align: 5, ..base })
        } else if name.eq_ignore_ascii_case(CellStyle::DATA) {
            Some(base)
        } else {
            None
        }
    }

    /// The cell style `name`: the style's own definition, else the implied Title/Header/Data
    /// style, else Data.
    pub fn cell_style(&self, name: &str) -> CellStyle {
        self.cell_styles
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
            .cloned()
            .or_else(|| self.base_cell_style(name))
            .or_else(|| self.cell_styles.iter().find(|c| c.name.eq_ignore_ascii_case(CellStyle::DATA)).cloned())
            .or_else(|| self.base_cell_style(CellStyle::DATA))
            .unwrap_or_default()
    }

    /// Whether the style has a cell style `name` (built-in or custom).
    pub fn has_cell_style(&self, name: &str) -> bool {
        CellStyle::is_builtin(name) || self.cell_styles.iter().any(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// The names of its cell styles: Title, Header, Data, then the custom ones.
    pub fn cell_style_names(&self) -> Vec<String> {
        let mut out: Vec<String> = [CellStyle::TITLE, CellStyle::HEADER, CellStyle::DATA].iter().map(ToString::to_string).collect();
        out.extend(self.cell_styles.iter().filter(|c| !CellStyle::is_builtin(&c.name)).map(|c| c.name.clone()));
        out
    }

    /// Store `cs` (replacing a cell style of the same name). A built-in style equal to its implied
    /// form is not stored, so plain styles stay plain.
    pub fn set_cell_style(&mut self, cs: CellStyle) {
        let cs = cs.sanitized();
        self.cell_styles.retain(|c| !c.name.eq_ignore_ascii_case(&cs.name));
        if self.base_cell_style(&cs.name).is_some_and(|b| b == CellStyle { name: b.name.clone(), ..cs.clone() }) {
            return;
        }
        if self.cell_styles.len() < MAX_CELL_STYLES {
            self.cell_styles.push(cs);
        }
    }
}

/// The cell style of row `row` by its position: Title for the first row of a titled table,
/// Header for the header row, else Data.
pub fn positional_cell_style(row: usize, title: bool, header: bool) -> &'static str {
    if title && row == 0 {
        CellStyle::TITLE
    } else if header && row == usize::from(title) {
        CellStyle::HEADER
    } else {
        CellStyle::DATA
    }
}

impl Default for Table {
    fn default() -> Self {
        Table {
            insert: cadcraft_geom::Vec3::new(0.0, 0.0, 0.0),
            col_widths: Vec::new(),
            row_heights: Vec::new(),
            cells: Vec::new(),
            style: "Standard".into(),
            text_height: 0.18,
            title: false,
            header: false,
            row_styles: Vec::new(),
            breaks: None,
            data_link: None,
        }
    }
}

impl Table {
    /// The cell style name of row `row`.
    pub fn row_style(&self, row: usize) -> &str {
        match self.row_styles.get(row).map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(s) => s,
            None => positional_cell_style(row, self.title, self.header),
        }
    }
    /// The cell style name of cell (`row`, `col`): its own, else its row's.
    pub fn cell_style_name(&self, row: usize, col: usize) -> &str {
        match self.cells.get(row).and_then(|r| r.get(col)).map(|c| c.style.trim()).filter(|s| !s.is_empty()) {
            Some(s) => s,
            None => self.row_style(row),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implied_styles_follow_the_table_style() {
        let st = TableStyle { text_height: 0.25, margin: 0.1, ..TableStyle::default() };
        let t = st.cell_style("title");
        assert_eq!((t.name.as_str(), t.align, t.merge), ("Title", 5, true));
        assert!((t.text_height - 0.35).abs() < 1e-12);
        assert_eq!(st.cell_style("nope").name, "Data");
        assert_eq!(st.cell_style_names(), ["Title", "Header", "Data"]);
    }

    #[test]
    fn storing_an_implied_style_keeps_the_style_plain() {
        let mut st = TableStyle::default();
        st.set_cell_style(st.cell_style("Header"));
        assert!(st.cell_styles.is_empty());
        st.set_cell_style(CellStyle { name: "Totals".into(), fill: Some(Color::Index(3)), ..CellStyle::default() });
        assert!(st.has_cell_style("TOTALS"));
        assert_eq!(st.cell_style_names().last().map(String::as_str), Some("Totals"));
    }

    #[test]
    fn numbers_format() {
        let c = CellStyle::default();
        assert_eq!(c.format_number(2.5), "2.5");
        assert_eq!(c.format_number(3.0), "3");
        assert_eq!(c.format_number(-0.0001), "0");
        let d = CellStyle { format: CellFormat::Decimal, precision: 3, ..c.clone() };
        assert_eq!(d.format_number(1.0), "1.000");
        let p = CellStyle { format: CellFormat::Percent, precision: 0, ..c };
        assert_eq!(p.format_number(0.25), "25%");
        assert_eq!(p.format_number(f64::NAN), "#NUM");
    }

    #[test]
    fn hostile_values_are_sanitized() {
        let c = CellStyle { text_height: f64::NAN, margin_h: -1.0, align: 42, precision: 200, ..CellStyle::default() }.sanitized();
        assert_eq!((c.text_height, c.margin_h, c.align, c.precision), (0.18, 0.0, 4, 8));
    }
}
