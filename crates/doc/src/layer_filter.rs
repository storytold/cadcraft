//! Layer filters of the Layer Properties Manager.
//!
//! A property filter lists the layers whose properties match its rules: a layer matches a rule
//! when it matches every condition the rule sets, and the filter when it matches any of its rules.
//! A group filter lists the layers named in it. A filter nested under another lists only layers
//! its parent lists. Text conditions are wildcard patterns ([`Wildcard`]).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::Layer;

/// Longest wildcard pattern accepted.
pub const MAX_PATTERN: usize = 1024;
/// Longest text a pattern is matched against; longer text never matches.
pub const MAX_MATCH_TEXT: usize = 65_536;
/// Most filters a drawing keeps.
pub const MAX_FILTERS: usize = 10_000;
/// Most rules of one property filter.
pub const MAX_RULES: usize = 256;
/// Most layers of one group filter.
pub const MAX_GROUP_LAYERS: usize = 100_000;
/// Deepest nesting of filters.
pub const MAX_FILTER_DEPTH: usize = 64;

/// A pattern or filter that cannot be used, with the reason.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct FilterError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, FilterError> {
    Err(FilterError(msg.into()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Star,
    Any,
    Digit,
    Alpha,
    NonAlnum,
    Lit(char),
    Set { negate: bool, items: Vec<(char, char)> },
}

impl Tok {
    fn accepts(&self, c: char) -> bool {
        match self {
            Tok::Star | Tok::Any => true,
            Tok::Digit => c.is_ascii_digit(),
            Tok::Alpha => c.is_alphabetic(),
            Tok::NonAlnum => !c.is_alphanumeric(),
            Tok::Lit(l) => *l == c,
            Tok::Set { negate, items } => items.iter().any(|(a, b)| *a <= c && c <= *b) != *negate,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Alternative {
    negate: bool,
    toks: Vec<Tok>,
}

/// A compiled wildcard pattern in AutoCAD's syntax, matched without regard to case.
///
/// `*` any run of characters, `?` any one character, `#` a digit, `@` a letter, `.` a character
/// that is neither, `[…]` one of the characters in the brackets (`a-z` a range, `[~…]` one not in
/// them), `~` first in a pattern matches what the rest does not, `` ` `` takes the next character
/// as it is, and `,` separates alternatives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wildcard {
    alternatives: Vec<Alternative>,
}

fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

impl Wildcard {
    /// Compile `pattern`; an error names what is wrong with it.
    pub fn parse(pattern: &str) -> Result<Self, FilterError> {
        if pattern.chars().count() > MAX_PATTERN {
            return err(format!("pattern is longer than {MAX_PATTERN} characters"));
        }
        let chars: Vec<char> = pattern.chars().collect();
        let mut alternatives = Vec::new();
        let mut cur = Alternative { negate: false, toks: Vec::new() };
        let mut start = true;
        let mut i = 0;
        while let Some(&c) = chars.get(i) {
            i += 1;
            if start && c == '~' && chars.get(i).is_some_and(|n| *n != ',') {
                cur.negate = true;
                start = false;
                continue;
            }
            start = false;
            let tok = match c {
                ',' => {
                    alternatives.push(std::mem::replace(&mut cur, Alternative { negate: false, toks: Vec::new() }));
                    start = true;
                    continue;
                }
                '*' => {
                    if cur.toks.last() != Some(&Tok::Star) {
                        cur.toks.push(Tok::Star);
                    }
                    continue;
                }
                '?' => Tok::Any,
                '#' => Tok::Digit,
                '@' => Tok::Alpha,
                '.' => Tok::NonAlnum,
                '`' => match chars.get(i) {
                    Some(&n) => {
                        i += 1;
                        Tok::Lit(fold(n))
                    }
                    None => return err("pattern ends with ` (escape) and nothing to escape"),
                },
                '[' => {
                    let (tok, next) = parse_set(&chars, i)?;
                    i = next;
                    tok
                }
                ']' => return err("] without an opening ["),
                other => Tok::Lit(fold(other)),
            };
            cur.toks.push(tok);
        }
        alternatives.push(cur);
        Ok(Wildcard { alternatives })
    }

    /// Whether `text` matches the pattern.
    pub fn matches(&self, text: &str) -> bool {
        if text.len() > MAX_MATCH_TEXT {
            return false;
        }
        self.matches_any(&[text])
    }

    /// Whether any of `texts` (aliases of one value) matches; `~` negates over the whole set.
    pub fn matches_any<S: AsRef<str>>(&self, texts: &[S]) -> bool {
        let texts: Vec<Vec<char>> =
            texts.iter().filter(|t| t.as_ref().len() <= MAX_MATCH_TEXT).map(|t| t.as_ref().chars().map(fold).collect()).collect();
        self.alternatives.iter().any(|a| texts.iter().any(|t| run(&a.toks, t).0) != a.negate)
    }
}

fn parse_set(chars: &[char], mut i: usize) -> Result<(Tok, usize), FilterError> {
    let mut members = Vec::new();
    let mut closed = false;
    while let Some(&c) = chars.get(i) {
        i += 1;
        match c {
            ']' if members.is_empty() && chars.get(i..).is_some_and(|r| r.contains(&']')) => {
                members.push((']', true));
            }
            ']' => {
                closed = true;
                break;
            }
            '`' => match chars.get(i) {
                Some(&n) => {
                    i += 1;
                    members.push((fold(n), true));
                }
                None => return err("pattern ends with ` (escape) inside [ ]"),
            },
            other => members.push((fold(other), false)),
        }
    }
    if !closed {
        return err("[ without a closing ]");
    }
    let negate = members.first().is_some_and(|(c, escaped)| *c == '~' && !escaped) && members.len() > 1;
    let members = if negate { members.get(1..).unwrap_or_default() } else { &members[..] };
    if members.is_empty() {
        return err("[ ] holds no characters");
    }
    let mut items = Vec::new();
    let mut k = 0;
    while let Some(&(a, _)) = members.get(k) {
        match (members.get(k + 1), members.get(k + 2)) {
            (Some(('-', false)), Some(&(b, _))) => {
                if b < a {
                    return err(format!("range {a}-{b} in [ ] runs backwards"));
                }
                items.push((a, b));
                k += 3;
            }
            _ => {
                items.push((a, a));
                k += 1;
            }
        }
    }
    Ok((Tok::Set { negate, items }, i))
}

fn run(toks: &[Tok], text: &[char]) -> (bool, usize) {
    let mut steps = 0;
    let mut on = vec![false; toks.len() + 1];
    on[0] = true;
    close_stars(toks, &mut on);
    let mut next = vec![false; toks.len() + 1];
    for &c in text {
        steps += toks.len() + 1;
        next.iter_mut().for_each(|v| *v = false);
        let mut any = false;
        for (i, tok) in toks.iter().enumerate() {
            if !on[i] || !tok.accepts(c) {
                continue;
            }
            let to = if *tok == Tok::Star { i } else { i + 1 };
            next[to] = true;
            any = true;
        }
        if !any {
            return (false, steps);
        }
        std::mem::swap(&mut on, &mut next);
        close_stars(toks, &mut on);
    }
    (on[toks.len()], steps)
}

fn close_stars(toks: &[Tok], on: &mut [bool]) {
    for (i, tok) in toks.iter().enumerate() {
        if on[i] && *tok == Tok::Star {
            on[i + 1] = true;
        }
    }
}

/// Whether `text` matches the wildcard `pattern` (see [`Wildcard`]).
pub fn wildcard_match(pattern: &str, text: &str) -> Result<bool, FilterError> {
    Wildcard::parse(pattern).map(|w| w.matches(text))
}

/// One row of a property filter. Empty text and `None` set no condition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterRule {
    /// Layer name pattern.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frozen: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_vp_freeze: Option<bool>,
    /// Whether objects are on the layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<bool>,
    /// Colour pattern, matched against the colour number, its name, and for a true colour `r-g-b`
    /// (`10-20-*`) or `r,g,b` with each comma escaped (`10`,20`,*`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub color: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub linetype: String,
    /// Lineweight pattern, matched against `Default` and the width in millimetres (`0.25`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub lineweight: String,
    /// Transparency pattern, matched against the percentage.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub transparency: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub plot_style: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

struct CompiledRule<'a> {
    rule: &'a FilterRule,
    name: Option<Wildcard>,
    color: Option<Wildcard>,
    linetype: Option<Wildcard>,
    lineweight: Option<Wildcard>,
    transparency: Option<Wildcard>,
    plot_style: Option<Wildcard>,
    description: Option<Wildcard>,
}

fn field(label: &str, pattern: &str) -> Result<Option<Wildcard>, FilterError> {
    if pattern.is_empty() {
        return Ok(None);
    }
    Wildcard::parse(pattern).map(Some).map_err(|e| FilterError(format!("{label} `{pattern}`: {e}")))
}

fn flag(want: Option<bool>, have: bool) -> bool {
    want.is_none_or(|w| w == have)
}

fn color_texts(c: cadcraft_color::Color) -> Vec<String> {
    match c {
        cadcraft_color::Color::Index(i) => vec![i.to_string(), c.name()],
        cadcraft_color::Color::True(cadcraft_color::Rgb(r, g, b)) => vec![format!("{r},{g},{b}"), format!("{r}-{g}-{b}")],
        other => vec![other.name()],
    }
}

fn lineweight_texts(lw: crate::Lineweight) -> Vec<String> {
    match lw {
        crate::Lineweight::Mm100(v) => vec![format!("{:.2}", f64::from(v) / 100.0), lw.name()],
        other => vec![other.name()],
    }
}

impl FilterRule {
    /// Whether the rule sets no condition (it matches every layer).
    pub fn is_empty(&self) -> bool {
        *self == FilterRule::default()
    }

    /// Check every pattern of the rule.
    pub fn validate(&self) -> Result<(), FilterError> {
        self.compile().map(|_| ())
    }

    fn compile(&self) -> Result<CompiledRule<'_>, FilterError> {
        Ok(CompiledRule {
            rule: self,
            name: field("name", &self.name)?,
            color: field("color", &self.color)?,
            linetype: field("linetype", &self.linetype)?,
            lineweight: field("lineweight", &self.lineweight)?,
            transparency: field("transparency", &self.transparency)?,
            plot_style: field("plot style", &self.plot_style)?,
            description: field("description", &self.description)?,
        })
    }

    /// Whether `layer` matches the rule; `used` tells whether objects are on it.
    pub fn matches(&self, layer: &Layer, used: bool) -> Result<bool, FilterError> {
        self.compile().map(|c| c.matches(layer, used))
    }
}

impl CompiledRule<'_> {
    fn matches(&self, l: &Layer, used: bool) -> bool {
        let r = self.rule;
        let text = |w: &Option<Wildcard>, t: &str| w.as_ref().is_none_or(|w| w.matches(t));
        let any = |w: &Option<Wildcard>, ts: Vec<String>| w.as_ref().is_none_or(|w| w.matches_any(&ts));
        flag(r.on, l.on)
            && flag(r.frozen, l.frozen)
            && flag(r.locked, l.locked)
            && flag(r.plot, l.plot)
            && flag(r.new_vp_freeze, l.vp_freeze_new)
            && flag(r.used, used)
            && text(&self.name, &l.name)
            && text(&self.linetype, &l.linetype)
            && text(&self.plot_style, &l.plot_style)
            && text(&self.description, &l.description)
            && text(&self.transparency, &l.transparency.to_string())
            && any(&self.color, color_texts(l.color))
            && any(&self.lineweight, lineweight_texts(l.lineweight))
    }
}

/// What a filter selects by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilterKind {
    /// Layers matching any of the rules; no rules match every layer.
    Property {
        #[serde(default)]
        rules: Vec<FilterRule>,
    },
    /// The named layers.
    Group {
        #[serde(default)]
        layers: Vec<String>,
    },
}

/// A named layer filter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerFilter {
    pub name: String,
    /// The filter this one is nested under; `None` at the top of the tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(flatten)]
    pub kind: FilterKind,
}

impl LayerFilter {
    /// A property filter with the given rules.
    pub fn property(name: &str, rules: Vec<FilterRule>) -> Self {
        LayerFilter { name: name.into(), parent: None, kind: FilterKind::Property { rules } }
    }

    /// A group filter holding the given layers.
    pub fn group(name: &str, layers: Vec<String>) -> Self {
        LayerFilter { name: name.into(), parent: None, kind: FilterKind::Group { layers } }
    }

    pub fn is_group(&self) -> bool {
        matches!(self.kind, FilterKind::Group { .. })
    }
}

enum Compiled<'a> {
    Rules(Vec<CompiledRule<'a>>),
    Group(HashSet<String>),
}

impl Compiled<'_> {
    fn new(f: &LayerFilter) -> Result<Compiled<'_>, FilterError> {
        match &f.kind {
            FilterKind::Property { rules } => rules
                .iter()
                .map(FilterRule::compile)
                .collect::<Result<_, _>>()
                .map(Compiled::Rules)
                .map_err(|e| FilterError(format!("filter `{}`: {e}", f.name))),
            FilterKind::Group { layers } => Ok(Compiled::Group(layers.iter().map(|n| n.to_ascii_lowercase()).collect())),
        }
    }

    fn matches(&self, l: &Layer, used: bool) -> bool {
        match self {
            Compiled::Rules(rules) => rules.is_empty() || rules.iter().any(|r| r.matches(l, used)),
            Compiled::Group(names) => names.contains(&l.name.to_ascii_lowercase()),
        }
    }
}

/// The drawing's layer filters and which one the Layer Properties Manager shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayerFilters {
    pub filters: Vec<LayerFilter>,
    /// The filter shown: a filter's name, [`LayerFilters::ALL_USED`], or `None` for all layers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// Show the layers the current filter does not list.
    pub invert: bool,
    /// The current filter also limits the layer list of the Layers palette.
    pub apply_to_palette: bool,
}

impl LayerFilters {
    /// The tree's root entry: every layer.
    pub const ALL: &'static str = "All";
    /// The built-in filter of layers that have objects on them.
    pub const ALL_USED: &'static str = "All Used Layers";

    /// Whether the drawing keeps nothing here worth saving.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty() && self.current.is_none() && !self.invert && !self.apply_to_palette
    }

    /// Whether `name` is one of the built-in entries.
    pub fn is_reserved(name: &str) -> bool {
        name.trim().eq_ignore_ascii_case(Self::ALL) || name.trim().eq_ignore_ascii_case(Self::ALL_USED)
    }

    /// A name a new filter may take: not empty, at most 255 characters, none of `<>/\":;?*|,=``.
    pub fn valid_name(name: &str) -> Result<(), FilterError> {
        if name.trim().is_empty() {
            return err("a filter name cannot be empty");
        }
        if name.chars().count() > 255 {
            return err("a filter name has at most 255 characters");
        }
        if name.chars().any(|c| c.is_control() || "<>/\\\":;?*|,=`".contains(c)) {
            return err("a filter name cannot hold < > / \\ \" : ; ? * | , = or `");
        }
        if Self::is_reserved(name) {
            return err(format!("`{}` is a built-in filter", name.trim()));
        }
        Ok(())
    }

    /// The filter named `name` (without regard to case).
    pub fn get(&self, name: &str) -> Option<&LayerFilter> {
        self.filters.iter().find(|f| f.name.eq_ignore_ascii_case(name))
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut LayerFilter> {
        self.filters.iter_mut().find(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// The filters nested directly under `parent` (`None`: the top level), in the order kept.
    pub fn children<'a>(&'a self, parent: Option<&'a str>) -> impl Iterator<Item = &'a LayerFilter> + 'a {
        self.filters.iter().filter(move |f| match (&f.parent, parent) {
            (None, None) => true,
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (Some(a), None) => self.get(a).is_none(),
            (None, Some(_)) => false,
        })
    }

    /// `name` and every filter nested under it, at any depth.
    pub fn subtree(&self, name: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut todo = vec![name.to_string()];
        while let Some(n) = todo.pop() {
            if out.iter().any(|o| o.eq_ignore_ascii_case(&n)) || out.len() > MAX_FILTERS {
                continue;
            }
            todo.extend(self.children(Some(&n)).map(|f| f.name.clone()));
            out.push(n);
        }
        out
    }

    /// The filters from the top of the tree down to `name`.
    pub fn chain(&self, name: &str) -> Result<Vec<&LayerFilter>, FilterError> {
        let mut out: Vec<&LayerFilter> = Vec::new();
        let mut at = self.get(name).ok_or_else(|| FilterError(format!("no layer filter `{name}`")))?;
        loop {
            if out.iter().any(|f| f.name.eq_ignore_ascii_case(&at.name)) {
                return err(format!("layer filter `{name}` is nested in itself"));
            }
            if out.len() >= MAX_FILTER_DEPTH {
                return err(format!("layer filter `{name}` is nested more than {MAX_FILTER_DEPTH} deep"));
            }
            out.push(at);
            match at.parent.as_deref().and_then(|p| self.get(p)) {
                Some(p) => at = p,
                None => break,
            }
        }
        out.reverse();
        Ok(out)
    }

    /// Indices into `layers` of the layers the filter `name` lists (a built-in name, or `None`
    /// for all), or of those it does not list when `invert`. `used` tells whether objects are on a
    /// layer.
    pub fn select(&self, name: Option<&str>, invert: bool, layers: &[Layer], used: &dyn Fn(&Layer) -> bool) -> Result<Vec<usize>, FilterError> {
        let shown: Vec<bool> = match name {
            None => vec![!invert; layers.len()],
            Some(n) if n.trim().eq_ignore_ascii_case(Self::ALL) => vec![!invert; layers.len()],
            Some(n) if n.trim().eq_ignore_ascii_case(Self::ALL_USED) => layers.iter().map(|l| used(l) != invert).collect(),
            Some(n) => {
                let chain = self.chain(n)?;
                let compiled: Vec<Compiled<'_>> = chain.into_iter().map(Compiled::new).collect::<Result<_, _>>()?;
                let Some((own, parents)) = compiled.split_last() else { return err(format!("no layer filter `{n}`")) };
                layers
                    .iter()
                    .map(|l| {
                        let u = used(l);
                        parents.iter().all(|c| c.matches(l, u)) && own.matches(l, u) != invert
                    })
                    .collect()
            }
        };
        Ok(shown.into_iter().enumerate().filter(|(_, k)| *k).map(|(i, _)| i).collect())
    }

    /// Indices of the layers the current filter shows, as [`LayerFilters::select`].
    pub fn shown(&self, layers: &[Layer], used: &dyn Fn(&Layer) -> bool) -> Result<Vec<usize>, FilterError> {
        self.select(self.current.as_deref(), self.invert, layers, used)
    }

    /// Follow a layer rename in the group filters.
    pub fn rename_layer(&mut self, old: &str, new: &str) {
        for f in &mut self.filters {
            if let FilterKind::Group { layers } = &mut f.kind {
                for l in layers.iter_mut().filter(|l| l.eq_ignore_ascii_case(old)) {
                    *l = new.to_string();
                }
            }
        }
    }

    /// Drop deleted layers (`gone` takes a lower-case name) from the group filters.
    pub fn forget_layers(&mut self, gone: &HashSet<String>) {
        for f in &mut self.filters {
            if let FilterKind::Group { layers } = &mut f.kind {
                layers.retain(|l| !gone.contains(&l.to_ascii_lowercase()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_table() {
        let table: &[(&str, &str, bool)] = &[
            ("A-WALL*", "A-WALL", true),
            ("A-WALL*", "a-wall-new", true),
            ("A-WALL*", "X-A-WALL", false),
            ("*ANNO*", "A-ANNO-TEXT", true),
            ("*ANNO*", "anno", true),
            ("*ANNO*", "ANN-O", false),
            ("?", "", false),
            ("?", "a", true),
            ("??", "a", false),
            ("", "", true),
            ("", "a", false),
            ("*", "", true),
            ("*", "anything", true),
            ("L#", "L1", true),
            ("L#", "LA", false),
            ("L##", "L12", true),
            ("@1", "a1", true),
            ("@1", "Ä1", true),
            ("@1", "11", false),
            ("A.B", "A-B", true),
            ("A.B", "A B", true),
            ("A.B", "AxB", false),
            ("A.B", "A1B", false),
            ("[AB]1", "b1", true),
            ("[AB]1", "C1", false),
            ("[A-C]x", "Bx", true),
            ("[A-C]x", "Dx", false),
            ("[~A-C]x", "Dx", true),
            ("[~A-C]x", "bx", false),
            ("[-a]", "-", true),
            ("[a-]", "-", true),
            ("~A*", "B1", true),
            ("~A*", "A1", false),
            ("~", "~", true),
            ("~", "x", false),
            ("A*,B*", "B2", true),
            ("A*,B*", "C2", false),
            ("A*,~B*", "C2", true),
            ("`*", "*", true),
            ("`*", "x", false),
            ("a`,b", "a,b", true),
            ("[`]]", "]", true),
            ("[]a]", "]", true),
            ("[]a]", "a", true),
            ("[]a]", "b", false),
            ("[]a,]", "]", true),
            ("[]a,]", "a", true),
            ("[]a,]", ",", true),
            ("[]a,]", "b", false),
            ("[~],[a]", "~", true),
            ("[~],[a]", "a", true),
            ("[~],[a]", "b", false),
            ("[~],[a]", "]", false),
            ("[]],[a]", "]", true),
            ("[~]", "~", true),
            ("*a*a*a*a*a*b", &"a".repeat(60), false),
            ("Mauer*", "MAUER-Süd", true),
            ("*süd", "MAUER-SÜD", true),
        ];
        for (p, t, want) in table {
            assert_eq!(wildcard_match(p, t), Ok(*want), "pattern {p:?} on {t:?}");
        }
    }

    #[test]
    fn bad_patterns_say_why() {
        for p in ["[abc", "abc]", "x`", "[]", "[~a`", "[z-a]"] {
            let e = Wildcard::parse(p).expect_err(p);
            assert!(!e.0.is_empty(), "{p}");
        }
        assert!(Wildcard::parse(&"*".repeat(MAX_PATTERN + 1)).is_err());
    }

    #[test]
    fn long_text_and_many_stars_match_quickly() {
        let w = Wildcard::parse(&"*a".repeat(400)).unwrap();
        let text = "a".repeat(20_000);
        assert!(w.matches(&text));
        assert!(!w.matches(&format!("{text}b")));
        let chars: Vec<char> = text.chars().collect();
        let (hit, steps) = run(&w.alternatives[0].toks, &chars);
        assert!(hit);
        assert!(steps <= (chars.len() + 1) * (w.alternatives[0].toks.len() + 1));
        assert!(!Wildcard::parse("*").unwrap().matches(&"a".repeat(MAX_MATCH_TEXT + 1)));
    }

    fn layer(name: &str) -> Layer {
        Layer::new(name)
    }

    #[test]
    fn rule_conditions_are_all_required() {
        let mut l = layer("A-WALL-FULL");
        l.color = cadcraft_color::Color::Index(1);
        l.frozen = true;
        l.lineweight = crate::Lineweight::Mm100(25);
        let rule = |r: FilterRule| r.matches(&l, false).unwrap();
        assert!(rule(FilterRule::default()));
        assert!(rule(FilterRule { name: "a-wall*".into(), ..Default::default() }));
        assert!(rule(FilterRule { name: "A-WALL*".into(), frozen: Some(true), ..Default::default() }));
        assert!(!rule(FilterRule { name: "A-WALL*".into(), frozen: Some(false), ..Default::default() }));
        assert!(rule(FilterRule { color: "1".into(), ..Default::default() }));
        assert!(rule(FilterRule { color: "red".into(), ..Default::default() }));
        assert!(!rule(FilterRule { color: "2".into(), ..Default::default() }));
        assert!(rule(FilterRule { lineweight: "0.25".into(), ..Default::default() }));
        assert!(rule(FilterRule { linetype: "cont*".into(), on: Some(true), locked: Some(false), plot: Some(true), ..Default::default() }));
        assert!(!rule(FilterRule { used: Some(true), ..Default::default() }));
        assert!(rule(FilterRule { transparency: "0".into(), plot_style: "Normal".into(), ..Default::default() }));
        let mut t = layer("T");
        t.color = cadcraft_color::Color::True(cadcraft_color::Rgb(10, 20, 30));
        let rgb = |p: &str, l: &Layer| FilterRule { color: p.into(), ..Default::default() }.matches(l, false).unwrap();
        let mut o = layer("O");
        o.color = cadcraft_color::Color::True(cadcraft_color::Rgb(40, 50, 60));
        assert!(rgb("10-20-30", &t) && !rgb("10-20-30", &o));
        assert!(rgb("10-20-*", &t) && !rgb("10-20-*", &o));
        assert!(rgb("10`,20`,30", &t) && !rgb("10`,20`,30", &o));
        assert!(rgb("10`,20`,*", &t) && !rgb("10`,20`,*", &o));
        assert!(!rgb("10,20,30", &o) && !rgb("10,20,30", &t));
        let mut red = layer("R");
        red.color = cadcraft_color::Color::Index(1);
        assert!(rgb("red", &red) && rgb("1", &red) && !rgb("~red", &red) && !rgb("~1", &red));
        assert!(rgb("~red", &t));
        assert!(FilterRule { color: "[x".into(), ..Default::default() }.matches(&t, false).is_err());
    }

    fn names(fs: &LayerFilters, name: Option<&str>, invert: bool, layers: &[Layer]) -> Vec<String> {
        let used = |l: &Layer| l.name.starts_with('U');
        fs.select(name, invert, layers, &used).unwrap().into_iter().map(|i| layers[i].name.clone()).collect()
    }

    #[test]
    fn inverted_nested_filter_stays_inside_its_parent() {
        let layers: Vec<Layer> = ["A", "B", "C"].into_iter().map(layer).collect();
        let mut fs = LayerFilters::default();
        fs.filters.push(LayerFilter::group("P", vec!["A".into(), "B".into()]));
        let mut c = LayerFilter::group("C1", vec!["A".into()]);
        c.parent = Some("P".into());
        fs.filters.push(c);
        assert_eq!(names(&fs, Some("C1"), false, &layers), ["A"]);
        assert_eq!(names(&fs, Some("C1"), true, &layers), ["B"]);
        assert_eq!(names(&fs, Some("P"), true, &layers), ["C"]);
    }

    #[test]
    fn rules_or_filters_nest_and_invert() {
        let mut layers: Vec<Layer> = ["0", "A-WALL", "A-WALL-EXT", "A-ANNO", "U-ANNO-DIM", "S-COL"].into_iter().map(layer).collect();
        let mut fs = LayerFilters::default();
        fs.filters.push(LayerFilter::property("Walls", vec![FilterRule { name: "A-WALL*".into(), ..Default::default() }]));
        fs.filters.push(LayerFilter::property(
            "Anno or columns",
            vec![FilterRule { name: "*ANNO*".into(), ..Default::default() }, FilterRule { name: "S-*".into(), ..Default::default() }],
        ));
        let mut ext = LayerFilter::property("Exterior", vec![FilterRule { name: "*EXT".into(), ..Default::default() }]);
        ext.parent = Some("walls".into());
        fs.filters.push(ext);
        fs.filters.push(LayerFilter::group("Picked", vec!["0".into(), "a-anno".into(), "S-COL".into()]));
        let mut picked_anno = LayerFilter::property("Picked anno", vec![FilterRule { name: "*anno*".into(), ..Default::default() }]);
        picked_anno.parent = Some("Picked".into());
        fs.filters.push(picked_anno);
        assert_eq!(names(&fs, Some("Walls"), false, &layers), ["A-WALL", "A-WALL-EXT"]);
        assert_eq!(names(&fs, Some("Anno or columns"), false, &layers), ["A-ANNO", "U-ANNO-DIM", "S-COL"]);
        assert_eq!(names(&fs, Some("Exterior"), false, &layers), ["A-WALL-EXT"]);
        assert_eq!(names(&fs, Some("Picked"), false, &layers), ["0", "A-ANNO", "S-COL"]);
        assert_eq!(names(&fs, Some("Picked anno"), false, &layers), ["A-ANNO"]);
        assert_eq!(names(&fs, Some("Walls"), true, &layers), ["0", "A-ANNO", "U-ANNO-DIM", "S-COL"]);
        assert_eq!(names(&fs, None, false, &layers).len(), layers.len());
        assert_eq!(names(&fs, Some("All Used Layers"), false, &layers), ["U-ANNO-DIM"]);
        layers.push(layer("A-WALL-NEW"));
        assert_eq!(names(&fs, Some("Walls"), false, &layers), ["A-WALL", "A-WALL-EXT", "A-WALL-NEW"]);
        fs.rename_layer("S-COL", "S-COLUMN");
        layers[5].name = "S-COLUMN".into();
        assert_eq!(names(&fs, Some("Picked"), false, &layers), ["0", "A-ANNO", "S-COLUMN"]);
        fs.forget_layers(&HashSet::from(["0".to_string()]));
        assert_eq!(names(&fs, Some("Picked"), false, &layers), ["A-ANNO", "S-COLUMN"]);
        assert_eq!(fs.subtree("Walls"), ["Walls", "Exterior"]);
        assert_eq!(fs.children(None).map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Walls", "Anno or columns", "Picked"]);
    }

    #[test]
    fn hostile_trees_give_errors() {
        let mut fs = LayerFilters::default();
        let mut a = LayerFilter::group("A", Vec::new());
        a.parent = Some("B".into());
        let mut b = LayerFilter::group("B", Vec::new());
        b.parent = Some("A".into());
        fs.filters = vec![a, b];
        let layers = vec![layer("0")];
        assert!(fs.select(Some("A"), false, &layers, &|_| false).is_err());
        assert!(fs.select(Some("missing"), false, &layers, &|_| false).is_err());
        fs.filters = (0..200)
            .map(|i| LayerFilter {
                name: format!("F{i}"),
                parent: (i > 0).then(|| format!("F{}", i - 1)),
                kind: FilterKind::Group { layers: Vec::new() },
            })
            .collect();
        assert!(fs.select(Some("F199"), false, &layers, &|_| false).is_err());
        assert!(fs.select(Some("F10"), false, &layers, &|_| false).is_ok());
        let mut bad = LayerFilters::default();
        bad.filters.push(LayerFilter::property("Bad", vec![FilterRule { name: "[".into(), ..Default::default() }]));
        let e = bad.select(Some("Bad"), false, &layers, &|_| false).unwrap_err();
        assert!(e.0.contains("Bad") && e.0.contains("closing"), "{e}");
    }

    #[test]
    fn names_are_checked() {
        assert!(LayerFilters::valid_name("Walls").is_ok());
        for n in ["", "  ", "all", "All Used Layers", "a*b", "a,b"] {
            assert!(LayerFilters::valid_name(n).is_err(), "{n:?}");
        }
    }

    #[test]
    fn serde_keeps_everything() {
        let mut fs = LayerFilters { current: Some("Walls".into()), invert: true, apply_to_palette: true, ..Default::default() };
        fs.filters.push(LayerFilter::property("Walls", vec![FilterRule { name: "A-WALL*".into(), frozen: Some(false), ..Default::default() }]));
        let mut g = LayerFilter::group("G", vec!["0".into()]);
        g.parent = Some("Other".into());
        fs.filters.push(g);
        let text = serde_json::to_string(&fs).unwrap();
        assert_eq!(serde_json::from_str::<LayerFilters>(&text).unwrap(), fs);
    }
}
