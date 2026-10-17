//! AUDIT: find and repair errors in a drawing's database.
//!
//! [`check`] reports what is wrong; [`repair`] fixes it. Repairs never guess at content: a missing
//! layer or block definition is recreated with default properties (an empty block), a reference to
//! a missing style or linetype is reassigned to `Standard` or `ByLayer`, references to objects that
//! don't exist are dropped, objects whose geometry can't be drawn are removed, block definitions
//! that contain themselves lose the reference that closes the loop, and objects sharing a handle
//! get new ones.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Block, DimStyle, Drawing, Entity, EntityKind, EntityStore, Handle, Layer, Linetype, MLeaderStyle, TableStyle, TextStyle};
use serde::Serialize;
use serde_json::{Value, json};

/// One error found in the drawing.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// What is wrong, as a stable identifier (`missingLayer`, `invalidGeometry`, `blockCycle`…).
    pub kind: &'static str,
    /// Where: `header`, `tables`, `Model`, `layout <name>`, `block <name>`, `groups` or `constraints`.
    pub location: String,
    /// The object concerned, as a hex handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub message: String,
    /// What the repair does (or did).
    pub fix: String,
}

/// The result of an audit.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub issues: Vec<Issue>,
    /// Whether the issues were fixed.
    pub fixed: bool,
}

/// The most issue lines a report message lists; the rest are counted.
const MAX_LINES: usize = 100;

impl Report {
    pub fn errors(&self) -> usize {
        self.issues.len()
    }

    /// The command-line text: one line per issue (up to [`MAX_LINES`]), then the totals.
    pub fn message(&self) -> String {
        let mut lines: Vec<String> = self
            .issues
            .iter()
            .take(MAX_LINES)
            .map(|i| {
                let what = match &i.handle {
                    Some(h) => format!("{}: object {h}: {}", i.location, i.message),
                    None => format!("{}: {}", i.location, i.message),
                };
                if self.fixed { format!("{what} ({})", i.fix) } else { what }
            })
            .collect();
        if self.issues.len() > MAX_LINES {
            lines.push(format!("… and {} more", self.issues.len() - MAX_LINES));
        }
        let n = self.errors();
        let fixed = if self.fixed { n } else { 0 };
        lines.push(format!("Audit: {n} error{} found, {fixed} fixed.", if n == 1 { "" } else { "s" }));
        if n > 0 && !self.fixed {
            lines.push("Run AUDIT and answer Yes to fix them.".into());
        }
        lines.join("\n")
    }

    /// The programmatic result: `{errors, fixed, issues, message}`.
    pub fn to_json(&self) -> Value {
        json!({
            "errors": self.errors(),
            "fixed": if self.fixed { self.errors() } else { 0 },
            "issues": self.issues,
            "message": self.message(),
        })
    }

    /// Format this report for an AUDITCTL `.adt` log file.
    pub fn format_adt(&self, title: &str, path: Option<&str>) -> String {
        let name = path.unwrap_or(title);
        format!("Audit of drawing: {name}\n\n{}\n", self.message())
    }
}

/// Audit `d` without changing it.
pub fn check(d: &Drawing) -> Report {
    let mut copy = d.clone();
    Report { issues: repair(&mut copy), fixed: false }
}

/// Audit `d` and fix what is wrong in place. A drawing without errors is left untouched.
pub fn fix(d: &mut Drawing) -> Report {
    Report { issues: repair(d), fixed: true }
}

/// Where a set of objects lives.
#[derive(Clone, Debug)]
enum Loc {
    Model,
    Layout(usize),
    Block(String),
}

impl Loc {
    fn label(&self, d: &Drawing) -> String {
        match self {
            Loc::Model => "Model".into(),
            Loc::Layout(i) => format!("layout {}", d.layouts.get(*i).map(|l| l.name.as_str()).unwrap_or("?")),
            Loc::Block(n) => format!("block {n}"),
        }
    }
}

fn locations(d: &Drawing) -> Vec<Loc> {
    std::iter::once(Loc::Model).chain((0..d.layouts.len()).map(Loc::Layout)).chain(d.blocks.keys().cloned().map(Loc::Block)).collect()
}

fn store<'a>(d: &'a Drawing, l: &Loc) -> Option<&'a EntityStore> {
    match l {
        Loc::Model => Some(&d.model),
        Loc::Layout(i) => d.layouts.get(*i).map(|l| &l.entities),
        Loc::Block(n) => d.blocks.get(n).map(|b| &b.entities),
    }
}

fn store_mut<'a>(d: &'a mut Drawing, l: &Loc) -> Option<&'a mut EntityStore> {
    match l {
        Loc::Model => Some(&mut d.model),
        Loc::Layout(i) => d.layouts.get_mut(*i).map(|l| &mut l.entities),
        Loc::Block(n) => d.blocks.get_mut(n).map(|b| &mut Arc::make_mut(b).entities),
    }
}

/// A change to one object, applied after its store was read.
enum Action {
    Remove(Handle),
    Replace(Box<Entity>),
    /// Give the object (by its old handle) a new handle.
    Rehandle(Handle),
}

fn apply(d: &mut Drawing, loc: &Loc, actions: Vec<Action>) {
    if actions.is_empty() {
        return;
    }
    // New handles first: the store is borrowed below.
    let fresh: Vec<Handle> = actions.iter().filter(|a| matches!(a, Action::Rehandle(_))).map(|_| d.new_handle()).collect();
    let mut fresh = fresh.into_iter();
    let Some(st) = store_mut(d, loc) else { return };
    for a in actions {
        match a {
            Action::Remove(h) => {
                st.remove(h);
            }
            Action::Replace(e) => {
                st.replace(*e);
            }
            Action::Rehandle(h) => {
                if let (Some(e), Some(new)) = (st.remove(h), fresh.next()) {
                    let mut e = (*e).clone();
                    e.handle = new;
                    st.push(e);
                }
            }
        }
    }
}

fn issue(kind: &'static str, location: impl Into<String>, handle: Option<Handle>, message: String, fix: impl Into<String>) -> Issue {
    Issue { kind, location: location.into(), handle: handle.map(Handle::hex), message, fix: fix.into() }
}

/// Find every error in `d` and fix it; returns what was found. Nothing is changed when nothing is
/// wrong.
pub fn repair(d: &mut Drawing) -> Vec<Issue> {
    let mut out = Vec::new();
    tables(d, &mut out);
    header(d, &mut out);
    handseed(d, &mut out);
    objects(d, &mut out);
    missing_blocks(d, &mut out);
    references(d, &mut out);
    block_cycles(d, &mut out);
    let all = all_handles(d);
    groups(d, &all, &mut out);
    constraints(d, &all, &mut out);
    out
}

fn has_name<T>(items: &[T], name: &str, of: impl Fn(&T) -> &str) -> bool {
    items.iter().any(|t| of(t).eq_ignore_ascii_case(name))
}

/// The entries every drawing has, and references between table entries.
fn tables(d: &mut Drawing, out: &mut Vec<Issue>) {
    let missing = |what: &str, name: &str, out: &mut Vec<Issue>| {
        out.push(issue("missingTableEntry", "tables", None, format!("the {what} \"{name}\" is missing"), "recreated with default properties"));
    };
    if d.layer("0").is_none() {
        missing("layer", "0", out);
        d.layers.insert(0, Layer::default());
    }
    for name in ["ByBlock", "ByLayer"] {
        if d.linetype(name).is_none() {
            missing("linetype", name, out);
            d.linetypes.push(Linetype { name: name.into(), description: String::new(), pattern: Vec::new() });
        }
    }
    if d.linetype("Continuous").is_none() {
        missing("linetype", "Continuous", out);
        d.linetypes.push(Linetype::continuous());
    }
    if d.text_style("Standard").is_none() {
        missing("text style", "Standard", out);
        d.text_styles.insert(0, TextStyle::default());
    }
    if d.dim_style("Standard").is_none() {
        missing("dimension style", "Standard", out);
        d.dim_styles.insert(0, DimStyle::default());
    }
    if !has_name(&d.mleader_styles, "Standard", |s| &s.name) {
        missing("multileader style", "Standard", out);
        d.mleader_styles.insert(0, MLeaderStyle::default());
    }
    if !has_name(&d.table_styles, "Standard", |s| &s.name) {
        missing("table style", "Standard", out);
        d.table_styles.insert(0, TableStyle::default());
    }
    let linetypes: Vec<String> = d.linetypes.iter().map(|l| l.name.clone()).collect();
    for l in &mut d.layers {
        if !linetypes.iter().any(|n| n.eq_ignore_ascii_case(&l.linetype)) {
            out.push(issue(
                "missingLinetype",
                "tables",
                None,
                format!("layer \"{}\" uses linetype \"{}\", which doesn't exist", l.name, l.linetype),
                "set to Continuous",
            ));
            l.linetype = "Continuous".into();
        }
    }
    let styles: Vec<String> = d.text_styles.iter().map(|s| s.name.clone()).collect();
    let known = |n: &str| styles.iter().any(|s| s.eq_ignore_ascii_case(n));
    for s in &mut d.dim_styles {
        if !known(&s.text_style) {
            out.push(issue(
                "missingTextStyle",
                "tables",
                None,
                format!("dimension style \"{}\" uses text style \"{}\", which doesn't exist", s.name, s.text_style),
                "set to Standard",
            ));
            s.text_style = "Standard".into();
        }
    }
    for s in &mut d.mleader_styles {
        if !known(&s.text_style) {
            out.push(issue(
                "missingTextStyle",
                "tables",
                None,
                format!("multileader style \"{}\" uses text style \"{}\", which doesn't exist", s.name, s.text_style),
                "set to Standard",
            ));
            s.text_style = "Standard".into();
        }
    }
}

/// The current layer, linetype and style variables name entries that exist.
fn header(d: &mut Drawing, out: &mut Vec<Issue>) {
    let vars: [(&str, &str, &str); 6] = [
        ("CLAYER", "layer", "0"),
        ("CELTYPE", "linetype", "ByLayer"),
        ("TEXTSTYLE", "text style", "Standard"),
        ("DIMSTYLE", "dimension style", "Standard"),
        ("CMLEADERSTYLE", "multileader style", "Standard"),
        ("CTABLESTYLE", "table style", "Standard"),
    ];
    for (var, what, default) in vars {
        let Some(name) = d.header.get(var).map(|v| v.as_str().map(str::to_string)) else { continue };
        let exists = name.as_deref().is_some_and(|n| match var {
            "CLAYER" => d.layer(n).is_some(),
            "CELTYPE" => d.linetype(n).is_some(),
            "TEXTSTYLE" => d.text_style(n).is_some(),
            "DIMSTYLE" => d.dim_style(n).is_some(),
            "CMLEADERSTYLE" => has_name(&d.mleader_styles, n, |s| &s.name),
            _ => has_name(&d.table_styles, n, |s| &s.name),
        });
        if !exists {
            let shown = name.map(|n| format!("\"{n}\"")).unwrap_or_else(|| "a value that is not a name".into());
            out.push(issue(
                "missingCurrent",
                "header",
                None,
                format!("the current {what} ({var}) is {shown}, which doesn't exist"),
                format!("set to {default}"),
            ));
            d.header.set_str(var, default);
        }
    }
}

fn each_entity(d: &Drawing) -> impl Iterator<Item = &Arc<Entity>> {
    std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities)).flat_map(|s| s.iter())
}

fn all_handles(d: &Drawing) -> HashSet<Handle> {
    each_entity(d).map(|e| e.handle).collect()
}

/// The next handle is above every handle in use, so new objects never take an existing one.
fn handseed(d: &mut Drawing, out: &mut Vec<Issue>) {
    let Some(max) = each_entity(d).map(|e| e.handle).max() else { return };
    if max.0 >= d.handseed {
        out.push(issue(
            "handseed",
            "header",
            None,
            format!("the next handle ({:X}) is not above the largest one in use ({})", d.handseed, max.hex()),
            "moved past it",
        ));
        d.bump_handseed(max);
    }
}

/// Why `e`'s geometry is invalid, if it is.
fn invalid_geometry(e: &Entity) -> Option<&'static str> {
    if !crate::finite::is_finite(e) {
        return Some("has infinite or undefined numbers");
    }
    match &e.kind {
        EntityKind::Line(l) if l.a == l.b => Some("has zero length"),
        EntityKind::Circle(c) if c.radius <= 0.0 => Some("has no radius"),
        EntityKind::Arc(a) if a.radius <= 0.0 => Some("has no radius"),
        EntityKind::Ellipse(el) if el.major.len() <= 0.0 || el.ratio <= 0.0 => Some("has no size"),
        _ => None,
    }
}

/// Objects with invalid geometry are removed; handle 0 and handles used twice get new ones.
fn objects(d: &mut Drawing, out: &mut Vec<Issue>) {
    let mut seen: HashSet<Handle> = HashSet::new();
    for loc in locations(d) {
        let Some(st) = store(d, &loc) else { continue };
        let label = loc.label(d);
        let mut actions = Vec::new();
        for e in st.iter() {
            if let Some(why) = invalid_geometry(e) {
                out.push(issue("invalidGeometry", &label, Some(e.handle), format!("{} {why}", e.kind.type_name()), "removed"));
                actions.push(Action::Remove(e.handle));
            } else if e.handle.0 == 0 || !seen.insert(e.handle) {
                let why = if e.handle.0 == 0 { "has no handle" } else { "has the same handle as another object" };
                out.push(issue("duplicateHandle", &label, Some(e.handle), format!("{} {why}", e.kind.type_name()), "given a new handle"));
                actions.push(Action::Rehandle(e.handle));
            }
        }
        apply(d, &loc, actions);
    }
}

/// Block references whose block definition is missing get an empty definition, keeping the
/// references (and their attributes) until the block is redefined.
fn missing_blocks(d: &mut Drawing, out: &mut Vec<Issue>) {
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    for e in each_entity(d) {
        if let EntityKind::Insert(i) = &e.kind
            && d.block(&i.block).is_none()
        {
            *missing.entry(i.block.clone()).or_insert(0) += 1;
        }
    }
    for (name, n) in missing {
        out.push(issue(
            "missingBlock",
            "tables",
            None,
            format!("{n} block reference{} to \"{name}\", which has no definition", if n == 1 { "" } else { "s" }),
            "created an empty block definition",
        ));
        d.blocks.insert(name.clone(), Arc::new(Block::new(&name)));
    }
}

/// Layers objects are on that don't exist are created with default properties.
fn missing_layers(d: &mut Drawing, out: &mut Vec<Issue>) {
    // By folded name: a file may have very many layers.
    let mut known: HashSet<String> = d.layers.iter().map(|l| l.name.to_ascii_lowercase()).collect();
    let mut missing: Vec<String> = Vec::new();
    for e in each_entity(d) {
        let n = &e.common.layer;
        if !n.trim().is_empty() && known.insert(n.to_ascii_lowercase()) {
            missing.push(n.clone());
        }
    }
    for n in missing {
        out.push(issue(
            "missingLayer",
            "tables",
            None,
            format!("layer \"{n}\" is used by objects but doesn't exist"),
            "created with default properties",
        ));
        d.layers.push(Layer::new(&n));
    }
}

/// Missing layers, linetypes, styles, dimension blocks and associated objects referenced by objects.
fn references(d: &mut Drawing, out: &mut Vec<Issue>) {
    let all = all_handles(d);
    let ok_text = |d: &Drawing, n: &str| d.text_style(n).is_some();
    let ok_dim = |d: &Drawing, n: &str| d.dim_style(n).is_some();
    // Missing names, counted: (what, name) → (objects, first location).
    let mut counts: BTreeMap<(&'static str, String), (usize, String)> = BTreeMap::new();
    missing_layers(d, out);
    for loc in locations(d) {
        let Some(st) = store(d, &loc) else { continue };
        let label = loc.label(d);
        let mut actions = Vec::new();
        for e in st.iter() {
            let mut fixed = (**e).clone();
            let mut note = |what: &'static str, name: &str| {
                let c = counts.entry((what, name.to_string())).or_insert((0, label.clone()));
                c.0 += 1;
            };
            if fixed.common.layer.trim().is_empty() {
                note("layer", "");
                fixed.common.layer = "0".into();
            }
            let lt = &fixed.common.linetype;
            if !lt.eq_ignore_ascii_case("ByLayer") && !lt.eq_ignore_ascii_case("ByBlock") && d.linetype(lt).is_none() {
                note("linetype", lt);
                fixed.common.linetype = "ByLayer".into();
            }
            let mut text_style = |s: &mut String| {
                if !ok_text(d, s) {
                    note("text style", s);
                    *s = "Standard".into();
                }
            };
            match &mut fixed.kind {
                EntityKind::Text(t) => text_style(&mut t.style),
                EntityKind::MText(t) => text_style(&mut t.style),
                EntityKind::AttDef(a) => text_style(&mut a.text.style),
                EntityKind::Insert(i) => i.attribs.iter_mut().for_each(|a| text_style(&mut a.text.style)),
                EntityKind::MLeader(m) => {
                    if let Some(t) = &mut m.text {
                        text_style(&mut t.style);
                    }
                    if !has_name(&d.mleader_styles, &m.style, |s| &s.name) {
                        note("multileader style", &m.style);
                        m.style = "Standard".into();
                    }
                }
                EntityKind::Table(t) => {
                    if !has_name(&d.table_styles, &t.style, |s| &s.name) {
                        note("table style", &t.style);
                        t.style = "Standard".into();
                    }
                }
                EntityKind::Leader(l) => {
                    if !ok_dim(d, &l.style) {
                        note("dimension style", &l.style);
                        l.style = "Standard".into();
                    }
                }
                EntityKind::Dimension(dim) => {
                    if !ok_dim(d, &dim.style) {
                        note("dimension style", &dim.style);
                        dim.style = "Standard".into();
                    }
                    if let Some(b) = &dim.block
                        && d.block(b).is_none()
                    {
                        out.push(issue(
                            "missingBlock",
                            &label,
                            Some(e.handle),
                            format!("dimension's block \"{b}\" doesn't exist"),
                            "the dimension draws itself again",
                        ));
                        dim.block = None;
                    }
                    let before = dim.assoc.len();
                    dim.assoc.retain(|a| all.contains(&a.handle));
                    if dim.assoc.len() < before {
                        out.push(issue(
                            "danglingReference",
                            &label,
                            Some(e.handle),
                            "dimension is associated with an object that doesn't exist".into(),
                            "association removed",
                        ));
                    }
                }
                EntityKind::Viewport(vp) => {
                    let before = vp.frozen_layers.len() + vp.layer_colors.len();
                    vp.frozen_layers.retain(|n| d.layer(n).is_some());
                    vp.layer_colors.retain(|(n, _)| d.layer(n).is_some());
                    if vp.frozen_layers.len() + vp.layer_colors.len() < before {
                        out.push(issue(
                            "danglingReference",
                            &label,
                            Some(e.handle),
                            "viewport lists layer settings for layers that don't exist".into(),
                            "those entries removed",
                        ));
                    }
                    if let Some(b) = vp.clip
                        && (b == e.handle || !all.contains(&b))
                    {
                        vp.clip = None;
                        out.push(issue(
                            "danglingReference",
                            &label,
                            Some(e.handle),
                            "viewport's clip boundary doesn't exist".into(),
                            "the viewport is rectangular again",
                        ));
                    }
                }
                _ => {}
            }
            if fixed != **e {
                actions.push(Action::Replace(Box::new(fixed)));
            }
        }
        apply(d, &loc, actions);
    }
    for ((what, name), (n, first)) in counts {
        let (fix, shown) = match what {
            "layer" => ("put on layer 0".to_string(), "no name".to_string()),
            "linetype" => ("set to ByLayer".into(), format!("\"{name}\"")),
            _ => ("set to Standard".into(), format!("\"{name}\"")),
        };
        out.push(issue(
            "missingReference",
            first,
            None,
            format!("{n} object{} {what} {shown}, which doesn't exist", if n == 1 { " uses" } else { "s use" }),
            fix,
        ));
    }
}

/// Block definitions that contain themselves (directly or through other blocks) lose the
/// reference that closes the loop.
fn block_cycles(d: &mut Drawing, out: &mut Vec<Issue>) {
    let names: Vec<String> = d.blocks.keys().cloned().collect();
    // Names resolve as `Drawing::block` does: the exact name, else one differing only in case.
    let exact: HashMap<&str, usize> = names.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
    let folded: HashMap<String, usize> = names.iter().enumerate().rev().map(|(i, n)| (n.to_ascii_lowercase(), i)).collect();
    let lookup = |n: &str| exact.get(n).or_else(|| folded.get(&n.to_ascii_lowercase())).copied();
    let edges: Vec<Vec<(Handle, usize)>> = names
        .iter()
        .map(|n| {
            d.blocks
                .get(n)
                .map(|b| {
                    b.entities
                        .iter()
                        .filter_map(|e| match &e.kind {
                            EntityKind::Insert(i) => lookup(&i.block).map(|t| (e.handle, t)),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    // Iterative depth-first search: 0 unvisited, 1 on the current path, 2 done.
    let mut state = vec![0u8; names.len()];
    let mut cuts: Vec<(usize, Handle, usize)> = Vec::new();
    for root in 0..names.len() {
        if state.get(root) != Some(&0) {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        if let Some(s) = state.get_mut(root) {
            *s = 1;
        }
        while let Some((node, next)) = stack.last_mut() {
            let node = *node;
            match edges.get(node).and_then(|es| es.get(*next)) {
                Some(&(h, to)) => {
                    *next += 1;
                    match state.get(to) {
                        Some(1) => cuts.push((node, h, to)),
                        Some(0) => {
                            if let Some(s) = state.get_mut(to) {
                                *s = 1;
                            }
                            stack.push((to, 0));
                        }
                        _ => {}
                    }
                }
                None => {
                    if let Some(s) = state.get_mut(node) {
                        *s = 2;
                    }
                    stack.pop();
                }
            }
        }
    }
    for (from, h, to) in cuts {
        let (Some(from), Some(to)) = (names.get(from), names.get(to)) else { continue };
        let msg = if from == to {
            "block contains a reference to itself".to_string()
        } else {
            format!("reference to block \"{to}\" makes the block contain itself")
        };
        out.push(issue("blockCycle", format!("block {from}"), Some(h), msg, "reference removed"));
        apply(d, &Loc::Block(from.clone()), vec![Action::Remove(h)]);
    }
}

/// Groups list only objects that exist.
fn groups(d: &mut Drawing, all: &HashSet<Handle>, out: &mut Vec<Issue>) {
    for g in &mut d.groups {
        let before = g.members.len();
        g.members.retain(|h| all.contains(h));
        let gone = before - g.members.len();
        if gone > 0 {
            out.push(issue(
                "danglingReference",
                "groups",
                None,
                format!("group \"{}\" lists {gone} object{} exist", g.name, if gone == 1 { " that doesn't" } else { "s that don't" }),
                "removed from the group",
            ));
        }
    }
}

/// Constraints refer only to objects that exist.
fn constraints(d: &mut Drawing, all: &HashSet<Handle>, out: &mut Vec<Issue>) {
    let mut kept = Vec::with_capacity(d.constraints.len());
    for c in std::mem::take(&mut d.constraints) {
        match c.refs.iter().find(|r| !all.contains(&r.handle)) {
            Some(r) => out.push(issue(
                "danglingReference",
                "constraints",
                Some(r.handle),
                format!("constraint {} refers to an object that doesn't exist", c.id),
                "constraint removed",
            )),
            None => kept.push(c),
        }
    }
    d.constraints = kept;
}
