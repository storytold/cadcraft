//! Round-trip checks (gaps FF1 and FF5): open a drawing, save it, open the saved file again and
//! list everything that differs between the two drawings.
//!
//! [`compare`] works on CADCraft's own model ([`Drawing`]), so it measures what saving and
//! reopening lose or change: entities per type in every space and block (matched by handle,
//! geometry and properties within a tolerance), table records, blocks, layouts, the data CADCraft
//! keeps without modelling it (other programs' objects and entities, dictionaries, CLASS records,
//! extension dictionaries, reactors, xdata) and header variables. [`census`] counts what the
//! file itself holds, so opening a file can be checked against it too.
//!
//! The corpus harness ([`corpus`], native only) runs both over a folder of real files and writes
//! a Markdown and JSON report; `cadcraft-cli roundtrip` and `cargo xtask corpus` run it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadcraft_doc::foreign::{Foreign, ForeignExtra};
use cadcraft_doc::{Drawing, Entity, EntityKind, EntityStore, Handle, RawTag};
use serde::Serialize;
use serde_json::Value;

mod census;
#[cfg(not(target_arch = "wasm32"))]
pub mod corpus;

pub use census::{Census, census, census_of, compare_census};

/// What a difference is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// Opening the file: what the file holds that the drawing read from it doesn't.
    Read,
    /// Entities of a type in a space or block.
    Count,
    /// Entity, object or dictionary entry handles.
    Handle,
    /// An entity's geometry and type-specific data.
    Geometry,
    /// An entity's common properties (layer, colour, linetype, lineweight, transparency…).
    Properties,
    /// Entities CADCraft doesn't model, kept as their DXF groups.
    Opaque,
    /// Other applications' extended data.
    Xdata,
    /// Extension dictionaries and persistent reactors.
    ExtDict,
    /// Table records: layers, linetypes, text, dimension, multileader, table and multiline styles,
    /// views, viewport configurations, UCSs, layer states, groups, scales, plot style tables.
    Table,
    /// Block definitions (their own properties; their entities are under the other categories).
    Block,
    /// Layouts and their page setups.
    Layout,
    /// Non-graphical objects CADCraft doesn't model, kept as their DXF groups; constraints.
    Object,
    /// Entries of the named object dictionary.
    Dictionary,
    /// CLASS records.
    Class,
    /// Header variables.
    Header,
}

impl Category {
    pub const ALL: [Category; 15] = [
        Category::Read,
        Category::Count,
        Category::Handle,
        Category::Geometry,
        Category::Properties,
        Category::Opaque,
        Category::Xdata,
        Category::ExtDict,
        Category::Table,
        Category::Block,
        Category::Layout,
        Category::Object,
        Category::Dictionary,
        Category::Class,
        Category::Header,
    ];

    /// A short name for reports.
    pub fn label(self) -> &'static str {
        match self {
            Category::Read => "read",
            Category::Count => "entity count",
            Category::Handle => "handles",
            Category::Geometry => "geometry",
            Category::Properties => "properties",
            Category::Opaque => "unknown entities",
            Category::Xdata => "xdata",
            Category::ExtDict => "ext. dictionaries/reactors",
            Category::Table => "table records",
            Category::Block => "blocks",
            Category::Layout => "layouts",
            Category::Object => "objects",
            Category::Dictionary => "dictionary entries",
            Category::Class => "classes",
            Category::Header => "header",
        }
    }
}

/// Whether something went missing, appeared, or is there with other values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Change {
    Lost,
    Added,
    Changed,
}

impl Change {
    pub fn label(self) -> &'static str {
        match self {
            Change::Lost => "lost",
            Change::Added => "added",
            Change::Changed => "changed",
        }
    }
}

/// One difference between two drawings.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Difference {
    pub category: Category,
    pub change: Change,
    /// Where: `Model LINE 1A`, `Block DOOR`, `LAYER Walls`, `$LTSCALE`…
    pub place: String,
    /// What: the values before and after, or the items lost.
    pub detail: String,
    /// How many items this difference stands for (several entities lost of one type: one row).
    pub count: usize,
}

/// How many items were lost, added and changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Tally {
    pub lost: usize,
    pub added: usize,
    pub changed: usize,
}

impl Tally {
    pub fn total(&self) -> usize {
        self.lost.saturating_add(self.added).saturating_add(self.changed)
    }
    fn add(&mut self, change: Change, n: usize) {
        let slot = match change {
            Change::Lost => &mut self.lost,
            Change::Added => &mut self.added,
            Change::Changed => &mut self.changed,
        };
        *slot = slot.saturating_add(n);
    }
}

/// How two drawings are compared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Absolute tolerance for numbers (drawing units, radians…).
    pub abs_tol: f64,
    /// Relative tolerance for numbers, of the larger magnitude.
    pub rel_tol: f64,
    /// Most differences listed; the rest are only counted ([`Comparison::omitted`]).
    pub max_differences: usize,
    /// Most changed values named for one object.
    pub max_paths: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { abs_tol: 1e-6, rel_tol: 1e-9, max_differences: 5000, max_paths: 6 }
    }
}

/// Every difference found between two drawings.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Comparison {
    pub differences: Vec<Difference>,
    /// Differences found beyond [`Options::max_differences`] (counted in `totals`, not listed).
    pub omitted: usize,
    /// Items lost, added and changed per category.
    pub totals: BTreeMap<Category, Tally>,
}

impl Comparison {
    pub fn is_empty(&self) -> bool {
        self.totals.values().all(|t| t.total() == 0)
    }
    /// Items lost, added and changed in all categories.
    pub fn total(&self) -> Tally {
        let mut all = Tally::default();
        for t in self.totals.values() {
            all.add(Change::Lost, t.lost);
            all.add(Change::Added, t.added);
            all.add(Change::Changed, t.changed);
        }
        all
    }
    /// The tally of one category.
    pub fn tally(&self, c: Category) -> Tally {
        self.totals.get(&c).copied().unwrap_or_default()
    }
    /// Add another comparison's differences (within this one's limit).
    pub fn merge(&mut self, other: Comparison, max: usize) {
        for (c, t) in other.totals {
            let mine = self.totals.entry(c).or_default();
            mine.add(Change::Lost, t.lost);
            mine.add(Change::Added, t.added);
            mine.add(Change::Changed, t.changed);
        }
        for d in other.differences {
            if self.differences.len() < max {
                self.differences.push(d);
            } else {
                self.omitted = self.omitted.saturating_add(1);
            }
        }
        self.omitted = self.omitted.saturating_add(other.omitted);
    }
}

/// Collects differences within the limit.
struct Out<'o> {
    opts: &'o Options,
    cmp: Comparison,
    /// Anonymous blocks saved under another name (`*D344` → `*D10`), upper case: references
    /// to them compare equal.
    renamed: HashMap<String, String>,
    /// Things that only got new handles, summed up in one row each: (what) → (how many, an
    /// example).
    renumbered: BTreeMap<String, (usize, String)>,
    /// Anonymous blocks of the first drawing that nothing refers to and that the second one
    /// doesn't have (saving drops them, as PURGE would), upper case: reported in one row.
    orphans: BTreeSet<String>,
}

/// What values are compared with: the tolerances and the renamed anonymous blocks.
struct Norm<'a> {
    o: &'a Options,
    renamed: &'a HashMap<String, String>,
}

impl Norm<'_> {
    /// Equal strings, or an anonymous block's name and the name it was saved under.
    fn same_str(&self, a: &str, b: &str) -> bool {
        a == b || (a.starts_with('*') && self.renamed.get(&a.to_ascii_uppercase()).is_some_and(|n| n.eq_ignore_ascii_case(b)))
    }
}

impl<'o> Out<'o> {
    fn new(opts: &'o Options) -> Self {
        Out { opts, cmp: Comparison::default(), renamed: HashMap::new(), renumbered: BTreeMap::new(), orphans: BTreeSet::new() }
    }
    fn norm(&self) -> Norm<'_> {
        Norm { o: self.opts, renamed: &self.renamed }
    }
    /// Note that one of `what` only got a new handle.
    fn renumber(&mut self, what: &str, example: impl FnOnce() -> String) {
        match self.renumbered.get_mut(what) {
            Some(slot) => slot.0 = slot.0.saturating_add(1),
            None => {
                self.renumbered.insert(what.to_string(), (1, example()));
            }
        }
    }
    /// The rows of [`Out::renumber`].
    fn flush_renumbered(&mut self) {
        for (what, (n, example)) in std::mem::take(&mut self.renumbered) {
            self.push_n(Category::Handle, Change::Changed, n, what.clone(), format!("{n} {what} got new handles (e.g. {example})"));
        }
    }
    fn push(&mut self, category: Category, change: Change, place: impl Into<String>, detail: impl Into<String>) {
        self.push_n(category, change, 1, place, detail);
    }
    fn push_n(&mut self, category: Category, change: Change, count: usize, place: impl Into<String>, detail: impl Into<String>) {
        self.cmp.totals.entry(category).or_default().add(change, count);
        if self.cmp.differences.len() < self.opts.max_differences {
            self.cmp.differences.push(Difference { category, change, place: place.into(), detail: detail.into(), count });
        } else {
            self.cmp.omitted = self.cmp.omitted.saturating_add(1);
        }
    }
}

/// Header variables that change on every save (time stamps, the handle seed, GUIDs): not
/// compared.
pub const VOLATILE_HEADER: &[&str] =
    &["HANDSEED", "TDCREATE", "TDUCREATE", "TDUPDATE", "TDUUPDATE", "TDINDWG", "TDUSRTIMER", "VERSIONGUID", "FINGERPRINTGUID"];

/// Compare `before` (the drawing as first opened) with `after` (the same drawing saved and
/// reopened). Differences are reported as what `after` lost, gained or changed.
pub fn compare(before: &Drawing, after: &Drawing, opts: &Options) -> Comparison {
    let mut out = Out::new(opts);
    compare_entities(before, after, &mut out);
    compare_tables(before, after, &mut out);
    compare_foreign(&before.foreign, &after.foreign, &mut out);
    compare_header(before, after, &mut out);
    out.flush_renumbered();
    out.cmp
}

// ---------------------------------------------------------------------------------------------
// Values

fn close(a: f64, b: f64, o: &Options) -> bool {
    a == b || (a - b).abs() <= o.abs_tol + o.rel_tol * a.abs().max(b.abs())
}

/// The changed values of one object, as `path: before → after`.
struct Paths {
    list: Vec<String>,
    cap: usize,
    more: usize,
}

impl Paths {
    fn new(cap: usize) -> Self {
        Paths { list: Vec::new(), cap: cap.max(1), more: 0 }
    }
    fn add(&mut self, s: String) {
        if self.list.len() < self.cap {
            self.list.push(s);
        } else {
            self.more = self.more.saturating_add(1);
        }
    }
    fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
    /// Far more changes than can be listed: stop looking.
    fn saturated(&self) -> bool {
        self.more > 1000
    }
    fn text(&self) -> String {
        let mut s = self.list.join("; ");
        if self.more > 0 {
            s.push_str(&format!("; and {} more", if self.saturated() { "many".to_string() } else { self.more.to_string() }));
        }
        s
    }
}

/// A value for a report: strings quoted, at most 60 characters.
fn short(v: &Value) -> String {
    let s = match v {
        Value::String(s) => format!("\"{s}\""),
        Value::Array(a) => format!("[{} items]", a.len()),
        Value::Object(_) => "{…}".to_string(),
        other => other.to_string(),
    };
    clip(&s, 60)
}

/// `s` cut to `n` characters, with an ellipsis when cut.
pub(crate) fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn show(path: &str) -> &str {
    if path.is_empty() { "value" } else { path }
}

const MAX_DEPTH: usize = 64;

/// Whether a value's path names an angle (in radians in the model): `rotation`, `start`,
/// `end`, `angle`, `twist`, `oblique`.
fn is_angle(path: &str) -> bool {
    let last = path.rsplit('.').next().unwrap_or(path);
    let last = last.split('[').next().unwrap_or(last).to_ascii_lowercase();
    ["rotation", "start", "end", "angle", "twist", "oblique"].iter().any(|k| last.ends_with(k))
}

/// The same angle, a whole number of turns apart (-π/4 and 7π/4).
fn same_angle(a: f64, b: f64, o: &Options) -> bool {
    let d = (a - b).rem_euclid(std::f64::consts::TAU);
    let tol = o.abs_tol + o.rel_tol * a.abs().max(b.abs());
    d <= tol || std::f64::consts::TAU - d <= tol
}

fn diff_values(a: &Value, b: &Value, path: &mut String, n: &Norm, p: &mut Paths, depth: usize) {
    if p.saturated() {
        return;
    }
    if depth > MAX_DEPTH {
        if a != b {
            p.add(format!("{}: differs", show(path)));
        }
        return;
    }
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let same = match (x.as_i64(), y.as_i64()) {
                (Some(i), Some(j)) => i == j,
                _ => match (x.as_f64(), y.as_f64()) {
                    (Some(f), Some(g)) => close(f, g, n.o) || (is_angle(path) && same_angle(f, g, n.o)),
                    _ => x == y,
                },
            };
            if !same {
                p.add(format!("{}: {x} → {y}", show(path)));
            }
        }
        (Value::String(x), Value::String(y)) => {
            if !n.same_str(x, y) {
                p.add(format!("{}: {} → {}", show(path), short(a), short(b)));
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                let len = path.len();
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(k);
                match (x.get(k.as_str()), y.get(k.as_str())) {
                    (Some(u), Some(v)) => diff_values(u, v, path, n, p, depth + 1),
                    (Some(Value::Null), None) | (None, Some(Value::Null)) | (None, None) => {}
                    (Some(u), None) => p.add(format!("{}: {} → (none)", show(path), short(u))),
                    (None, Some(v)) => p.add(format!("{}: (none) → {}", show(path), short(v))),
                }
                path.truncate(len);
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                p.add(format!("{}: {} items → {}", show(path), x.len(), y.len()));
            }
            for (i, (u, v)) in x.iter().zip(y).enumerate() {
                if p.saturated() {
                    return;
                }
                let len = path.len();
                path.push_str(&format!("[{i}]"));
                diff_values(u, v, path, n, p, depth + 1);
                path.truncate(len);
            }
        }
        _ if a == b => {}
        _ => p.add(format!("{}: {} → {}", show(path), short(a), short(b))),
    }
}

/// The values that differ between two serializable things.
fn diff_ser<T: Serialize + PartialEq>(a: &T, b: &T, n: &Norm) -> Paths {
    let mut p = Paths::new(n.o.max_paths);
    match (serde_json::to_value(a), serde_json::to_value(b)) {
        (Ok(x), Ok(y)) => diff_values(&x, &y, &mut String::new(), n, &mut p, 0),
        _ if a != b => p.add("differs (not comparable field by field)".into()),
        _ => {}
    }
    p
}

/// A key for matching things that may have been renumbered: the value with numbers rounded.
fn signature(v: &Value, out: &mut String, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.is_finite() => {
                let r = if f.abs() < 1e-9 { 0.0 } else { f };
                out.push_str(&format!("{r:.6e}"));
            }
            _ => out.push_str(&n.to_string()),
        },
        Value::Array(a) => {
            out.push('[');
            for x in a {
                signature(x, out, depth + 1);
                out.push(',');
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (k, x) in m {
                out.push_str(k);
                out.push(':');
                signature(x, out, depth + 1);
                out.push(',');
            }
            out.push('}');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn sig_of<T: Serialize>(t: &T) -> String {
    let mut s = String::new();
    if let Ok(v) = serde_json::to_value(t) {
        signature(&v, &mut s, 0);
    }
    s
}

/// Pair up the items of `a` and `b` that weren't matched by handle: equal signatures first, then
/// in order. Returns the pairs and the items left over on each side.
fn pair_up<'x, T>(a: Vec<&'x T>, b: Vec<&'x T>, sig: impl Fn(&T) -> String) -> (Vec<(&'x T, &'x T)>, Vec<&'x T>, Vec<&'x T>) {
    let mut by_sig: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, y) in b.iter().enumerate().rev() {
        by_sig.entry(sig(y)).or_default().push(i);
    }
    let mut used = vec![false; b.len()];
    let mut pairs = Vec::new();
    let mut rest_a = Vec::new();
    for x in a {
        match by_sig.get_mut(&sig(x)).and_then(Vec::pop) {
            Some(i) => {
                if let (Some(y), Some(u)) = (b.get(i), used.get_mut(i)) {
                    *u = true;
                    pairs.push((x, *y));
                }
            }
            None => rest_a.push(x),
        }
    }
    let mut rest_b: Vec<&T> = b.iter().zip(&used).filter(|(_, u)| !**u).map(|(y, _)| *y).collect();
    let n = rest_a.len().min(rest_b.len());
    let tail_a = rest_a.split_off(n);
    let tail_b = rest_b.split_off(n);
    pairs.extend(rest_a.into_iter().zip(rest_b));
    (pairs, tail_a, tail_b)
}

fn hex_list(hs: impl Iterator<Item = Handle>, n: usize) -> String {
    let all: Vec<String> = hs.map(Handle::hex).collect();
    let mut s = all.iter().take(n).cloned().collect::<Vec<_>>().join(", ");
    if all.len() > n {
        s.push_str(", …");
    }
    s
}

/// Group codes that hold handles (DXF Reference: 5, 105, 320–369, 390–399, 480–481, xdata
/// 1005). A file's objects can be renumbered on save (gap FF7), so a handle compares equal to
/// any other handle; only a reference that became null (or stopped being null) counts.
fn is_handle_code(code: i32) -> bool {
    matches!(code, 5 | 105 | 320..=369 | 390..=399 | 480 | 481 | 1005)
}

fn null_handle(v: &str) -> bool {
    v.trim().trim_start_matches('0').is_empty()
}

/// Whether two groups hold the same value: the same text, numbers within the tolerance, or two
/// (non-null) handles.
fn tag_eq(a: &RawTag, b: &RawTag, o: &Options) -> bool {
    if a.code != b.code {
        return false;
    }
    if is_handle_code(a.code) {
        return null_handle(&a.value) == null_handle(&b.value);
    }
    a.value == b.value
        || a.value.trim() == b.value.trim()
        || matches!((a.value.trim().parse::<f64>(), b.value.trim().parse::<f64>()), (Ok(x), Ok(y)) if close(x, y, o))
}

/// How two group lists differ, or `None` when they hold the same values.
fn tags_diff(a: &[RawTag], b: &[RawTag], o: &Options) -> Option<String> {
    let first = a.iter().zip(b).position(|(x, y)| !tag_eq(x, y, o));
    if first.is_none() && a.len() == b.len() {
        return None;
    }
    let mut s = String::new();
    if a.len() != b.len() {
        s.push_str(&format!("{} groups → {}; ", a.len(), b.len()));
    }
    let at = first.unwrap_or(a.len().min(b.len()));
    let show_tag = |t: Option<&RawTag>| t.map_or("(end)".to_string(), |t| format!("{} `{}`", t.code, clip(&t.value, 40)));
    s.push_str(&format!("first difference at group #{at}: {} → {}", show_tag(a.get(at)), show_tag(b.get(at))));
    Some(s)
}

// ---------------------------------------------------------------------------------------------
// Entities

/// An entity collection of a drawing: model space, a layout's paper space or a block. `key` is
/// upper case (`BLOCK *D12`), `name` for reports (`Block *D12`).
struct Container<'d> {
    key: String,
    name: String,
    store: &'d EntityStore,
}

fn containers(d: &Drawing) -> Vec<Container<'_>> {
    let mut v = vec![Container { key: "MODEL".into(), name: "Model".into(), store: &d.model }];
    for l in &d.layouts {
        v.push(Container { key: format!("LAYOUT {}", l.name.to_ascii_uppercase()), name: format!("Layout {}", l.name), store: &l.entities });
    }
    for (name, b) in &d.blocks {
        v.push(Container { key: format!("BLOCK {}", name.to_ascii_uppercase()), name: format!("Block {name}"), store: &b.entities });
    }
    v
}

/// Which container each entity is in.
fn locate(cs: &[Container]) -> HashMap<Handle, usize> {
    cs.iter().enumerate().flat_map(|(i, c)| c.store.iter().map(move |e| (e.handle, i))).collect()
}

/// Anonymous blocks of `a` whose entities (by handle) are, at least half of them, in another
/// anonymous block of `b`: saving renamed them (`*D344` → `*D10`). Upper-case names.
fn anonymous_renames(ca: &[Container], cb: &[Container], where_b: &HashMap<Handle, usize>) -> HashMap<String, String> {
    let mut renamed = HashMap::new();
    let mut taken: HashSet<usize> = HashSet::new();
    for c in ca {
        let Some(name) = c.key.strip_prefix("BLOCK ").filter(|n| n.starts_with('*')) else { continue };
        if c.store.is_empty() {
            continue;
        }
        let mut votes: BTreeMap<usize, usize> = BTreeMap::new();
        for e in c.store.iter() {
            if let Some(j) = where_b.get(&e.handle) {
                *votes.entry(*j).or_default() += 1;
            }
        }
        let best = votes.into_iter().max_by(|x, y| x.1.cmp(&y.1).then(y.0.cmp(&x.0)));
        let Some((j, n)) = best else { continue };
        let Some(target) = cb.get(j).and_then(|t| t.key.strip_prefix("BLOCK ")) else { continue };
        if n.saturating_mul(2) >= c.store.len() && target != name && target.starts_with('*') && taken.insert(j) {
            renamed.insert(name.to_string(), target.to_string());
        }
    }
    renamed
}

struct Ctx<'d> {
    a: &'d Drawing,
    b: &'d Drawing,
    cb: &'d [Container<'d>],
    where_a: &'d HashMap<Handle, usize>,
    where_b: &'d HashMap<Handle, usize>,
}

fn compare_entities(a: &Drawing, b: &Drawing, out: &mut Out) {
    let ca = containers(a);
    let cb = containers(b);
    let where_a = locate(&ca);
    let where_b = locate(&cb);
    out.renamed = anonymous_renames(&ca, &cb, &where_b);
    let index_b: HashMap<&str, usize> = cb.iter().enumerate().map(|(i, c)| (c.key.as_str(), i)).collect();
    let target = |c: &Container| match c.key.strip_prefix("BLOCK ").and_then(|n| out.renamed.get(n)) {
        Some(n) => index_b.get(format!("BLOCK {n}").as_str()).copied(),
        None => index_b.get(c.key.as_str()).copied(),
    };
    let targets: Vec<Option<usize>> = ca.iter().map(target).collect();
    // A block renamed onto the name another block had: that one has no counterpart by name.
    let renamed_onto: HashSet<usize> = ca
        .iter()
        .zip(&targets)
        .filter(|(c, _)| c.key.strip_prefix("BLOCK ").is_some_and(|n| out.renamed.contains_key(n)))
        .filter_map(|(_, t)| *t)
        .collect();
    let ctx = Ctx { a, b, cb: &cb, where_a: &where_a, where_b: &where_b };
    let empty = EntityStore::new();
    let mut matched: HashSet<usize> = HashSet::new();
    let mut moves: BTreeMap<(String, String), Vec<Handle>> = BTreeMap::new();
    let referenced = referenced_blocks(&ca);
    let mut orphan_entities = 0usize;
    for (c, t) in ca.iter().zip(&targets) {
        let renamed = c.key.strip_prefix("BLOCK ").is_some_and(|n| out.renamed.contains_key(n));
        let j = t.filter(|j| renamed || !renamed_onto.contains(j)).filter(|j| matched.insert(*j));
        if j.is_none()
            && let Some(name) = c.key.strip_prefix("BLOCK ").filter(|n| n.starts_with('*') && !referenced.contains(*n))
            && c.store.iter().all(|e| !where_b.contains_key(&e.handle))
        {
            out.orphans.insert(name.to_string());
            orphan_entities = orphan_entities.saturating_add(c.store.len());
            continue;
        }
        let sb = j.and_then(|j| cb.get(j)).map_or(&empty, |x| x.store);
        compare_store(&c.name, c.store, sb, &ctx, &mut moves, out);
    }
    if let Some(first) = out.orphans.first().cloned() {
        let n = out.orphans.len();
        let detail = format!("{orphan_entities} entities of {n} anonymous blocks nothing refers to (e.g. {first}) were not saved");
        out.push_n(Category::Count, Change::Lost, orphan_entities, "unreferenced anonymous blocks", detail);
    }
    for (j, c) in cb.iter().enumerate() {
        if !matched.contains(&j) {
            compare_store(&c.name, &empty, c.store, &ctx, &mut moves, out);
        }
    }
    for ((from, to), hs) in moves {
        let detail = format!("{} entities moved from {from} to {to} ({})", hs.len(), hex_list(hs.iter().copied(), 6));
        out.push_n(Category::Handle, Change::Changed, hs.len(), from, detail);
    }
}

/// The blocks a drawing's entities refer to (block references, dimensions, multileaders), upper
/// case.
fn referenced_blocks(cs: &[Container]) -> HashSet<String> {
    let mut out = HashSet::new();
    for e in cs.iter().flat_map(|c| c.store.iter()) {
        match &e.kind {
            EntityKind::Insert(i) => {
                out.insert(i.block.to_ascii_uppercase());
            }
            EntityKind::Dimension(d) => out.extend(d.block.as_ref().map(|b| b.to_ascii_uppercase())),
            EntityKind::MLeader(m) => out.extend(m.block.as_ref().map(|b| b.block.to_ascii_uppercase())),
            _ => {}
        }
    }
    out
}

fn kind_sig(e: &Entity) -> String {
    sig_of(&e.kind)
}

fn compare_store(name: &str, sa: &EntityStore, sb: &EntityStore, ctx: &Ctx, moves: &mut BTreeMap<(String, String), Vec<Handle>>, out: &mut Out) {
    let ib: HashMap<Handle, &Entity> = sb.iter().map(|e| (e.handle, e.as_ref())).collect();
    let ia: HashSet<Handle> = sa.iter().map(|e| e.handle).collect();
    let mut count_a: BTreeMap<String, usize> = BTreeMap::new();
    let mut count_b: BTreeMap<String, usize> = BTreeMap::new();
    let mut un_a: BTreeMap<String, Vec<&Entity>> = BTreeMap::new();
    let mut un_b: BTreeMap<String, Vec<&Entity>> = BTreeMap::new();
    for e in sa.iter() {
        let t = e.kind.dxf_name().to_string();
        *count_a.entry(t.clone()).or_default() += 1;
        if let Some(f) = ib.get(&e.handle) {
            compare_entity(name, e, f, ctx, out);
        } else if let Some(other) = ctx.where_b.get(&e.handle).and_then(|j| ctx.cb.get(*j)) {
            moves.entry((name.to_string(), other.name.clone())).or_default().push(e.handle);
            if let Some(f) = other.store.get(e.handle) {
                compare_entity(name, e, f, ctx, out);
            }
        } else {
            un_a.entry(t).or_default().push(e);
        }
    }
    for e in sb.iter() {
        let t = e.kind.dxf_name().to_string();
        *count_b.entry(t.clone()).or_default() += 1;
        if !ia.contains(&e.handle) && !ctx.where_a.contains_key(&e.handle) {
            un_b.entry(t).or_default().push(e);
        }
    }
    let types: BTreeSet<String> = un_a.keys().chain(un_b.keys()).cloned().collect();
    for t in types {
        let xs = un_a.remove(&t).unwrap_or_default();
        let ys = un_b.remove(&t).unwrap_or_default();
        let (pairs, lost, added) = pair_up(xs, ys, kind_sig);
        for (x, y) in &pairs {
            out.renumber(&format!("{t} entities"), || format!("{} → {} in {name}", x.handle.hex(), y.handle.hex()));
            compare_entity(name, x, y, ctx, out);
        }
        let (na, nb) = (count_a.get(&t).copied().unwrap_or(0), count_b.get(&t).copied().unwrap_or(0));
        if !lost.is_empty() {
            let detail = format!("{t}: {na} → {nb} ({} lost: {})", lost.len(), hex_list(lost.iter().map(|e| e.handle), 8));
            out.push_n(Category::Count, Change::Lost, lost.len(), format!("{name} {t}"), detail);
        }
        if !added.is_empty() {
            let detail = format!("{t}: {na} → {nb} ({} added: {})", added.len(), hex_list(added.iter().map(|e| e.handle), 8));
            out.push_n(Category::Count, Change::Added, added.len(), format!("{name} {t}"), detail);
        }
    }
}

fn compare_entity(container: &str, x: &Entity, y: &Entity, ctx: &Ctx, out: &mut Out) {
    let handles = if x.handle == y.handle { x.handle.hex() } else { format!("{}→{}", x.handle.hex(), y.handle.hex()) };
    let place = format!("{container} {} {handles}", x.kind.dxf_name());
    if std::mem::discriminant(&x.kind) != std::mem::discriminant(&y.kind) || x.kind.dxf_name() != y.kind.dxf_name() {
        out.push(Category::Geometry, Change::Changed, place, format!("type {} → {}", x.kind.dxf_name(), y.kind.dxf_name()));
        return;
    }
    match (&x.kind, &y.kind) {
        (EntityKind::Unknown(u), EntityKind::Unknown(v)) => {
            if let Some(d) = tags_diff(&u.tags, &v.tags, out.opts) {
                out.push(Category::Opaque, Change::Changed, place.clone(), d);
            }
        }
        _ => {
            let p = diff_ser(&x.kind, &y.kind, &out.norm());
            if !p.is_empty() {
                out.push(Category::Geometry, Change::Changed, place.clone(), p.text());
            }
        }
    }
    let common = |e: &Entity| {
        let mut v = serde_json::to_value(&e.common).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut v {
            m.remove("xdata");
        }
        v
    };
    let mut p = Paths::new(out.opts.max_paths);
    diff_values(&common(x), &common(y), &mut String::new(), &out.norm(), &mut p, 0);
    if !p.is_empty() {
        out.push(Category::Properties, Change::Changed, place.clone(), p.text());
    }
    compare_xdata(&place, &x.common.xdata, &y.common.xdata, out);
    compare_extra(&place, ctx.a.foreign.entities.get(&x.handle), ctx.b.foreign.entities.get(&y.handle), out);
}

/// Extended data split per application (from each 1001 group).
fn xdata_apps(tags: &[RawTag]) -> Vec<(String, &[RawTag])> {
    let starts: Vec<usize> = tags.iter().enumerate().filter(|(_, t)| t.code == 1001).map(|(i, _)| i).collect();
    let mut v = Vec::new();
    for (k, &s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(tags.len());
        let name = tags.get(s).map(|t| t.value.trim().to_ascii_uppercase()).unwrap_or_default();
        v.push((name, tags.get(s..end).unwrap_or(&[])));
    }
    if starts.first().is_some_and(|&s| s > 0) || (starts.is_empty() && !tags.is_empty()) {
        let end = starts.first().copied().unwrap_or(tags.len());
        v.insert(0, (String::new(), tags.get(..end).unwrap_or(&[])));
    }
    v
}

fn compare_xdata(place: &str, a: &[RawTag], b: &[RawTag], out: &mut Out) {
    if a.is_empty() && b.is_empty() {
        return;
    }
    let (xa, xb) = (xdata_apps(a), xdata_apps(b));
    let mb: HashMap<&str, &[RawTag]> = xb.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    let ma: HashSet<&str> = xa.iter().map(|(n, _)| n.as_str()).collect();
    for (name, ta) in &xa {
        let label = if name.is_empty() { "(no application)" } else { name };
        match mb.get(name.as_str()) {
            None => out.push(Category::Xdata, Change::Lost, place, format!("application {label} ({} groups)", ta.len())),
            Some(tb) => {
                if let Some(d) = tags_diff(ta, tb, out.opts) {
                    out.push(Category::Xdata, Change::Changed, place, format!("application {label}: {d}"));
                }
            }
        }
    }
    for (name, tb) in &xb {
        if !ma.contains(name.as_str()) {
            let label = if name.is_empty() { "(no application)" } else { name };
            out.push(Category::Xdata, Change::Added, place, format!("application {label} ({} groups)", tb.len()));
        }
    }
}

fn compare_extra(place: &str, a: Option<&ForeignExtra>, b: Option<&ForeignExtra>, out: &mut Out) {
    let none = ForeignExtra::default();
    let (a, b) = (a.unwrap_or(&none), b.unwrap_or(&none));
    if a == b {
        return;
    }
    match (a.xdict, b.xdict) {
        (Some(h), None) => out.push(Category::ExtDict, Change::Lost, place, format!("extension dictionary {}", h.hex())),
        (None, Some(h)) => out.push(Category::ExtDict, Change::Added, place, format!("extension dictionary {}", h.hex())),
        (Some(h), Some(k)) if h != k => out.renumber("extension dictionaries", || format!("{} → {} of {place}", h.hex(), k.hex())),
        _ => {}
    }
    if a.reactors.len() > b.reactors.len() {
        let n = a.reactors.len() - b.reactors.len();
        out.push_n(Category::ExtDict, Change::Lost, n, place, format!("reactors: {} → {}", a.reactors.len(), b.reactors.len()));
    } else if b.reactors.len() > a.reactors.len() {
        let n = b.reactors.len() - a.reactors.len();
        out.push_n(Category::ExtDict, Change::Added, n, place, format!("reactors: {} → {}", a.reactors.len(), b.reactors.len()));
    }
    compare_xdata(place, &a.xdata, &b.xdata, out);
    if a.color_book != b.color_book {
        let show = |c: &Option<(String, String)>| c.as_ref().map_or("(none)".to_string(), |(book, tc)| format!("`{}` ({tc})", clip(book, 40)));
        let change = match (&a.color_book, &b.color_book) {
            (Some(_), None) => Change::Lost,
            (None, Some(_)) => Change::Added,
            _ => Change::Changed,
        };
        out.push(Category::Properties, change, place, format!("colour book colour {} → {}", show(&a.color_book), show(&b.color_book)));
    }
    if let Some(d) = tags_diff(&a.props, &b.props, out.opts) {
        out.push(Category::Properties, Change::Changed, place, format!("other property groups (material, shadows…): {d}"));
    }
}

// ---------------------------------------------------------------------------------------------
// Tables, blocks, layouts

/// Compare two lists of named records, matched by name without case.
fn compare_named<T: Serialize + PartialEq>(out: &mut Out, cat: Category, kind: &str, a: &[&T], b: &[&T], name: impl Fn(&T) -> &str) {
    let mut mb: HashMap<String, &T> = HashMap::new();
    for y in b.iter().rev() {
        mb.insert(name(y).to_ascii_uppercase(), *y);
    }
    let mut seen: HashSet<String> = HashSet::new();
    for x in a {
        let key = name(x).to_ascii_uppercase();
        if !seen.insert(key.clone()) {
            continue;
        }
        let place = format!("{kind} {}", name(x));
        match mb.get(&key) {
            None => out.push(cat, Change::Lost, place, String::new()),
            Some(y) => {
                let p = diff_ser(*x, *y, &out.norm());
                if !p.is_empty() {
                    out.push(cat, Change::Changed, place, p.text());
                }
            }
        }
    }
    let mut seen_b: HashSet<String> = HashSet::new();
    for y in b {
        let key = name(y).to_ascii_uppercase();
        if !seen.contains(&key) && seen_b.insert(key) {
            out.push(cat, Change::Added, format!("{kind} {}", name(y)), String::new());
        }
    }
}

fn refs<T>(v: &[T]) -> Vec<&T> {
    v.iter().collect()
}

fn compare_tables(a: &Drawing, b: &Drawing, out: &mut Out) {
    use Category::{Block, Layout, Table};
    compare_named(out, Table, "LAYER", &refs(&a.layers), &refs(&b.layers), |x| &x.name);
    compare_named(out, Table, "LTYPE", &refs(&a.linetypes), &refs(&b.linetypes), |x| &x.name);
    compare_named(out, Table, "STYLE", &refs(&a.text_styles), &refs(&b.text_styles), |x| &x.name);
    compare_named(out, Table, "DIMSTYLE", &refs(&a.dim_styles), &refs(&b.dim_styles), |x| &x.name);
    compare_named(out, Table, "MLEADERSTYLE", &refs(&a.mleader_styles), &refs(&b.mleader_styles), |x| &x.name);
    compare_named(out, Table, "TABLESTYLE", &refs(&a.table_styles), &refs(&b.table_styles), |x| &x.name);
    compare_named(out, Table, "MLINESTYLE", &refs(&a.mline_styles), &refs(&b.mline_styles), |x| &x.name);
    compare_named(out, Table, "VIEW", &refs(&a.views), &refs(&b.views), |x| &x.name);
    compare_named(out, Table, "VPORT", &refs(&a.vports), &refs(&b.vports), |x| &x.name);
    compare_named(out, Table, "UCS", &refs(&a.ucss), &refs(&b.ucss), |x| &x.name);
    compare_named(out, Table, "LAYERSTATE", &refs(&a.layer_states), &refs(&b.layer_states), |x| &x.name);
    compare_named(out, Table, "GROUP", &refs(&a.groups), &refs(&b.groups), |x| &x.name);
    compare_named(out, Table, "SCALE", &refs(&a.scales), &refs(&b.scales), |x| &x.name);
    compare_named(out, Table, "PLOTSTYLE", &refs(&a.plot_style_tables), &refs(&b.plot_style_tables), |x| &x.name);
    // Blocks: renamed anonymous blocks are compared with what they were saved as.
    let renamed_to: HashSet<String> = out.renamed.values().cloned().collect();
    let ba: Vec<&cadcraft_doc::Block> = a
        .blocks
        .values()
        .map(|x| x.as_ref())
        .filter(|x| {
            let up = x.name.to_ascii_uppercase();
            !out.renamed.contains_key(&up) && !out.orphans.contains(&up)
        })
        .collect();
    if let Some(first) = out.orphans.first().cloned() {
        let n = out.orphans.len();
        out.push_n(
            Block,
            Change::Lost,
            n,
            "unreferenced anonymous blocks",
            format!("{n} anonymous blocks nothing refers to (e.g. {first}) were not saved"),
        );
    }
    let bb: Vec<&cadcraft_doc::Block> =
        b.blocks.values().map(|x| x.as_ref()).filter(|x| !renamed_to.contains(&x.name.to_ascii_uppercase())).collect();
    let mut pairs: Vec<(&cadcraft_doc::Block, &cadcraft_doc::Block)> = Vec::new();
    for (from, to) in &out.renamed {
        if let (Some(x), Some(y)) = (a.block(from), b.block(to)) {
            pairs.push((x.as_ref(), y.as_ref()));
        }
    }
    compare_named(out, Block, "BLOCK", &ba, &bb, |x| &x.name);
    pairs.sort_by(|x, y| x.0.name.cmp(&y.0.name));
    if let Some((x, y)) = pairs.first() {
        let detail = format!("{} anonymous blocks saved under new names (e.g. {} → {})", pairs.len(), x.name, y.name);
        out.push_n(Block, Change::Changed, pairs.len(), "anonymous blocks", detail);
    }
    for (x, y) in pairs {
        let p = diff_ser(x, y, &out.norm());
        if !p.is_empty() {
            out.push(Block, Change::Changed, format!("BLOCK {} → {}", x.name, y.name), p.text());
        }
    }
    compare_named(out, Layout, "LAYOUT", &refs(&a.layouts), &refs(&b.layouts), |x| &x.name);
    for (place, p) in
        [("CONSTRAINTS", diff_ser(&a.constraints, &b.constraints, &out.norm())), ("PARAMETERS", diff_ser(&a.parametric, &b.parametric, &out.norm()))]
    {
        if !p.is_empty() {
            out.push(Category::Object, Change::Changed, place, p.text());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Data kept without modelling it

fn class_name(c: &[RawTag]) -> String {
    c.iter().find(|t| t.code == 1).map(|t| t.value.trim().to_string()).unwrap_or_default()
}

/// A CLASS record without its instance count (group 91), which follows the objects written.
fn class_groups(c: &[RawTag]) -> Vec<RawTag> {
    c.iter().filter(|t| t.code != 91).cloned().collect()
}

fn compare_foreign(a: &Foreign, b: &Foreign, out: &mut Out) {
    let o = out.opts;
    type Obj = cadcraft_doc::foreign::ForeignObject;
    // Objects, by handle, then by content.
    let ib: HashMap<Handle, &Obj> = b.objects.iter().map(|x| (x.handle, x)).collect();
    let ia: HashSet<Handle> = a.objects.iter().map(|x| x.handle).collect();
    let mut un_a: BTreeMap<String, Vec<&Obj>> = BTreeMap::new();
    let mut un_b: BTreeMap<String, Vec<&Obj>> = BTreeMap::new();
    for x in &a.objects {
        match ib.get(&x.handle) {
            Some(y) if y.kind != x.kind => {
                out.push(Category::Object, Change::Changed, format!("{} {}", x.kind, x.handle.hex()), format!("type {} → {}", x.kind, y.kind))
            }
            Some(y) => {
                if let Some(d) = tags_diff(&x.tags, &y.tags, o) {
                    out.push(Category::Object, Change::Changed, format!("{} {}", x.kind, x.handle.hex()), d);
                }
            }
            None => un_a.entry(x.kind.clone()).or_default().push(x),
        }
    }
    for y in &b.objects {
        if !ia.contains(&y.handle) {
            un_b.entry(y.kind.clone()).or_default().push(y);
        }
    }
    // Content without handles, so renumbered objects pair up.
    let content = |x: &Obj| sig_of(&x.tags.iter().filter(|t| !is_handle_code(t.code)).map(|t| (t.code, &t.value)).collect::<Vec<_>>());
    let kinds: BTreeSet<String> = un_a.keys().chain(un_b.keys()).cloned().collect();
    for k in kinds {
        let xs = un_a.remove(&k).unwrap_or_default();
        let ys = un_b.remove(&k).unwrap_or_default();
        let (pairs, lost, added) = pair_up(xs, ys, content);
        for (x, y) in &pairs {
            out.renumber(&format!("{k} objects"), || format!("{} → {}", x.handle.hex(), y.handle.hex()));
            if let Some(d) = tags_diff(&x.tags, &y.tags, o) {
                out.push(Category::Object, Change::Changed, format!("{k} {}→{}", x.handle.hex(), y.handle.hex()), d);
            }
        }
        if !lost.is_empty() {
            out.push_n(
                Category::Object,
                Change::Lost,
                lost.len(),
                format!("objects {k}"),
                format!("{} lost: {}", lost.len(), hex_list(lost.iter().map(|x| x.handle), 8)),
            );
        }
        if !added.is_empty() {
            out.push_n(
                Category::Object,
                Change::Added,
                added.len(),
                format!("objects {k}"),
                format!("{} added: {}", added.len(), hex_list(added.iter().map(|x| x.handle), 8)),
            );
        }
    }
    // Named object dictionary entries.
    let db: HashMap<String, Handle> = b.dictionary.iter().map(|(k, h)| (k.to_ascii_uppercase(), *h)).collect();
    let da: HashSet<String> = a.dictionary.iter().map(|(k, _)| k.to_ascii_uppercase()).collect();
    for (k, h) in &a.dictionary {
        match db.get(&k.to_ascii_uppercase()) {
            None => out.push(Category::Dictionary, Change::Lost, format!("NOD {k}"), format!("entry for {}", h.hex())),
            Some(g) if g != h => out.renumber("dictionary entries", || format!("{k}: {} → {}", h.hex(), g.hex())),
            Some(_) => {}
        }
    }
    for (k, h) in &b.dictionary {
        if !da.contains(&k.to_ascii_uppercase()) {
            out.push(Category::Dictionary, Change::Added, format!("NOD {k}"), format!("entry for {}", h.hex()));
        }
    }
    // CLASS records.
    let cb: HashMap<String, &Vec<RawTag>> = b.classes.iter().map(|c| (class_name(c).to_ascii_uppercase(), c)).collect();
    let ca: HashSet<String> = a.classes.iter().map(|c| class_name(c).to_ascii_uppercase()).collect();
    for c in &a.classes {
        let n = class_name(c);
        match cb.get(&n.to_ascii_uppercase()) {
            None => out.push(Category::Class, Change::Lost, format!("CLASS {n}"), String::new()),
            Some(d) => {
                if let Some(t) = tags_diff(&class_groups(c), &class_groups(d), o) {
                    out.push(Category::Class, Change::Changed, format!("CLASS {n}"), t);
                }
            }
        }
    }
    for c in &b.classes {
        let n = class_name(c);
        if !ca.contains(&n.to_ascii_uppercase()) {
            out.push(Category::Class, Change::Added, format!("CLASS {n}"), String::new());
        }
    }
    // Extras of table records, tables and blocks (renamed anonymous blocks by their new name).
    let key_of = |k: &(String, String)| -> (String, String) {
        match out.renamed.get(&k.1.to_ascii_uppercase()) {
            Some(n) if k.0.starts_with("BLOCK") => (k.0.clone(), n.clone()),
            _ => k.clone(),
        }
    };
    let ra: BTreeMap<(String, String), &ForeignExtra> = a.records.iter().map(|(k, v)| (key_of(k), v)).collect();
    let rb: BTreeMap<(String, String), &ForeignExtra> = b.records.iter().map(|(k, v)| ((k.0.clone(), k.1.to_ascii_uppercase()), v)).collect();
    let ra: BTreeMap<(String, String), &ForeignExtra> = ra.into_iter().map(|((k0, k1), v)| ((k0, k1.to_ascii_uppercase()), v)).collect();
    let keys: BTreeSet<&(String, String)> = ra.keys().chain(rb.keys()).collect();
    for k in keys {
        compare_extra(&format!("{} {}", k.0, k.1), ra.get(k).copied(), rb.get(k).copied(), out);
    }
    // The data store (ACIS data of 2013+ files).
    if let Some(d) = tags_diff(&a.data_store, &b.data_store, o) {
        let change = match (a.data_store.is_empty(), b.data_store.is_empty()) {
            (false, true) => Change::Lost,
            (true, false) => Change::Added,
            _ => Change::Changed,
        };
        out.push(Category::Object, change, "ACDSDATA", d);
    }
}

// ---------------------------------------------------------------------------------------------
// Header

fn compare_header(a: &Drawing, b: &Drawing, out: &mut Out) {
    let skip = |k: &str| VOLATILE_HEADER.contains(&k);
    for (k, va) in &a.header.vars {
        if skip(k) {
            continue;
        }
        let place = format!("${k}");
        match b.header.vars.get(k) {
            None => out.push(Category::Header, Change::Lost, place, va.display()),
            Some(vb) => {
                let p = diff_ser(va, vb, &out.norm());
                if !p.is_empty() {
                    out.push(Category::Header, Change::Changed, place, format!("{} → {}", clip(&va.display(), 60), clip(&vb.display(), 60)));
                }
            }
        }
    }
    for (k, vb) in &b.header.vars {
        if !skip(k) && !a.header.vars.contains_key(k) {
            out.push(Category::Header, Change::Added, format!("${k}"), vb.display());
        }
    }
}

#[cfg(test)]
mod tests;
