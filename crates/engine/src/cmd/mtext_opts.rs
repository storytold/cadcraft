//! MTEXT's options before the text (Height, Justify, Line spacing, Rotation, Style, Width,
//! Columns) for the prompt machine in `draw.rs`.

use cadcraft_doc::{MText, MTextColumnType, MTextColumns};
use cadcraft_geom::{Vec2, Vec3};

use super::curves;
use super::machines::number;
use crate::{Accept, EngineError, Input, Prompt, Result, Session};

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

/// The justification keywords, in attachment order 1..=9.
pub(super) const JUSTIFY: [&str; 9] = ["TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"];

/// Attachment code (1..=9) for a justification keyword.
pub(super) fn attach_code(k: &str) -> Option<u8> {
    JUSTIFY.iter().position(|j| j.eq_ignore_ascii_case(k.trim())).map(|i| i as u8 + 1)
}

/// What the options set before the text is typed.
pub(super) struct MTextOpts {
    pub height: f64,
    pub attach: u8,
    /// Line spacing factor (1 = 5/3 of the text height between baselines).
    pub spacing: f64,
    pub exact: bool,
    pub rotation: f64,
    pub style: String,
    /// The Width option replaces the opposite corner.
    pub width: Option<f64>,
    /// Columns set by the Columns option (static or dynamic); their answers replace the
    /// opposite corner too.
    pub columns: Option<MTextColumns>,
    /// Static columns: the total width answered, split into columns once the gutter is known.
    pub total_width: f64,
}

impl Default for MTextOpts {
    fn default() -> Self {
        MTextOpts {
            height: 0.2,
            attach: 1,
            spacing: 1.0,
            exact: false,
            rotation: 0.0,
            style: "Standard".into(),
            width: None,
            columns: None,
            total_width: 0.0,
        }
    }
}

impl MTextOpts {
    pub(super) fn new(s: &Session) -> Self {
        let (height, style) =
            s.doc().map(|d| (d.header.f64("TEXTSIZE", 0.2), d.header.str("TEXTSTYLE", "Standard"))).unwrap_or((0.2, "Standard".into()));
        // The line spacing new multiline text gets (TSPACEFAC, TSPACETYPE).
        let (spacing, exact) = (s.settings.line_spacing(), s.settings.tspace_exact);
        MTextOpts { height, style, spacing, exact, ..MTextOpts::default() }
    }

    /// The text box from the first corner and either the opposite corner or the Width answer:
    /// the insertion point is the box's attachment point, the box turned by the rotation about
    /// the first corner.
    pub(super) fn mtext(&self, first: Vec2, second: Option<Vec2>, contents: String) -> MText {
        let (insert, width) = match (second, self.width) {
            (Some(b), _) => {
                // The opposite corner in the box's own (rotated) frame.
                let l = (b - first).rotate(-self.rotation);
                let (x0, x1) = (l.x.min(0.0), l.x.max(0.0));
                let (y0, y1) = (l.y.min(0.0), l.y.max(0.0));
                let i = usize::from(self.attach.clamp(1, 9) - 1);
                let x = [x0, (x0 + x1) / 2.0, x1][i % 3];
                let y = [y1, (y0 + y1) / 2.0, y0][i / 3];
                (first + Vec2::new(x, y).rotate(self.rotation), x1 - x0)
            }
            (None, w) => (first, w.unwrap_or(0.0)),
        };
        MText {
            insert: Vec3::new(insert.x, insert.y, 0.0),
            height: self.height,
            width,
            attach: self.attach.clamp(1, 9),
            rotation: self.rotation,
            style: self.style.clone(),
            contents,
            line_spacing: self.spacing,
            line_spacing_exact: self.exact,
            columns: self.columns.clone().filter(|_| width > 0.0),
        }
    }
}

/// A question an option asks before MTEXT goes back to "Specify opposite corner".
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MTextAsk {
    Height,
    Justify,
    SpacingType,
    SpacingFactor,
    Rotation,
    Style,
    Width,
    Columns,
    /// Static columns: the total width; dynamic: the width of one column.
    ColumnWidth,
    ColumnCount,
    ColumnGutter,
    ColumnHeight,
}

pub(super) enum Outcome {
    /// Ask this next (the same question again after an invalid answer).
    Ask(MTextAsk),
    /// Back to the opposite-corner prompt.
    Back,
    /// The Width answer replaces the opposite corner: go on to the text.
    Text,
}

impl MTextAsk {
    pub(super) fn from_keyword(k: &str) -> Option<MTextAsk> {
        Some(match k {
            "Height" => MTextAsk::Height,
            "Justify" => MTextAsk::Justify,
            "Line spacing" => MTextAsk::SpacingType,
            "Rotation" => MTextAsk::Rotation,
            "Style" => MTextAsk::Style,
            "Width" => MTextAsk::Width,
            "Columns" => MTextAsk::Columns,
            _ => return None,
        })
    }

    pub(super) fn prompt(self, o: &MTextOpts, first: Vec2) -> Prompt {
        match self {
            MTextAsk::Height => Prompt::new("Specify height", Accept::POINT_OR_NUMBER).default(format!("{:.4}", o.height)).base(first),
            MTextAsk::Justify => Prompt::new("Enter justification", curves::KW)
                .kw(&JUSTIFY)
                .default(JUSTIFY.get(usize::from(o.attach.clamp(1, 9) - 1)).copied().unwrap_or("TL")),
            MTextAsk::SpacingType => {
                Prompt::new("Enter line spacing type", curves::KW).kw(&["At least", "Exactly"]).default(if o.exact { "Exactly" } else { "At least" })
            }
            MTextAsk::SpacingFactor => Prompt::new("Enter line spacing factor or distance", Accept::NUMBER).default(format!("{}x", trim(o.spacing))),
            MTextAsk::Rotation => Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER).default(trim(o.rotation.to_degrees())).base(first),
            MTextAsk::Style => Prompt::new("Enter style name", curves::KW).kw(&["?"]).default(o.style.clone()),
            MTextAsk::Width => Prompt::new("Specify width", Accept::POINT_OR_NUMBER).base(first),
            MTextAsk::Columns => Prompt::new("Enter column type", curves::KW).kw(&["Dynamic", "Static", "No"]).default(match &o.columns {
                Some(c) if c.kind == MTextColumnType::Dynamic => "Dynamic",
                Some(_) => "Static",
                None => "No",
            }),
            MTextAsk::ColumnWidth => {
                let stat = o.columns.as_ref().is_some_and(|c| c.kind == MTextColumnType::Static);
                let p = Prompt::new(if stat { "Specify total width" } else { "Specify column width" }, Accept::POINT_OR_NUMBER).base(first);
                match o.width {
                    Some(w) if w > 0.0 => p.default(format!("{:.4}", if stat { o.total_width.max(w) } else { w })),
                    _ => p,
                }
            }
            MTextAsk::ColumnCount => {
                Prompt::new("Specify number of columns", Accept::NUMBER).default(o.columns.as_ref().map_or(2, |c| c.count.max(2)).to_string())
            }
            MTextAsk::ColumnGutter => Prompt::new("Specify gutter width", Accept::POINT_OR_NUMBER)
                .default(format!("{:.4}", o.columns.as_ref().map_or(o.height, |c| c.gutter)))
                .base(first),
            MTextAsk::ColumnHeight => {
                let p = Prompt::new("Specify column height", Accept::POINT_OR_NUMBER).base(first);
                match o.columns.as_ref().map(|c| c.height).filter(|h| *h > 0.0) {
                    Some(h) => p.default(format!("{h:.4}")),
                    None => p,
                }
            }
        }
    }

    pub(super) fn input(self, s: &mut Session, i: Input, o: &mut MTextOpts, first: Vec2) -> Result<Outcome> {
        let word = |i: &Input| match i {
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_string()),
            _ => None,
        };
        if self.is_column_question() {
            return column_input(self, i, o, first);
        }
        match (self, &i) {
            (_, Input::Enter) => Ok(if self == MTextAsk::SpacingType { Outcome::Ask(MTextAsk::SpacingFactor) } else { Outcome::Back }),
            (MTextAsk::Height, Input::Point(p)) => set_height(s, o, first.dist(*p)),
            (MTextAsk::Height, Input::Text(t)) => set_height(s, o, number(t).unwrap_or(f64::NAN)),
            (MTextAsk::Justify, _) => {
                let k = word(&i).unwrap_or_default();
                o.attach = attach_code(&k).ok_or_else(|| other("Invalid option keyword."))?;
                Ok(Outcome::Back)
            }
            (MTextAsk::SpacingType, _) => {
                let k = word(&i).unwrap_or_default().to_ascii_lowercase();
                o.exact = match k.as_str() {
                    "at least" | "a" | "at" => false,
                    "exactly" | "e" => true,
                    _ => return Err(other("Invalid option keyword.")),
                };
                s.settings.tspace_exact = o.exact;
                Ok(Outcome::Ask(MTextAsk::SpacingFactor))
            }
            (MTextAsk::SpacingFactor, Input::Text(t)) => {
                // "1.5x" is a factor; a plain number is the distance between lines (1x = 5/3 of the height).
                let t = t.trim();
                let f = match t.strip_suffix(['x', 'X']) {
                    Some(f) => number(f),
                    None => number(t).map(|d| d / (o.height * 5.0 / 3.0)),
                };
                o.spacing = f
                    .filter(|f| (0.25 - 1e-9..=4.0 + 1e-9).contains(f))
                    .ok_or_else(|| other("Requires a factor between 0.25x and 4x, or a distance."))?
                    .clamp(0.25, 4.0);
                // The next multiline text starts with it too.
                s.settings.tspacefac = o.spacing;
                Ok(Outcome::Back)
            }
            (MTextAsk::Rotation, Input::Point(p)) => {
                o.rotation = first.angle_to(*p);
                Ok(Outcome::Back)
            }
            (MTextAsk::Rotation, Input::Text(t)) => {
                o.rotation = s.angle_settings().direction(t).filter(|a| a.is_finite()).ok_or_else(|| other("Requires an angle."))?;
                Ok(Outcome::Back)
            }
            (MTextAsk::Style, _) => {
                let name = word(&i).unwrap_or_default();
                let d = s.doc()?;
                if name == "?" {
                    let names: Vec<String> = d.text_styles.iter().map(|t| t.name.clone()).collect();
                    s.echo(format!("Text styles: {}", names.join(", ")));
                    return Ok(Outcome::Ask(MTextAsk::Style));
                }
                o.style = d.text_style(&name).map(|t| t.name.clone()).ok_or_else(|| other(format!("Cannot find text style \"{name}\".")))?;
                Ok(Outcome::Back)
            }
            (MTextAsk::Width, Input::Point(p)) => {
                // The distance along the text direction from the first corner.
                o.width = Some((*p - first).rotate(-o.rotation).x.abs());
                Ok(Outcome::Text)
            }
            (MTextAsk::Width, Input::Text(t)) => {
                o.width = Some(number(t).filter(|w| w.is_finite() && *w >= 0.0).ok_or_else(|| other("Requires a non-negative width."))?);
                Ok(Outcome::Text)
            }
            _ => Ok(Outcome::Ask(self)),
        }
    }
}

impl MTextAsk {
    fn is_column_question(self) -> bool {
        matches!(self, MTextAsk::Columns | MTextAsk::ColumnWidth | MTextAsk::ColumnCount | MTextAsk::ColumnGutter | MTextAsk::ColumnHeight)
    }
}

/// The Columns option: the column type, then (static) total width, number of columns, gutter
/// and column height, or (dynamic) column width, gutter and column height. The answers replace
/// the opposite corner: the text comes next.
fn column_input(q: MTextAsk, i: Input, o: &mut MTextOpts, first: Vec2) -> Result<Outcome> {
    let local = |p: Vec2| (p - first).rotate(-o.rotation);
    // A distance: typed, picked (along the text or down it from the first corner) or the default.
    let length = |i: &Input, current: Option<f64>, down: bool| -> Result<f64> {
        let v = match i {
            Input::Point(p) => Some(if down { local(*p).y.abs() } else { local(*p).x.abs() }),
            Input::Text(t) => number(t),
            Input::Enter => current,
            _ => None,
        };
        v.filter(|v| v.is_finite() && *v > 0.0).ok_or_else(|| other("Requires a positive value."))
    };
    let kind = o.columns.as_ref().map_or(MTextColumnType::Static, |c| c.kind);
    match q {
        MTextAsk::Columns => {
            let k = match &i {
                Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
                Input::Enter => return Ok(Outcome::Back),
                _ => String::new(),
            };
            let kind = match k.as_str() {
                "no" | "n" => {
                    o.columns = None;
                    return Ok(Outcome::Back);
                }
                "dynamic" | "d" => MTextColumnType::Dynamic,
                "static" | "s" => MTextColumnType::Static,
                _ => return Err(other("Invalid option keyword.")),
            };
            let prev = o.columns.take().unwrap_or(MTextColumns { count: 2, gutter: o.height, ..MTextColumns::default() });
            o.columns = Some(MTextColumns { kind, auto_height: kind == MTextColumnType::Dynamic, heights: Vec::new(), ..prev });
            Ok(Outcome::Ask(MTextAsk::ColumnWidth))
        }
        MTextAsk::ColumnWidth => {
            let current = o.width.map(|w| if kind == MTextColumnType::Static { o.total_width.max(w) } else { w });
            let w = length(&i, current, false)?;
            if kind == MTextColumnType::Static {
                o.total_width = w;
                Ok(Outcome::Ask(MTextAsk::ColumnCount))
            } else {
                o.width = Some(w);
                Ok(Outcome::Ask(MTextAsk::ColumnGutter))
            }
        }
        MTextAsk::ColumnCount => {
            let current = o.columns.as_ref().map_or(2, |c| c.count.max(2));
            let n = match &i {
                Input::Text(t) => number(t).filter(|n| n.fract() == 0.0 && (1.0..=100.0).contains(n)).map(|n| n as u16),
                Input::Enter => Some(current),
                _ => None,
            }
            .ok_or_else(|| other("Requires a whole number of columns from 1 to 100."))?;
            if let Some(c) = &mut o.columns {
                c.count = n;
            }
            Ok(Outcome::Ask(MTextAsk::ColumnGutter))
        }
        MTextAsk::ColumnGutter => {
            let current = o.columns.as_ref().map(|c| c.gutter);
            let g = match &i {
                Input::Text(t) => number(t).filter(|g| g.is_finite() && *g >= 0.0),
                Input::Point(p) => Some(local(*p).x.abs()),
                Input::Enter => current,
                _ => None,
            }
            .ok_or_else(|| other("Requires a gutter width of 0 or more."))?;
            if kind == MTextColumnType::Static {
                let n = f64::from(o.columns.as_ref().map_or(1, |c| c.count.max(1)));
                let w = (o.total_width - g * (n - 1.0)) / n;
                if !(w.is_finite() && w > 0.0) {
                    return Err(other("The columns and gutters don't fit in the total width."));
                }
                o.width = Some(w);
            }
            if let Some(c) = &mut o.columns {
                c.gutter = g;
            }
            Ok(Outcome::Ask(MTextAsk::ColumnHeight))
        }
        MTextAsk::ColumnHeight => {
            let current = o.columns.as_ref().map(|c| c.height).filter(|h| *h > 0.0);
            let h = length(&i, current, true)?;
            if let Some(c) = &mut o.columns {
                c.height = h;
            }
            Ok(Outcome::Text)
        }
        _ => Ok(Outcome::Back),
    }
}

fn set_height(s: &mut Session, o: &mut MTextOpts, h: f64) -> Result<Outcome> {
    if !h.is_finite() || h <= 0.0 {
        return Err(other("Requires a positive height."));
    }
    o.height = h;
    s.doc_mut()?.header.set_f64("TEXTSIZE", h);
    Ok(Outcome::Back)
}

/// A number without trailing zeros (`1.5`, `2`).
fn trim(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The JSON form's paragraph formatting: `list` (`bullet`, `number`, `lower`, `upper`: each
/// paragraph labelled, with a hanging `indent`), or `indent` / `firstIndent` / `rightIndent`
/// (drawing units; a list's default is 3 text heights), `tabs` (tab stops, drawing units) and `align` (`left`, `center`, `right`,
/// `justify`, `distributed`) as a `\p` code in front of the text.
pub(super) fn json_paragraphs(p: &serde_json::Value, text: String, height: f64) -> Result<String> {
    use cadcraft_fonts::{ListStyle, ParaAlign, ParaProps, TabKind};
    let bad = |m: &str| super::bad("mtext", m);
    let num = |k: &str| -> Result<Option<f64>> {
        match p.get(k) {
            None => Ok(None),
            Some(v) => v.as_f64().filter(|v| v.is_finite() && v.abs() <= 1000.0).map(Some).ok_or_else(|| bad(&format!("`{k}` must be a number"))),
        }
    };
    if let Some(l) = p.get("list") {
        let style = match l.as_str().unwrap_or("").to_ascii_lowercase().as_str() {
            "bullet" | "bullets" => ListStyle::Bullet,
            "number" | "numbers" | "numbered" => ListStyle::Number,
            "lower" | "letter" | "letters" => ListStyle::LowerLetter,
            "upper" => ListStyle::UpperLetter,
            _ => return Err(bad("`list` must be bullet, number, lower or upper")),
        };
        let paras: Vec<&str> = text.split("\\P").collect();
        return Ok(cadcraft_fonts::list_contents(&paras, style, num("indent")?.unwrap_or(3.0 * height)));
    }
    let mut props = ParaProps {
        left: num("indent")?.unwrap_or(0.0).max(0.0),
        first: num("firstIndent")?.unwrap_or(0.0),
        right: num("rightIndent")?.unwrap_or(0.0).max(0.0),
        ..ParaProps::default()
    };
    if let Some(a) = p.get("align") {
        props.align = match a.as_str().unwrap_or("").to_ascii_lowercase().as_str() {
            "left" | "l" => ParaAlign::Left,
            "center" | "centre" | "c" => ParaAlign::Center,
            "right" | "r" => ParaAlign::Right,
            "justify" | "j" => ParaAlign::Justify,
            "distributed" | "d" => ParaAlign::Distributed,
            _ => return Err(bad("`align` must be left, center, right, justify or distributed")),
        };
    }
    if let Some(t) = p.get("tabs") {
        let stops = t.as_array().ok_or_else(|| bad("`tabs` must be an array of numbers"))?;
        for v in stops.iter().take(64) {
            let v = v.as_f64().filter(|v| v.is_finite() && (0.0..=1000.0).contains(v)).ok_or_else(|| bad("`tabs` must be an array of numbers"))?;
            props.tabs.push((v, TabKind::Left));
        }
        props.tabs.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    if props == ParaProps::default() {
        return Ok(text);
    }
    Ok(format!("{}{text}", props.code()))
}

/// The JSON form's `columns`: `{type: static|dynamic, count?, gutter?, height, heights?,
/// reversed?}`; the MTEXT's `width` is the width of one column.
pub(super) fn json_columns(p: &serde_json::Value, text_height: f64, width: f64) -> Result<Option<MTextColumns>> {
    let Some(c) = p.get("columns").filter(|c| !c.is_null()) else { return Ok(None) };
    let bad = |m: &str| super::bad("mtext", m);
    let kind = match c.get("type").and_then(serde_json::Value::as_str).unwrap_or("static").to_ascii_lowercase().as_str() {
        "static" | "s" => MTextColumnType::Static,
        "dynamic" | "d" => MTextColumnType::Dynamic,
        "no" | "none" => return Ok(None),
        _ => return Err(bad("`columns.type` must be static or dynamic")),
    };
    if width <= 0.0 {
        return Err(bad("columns need a `width` (the width of one column)"));
    }
    let len = |k: &str| c.get(k).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite() && *v >= 0.0);
    let heights: Vec<f64> = c
        .get("heights")
        .and_then(serde_json::Value::as_array)
        .map(|a| a.iter().take(1000).filter_map(serde_json::Value::as_f64).filter(|v| v.is_finite() && *v > 0.0).collect())
        .unwrap_or_default();
    let height = len("height").or_else(|| heights.iter().copied().reduce(f64::max)).unwrap_or(0.0);
    if height <= 0.0 {
        return Err(bad("columns need a positive `height`"));
    }
    let count = c.get("count").and_then(serde_json::Value::as_u64).unwrap_or(if kind == MTextColumnType::Static { 2 } else { 1 }).clamp(1, 100);
    Ok(Some(MTextColumns {
        kind,
        count: u16::try_from(count).unwrap_or(2),
        gutter: len("gutter").unwrap_or(text_height),
        height,
        auto_height: kind == MTextColumnType::Dynamic && heights.is_empty(),
        heights: if kind == MTextColumnType::Dynamic { heights } else { Vec::new() },
        flow_reversed: c.get("reversed").and_then(serde_json::Value::as_bool).unwrap_or(false),
    }))
}
