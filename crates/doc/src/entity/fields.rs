//! Fields in text: codes such as `%<\AcVar Date \f "yyyy-MM-dd">%` or
//! `%<\AcObjProp Object(%<\_ObjId 2A>%).Area \f "%lu2%pr2">%` inside a TEXT value, an MTEXT's
//! contents or an attribute value, shown as their value.
//!
//! The codes follow the publicly documented field syntax. Two parts are CADCRAFT's own: the
//! object id is the object's handle (hex), and each field keeps its last evaluated value in the
//! code as `\CcValue "…"`, so a drawing shows its fields without evaluating them again until
//! UPDATEFIELD (or FIELD) refreshes them.

use std::borrow::Cow;
use std::ops::Range;

use super::Handle;

/// Shown for a field that has no value (never evaluated, or its object is gone).
pub const NO_VALUE: &str = "####";
/// Upper bound on fields found in one text (hostile contents).
const MAX_FIELDS: usize = 1_000;
/// Upper bound on fields nested in a field.
const MAX_DEPTH: usize = 8;
/// The cached value's option name.
const VALUE_OPT: &str = "\\CcValue";

/// What a field shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// `\AcVar Name`: Date, SaveDate, Filename, Author, Title, Subject, Keywords, Comments or a
    /// system variable.
    Var(String),
    /// `\AcObjProp Object(%<\_ObjId id>%).Property`: a property of the object with handle `id`
    /// (`None`: no valid id).
    ObjProp { handle: Option<Handle>, property: String },
    /// Any other field: kept as it is and shown with its cached value.
    Other,
}

/// A parsed field code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub kind: FieldKind,
    /// `\f "…"`: the format.
    pub format: Option<String>,
    /// The last evaluated value.
    pub value: Option<String>,
}

/// A field in a text: its byte range (from `%<` to `>%`) and the parsed code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSpan {
    pub range: Range<usize>,
    pub field: Field,
}

/// Skip a quoted string starting at `b[i] == '"'`; returns the index after the closing quote.
fn skip_quoted(b: &[u8], mut i: usize) -> usize {
    i += 1;
    while let Some(&c) = b.get(i) {
        match c {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

/// The end (after `>%`) of the field starting at `start` (`%<`), if it is closed.
fn field_end(b: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = start;
    while i < b.len() {
        match (b.get(i), b.get(i + 1)) {
            (Some(b'%'), Some(b'<')) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return None;
                }
                i += 2;
            }
            (Some(b'>'), Some(b'%')) => {
                depth = depth.checked_sub(1)?;
                i += 2;
                if depth == 0 {
                    return Some(i);
                }
            }
            (Some(b'"'), _) if depth > 0 => i = skip_quoted(b, i),
            _ => i += 1,
        }
    }
    None
}

/// The fields of a text, outermost only, in order. An unclosed `%<` is plain text.
pub fn find(s: &str) -> Vec<FieldSpan> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = s.get(i..).and_then(|t| t.find("%<")) {
        let start = i + off;
        match field_end(b, start) {
            Some(end) => {
                let inner = s.get(start + 2..end - 2).unwrap_or("");
                out.push(FieldSpan { range: start..end, field: parse(inner) });
                if out.len() >= MAX_FIELDS {
                    break;
                }
                i = end;
            }
            None => i = start + 2,
        }
    }
    out
}

/// Whether a text has any field.
pub fn has_fields(s: &str) -> bool {
    s.contains("%<") && !find(s).is_empty()
}

/// The quoted string after option `opt` (`\f "…"`) and its byte range in `inner` (from the
/// option to the closing quote).
fn quoted_opt(inner: &str, opt: &str) -> Option<(String, Range<usize>)> {
    let b = inner.as_bytes();
    let mut i = 0;
    while let Some(off) = inner.get(i..).and_then(|t| t.find(opt)) {
        let at = i + off;
        let mut j = at + opt.len();
        // The option name must end here (`\f` but not `\foo`).
        if b.get(j).is_some_and(|c| c.is_ascii_alphanumeric()) {
            i = j;
            continue;
        }
        while b.get(j) == Some(&b' ') {
            j += 1;
        }
        if b.get(j) != Some(&b'"') {
            i = j;
            continue;
        }
        let end = skip_quoted(b, j);
        let raw = inner.get(j + 1..end.saturating_sub(1).max(j + 1)).unwrap_or("");
        return Some((unescape(raw), at..end.min(inner.len())));
    }
    None
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some(n @ ('"' | '\\')) => out.push(n),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The word after a keyword (up to a blank or a backslash).
fn word_after<'a>(inner: &'a str, key: &str) -> Option<&'a str> {
    let rest = inner.trim_start().strip_prefix(key)?;
    // `\AcObjProp.16.2` style suffixes belong to the keyword.
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit()).trim_start();
    let end = rest.find([' ', '\\']).unwrap_or(rest.len());
    rest.get(..end).filter(|w| !w.is_empty())
}

/// Parse the code between `%<` and `>%`.
pub fn parse(inner: &str) -> Field {
    let format = quoted_opt(inner, "\\f").map(|(f, _)| f);
    let value = quoted_opt(inner, VALUE_OPT).map(|(v, _)| v);
    let t = inner.trim_start();
    let kind = if t.starts_with("\\AcVar") {
        word_after(t, "\\AcVar").map(|w| FieldKind::Var(w.to_string())).unwrap_or(FieldKind::Other)
    } else if t.starts_with("\\AcObjProp") {
        let handle = t.find("\\_ObjId").and_then(|i| t.get(i + "\\_ObjId".len()..)).and_then(|r| {
            let r = r.trim_start();
            let end = r.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(r.len());
            r.get(..end).and_then(Handle::parse_hex)
        });
        let property = t
            .find(").")
            .and_then(|i| t.get(i + 2..))
            .map(|r| r.get(..r.find([' ', '\\']).unwrap_or(r.len())).unwrap_or(""))
            .unwrap_or("")
            .to_string();
        FieldKind::ObjProp { handle, property }
    } else {
        FieldKind::Other
    };
    Field { kind, format, value }
}

/// A field code (`%<…>%`) for a variable (`Date`, `Filename`, a system variable …).
pub fn var_code(name: &str, format: Option<&str>) -> String {
    let f = format.map(|f| format!(" \\f \"{}\"", escape(f))).unwrap_or_default();
    format!("%<\\AcVar {name}{f}>%")
}

/// A field code (`%<…>%`) for a property (`Area`, `Length` …) of the object with `handle`.
pub fn object_code(handle: Handle, property: &str, format: Option<&str>) -> String {
    let f = format.map(|f| format!(" \\f \"{}\"", escape(f))).unwrap_or_default();
    format!("%<\\AcObjProp Object(%<\\_ObjId {}>%).{property}{f}>%", handle.hex())
}

/// Store each field's new value in the text: `value` returns the value of a field (`None`: no
/// value, shown as [`NO_VALUE`]). Returns the new text and the number of fields.
pub fn set_values(s: &str, mut value: impl FnMut(&Field) -> Option<String>) -> (String, usize) {
    let spans = find(s);
    if spans.is_empty() {
        return (s.to_string(), 0);
    }
    let mut out = String::with_capacity(s.len() + 32 * spans.len());
    let mut last = 0;
    for sp in &spans {
        out.push_str(s.get(last..sp.range.start).unwrap_or(""));
        let inner = s.get(sp.range.start + 2..sp.range.end - 2).unwrap_or("");
        // The code without its old value.
        let mut code = match quoted_opt(inner, VALUE_OPT) {
            Some((_, r)) => format!("{}{}", inner.get(..r.start).unwrap_or("").trim_end(), inner.get(r.end..).unwrap_or("")),
            None => inner.to_string(),
        };
        if let Some(v) = value(&sp.field) {
            code = format!("{} {VALUE_OPT} \"{}\"", code.trim_end(), escape(&v));
        }
        out.push_str("%<");
        out.push_str(&code);
        out.push_str(">%");
        last = sp.range.end;
    }
    out.push_str(s.get(last..).unwrap_or(""));
    (out, spans.len())
}

/// The text as shown: each field replaced by `show(value)` (its cached value or [`NO_VALUE`]).
pub fn display_with(s: &str, show: impl Fn(&str) -> String) -> Cow<'_, str> {
    if !s.contains("%<") {
        return Cow::Borrowed(s);
    }
    let spans = find(s);
    if spans.is_empty() {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for sp in &spans {
        out.push_str(s.get(last..sp.range.start).unwrap_or(""));
        out.push_str(&show(sp.field.value.as_deref().unwrap_or(NO_VALUE)));
        last = sp.range.end;
    }
    out.push_str(s.get(last..).unwrap_or(""));
    Cow::Owned(out)
}

/// A TEXT or attribute value as shown.
pub fn display(s: &str) -> Cow<'_, str> {
    display_with(s, str::to_string)
}

/// MTEXT contents as shown (values escaped so they read as text, not as format codes).
pub fn display_mtext(s: &str) -> Cow<'_, str> {
    display_with(s, mtext_escape)
}

/// Escape text for MTEXT contents: `\`, `{` and `}` become literal.
pub fn mtext_escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_parses_fields() {
        let s = "Date: %<\\AcVar Date \\f \"yyyy-MM-dd\">% / A=%<\\AcObjProp Object(%<\\_ObjId 2A>%).Area \\f \"%lu2%pr2\">%";
        let f = find(s);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].field.kind, FieldKind::Var("Date".into()));
        assert_eq!(f[0].field.format.as_deref(), Some("yyyy-MM-dd"));
        assert_eq!(f[1].field.kind, FieldKind::ObjProp { handle: Some(Handle(0x2A)), property: "Area".into() });
        assert_eq!(display(s), "Date: #### / A=####");
    }

    #[test]
    fn values_round_trip_through_the_code() {
        let s = format!("x {} y", var_code("Filename", None));
        let (v, n) = set_values(&s, |_| Some("a \"b\" \\c".into()));
        assert_eq!(n, 1);
        assert_eq!(display(&v), "x a \"b\" \\c y");
        assert_eq!(find(&v)[0].field.kind, FieldKind::Var("Filename".into()));
        // A new value replaces the old one; none removes it.
        let (v2, _) = set_values(&v, |_| Some("new".into()));
        assert_eq!(display(&v2), "x new y");
        assert_eq!(v2.matches("CcValue").count(), 1);
        assert_eq!(set_values(&v2, |_| None).0, s);
        assert_eq!(display_mtext(&set_values(&s, |_| Some("{a}".into())).0), "x \\{a\\} y");
    }

    #[test]
    fn hostile_codes_are_plain_text() {
        for s in [
            "%<",
            "%<\\AcVar",
            "%<%<%<%<%<%<%<%<%<%<x>%>%>%>%>%>%>%>%>%>%",
            "%<\\AcVar Date \\f \"unterminated>%",
            ">%%<>%",
            "%<\\AcObjProp Object(%<\\_ObjId zz>%).>%",
        ] {
            let _ = display(s);
            let _ = set_values(s, |_| Some("v".into()));
        }
        let many = "%<\\AcVar Date>%".repeat(5000);
        assert_eq!(find(&many).len(), MAX_FIELDS);
    }
}
