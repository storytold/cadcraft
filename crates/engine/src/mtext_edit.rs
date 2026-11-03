//! The model behind the in-place MTEXT editor: MTEXT contents as plain text with a format per
//! character and the properties of each paragraph, so a text box can show and change bold,
//! italic, underline, font, height, colour, indents and tabs and hand back MTEXT contents.
//!
//! Codes the editor has no control for (stacked fractions `\S`, `\W`, `\T`, `\Q`, `\A`, column
//! breaks `\N`, non-breaking spaces …) are kept as opaque *atoms*: one placeholder character in
//! the text ([`ATOM_BASE`] + n) that [`Formatted::to_contents`] writes back as the original
//! code, so opening and closing the editor never loses them.

use cadcraft_fonts::ParaProps;
pub use cadcraft_fonts::{ListStyle, MTextColor, ParaAlign, TabKind};

/// Largest paragraph indent or tab stop, in drawing units.
const MAX_INDENT: f64 = 1e6;

/// First placeholder character of the opaque codes (Unicode private use area).
pub const ATOM_BASE: u32 = 0xE000;
/// Most atoms one text keeps; further codes are dropped.
const MAX_ATOMS: usize = 1000;
/// Longest text the editor takes (characters).
pub const MAX_CHARS: usize = 100_000;
/// Most paragraphs.
const MAX_PARAS: usize = 20_000;

/// Formatting of one character.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharFmt {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub overline: bool,
    /// Font family set inline (`\f`); `None`: the text style's.
    pub font: Option<String>,
    /// Absolute character height set inline (`\H`); `None`: the text's.
    pub height: Option<f64>,
    /// Colour set inline (`\C`, `\c`); `None`: the entity's (ByLayer/ByBlock).
    pub color: Option<MTextColor>,
}

/// A character format switch of the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    Bold,
    Italic,
    Underline,
    Overline,
}

impl CharFmt {
    fn flag(&self, f: Flag) -> bool {
        match f {
            Flag::Bold => self.bold,
            Flag::Italic => self.italic,
            Flag::Underline => self.underline,
            Flag::Overline => self.overline,
        }
    }

    fn set_flag(&mut self, f: Flag, on: bool) {
        match f {
            Flag::Bold => self.bold = on,
            Flag::Italic => self.italic = on,
            Flag::Underline => self.underline = on,
            Flag::Overline => self.overline = on,
        }
    }
}

/// MTEXT contents as editable text.
#[derive(Clone, Debug, PartialEq)]
pub struct Formatted {
    /// The text, paragraphs separated by `\n`; atoms are placeholders.
    text: String,
    /// One format per character of `text`.
    fmt: Vec<CharFmt>,
    /// One per paragraph (`\n` count + 1).
    paras: Vec<ParaProps>,
    /// The original code of each atom.
    atoms: Vec<String>,
    /// The font family bold and italic runs are written with when no font was set inline
    /// (`\fFamily|b1;`): the text style's.
    pub base_font: String,
}

impl Default for Formatted {
    fn default() -> Self {
        Formatted { text: String::new(), fmt: Vec::new(), paras: vec![ParaProps::default()], atoms: Vec::new(), base_font: "Arial".into() }
    }
}

fn arg_end(chars: &[char], from: usize) -> usize {
    let mut j = from;
    while chars.get(j).is_some_and(|c| *c != ';') {
        j += 1;
    }
    j
}

impl Formatted {
    /// Parse MTEXT contents. Never fails: what isn't understood is kept as atoms or dropped.
    pub fn from_contents(contents: &str) -> Formatted {
        let mut f = Formatted::default();
        let chars: Vec<char> = contents.chars().collect();
        let mut stack: Vec<CharFmt> = Vec::new();
        let mut cur = CharFmt::default();
        let mut props = ParaProps::default();
        f.paras = vec![props.clone()];
        let mut i = 0;
        while let Some(&c) = chars.get(i) {
            if f.fmt.len() >= MAX_CHARS {
                break;
            }
            match c {
                '{' => {
                    if stack.len() < 64 {
                        stack.push(cur.clone());
                    }
                    i += 1;
                }
                '}' => {
                    if let Some(p) = stack.pop() {
                        cur = p;
                    }
                    i += 1;
                }
                '\r' => i += 1,
                '\n' => {
                    if f.paras.len() < MAX_PARAS {
                        f.push_char('\n', &cur);
                        f.paras.push(props.clone());
                    }
                    i += 1;
                }
                '\\' => {
                    let code = chars.get(i + 1).copied();
                    i += 2;
                    match code {
                        None => {}
                        Some('P') if f.paras.len() < MAX_PARAS => {
                            f.push_char('\n', &cur);
                            f.paras.push(props.clone());
                        }
                        Some(c @ ('\\' | '{' | '}')) => f.push_char(c, &cur),
                        Some('L') => cur.underline = true,
                        Some('l') => cur.underline = false,
                        Some('O') => cur.overline = true,
                        Some('o') => cur.overline = false,
                        Some(k @ ('f' | 'F' | 'H' | 'C' | 'c' | 'p')) => {
                            let end = arg_end(&chars, i);
                            let a: String = chars.get(i..end).unwrap_or_default().iter().collect();
                            i = end + 1;
                            match k {
                                'f' | 'F' => apply_font(&mut cur, &a),
                                'H' => cur.height = parse_height(&a, cur.height),
                                'C' => {
                                    cur.color = a.trim().parse::<u16>().ok().and_then(|v| (v > 0 && v < 256).then_some(MTextColor::Aci(v)));
                                }
                                'c' => {
                                    if let Ok(v) = a.trim().parse::<u32>() {
                                        cur.color = Some(MTextColor::Rgb(v & 0xFF_FFFF));
                                    }
                                }
                                _ => {
                                    props.apply(&a);
                                    if let Some(p) = f.paras.last_mut() {
                                        *p = props.clone();
                                    }
                                }
                            }
                        }
                        Some(k) => {
                            // Any other code is kept verbatim: the ones with an argument up to
                            // their `;`, the others alone.
                            let mut code = format!("\\{k}");
                            if !matches!(k, '~' | 'K' | 'k' | 'N') {
                                let end = arg_end(&chars, i);
                                code.extend(chars.get(i..=end.min(chars.len().saturating_sub(1))).unwrap_or_default().iter());
                                i = end + 1;
                            }
                            f.push_atom(code, &cur);
                        }
                    }
                }
                _ => {
                    f.push_char(c, &cur);
                    i += 1;
                }
            }
        }
        f
    }

    fn push_char(&mut self, c: char, fmt: &CharFmt) {
        self.text.push(c);
        self.fmt.push(fmt.clone());
    }

    fn push_atom(&mut self, code: String, fmt: &CharFmt) {
        if self.atoms.len() >= MAX_ATOMS {
            return;
        }
        let ch = char::from_u32(ATOM_BASE + self.atoms.len() as u32).unwrap_or('\u{FFFD}');
        self.atoms.push(code);
        self.push_char(ch, fmt);
    }

    /// The text as the editor shows it.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Number of characters.
    pub fn len(&self) -> usize {
        self.fmt.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fmt.is_empty()
    }

    /// Number of paragraphs.
    pub fn paragraph_count(&self) -> usize {
        self.paras.len()
    }

    /// Format of character `i` (the last one past the end, the default for no text).
    pub fn fmt_at(&self, i: usize) -> CharFmt {
        self.fmt.get(i).or(self.fmt.last()).cloned().unwrap_or_default()
    }

    /// The paragraph character `i` is in.
    pub fn paragraph_of(&self, i: usize) -> usize {
        self.text.chars().take(i).filter(|c| *c == '\n').count().min(self.paras.len().saturating_sub(1))
    }

    /// Properties of paragraph `p`.
    pub fn para(&self, p: usize) -> ParaProps {
        self.paras.get(p).or(self.paras.last()).cloned().unwrap_or_default()
    }

    /// The edited text replaced by `new` (what a text box hands back after a keystroke): the
    /// characters that stay keep their format, new ones take `typing`, and paragraphs follow
    /// the line breaks added and removed. Text beyond [`MAX_CHARS`] is refused (`false`).
    pub fn set_text(&mut self, new: &str, typing: &CharFmt) -> bool {
        if new == self.text {
            return true;
        }
        let old: Vec<char> = self.text.chars().collect();
        let new_c: Vec<char> = new.chars().collect();
        if new_c.len() > MAX_CHARS || new_c.iter().filter(|c| **c == '\n').count() >= MAX_PARAS {
            return false;
        }
        let pre = old.iter().zip(&new_c).take_while(|(a, b)| a == b).count();
        let suf = old.iter().rev().zip(new_c.iter().rev()).take(old.len().min(new_c.len()) - pre).take_while(|(a, b)| a == b).count();
        let removed = old.get(pre..old.len() - suf).unwrap_or_default();
        let inserted = new_c.get(pre..new_c.len() - suf).unwrap_or_default();
        // Typing after a character takes its format unless the toolbar changed it.
        let first_para = old.iter().take(pre).filter(|c| **c == '\n').count();
        let gone = removed.iter().filter(|c| **c == '\n').count();
        let added = inserted.iter().filter(|c| **c == '\n').count();
        let end = (first_para + 1 + gone).min(self.paras.len());
        self.paras.drain((first_para + 1).min(end)..end);
        let template = self.para(first_para);
        for _ in 0..added {
            self.paras.insert((first_para + 1).min(self.paras.len()), template.clone());
        }
        let ins: Vec<CharFmt> = inserted.iter().map(|_| typing.clone()).collect();
        self.fmt.splice(pre..old.len() - suf, ins);
        self.text = new.to_string();
        if self.paras.is_empty() {
            self.paras.push(ParaProps::default());
        }
        true
    }

    fn clamp(&self, r: std::ops::Range<usize>) -> std::ops::Range<usize> {
        r.start.min(self.fmt.len())..r.end.min(self.fmt.len())
    }

    /// Apply `f` to the format of the characters in `range`.
    pub fn edit(&mut self, range: std::ops::Range<usize>, f: impl Fn(&mut CharFmt)) {
        let r = self.clamp(range);
        if let Some(s) = self.fmt.get_mut(r) {
            s.iter_mut().for_each(f);
        }
    }

    /// Whether every character of `range` has `flag`.
    pub fn all_have(&self, range: std::ops::Range<usize>, flag: Flag) -> bool {
        let r = self.clamp(range);
        !r.is_empty() && self.fmt.get(r).is_some_and(|s| s.iter().all(|c| c.flag(flag)))
    }

    /// Toggle `flag` on `range`: set unless every character has it already.
    pub fn toggle(&mut self, range: std::ops::Range<usize>, flag: Flag) {
        let on = !self.all_have(range.clone(), flag);
        self.edit(range, |c| c.set_flag(flag, on));
    }

    /// The paragraphs `range` touches (a caret touches the one it is in).
    fn paras_of(&self, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
        let a = self.paragraph_of(range.start);
        let b = self.paragraph_of(range.end.max(range.start));
        a..b + 1
    }

    /// Change the properties of the paragraphs `range` touches.
    pub fn edit_paras(&mut self, range: std::ops::Range<usize>, f: impl Fn(&mut ParaProps)) {
        let ps = self.paras_of(range);
        if let Some(s) = self.paras.get_mut(ps) {
            s.iter_mut().for_each(f);
        }
    }

    /// Set the indents (in drawing units, as `\p` codes have them) of the paragraphs `range` touches: `first` is relative
    /// to `left`.
    pub fn set_indents(&mut self, range: std::ops::Range<usize>, first: f64, left: f64) {
        if first.is_finite() && left.is_finite() {
            let left = left.clamp(0.0, MAX_INDENT);
            self.edit_paras(range, |p| {
                p.first = first.clamp(-left, MAX_INDENT);
                p.left = left;
            });
        }
    }

    /// Add a tab stop at `pos` drawing units to the paragraphs `range` touches.
    pub fn add_tab(&mut self, range: std::ops::Range<usize>, pos: f64, kind: TabKind) {
        if !pos.is_finite() || !(0.0..=MAX_INDENT).contains(&pos) {
            return;
        }
        self.edit_paras(range, |p| {
            if p.tabs.len() < 64 && !p.tabs.iter().any(|(t, _)| (t - pos).abs() < 1e-6) {
                let at = p.tabs.partition_point(|(t, _)| *t < pos);
                p.tabs.insert(at, (pos, kind));
            }
        });
    }

    /// Remove the tab stop nearest `pos` (within `tolerance`) from the paragraphs `range` touches.
    pub fn remove_tab(&mut self, range: std::ops::Range<usize>, pos: f64, tolerance: f64) {
        self.edit_paras(range, |p| {
            if let Some(i) = p.tabs.iter().position(|(t, _)| (t - pos).abs() <= tolerance) {
                p.tabs.remove(i);
            }
        });
    }

    /// Make the paragraphs `range` touches a list: a hanging indent of `indent` drawing units
    /// with a tab stop there, and each paragraph a label and a tab.
    pub fn make_list(&mut self, range: std::ops::Range<usize>, style: ListStyle, indent: f64) {
        let indent = if indent.is_finite() && indent > 0.0 { indent.min(MAX_INDENT) } else { 3.0 };
        let ps = self.paras_of(range);
        let lines: Vec<&str> = self.text.split('\n').collect();
        let mut out = String::new();
        let mut fmts: Vec<CharFmt> = Vec::new();
        let mut at = 0usize;
        for (k, line) in lines.iter().enumerate() {
            let n = line.chars().count();
            if k > 0 {
                out.push('\n');
                fmts.push(self.fmt.get(at).cloned().unwrap_or_default());
                at += 1;
            }
            if ps.contains(&k) {
                let label = format!("{}\t", style.label(k - ps.start));
                let f = self.fmt.get(at).cloned().unwrap_or_else(|| self.fmt_at(at));
                label.chars().for_each(|_| fmts.push(f.clone()));
                out.push_str(&label);
            }
            out.push_str(line);
            fmts.extend(self.fmt.get(at..at + n).unwrap_or_default().iter().cloned());
            at += n;
        }
        if fmts.len() > MAX_CHARS {
            return;
        }
        self.text = out;
        self.fmt = fmts;
        let props = ParaProps { first: -indent, left: indent, tabs: vec![(indent, TabKind::Left)], ..ParaProps::default() };
        for p in ps {
            if let Some(q) = self.paras.get_mut(p) {
                *q = props.clone();
            }
        }
    }

    /// The MTEXT contents.
    pub fn to_contents(&self) -> String {
        let mut out = String::new();
        let mut emitted = ParaProps::default();
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = 0;
        for (pi, props) in self.paras.iter().enumerate() {
            if pi > 0 {
                out.push_str("\\P");
            }
            if *props != emitted {
                out.push_str(&props.code());
                emitted = props.clone();
            }
            let end = chars.iter().skip(i).position(|c| *c == '\n').map_or(chars.len(), |n| i + n);
            while i < end {
                let f = self.fmt.get(i).cloned().unwrap_or_default();
                let mut j = i + 1;
                while j < end && self.fmt.get(j) == Some(&f) {
                    j += 1;
                }
                let run: String = chars.get(i..j).unwrap_or_default().iter().map(|c| self.escape(*c)).collect();
                if f == CharFmt::default() {
                    out.push_str(&run);
                } else {
                    out.push('{');
                    out.push_str(&self.codes(&f));
                    out.push_str(&run);
                    out.push('}');
                }
                i = j;
            }
            i = end + 1;
        }
        out
    }

    fn escape(&self, c: char) -> String {
        match c {
            '\\' | '{' | '}' => format!("\\{c}"),
            c if (c as u32) >= ATOM_BASE && (c as u32) < ATOM_BASE + MAX_ATOMS as u32 => {
                self.atoms.get((c as u32 - ATOM_BASE) as usize).cloned().unwrap_or_default()
            }
            c => c.to_string(),
        }
    }

    fn codes(&self, f: &CharFmt) -> String {
        let mut s = String::new();
        if f.underline {
            s.push_str("\\L");
        }
        if f.overline {
            s.push_str("\\O");
        }
        if f.bold || f.italic || f.font.is_some() {
            let fam = f.font.clone().unwrap_or_else(|| self.base_font.clone());
            let fam: String = fam.chars().filter(|c| !matches!(c, ';' | '|' | '{' | '}' | '\\')).collect();
            s.push_str(&format!("\\f{fam}|b{}|i{};", u8::from(f.bold), u8::from(f.italic)));
        }
        if let Some(h) = f.height {
            s.push_str(&format!("\\H{};", trim(h)));
        }
        match f.color {
            Some(MTextColor::Aci(v)) => s.push_str(&format!("\\C{v};")),
            Some(MTextColor::Rgb(v)) => s.push_str(&format!("\\c{v};")),
            None => {}
        }
        s
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn parse_height(a: &str, cur: Option<f64>) -> Option<f64> {
    let a = a.trim();
    match a.strip_suffix(['x', 'X']) {
        // A relative height needs the current one; without it the code is dropped.
        Some(r) => r.trim().parse::<f64>().ok().zip(cur).map(|(m, h)| h * m).filter(|h| h.is_finite() && *h > 0.0 && *h < 1e6),
        None => a.parse::<f64>().ok().filter(|h| h.is_finite() && *h > 0.0 && *h < 1e6).or(cur),
    }
}

fn apply_font(cur: &mut CharFmt, a: &str) {
    let mut parts = a.split('|');
    let name = parts.next().unwrap_or("").trim();
    if name.is_empty() {
        return;
    }
    cur.font = Some(name.to_string());
    cur.bold = false;
    cur.italic = false;
    for p in parts {
        match p.as_bytes() {
            [b'b', v] => cur.bold = *v == b'1',
            [b'i', v] => cur.italic = *v == b'1',
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_formats_and_atoms() {
        for src in [
            "plain",
            "a\\Pb",
            "{\\fArial|b1|i0;bold} and {\\Lunder}",
            "x \\S1/2; y",
            "{\\H2;big}{\\C1;red}{\\c16711680;rgb}",
            "\\pxi-3,l3,t3;1.\tone\\P2.\ttwo",
            "esc \\{ \\} \\\\",
        ] {
            let f = Formatted::from_contents(src);
            let again = Formatted::from_contents(&f.to_contents());
            assert_eq!(f, again, "{src}");
        }
        assert_eq!(Formatted::from_contents("plain").to_contents(), "plain");
        assert!(Formatted::from_contents("x \\S1/2; y").to_contents().contains("\\S1/2;"));
    }

    #[test]
    fn toggles_and_typing_keep_formats() {
        let mut f = Formatted::from_contents("hello world");
        f.toggle(0..5, Flag::Bold);
        assert_eq!(f.to_contents(), "{\\fArial|b1|i0;hello} world");
        assert!(f.all_have(0..5, Flag::Bold));
        // Typing inside the bold run takes the typing format; paragraphs follow line breaks.
        let typing = f.fmt_at(2);
        assert!(f.set_text("hello big world", &CharFmt::default()));
        assert!(f.fmt_at(6) == CharFmt::default() && typing.bold);
        assert!(f.set_text("hello big\nworld", &CharFmt::default()));
        assert_eq!(f.paragraph_count(), 2);
        assert_eq!(f.to_contents(), "{\\fArial|b1|i0;hello} big\\Pworld");
        f.toggle(0..5, Flag::Bold);
        assert_eq!(f.to_contents(), "hello big\\Pworld");
    }

    #[test]
    fn ruler_and_lists() {
        let mut f = Formatted::from_contents("one\ntwo");
        f.set_indents(0..0, -2.0, 4.0);
        f.add_tab(0..3, 4.0, TabKind::Left);
        let c = f.to_contents();
        assert!(c.starts_with("\\pxi-2,l4,t4;one"), "{c}");
        f.make_list(0..f.len(), ListStyle::Number, 3.0);
        assert_eq!(f.text(), "1.\tone\n2.\ttwo");
        assert_eq!(f.to_contents(), "\\pxi-3,l3,t3;1.\tone\\P2.\ttwo");
        f.remove_tab(0..f.len(), 3.0, 0.1);
        assert!(!f.to_contents().contains("t3"));
    }

    #[test]
    fn hostile_input_never_panics() {
        for src in
            ["\\", "{{{{", "}}}}", "\\f", "\\fArial|b", "\\H;\\H2x;\\Hnan;", "\\S", "\\p;\\pq;\\pt,,,;", "\\C999999999999;", "\u{E000}\u{E001}"]
        {
            let f = Formatted::from_contents(src);
            let _ = f.to_contents();
        }
        let mut f = Formatted::from_contents("ab");
        let (a, b) = (5, 1);
        f.edit(a..b, |c| c.bold = true);
        f.toggle(9..9, Flag::Italic);
        f.set_indents(7..9, f64::NAN, 1.0);
        f.add_tab(0..99, f64::INFINITY, TabKind::Left);
        f.make_list(50..60, ListStyle::Bullet, f64::NAN);
        assert!(!f.set_text(&"x".repeat(MAX_CHARS + 1), &CharFmt::default()));
        let _ = f.paragraph_of(usize::MAX);
        let _ = f.fmt_at(usize::MAX);
        let _ = Formatted::from_contents(&"{".repeat(100_000));
    }
}
