//! MTEXT paragraph formatting and columns: the `\p…;` paragraph codes (first-line, left and
//! right indents, tab stops, alignment, space before and after), tab characters (bulleted and
//! numbered lists are a label, a tab and a hanging indent), column breaks (`\N`) and text
//! flowing through static or dynamic columns.
//!
//! Contents without any of these lay out exactly as in [`super::layout_mtext_with`]'s plain path.

use cadcraft_geom::{Bounds2, Vec2};

use super::{Atom, Fmt, MTextLayout, MTextParams, MTextPiece, Seg, arg, num, parse, seg_plain, seg_shape, seg_width};

/// Columns an MTEXT's text flows through, each as wide as the MTEXT's width.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MTextColumnSpec {
    /// Dynamic columns: as many as the text needs. Static: exactly `count`.
    pub dynamic: bool,
    /// Number of static columns.
    pub count: usize,
    /// Space between two columns.
    pub gutter: f64,
    /// Height of every column (0 = unbounded: one column).
    pub height: f64,
    /// Heights of the columns one by one (dynamic columns with manual heights); the last one
    /// repeats. Empty: `height` for all.
    pub heights: Vec<f64>,
    /// Columns run right to left.
    pub reversed: bool,
}

/// Where a tab stop puts the text after the tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabKind {
    /// Text starts at the stop.
    #[default]
    Left,
    /// Text is centred on the stop.
    Center,
    /// Text ends at the stop.
    Right,
    /// The decimal point is at the stop (laid out like [`TabKind::Right`]).
    Decimal,
}

/// A paragraph's horizontal alignment (`\pq…`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParaAlign {
    /// The attachment point's column: left, centre or right.
    #[default]
    Default,
    Left,
    Center,
    Right,
    /// Lines spread to the full width, the last line of the paragraph left.
    Justify,
    /// Every line spread to the full width.
    Distributed,
}

/// Paragraph properties set by `\p…;`. Indents and tab stops are in drawing units (as in
/// AutoCAD files: `\pi4.86;` indents the first line by 4.86 units), space before and after in
/// text heights. They stay in effect for the following paragraphs until another `\p` changes them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParaProps {
    /// `i`: first-line indent, relative to the left indent (negative: a hanging indent).
    pub first: f64,
    /// `l`: left indent of every line.
    pub left: f64,
    /// `r`: right indent.
    pub right: f64,
    /// `q`: alignment.
    pub align: ParaAlign,
    /// `t`: tab stops from the left edge, ascending.
    pub tabs: Vec<(f64, TabKind)>,
    /// `b`: space before the paragraph.
    pub before: f64,
    /// `a`: space after the paragraph.
    pub after: f64,
}

/// Upper bound on tab stops per paragraph (hostile contents).
const MAX_TABS: usize = 64;
/// Upper bound on distinct paragraph property sets per MTEXT.
const MAX_STATES: usize = 10_000;
/// Upper bound on columns.
const MAX_COLUMNS: usize = 1_000;
/// Default tab interval, in text heights, past the last tab stop.
pub const DEFAULT_TAB: f64 = 4.0;
/// Largest indent, tab position or spacing accepted.
const MAX_UNITS: f64 = 10_000.0;

fn units(s: &str) -> Option<f64> {
    if s.trim() == "*" {
        return Some(0.0);
    }
    num(s).filter(|v| v.abs() <= MAX_UNITS)
}

impl ParaProps {
    /// Apply one `\p` argument (`xi-3,l3,t3`, `qc`, `t2,c4,r6`, `tz` …) on top of these
    /// properties. Unknown items are ignored.
    pub fn apply(&mut self, arg: &str) {
        let a = arg.trim();
        let a = a.strip_prefix('x').unwrap_or(a);
        let mut in_tabs = false;
        for item in a.split(',').map(str::trim) {
            let mut cs = item.chars();
            let Some(key) = cs.next() else { continue };
            let rest = cs.as_str();
            if in_tabs {
                let tab = match key {
                    'c' => units(rest).map(|v| (v, TabKind::Center)),
                    'r' => units(rest).map(|v| (v, TabKind::Right)),
                    'd' => units(rest).map(|v| (v, TabKind::Decimal)),
                    _ if key.is_ascii_digit() || key == '.' || key == '-' => units(item).map(|v| (v, TabKind::Left)),
                    _ => None,
                };
                if let Some(t) = tab {
                    self.push_tab(t);
                    continue;
                }
                if matches!(key, 'c' | 'r' | 'd') || key.is_ascii_digit() {
                    continue;
                }
                in_tabs = false;
            }
            match key {
                'i' => self.first = units(rest).unwrap_or(self.first),
                'l' => self.left = units(rest).unwrap_or(self.left).max(0.0),
                'r' => self.right = units(rest).unwrap_or(self.right).max(0.0),
                'b' => self.before = units(rest).unwrap_or(self.before).max(0.0),
                'a' => self.after = units(rest).unwrap_or(self.after).max(0.0),
                'q' => {
                    self.align = match rest.chars().next() {
                        Some('l') => ParaAlign::Left,
                        Some('c') => ParaAlign::Center,
                        Some('r') => ParaAlign::Right,
                        Some('j') => ParaAlign::Justify,
                        Some('d') => ParaAlign::Distributed,
                        Some('*') | None => ParaAlign::Default,
                        Some(_) => self.align,
                    }
                }
                't' => {
                    self.tabs.clear();
                    in_tabs = true;
                    let rest = rest.trim();
                    if !rest.is_empty() && rest != "z" && rest != "*" {
                        let t = match rest.split_at(rest.chars().next().map_or(0, char::len_utf8)) {
                            ("c", v) => units(v).map(|v| (v, TabKind::Center)),
                            ("r", v) => units(v).map(|v| (v, TabKind::Right)),
                            ("d", v) => units(v).map(|v| (v, TabKind::Decimal)),
                            _ => units(rest).map(|v| (v, TabKind::Left)),
                        };
                        if let Some(t) = t {
                            self.push_tab(t);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn push_tab(&mut self, t: (f64, TabKind)) {
        if self.tabs.len() < MAX_TABS && t.0 >= 0.0 {
            let at = self.tabs.partition_point(|(p, _)| *p < t.0);
            self.tabs.insert(at, t);
        }
    }

    /// The `\p…;` code for these properties: the indents, then the right indent, spacing and
    /// alignment where they aren't the default, then the tab stops.
    pub fn code(&self) -> String {
        let n = |v: f64| {
            let s = format!("{v:.4}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s == "-0" { "0".to_string() } else { s.to_string() }
        };
        let mut items = vec![format!("i{}", n(self.first)), format!("l{}", n(self.left))];
        for (key, v) in [('r', self.right), ('b', self.before), ('a', self.after)] {
            if v > 0.0 {
                items.push(format!("{key}{}", n(v)));
            }
        }
        let q = match self.align {
            ParaAlign::Default => None,
            ParaAlign::Left => Some("ql"),
            ParaAlign::Center => Some("qc"),
            ParaAlign::Right => Some("qr"),
            ParaAlign::Justify => Some("qj"),
            ParaAlign::Distributed => Some("qd"),
        };
        items.extend(q.map(String::from));
        for (i, (p, k)) in self.tabs.iter().enumerate() {
            let kind = match k {
                TabKind::Left => "",
                TabKind::Center => "c",
                TabKind::Right => "r",
                TabKind::Decimal => "d",
            };
            items.push(format!("{}{kind}{}", if i == 0 { "t" } else { "" }, n(*p)));
        }
        format!("\\px{};", items.join(","))
    }
}

/// The labels of a list's paragraphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyle {
    /// `•`
    Bullet,
    /// `1.` `2.` …
    Number,
    /// `a.` `b.` … `z.` `aa.` …
    LowerLetter,
    /// `A.` `B.` …
    UpperLetter,
}

impl ListStyle {
    /// The label of item `i` (from 0).
    pub fn label(self, i: usize) -> String {
        let letters = |upper: bool| {
            // Bijective base 26: a … z, aa, ab …
            let mut n = i.saturating_add(1);
            let mut s = Vec::new();
            while n > 0 && s.len() < 16 {
                n -= 1;
                let c = b'a' + (n % 26) as u8;
                s.push(char::from(if upper { c.to_ascii_uppercase() } else { c }));
                n /= 26;
            }
            s.iter().rev().collect::<String>() + "."
        };
        match self {
            ListStyle::Bullet => "•".into(),
            ListStyle::Number => format!("{}.", i.saturating_add(1)),
            ListStyle::LowerLetter => letters(false),
            ListStyle::UpperLetter => letters(true),
        }
    }
}

/// Paragraphs as a list: a hanging indent of `indent` drawing units with a tab stop there, and
/// each paragraph its label and a tab (`\pxi-3,l3,t3;1.<tab>First\P2.<tab>Second`).
pub fn list_contents(paragraphs: &[&str], style: ListStyle, indent: f64) -> String {
    let indent = if indent.is_finite() && indent > 0.0 { indent.min(MAX_UNITS) } else { 3.0 };
    let props = ParaProps { first: -indent, left: indent, tabs: vec![(indent, TabKind::Left)], ..ParaProps::default() };
    let items: Vec<String> = paragraphs.iter().enumerate().map(|(i, p)| format!("{}\t{p}", style.label(i))).collect();
    format!("{}{}", props.code(), items.join("\\P"))
}

/// The paragraph properties of each paragraph of MTEXT contents and which paragraph breaks are
/// column breaks (`\N`), found with the same code grammar as [`super::parse`] so paragraph `k`
/// here is the text after the `k`-th [`Atom::Para`].
#[derive(Debug, Default)]
pub struct Paragraphs {
    states: Vec<ParaProps>,
    /// Index into `states` per paragraph.
    para: Vec<usize>,
    /// Per paragraph break: a column break.
    column_break: Vec<bool>,
}

impl Paragraphs {
    pub fn scan(contents: &str) -> Paragraphs {
        let chars: Vec<char> = contents.chars().collect();
        let mut out = Paragraphs { states: vec![ParaProps::default()], ..Default::default() };
        let mut cur = 0usize;
        let mut i = 0;
        let brk = |out: &mut Paragraphs, cur: usize, column: bool| {
            out.para.push(cur);
            out.column_break.push(column);
        };
        while let Some(&c) = chars.get(i) {
            match c {
                '\\' => {
                    let code = chars.get(i + 1).copied().unwrap_or(' ');
                    i += 2;
                    match code {
                        'P' => brk(&mut out, cur, false),
                        'N' => brk(&mut out, cur, true),
                        '~' | '\\' | '{' | '}' | 'L' | 'l' | 'O' | 'o' | 'K' | 'k' => {}
                        'S' => {
                            let mut j = i;
                            while let Some(&ch) = chars.get(j) {
                                if ch == ';' {
                                    break;
                                }
                                j += if ch == '\\' { 2 } else { 1 };
                            }
                            i = j + 1;
                        }
                        'p' => {
                            let (a, n) = arg(&chars, i);
                            i = n;
                            if out.states.len() < MAX_STATES
                                && let Some(st) = out.states.get(cur)
                            {
                                let mut st = st.clone();
                                st.apply(&a);
                                out.states.push(st);
                                cur = out.states.len() - 1;
                            }
                        }
                        _ => {
                            let (_, n) = arg(&chars, i);
                            i = n;
                        }
                    }
                }
                '\n' => {
                    brk(&mut out, cur, false);
                    i += 1;
                }
                _ => i += 1,
            }
        }
        // A `\p` anywhere in a paragraph applies to all of it: the break pushed the state at
        // the paragraph's end, so the last paragraph gets the final state.
        brk(&mut out, cur, false);
        out
    }

    /// Properties of paragraph `k`.
    pub fn props(&self, k: usize) -> &ParaProps {
        let st = self.para.get(k).or(self.para.last()).copied().unwrap_or(0);
        self.states.get(st).or(self.states.first()).unwrap_or(&DEFAULT_PROPS)
    }

    /// The break after paragraph `k` starts a new column.
    pub fn column_break(&self, k: usize) -> bool {
        self.column_break.get(k).copied().unwrap_or(false)
    }

    /// Any paragraph formatting at all.
    pub fn formatted(&self) -> bool {
        self.states.len() > 1 || self.column_break.iter().any(|b| *b)
    }
}

static DEFAULT_PROPS: ParaProps =
    ParaProps { first: 0.0, left: 0.0, right: 0.0, align: ParaAlign::Default, tabs: Vec::new(), before: 0.0, after: 0.0 };

/// Contents or columns that need the paragraph layout.
pub(super) fn needed(contents: &str, p: &MTextParams) -> bool {
    p.columns.is_some() || contents.contains('\t') || contents.contains("\\p")
}

/// One piece of a laid-out line: a segment, its format, x offset and the number of word gaps
/// before it on the line (for justified paragraphs).
struct Item {
    seg: Seg,
    fmt: usize,
    x: f64,
    gap: usize,
}

#[derive(Default)]
struct PLine {
    items: Vec<Item>,
    /// Right end of the content.
    width: f64,
    max_h: f64,
    text: String,
    gaps: usize,
    para: usize,
    /// The last line of its paragraph.
    para_end: bool,
}

/// A word split at its tab characters.
enum Part {
    Text(Vec<(Seg, usize)>),
    Tab,
}

fn split_tabs(segs: &[(Seg, usize)]) -> Vec<Part> {
    let mut out = Vec::new();
    let mut cur: Vec<(Seg, usize)> = Vec::new();
    for (sg, f) in segs {
        match sg {
            Seg::Text(t) if t.contains('\t') => {
                for (i, piece) in t.split('\t').enumerate() {
                    if i > 0 {
                        if !cur.is_empty() {
                            out.push(Part::Text(std::mem::take(&mut cur)));
                        }
                        out.push(Part::Tab);
                    }
                    if !piece.is_empty() {
                        cur.push((Seg::Text(piece.to_string()), *f));
                    }
                }
            }
            _ => cur.push((sg.clone(), *f)),
        }
    }
    if !cur.is_empty() {
        out.push(Part::Text(cur));
    }
    out
}

struct LineBuilder<'a> {
    paragraphs: &'a Paragraphs,
    formats: &'a [Fmt],
    base: &'a Fmt,
    width: f64,
    height: f64,
    lines: Vec<PLine>,
    line: PLine,
    paragraph: usize,
    pending_space: Option<usize>,
    last_format: usize,
}

impl<'a> LineBuilder<'a> {
    fn new(paragraphs: &'a Paragraphs, formats: &'a [Fmt], base: &'a Fmt, width: f64, height: f64) -> Self {
        let first = paragraphs.props(0);
        Self {
            paragraphs,
            formats,
            base,
            width,
            height,
            lines: Vec::new(),
            line: PLine { width: (first.left + first.first).max(0.0), ..Default::default() },
            paragraph: 0,
            pending_space: None,
            last_format: 0,
        }
    }

    fn format(&self, index: usize) -> &Fmt {
        self.formats.get(index).unwrap_or(self.base)
    }

    fn build(mut self, atoms: &[Atom]) -> Vec<PLine> {
        for atom in atoms {
            self.push_atom(atom);
        }
        self.finish(true, 0.0);
        self.lines
    }

    fn push_atom(&mut self, atom: &Atom) {
        match atom {
            Atom::Space(format) => {
                if !self.line.items.is_empty() || self.pending_space.is_some() {
                    self.pending_space = Some(*format);
                }
                self.last_format = *format;
            }
            Atom::Para(format) => {
                self.pending_space = None;
                let next_start = self.first_start(self.paragraph + 1);
                self.last_format = *format;
                self.finish(true, next_start);
                self.paragraph += 1;
            }
            Atom::Word(segments) => self.push_word(segments),
        }
    }

    fn first_start(&self, paragraph: usize) -> f64 {
        let props = self.paragraphs.props(paragraph);
        (props.left + props.first).max(0.0)
    }

    fn finish(&mut self, paragraph_end: bool, next_start: f64) {
        if self.line.items.is_empty() {
            self.line.max_h = self.format(self.last_format).height;
        }
        self.line.para = self.paragraph;
        self.line.para_end = paragraph_end;
        let next = PLine { width: next_start, ..Default::default() };
        self.lines.push(std::mem::replace(&mut self.line, next));
    }

    fn push_word(&mut self, segments: &[(Seg, usize)]) {
        let parts = split_tabs(segments);
        for (index, part) in parts.iter().enumerate() {
            match part {
                Part::Tab => self.push_tab(&parts, index),
                Part::Text(text) => self.push_text(index, text),
            }
        }
    }

    fn push_tab(&mut self, parts: &[Part], index: usize) {
        self.pending_space = None;
        let (available, left) = {
            let props = self.paragraphs.props(self.paragraph);
            (self.available_width(props), props.left.max(0.0))
        };
        let (stop, kind) = next_tab(self.line.width, self.paragraphs.props(self.paragraph), self.height);
        let next_width: f64 = match parts.get(index + 1) {
            Some(Part::Text(segments)) => segments.iter().map(|(segment, format)| seg_width(segment, self.format(*format))).sum(),
            _ => 0.0,
        };
        let x = match kind {
            TabKind::Left => stop,
            TabKind::Center => (stop - next_width / 2.0).max(self.line.width),
            TabKind::Right | TabKind::Decimal => (stop - next_width).max(self.line.width),
        };
        if x > available + 1e-9 && !self.line.items.is_empty() {
            self.finish(false, left);
        } else {
            self.line.width = x;
            self.line.text.push(' ');
        }
    }

    fn push_text(&mut self, index: usize, segments: &[(Seg, usize)]) {
        let word_width: f64 = segments.iter().map(|(segment, format)| seg_width(segment, self.format(*format))).sum();
        let space_width = if index == 0 {
            self.pending_space
                .map(|format| crate::space_width(&self.format(format).font, self.format(format).height, self.format(format).wf))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let (available, left) = {
            let props = self.paragraphs.props(self.paragraph);
            (self.available_width(props), props.left.max(0.0))
        };
        if self.width > 0.0 && !self.line.items.is_empty() && self.line.width + space_width + word_width > available + 1e-9 {
            self.finish(false, left);
        } else if index == 0 && (!self.line.items.is_empty() || self.pending_space.is_some()) {
            self.line.width += space_width;
            if self.pending_space.is_some() {
                self.line.text.push(' ');
                self.line.gaps += 1;
            }
        }
        self.pending_space = None;
        for (segment, format) in segments {
            let segment_width = seg_width(segment, self.format(*format));
            self.line.max_h = self.line.max_h.max(self.format(*format).height);
            self.line.text.push_str(&seg_plain(segment));
            self.line.items.push(Item { seg: segment.clone(), fmt: *format, x: self.line.width, gap: self.line.gaps });
            self.line.width += segment_width;
            self.last_format = *format;
        }
    }

    fn available_width(&self, props: &ParaProps) -> f64 {
        if self.width > 0.0 { (self.width - props.right).max(0.0) } else { f64::INFINITY }
    }
}

/// The next tab stop right of `x`: the paragraph's stops, the left indent of a hanging indent,
/// then every [`DEFAULT_TAB`] text heights.
fn next_tab(x: f64, pp: &ParaProps, h: f64) -> (f64, TabKind) {
    let eps = 1e-9;
    let mut best: Option<(f64, TabKind)> = pp.tabs.iter().map(|(p, k)| (*p, *k)).find(|(p, _)| *p > x + eps);
    let hang = pp.left;
    if hang > x + eps && best.is_none_or(|(p, _)| hang < p) {
        best = Some((hang, TabKind::Left));
    }
    if let Some(b) = best {
        return b;
    }
    let step = DEFAULT_TAB * h;
    ((((x + eps) / step).floor() + 1.0) * step, TabKind::Left)
}

/// Lay out MTEXT with paragraph formatting and columns. `base` is the entity's own format,
/// `h` its text height, `width` the column width (0: no wrapping, no columns).
pub(super) fn layout(contents: &str, p: &MTextParams, base: &Fmt) -> MTextLayout {
    let h = base.height;
    let width = if p.width.is_finite() && p.width > 0.0 { p.width } else { 0.0 };
    let rotation = if p.rotation.is_finite() { p.rotation } else { 0.0 };
    let paras = Paragraphs::scan(contents);
    let (atoms, fmts) = parse(contents, base.clone());
    let fmt = |i: usize| fmts.get(i).unwrap_or(base);
    let lines = LineBuilder::new(&paras, &fmts, base, width, h).build(&atoms);

    // Vertical placement: baselines below the top of each line's column.
    let ls = if p.line_spacing > 0.0 && p.line_spacing.is_finite() { p.line_spacing } else { 1.0 };
    let cols = p.columns.as_ref().filter(|_| width > 0.0);
    let max_cols = match cols {
        Some(c) if c.dynamic => MAX_COLUMNS,
        Some(c) => c.count.clamp(1, MAX_COLUMNS),
        None => 1,
    };
    let col_height = |c: usize| -> f64 {
        let Some(spec) = cols else { return 0.0 };
        let v = if spec.heights.is_empty() { spec.height } else { spec.heights.get(c).or(spec.heights.last()).copied().unwrap_or(0.0) };
        if v.is_finite() && v > 0.0 { v } else { 0.0 }
    };
    let mut placed: Vec<(usize, f64)> = Vec::with_capacity(lines.len());
    let (mut c, mut y, mut first_in_col, mut force) = (0usize, 0.0f64, true, false);
    let mut col_bottom = vec![0.0f64];
    let mut prev_para: Option<usize> = None;
    for l in &lines {
        let lh = if l.max_h > 0.0 { l.max_h } else { h };
        let new_para = prev_para != Some(l.para);
        let before = if new_para && prev_para.is_some() { paras.props(l.para).before * h } else { 0.0 };
        let step_h = if p.line_spacing_exact { h } else { lh };
        let pitch = |first: bool| if first { lh } else { step_h * 5.0 / 3.0 * ls + before };
        let hc = col_height(c);
        if !first_in_col && c + 1 < max_cols && (force || (hc > 0.0 && y + pitch(false) > hc + 1e-9)) {
            c += 1;
            col_bottom.push(0.0);
            y = 0.0;
            first_in_col = true;
        }
        force = false;
        y += pitch(first_in_col);
        first_in_col = false;
        placed.push((c, -y));
        if let Some(b) = col_bottom.get_mut(c) {
            *b = y;
        }
        if l.para_end {
            y += paras.props(l.para).after * h;
            force = paras.column_break(l.para);
        }
        prev_para = Some(l.para);
    }
    let n_cols = match cols {
        Some(spec) if !spec.dynamic => max_cols,
        _ => c + 1,
    };
    let content_h = col_bottom.iter().copied().fold(0.0, f64::max);
    let block_h = match cols {
        Some(_) => (0..n_cols).map(col_height).fold(content_h, f64::max),
        None => content_h,
    };
    let col_w = if width > 0.0 { width } else { lines.iter().map(|l| l.width + paras.props(l.para).right).fold(0.0, f64::max) };
    let gutter = cols.map(|s| s.gutter).filter(|g| g.is_finite() && *g >= 0.0).unwrap_or(0.0);
    let total_w = col_w * n_cols as f64 + gutter * n_cols.saturating_sub(1) as f64;
    let attach = p.attach.clamp(1, 9) - 1;
    let (acol, arow) = (attach % 3, attach / 3);
    let top = match arow {
        0 => 0.0,
        1 => block_h / 2.0,
        _ => block_h,
    };
    let box_x0 = match acol {
        0 => 0.0,
        1 => -total_w / 2.0,
        _ => -total_w,
    };
    let reversed = cols.is_some_and(|s| s.reversed);
    let insert = p.insert;
    let xf = |q: Vec2| insert + q.rotate(rotation);
    let mut out = MTextLayout { lines: lines.iter().map(|l| l.text.clone()).collect(), ..Default::default() };
    for (l, (c, base_y)) in lines.iter().zip(placed) {
        let pp = paras.props(l.para);
        let slot = if reversed { n_cols.saturating_sub(1).saturating_sub(c) } else { c };
        let col_x = box_x0 + slot as f64 * (col_w + gutter);
        let right_edge = col_w - pp.right;
        let free = right_edge - l.width;
        let align = match pp.align {
            ParaAlign::Default => [ParaAlign::Left, ParaAlign::Center, ParaAlign::Right].get(usize::from(acol)).copied().unwrap_or_default(),
            a => a,
        };
        let spread = width > 0.0 && l.gaps > 0 && free > 0.0 && (align == ParaAlign::Distributed || (align == ParaAlign::Justify && !l.para_end));
        let (shift, per_gap) = match align {
            ParaAlign::Center => (free / 2.0, 0.0),
            ParaAlign::Right => (free, 0.0),
            _ if spread => (0.0, free / l.gaps as f64),
            _ => (0.0, 0.0),
        };
        for it in &l.items {
            let fm = fmt(it.fmt);
            let mut sh = seg_shape(&it.seg, fm, it.x);
            let off = Vec2::new(col_x + shift + per_gap * it.gap as f64, top + base_y);
            sh.map(|q| xf(q + off));
            match out.pieces.last_mut() {
                Some(pc) if pc.color == fm.color => pc.shaped.extend(sh),
                _ => out.pieces.push(MTextPiece { color: fm.color, shaped: sh }),
            }
        }
    }
    for pc in &out.pieces {
        out.strokes.extend(pc.shaped.outlines());
    }
    out.bounds = Bounds2::from_points(
        [Vec2::new(box_x0, top), Vec2::new(box_x0 + total_w, top), Vec2::new(box_x0 + total_w, top - block_h), Vec2::new(box_x0, top - block_h)]
            .map(xf),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_contents_lay_out_as_the_plain_path() {
        for (s, w, a) in [
            ("the quick brown fox jumps over the lazy dog", 8.0, 1u8),
            ("one\\Ptwo\\P\\Pfour", 0.0, 5),
            ("\\H3x;\\Psmall", 0.0, 1),
            ("alpha {\\H2x;beta} gamma delta", 6.0, 9),
            ("A{\\C1;B}C \\S1/2; x", 0.0, 2),
            ("  lead  spaces  and\\P  more", 5.0, 3),
        ] {
            let p = MTextParams { width: w, attach: a, rotation: 0.3, insert: Vec2::new(1.0, 2.0), ..MTextParams::new(Vec2::ZERO, 1.0) };
            let plain = super::super::layout_mtext_with(s, &p);
            // An empty `\p;` code changes nothing but takes the paragraph path.
            let para = super::super::layout_mtext_with(&format!("{s}\\p;"), &p);
            assert_eq!(plain.lines, para.lines, "{s}");
            assert_eq!(plain.strokes.len(), para.strokes.len(), "{s}");
            for (x, y) in plain.strokes.iter().flatten().zip(para.strokes.iter().flatten()) {
                assert!(x.dist(*y) < 1e-9, "{s}: {x:?} vs {y:?}");
            }
            assert!(plain.bounds.min.dist(para.bounds.min) < 1e-9 && plain.bounds.max.dist(para.bounds.max) < 1e-9, "{s}");
        }
    }

    #[test]
    fn list_labels() {
        assert_eq!(ListStyle::Number.label(9), "10.");
        assert_eq!(ListStyle::LowerLetter.label(0), "a.");
        assert_eq!(ListStyle::LowerLetter.label(26), "aa.");
        assert_eq!(ListStyle::UpperLetter.label(27), "AB.");
        assert_eq!(list_contents(&["one", "two"], ListStyle::Number, 2.0), "\\pxi-2,l2,t2;1.\tone\\P2.\ttwo");
    }

    #[test]
    fn props_parse_and_persist() {
        let p = Paragraphs::scan("\\pxi-3,l3,t3;1.\tone\\P2.\ttwo\\P\\pi0,l0,tz;plain");
        assert_eq!(p.props(0).first, -3.0);
        assert_eq!(p.props(0).left, 3.0);
        assert_eq!(p.props(0).tabs, vec![(3.0, TabKind::Left)]);
        assert_eq!(p.props(1), p.props(0));
        assert_eq!(p.props(2).left, 0.0);
        assert!(p.props(2).tabs.is_empty());
        let mut pp = ParaProps::default();
        pp.apply("t2,c4,r6,d8,qc,b0.5");
        assert_eq!(pp.tabs, vec![(2.0, TabKind::Left), (4.0, TabKind::Center), (6.0, TabKind::Right), (8.0, TabKind::Decimal)]);
        assert_eq!(pp.align, ParaAlign::Center);
        assert_eq!(pp.before, 0.5);
        let mut back = ParaProps::default();
        back.apply(pp.code().trim_start_matches("\\p").trim_end_matches(';'));
        assert_eq!(back, pp);
    }
}
