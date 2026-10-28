//! Readers for the user's own linetype (`.lin`) and hatch pattern (`.pat`) files.
//!
//! Both follow the publicly documented text formats: a header line `*NAME[,description]`, then
//! the definition (`A,dash,gap,…` for a linetype, `angle, x, y, dx, dy[, dash, …]` lines for
//! a pattern), with `;` starting a comment line. CADCraft ships no such file; these read the
//! files users bring. Everything here treats the text as hostile: lines, definitions, dashes,
//! families and warnings are capped, numbers must be finite and reasonable, and nothing panics.
//! Problems become warnings and the definitions that can be read are kept.

use super::PatternLine;
use crate::{DashElement, Linetype};

/// Longest line read; longer lines are skipped with a warning.
pub const MAX_LINE_CHARS: usize = 4096;
/// Most definitions (linetypes or patterns) read from one file.
pub const MAX_DEFINITIONS: usize = 4096;
/// Most elements in one linetype.
pub const MAX_LINETYPE_ELEMENTS: usize = 256;
/// Most line families in one hatch pattern (what the renderer draws, too).
pub const MAX_PATTERN_FAMILIES: usize = 1024;
/// Most dashes in one pattern line family (as for a pattern definition read from DXF).
pub const MAX_PATTERN_DASHES: usize = 256;
/// Largest magnitude accepted for a length, offset or scale.
pub const MAX_VALUE: f64 = 1.0e6;
/// Longest name or description kept.
const MAX_NAME_CHARS: usize = 255;
/// Most warnings collected; the rest are counted in a last line.
const MAX_WARNINGS: usize = 100;

/// A hatch pattern read from a user's `.pat` file.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UserPattern {
    pub name: String,
    pub description: String,
    pub lines: Vec<PatternLine>,
}

/// What a file held: the definitions that could be read, and why anything else was skipped.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed<T> {
    pub items: Vec<T>,
    pub warnings: Vec<String>,
}

impl<T> Default for Parsed<T> {
    fn default() -> Self {
        Parsed { items: Vec::new(), warnings: Vec::new() }
    }
}

impl<T> Parsed<T> {
    fn warn(&mut self, line: usize, msg: impl Into<String>) {
        match self.warnings.len() {
            n if n < MAX_WARNINGS => self.warnings.push(format!("line {line}: {}", msg.into())),
            MAX_WARNINGS => self.warnings.push("more problems not listed".into()),
            _ => {}
        }
    }
}

/// A header line `*NAME[,description]`: the name and description, or why it can't be used.
fn header(rest: &str) -> std::result::Result<(String, String), String> {
    let (name, desc) = rest.split_once(',').unwrap_or((rest, ""));
    let name = name.trim();
    if name.is_empty() {
        return Err("a definition without a name".into());
    }
    if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err("a definition name that is too long or has control characters".into());
    }
    let desc: String = desc.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect();
    Ok((name.to_string(), desc))
}

/// A finite number within ±[`MAX_VALUE`].
fn number(t: &str) -> Option<f64> {
    t.trim().parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() <= MAX_VALUE)
}

/// The file's text lines with 1-based numbers, without a leading byte-order mark.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.trim_start_matches('\u{feff}').lines().enumerate().map(|(i, l)| (i.saturating_add(1), l.trim()))
}

// ---------- LIN ----------

/// Read the linetypes of a `.lin` file. Simple linetypes and complex ones with embedded text
/// (`["TEXT",STYLE,S=…,R=…,A=…,U=…,X=…,Y=…]`) are read; shape elements (`[SHAPE,file.shx,…]`)
/// need SHX shape fonts, so they are left out with a warning and the dashes around them kept.
/// Lengths are in drawing units as written: a user's file is never rescaled.
pub fn parse_lin(text: &str) -> Parsed<Linetype> {
    let mut out = Parsed::default();
    // The definition being read: name, description, header line.
    let mut pending: Option<(String, String, usize)> = None;
    for (no, line) in lines(text) {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.chars().count() > MAX_LINE_CHARS {
            out.warn(no, "line too long; skipped");
            continue;
        }
        if let Some(rest) = line.strip_prefix('*') {
            if let Some((name, _, at)) = pending.take() {
                out.warn(at, format!("linetype {name} has no pattern line; skipped"));
            }
            match header(rest) {
                Ok((name, desc)) => pending = Some((name, desc, no)),
                Err(e) => out.warn(no, format!("{e}; skipped")),
            }
            continue;
        }
        let Some((name, description, _)) = pending.take() else {
            out.warn(no, "pattern line without a *NAME line before it; skipped");
            continue;
        };
        if out.items.len() >= MAX_DEFINITIONS {
            out.warn(no, format!("more than {MAX_DEFINITIONS} linetypes; the rest are skipped"));
            break;
        }
        if out.items.iter().any(|l: &Linetype| l.name.eq_ignore_ascii_case(&name)) {
            out.warn(no, format!("linetype {name} is defined twice; the first definition is kept"));
            continue;
        }
        match lin_pattern(line, no, &name, &mut out) {
            Ok(pattern) => out.items.push(Linetype { handle: Default::default(), name, description, pattern }),
            Err(e) => out.warn(no, format!("linetype {name}: {e}; skipped")),
        }
    }
    if let Some((name, _, at)) = pending {
        out.warn(at, format!("linetype {name} has no pattern line; skipped"));
    }
    out
}

/// Split a pattern line at commas outside `[…]` and `"…"`.
fn lin_fields(line: &str) -> std::result::Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let (mut depth, mut quoted, mut start) = (0usize, false, 0usize);
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' if !quoted => depth = depth.saturating_add(1),
            ']' if !quoted => depth = depth.checked_sub(1).ok_or("a ] without its [")?,
            ',' if !quoted && depth == 0 => {
                out.push(line.get(start..i).unwrap_or("").trim());
                start = i.saturating_add(1);
            }
            _ => {}
        }
        if out.len() > MAX_LINETYPE_ELEMENTS.saturating_mul(2) {
            return Err(format!("more than {MAX_LINETYPE_ELEMENTS} elements"));
        }
    }
    if quoted || depth != 0 {
        return Err("an unclosed [ or \"".into());
    }
    out.push(line.get(start..).unwrap_or("").trim());
    Ok(out)
}

/// The elements of a pattern line `A,dash,gap,[…],…`.
fn lin_pattern(line: &str, no: usize, name: &str, out: &mut Parsed<Linetype>) -> std::result::Result<Vec<DashElement>, String> {
    let mut fields = lin_fields(line)?;
    // The alignment field ("A") comes first; a line of plain numbers is read as well.
    if fields.first().is_some_and(|f| f.eq_ignore_ascii_case("a")) {
        fields.remove(0);
    } else if fields.first().is_some_and(|f| number(f).is_none() && !f.starts_with('[')) {
        return Err(format!("unknown alignment `{}` (expected A)", fields.first().copied().unwrap_or("")));
    }
    let mut pattern: Vec<DashElement> = Vec::new();
    for f in fields.into_iter().filter(|f| !f.is_empty()) {
        if pattern.len() >= MAX_LINETYPE_ELEMENTS {
            return Err(format!("more than {MAX_LINETYPE_ELEMENTS} elements"));
        }
        let Some(inner) = f.strip_prefix('[') else {
            let len = number(f).ok_or_else(|| format!("`{f}` is not a dash length"))?;
            pattern.push(DashElement::dash(len));
            continue;
        };
        let inner = inner.strip_suffix(']').ok_or_else(|| format!("`{f}` is not a complete [ ] element"))?;
        match lin_text(inner)? {
            Some(text) => {
                // The text sits at the end of the element before it (a zero-length one if none).
                if pattern.last().is_none_or(|e| e.text.is_some()) {
                    pattern.push(DashElement::dash(0.0));
                }
                if let Some(last) = pattern.last_mut() {
                    *last = DashElement { length: last.length, ..text };
                }
            }
            None => {
                let shape = inner.split(',').next().unwrap_or("").trim();
                out.warn(no, format!("linetype {name}: shape {shape} needs an SHX shape file, which CADCraft can't read; left out"));
            }
        }
    }
    if pattern.is_empty() {
        return Err("no dashes".into());
    }
    if pattern.iter().all(|e| e.length == 0.0) && pattern.iter().all(|e| e.text.is_none()) {
        return Err("every element has zero length".into());
    }
    Ok(pattern)
}

/// A `"TEXT",STYLE,key=value…` element (the inside of `[…]`), or `None` for a shape element.
fn lin_text(inner: &str) -> std::result::Result<Option<DashElement>, String> {
    let inner = inner.trim();
    let Some(after) = inner.strip_prefix('"') else { return Ok(None) };
    let (text, rest) = after.split_once('"').ok_or("unclosed text quote")?;
    if text.chars().count() > MAX_NAME_CHARS || text.chars().any(char::is_control) {
        return Err("embedded text that is too long or has control characters".into());
    }
    let mut parts = rest.split(',').map(str::trim);
    if !parts.next().unwrap_or("").is_empty() {
        return Err("text element: expected a comma after the text".into());
    }
    let style = parts.next().unwrap_or("").trim();
    if style.is_empty() || style.contains('=') {
        return Err("text element: the text style is missing".into());
    }
    let mut e = DashElement { text: Some(text.to_string()), style: Some(style.to_string()), ..DashElement::dash(0.0) };
    for kv in parts.filter(|p| !p.is_empty()) {
        let (k, v) = kv.split_once('=').ok_or_else(|| format!("text element: `{kv}` is not key=value"))?;
        match k.trim().to_ascii_uppercase().as_str() {
            "S" => e.scale = number(v).filter(|s| *s > 0.0).ok_or_else(|| format!("text element: bad scale `{v}`"))?,
            "X" => e.offset.x = number(v).ok_or_else(|| format!("text element: bad X offset `{v}`"))?,
            "Y" => e.offset.y = number(v).ok_or_else(|| format!("text element: bad Y offset `{v}`"))?,
            // Upright text keeps a rotation relative to the line, like R.
            k @ ("R" | "A" | "U") => {
                e.rotation = angle(v).ok_or_else(|| format!("text element: bad rotation `{v}`"))?;
                e.absolute = k == "A";
            }
            other => return Err(format!("text element: unknown option `{other}`")),
        }
    }
    Ok(Some(e))
}

/// An angle in degrees, or with a `d` (degrees), `r` (radians) or `g` (grads) suffix; radians out.
fn angle(t: &str) -> Option<f64> {
    let t = t.trim().to_ascii_lowercase();
    let (num, unit) = match t.char_indices().last() {
        Some((i, c @ ('d' | 'r' | 'g'))) => (t.get(..i)?, c),
        _ => (t.as_str(), 'd'),
    };
    let v = num.trim().parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() <= 1.0e5)?;
    Some(match unit {
        'r' => v,
        'g' => v * std::f64::consts::PI / 200.0,
        _ => v.to_radians(),
    })
}

// ---------- PAT ----------

/// Read the hatch patterns of a `.pat` file. Values are used as written (an inch file for inch
/// drawings, a millimetre file for metric ones).
pub fn parse_pat(text: &str) -> Parsed<UserPattern> {
    let mut out: Parsed<UserPattern> = Parsed::default();
    // The pattern being read, its header line, and whether it already failed.
    let mut cur: Option<(UserPattern, usize, bool)> = None;
    let finish = |cur: Option<(UserPattern, usize, bool)>, out: &mut Parsed<UserPattern>| {
        let Some((p, at, failed)) = cur else { return };
        if failed {
            return;
        }
        if p.lines.is_empty() {
            out.warn(at, format!("pattern {} has no line families; skipped", p.name));
        } else if out.items.len() >= MAX_DEFINITIONS {
            out.warn(at, format!("more than {MAX_DEFINITIONS} patterns; {} skipped", p.name));
        } else if out.items.iter().any(|q| q.name.eq_ignore_ascii_case(&p.name)) {
            out.warn(at, format!("pattern {} is defined twice; the first definition is kept", p.name));
        } else {
            out.items.push(p);
        }
    };
    for (no, line) in lines(text) {
        // A comment runs to the end of the line.
        let line = line.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.chars().count() > MAX_LINE_CHARS {
            out.warn(no, "line too long; skipped");
            continue;
        }
        if let Some(rest) = line.strip_prefix('*') {
            finish(cur.take(), &mut out);
            match header(rest) {
                Ok((name, description)) => cur = Some((UserPattern { name: name.to_ascii_uppercase(), description, lines: Vec::new() }, no, false)),
                Err(e) => out.warn(no, format!("{e}; skipped")),
            }
            continue;
        }
        let Some((p, _, failed)) = cur.as_mut() else {
            out.warn(no, "line family without a *NAME line before it; skipped");
            continue;
        };
        if *failed {
            continue;
        }
        if p.lines.len() >= MAX_PATTERN_FAMILIES {
            out.warn(no, format!("pattern {}: more than {MAX_PATTERN_FAMILIES} line families; skipped", p.name));
            *failed = true;
            continue;
        }
        match pat_family(line) {
            Ok(l) => p.lines.push(l),
            Err(e) => {
                out.warn(no, format!("pattern {}: {e}; skipped", p.name));
                *failed = true;
            }
        }
    }
    finish(cur, &mut out);
    out
}

/// One line family: `angle, x-origin, y-origin, delta-x, delta-y[, dash-1, dash-2, …]`.
fn pat_family(line: &str) -> std::result::Result<PatternLine, String> {
    let mut v = Vec::new();
    for f in line.split(',') {
        if v.len() >= MAX_PATTERN_DASHES.saturating_add(5) {
            return Err(format!("more than {MAX_PATTERN_DASHES} dashes in a line family"));
        }
        let f = f.trim();
        if f.is_empty() && v.len() >= 5 {
            continue;
        }
        v.push(number(f).ok_or_else(|| format!("`{f}` is not a number"))?);
    }
    let (Some(&angle), Some(&x), Some(&y), Some(&dx), Some(&dy)) = (v.first(), v.get(1), v.get(2), v.get(3), v.get(4)) else {
        return Err("a line family needs angle, x, y, delta-x and delta-y".into());
    };
    if dy.abs() < 1e-9 {
        return Err("a line family with zero spacing (delta-y)".into());
    }
    Ok(PatternLine { angle, origin: (x, y), delta: (dx, dy), dashes: v.get(5..).unwrap_or(&[]).to_vec() })
}
