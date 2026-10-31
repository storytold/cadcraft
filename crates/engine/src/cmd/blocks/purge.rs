//! PURGE / -PURGE: remove unused named objects (blocks, layers, linetypes, styles, multiline
//! styles, groups) and zero-length geometry or empty text.
//!
//! The JSON form takes `{type?, names?}` and purges without asking. Typed, it asks for the type,
//! the names and whether to verify each one, as AutoCAD's -PURGE does.

use std::collections::{HashMap, HashSet, VecDeque};

use cadcraft_doc::{Drawing, EntityKind, Handle, STANDARD_MLINE_STYLE};
use serde_json::{Map, Value, json};

use super::super::qselect::wildcard;
use super::super::{bad, find_command, str_param};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// The named objects PURGE can remove. `USAGE` runs the same scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cmd) enum Kind {
    Blocks,
    Dimstyles,
    Groups,
    Layers,
    Linetypes,
    MleaderStyles,
    MlineStyles,
    TableStyles,
    TextStyles,
    ZeroLength,
    EmptyText,
    /// An object type CADCraft does not have: nothing to purge.
    Absent,
    All,
}

/// Prompt keyword, JSON names (normalised: lower case, letters and digits only), kind.
const KINDS: &[(&str, &[&str], Kind)] = &[
    ("Blocks", &["blocks", "block"], Kind::Blocks),
    ("DEtailviewstyles", &["detailviewstyles"], Kind::Absent),
    ("Dimstyles", &["dimstyles", "dimstyle"], Kind::Dimstyles),
    ("Groups", &["groups", "group"], Kind::Groups),
    ("LAyers", &["layers", "layer"], Kind::Layers),
    ("LTypes", &["ltypes", "linetypes", "linetype"], Kind::Linetypes),
    ("MAterials", &["materials"], Kind::Absent),
    ("MUltileaderstyles", &["multileaderstyles", "mleaderstyles", "mleaderstyle"], Kind::MleaderStyles),
    ("Plotstyles", &["plotstyles"], Kind::Absent),
    ("SHapes", &["shapes"], Kind::Absent),
    ("textSTyles", &["textstyles", "textstyle", "styles", "style"], Kind::TextStyles),
    ("Mlinestyles", &["mlinestyles", "mlinestyle"], Kind::MlineStyles),
    ("SEctionviewstyles", &["sectionviewstyles"], Kind::Absent),
    ("Tablestyles", &["tablestyles", "tablestyle"], Kind::TableStyles),
    ("Visualstyles", &["visualstyles"], Kind::Absent),
    ("Regapps", &["regapps"], Kind::Absent),
    ("Zero-length geometry", &["zerolengthgeometry", "zerolength"], Kind::ZeroLength),
    ("Empty text objects", &["emptytextobjects", "emptytext"], Kind::EmptyText),
    ("Orphaned data", &["orphaneddata", "orphaned"], Kind::Absent),
    ("All", &["all"], Kind::All),
];

/// The named kinds `All` purges, in an order where purging one frees the next (blocks free
/// layers, layers free linetypes, dimension styles free text styles).
const ALL: &[Kind] = &[
    Kind::Blocks,
    Kind::Groups,
    Kind::Dimstyles,
    Kind::MleaderStyles,
    Kind::MlineStyles,
    Kind::TableStyles,
    Kind::Layers,
    Kind::Linetypes,
    Kind::TextStyles,
];

fn norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

fn kind_of(name: &str) -> Option<(&'static str, Kind)> {
    let n = norm(name);
    KINDS.iter().find(|(k, alias, _)| norm(k) == n || alias.contains(&n.as_str())).map(|(k, _, kind)| (*k, *kind))
}

/// Singular noun for messages.
fn noun(kind: Kind) -> &'static str {
    match kind {
        Kind::Blocks => "block",
        Kind::Dimstyles => "dimension style",
        Kind::Groups => "group",
        Kind::Layers => "layer",
        Kind::Linetypes => "linetype",
        Kind::MleaderStyles => "multileader style",
        Kind::MlineStyles => "multiline style",
        Kind::TableStyles => "table style",
        Kind::TextStyles => "text style",
        Kind::ZeroLength => "zero-length object",
        Kind::EmptyText => "empty text object",
        Kind::Absent | Kind::All => "object",
    }
}

fn json_key(kind: Kind) -> &'static str {
    match kind {
        Kind::Blocks => "blocks",
        Kind::Dimstyles => "dimstyles",
        Kind::Groups => "groups",
        Kind::Layers => "layers",
        Kind::Linetypes => "linetypes",
        Kind::MleaderStyles => "mleaderstyles",
        Kind::MlineStyles => "mlinestyles",
        Kind::TableStyles => "tablestyles",
        Kind::TextStyles => "textstyles",
        Kind::ZeroLength => "zeroLength",
        Kind::EmptyText => "emptyText",
        Kind::Absent | Kind::All => "other",
    }
}

fn entities(d: &Drawing) -> impl Iterator<Item = &std::sync::Arc<cadcraft_doc::Entity>> {
    d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).chain(d.blocks.values().flat_map(|b| b.entities.iter())).take(5_000_000)
}

/// The arrowhead block names a dimension's overrides set.
pub(in crate::cmd) fn override_arrows(dm: &cadcraft_doc::Dimension) -> impl Iterator<Item = String> + '_ {
    dm.overrides
        .iter()
        .filter(|(k, _)| cadcraft_doc::DimStyle::field_name(k).is_some_and(|f| f.starts_with("arrowBlock") || f == "leaderArrowBlock"))
        .filter_map(|(_, v)| v.as_str().map(|n| n.trim().to_string()))
        .filter(|n| !n.is_empty())
}

/// Upper-case names of blocks reachable from model space, the layouts and dimensions, and the
/// arrowhead blocks dimension styles and dimension overrides name.
pub(in crate::cmd) fn used_blocks(d: &Drawing) -> HashSet<String> {
    let mut used = HashSet::new();
    let mut frontier: Vec<String> = Vec::new();
    for st in &d.dim_styles {
        frontier.extend(
            [&st.arrow_block, &st.arrow_block1, &st.arrow_block2, &st.leader_arrow_block]
                .into_iter()
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty()),
        );
    }
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        match &e.kind {
            EntityKind::Insert(i) => frontier.push(i.block.clone()),
            EntityKind::Dimension(dm) => {
                frontier.extend(dm.block.clone());
                frontier.extend(override_arrows(dm));
            }
            EntityKind::MLeader(m) => frontier.extend(m.block.as_ref().map(|b| b.block.clone())),
            _ => {}
        }
    }
    let mut guard = 0;
    while let Some(n) = frontier.pop() {
        guard += 1;
        if guard > 100_000 || !used.insert(n.to_ascii_uppercase()) {
            continue;
        }
        if let Some(b) = d.block(&n) {
            for e in b.entities.iter() {
                match &e.kind {
                    EntityKind::Insert(i) => frontier.push(i.block.clone()),
                    EntityKind::Dimension(dm) => {
                        frontier.extend(dm.block.clone());
                        frontier.extend(override_arrows(dm));
                    }
                    EntityKind::MLeader(m) => frontier.extend(m.block.as_ref().map(|b| b.block.clone())),
                    _ => {}
                }
            }
        }
    }
    used
}

fn is(name: &str, list: &[&str]) -> bool {
    list.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// Names of the unused objects of a named kind, in table order. This is PURGE's scan; `USAGE`
/// reads it too, without purging anything.
pub(in crate::cmd) fn candidates(d: &Drawing, kind: Kind) -> Vec<String> {
    let lower = |it: &mut dyn Iterator<Item = String>| it.map(|n| n.to_ascii_lowercase()).collect::<HashSet<String>>();
    match kind {
        Kind::Blocks => {
            let used = used_blocks(d);
            d.blocks.values().filter(|b| !used.contains(&b.name.to_ascii_uppercase())).map(|b| b.name.clone()).collect()
        }
        Kind::Layers => {
            let used = super::super::layer::used_layers(d);
            let cur = d.header.str("CLAYER", "0");
            d.layers
                .iter()
                .filter(|l| !is(&l.name, &["0", "Defpoints", &cur]) && !used.contains(&l.name.to_ascii_lowercase()))
                .map(|l| l.name.clone())
                .collect()
        }
        Kind::Linetypes => {
            let used = lower(&mut entities(d).map(|e| e.common.linetype.clone()).chain(d.layers.iter().map(|l| l.linetype.clone())));
            let cur = d.header.str("CELTYPE", "ByLayer");
            d.linetypes
                .iter()
                .filter(|t| !is(&t.name, &["ByBlock", "ByLayer", "Continuous", &cur]) && !used.contains(&t.name.to_ascii_lowercase()))
                .map(|t| t.name.clone())
                .collect()
        }
        Kind::TextStyles => {
            let used = lower(&mut super::super::props::text_styles_in_use(d).into_iter());
            let cur = d.header.str("TEXTSTYLE", "Standard");
            d.text_styles
                .iter()
                .filter(|t| !is(&t.name, &["Standard", &cur]) && !used.contains(&t.name.to_ascii_lowercase()))
                .map(|t| t.name.clone())
                .collect()
        }
        Kind::Dimstyles => {
            let used = lower(&mut entities(d).filter_map(|e| match &e.kind {
                EntityKind::Dimension(dm) => Some(dm.style.clone()),
                EntityKind::Tolerance(t) => Some(t.style.clone()),
                _ => None,
            }));
            let cur = d.header.str("DIMSTYLE", "Standard");
            d.dim_styles
                .iter()
                .filter(|t| !is(&t.name, &["Standard", &cur]) && !used.contains(&t.name.to_ascii_lowercase()))
                .map(|t| t.name.clone())
                .collect()
        }
        Kind::MleaderStyles => {
            let used = lower(&mut entities(d).filter_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m.style.clone()) } else { None }));
            let cur = d.header.str("CMLEADERSTYLE", "Standard");
            d.mleader_styles
                .iter()
                .filter(|t| !is(&t.name, &["Standard", &cur]) && !used.contains(&t.name.to_ascii_lowercase()))
                .map(|t| t.name.clone())
                .collect()
        }
        Kind::MlineStyles => {
            let used = lower(&mut entities(d).filter_map(|e| if let EntityKind::MLine(m) = &e.kind { Some(m.style.clone()) } else { None }));
            let cur = d.header.str("CMLSTYLE", STANDARD_MLINE_STYLE);
            d.mline_styles
                .iter()
                .filter(|t| !is(&t.name, &[STANDARD_MLINE_STYLE, &cur]) && !used.contains(&t.name.to_ascii_lowercase()))
                .map(|t| t.name.clone())
                .collect()
        }
        Kind::TableStyles => {
            let used = lower(&mut entities(d).filter_map(|e| if let EntityKind::Table(t) = &e.kind { Some(t.style.clone()) } else { None }));
            let cur = d.header.str("CTABLESTYLE", "Standard");
            d.table_styles
                .iter()
                .filter(|t| !is(&t.name, &["Standard", &cur]) && !used.contains(&t.name.to_ascii_lowercase()))
                .map(|t| t.name.clone())
                .collect()
        }
        // Groups whose members are all gone.
        Kind::Groups => d.groups.iter().filter(|g| !g.members.iter().any(|h| d.entity(*h).is_some())).map(|g| g.name.clone()).collect(),
        Kind::ZeroLength | Kind::EmptyText | Kind::Absent | Kind::All => Vec::new(),
    }
}

/// Most exact names one call takes.
const MAX_EXACT_NAMES: usize = 100_000;

/// The names of a kind to purge: wildcard patterns (`"A*,B"`, as typed), or exact names, which a
/// name holding `*`, `?`, `#` or `,` needs, for every kind or per kind (the Purge dialog's
/// checked items, in one call so they are one undo step whatever UNDO Control says).
enum Names {
    Patterns(Vec<String>),
    /// Lower case.
    Exact(HashSet<String>),
    /// Lower case, by [`json_key`] of the kind; a kind not listed purges nothing.
    ByKind(HashMap<&'static str, HashSet<String>>),
}

impl Names {
    fn patterns(text: &str) -> Names {
        Names::Patterns(text.split(',').map(str::trim).filter(|p| !p.is_empty()).take(1000).map(str::to_string).collect())
    }
    fn matches(&self, kind: Kind, name: &str) -> bool {
        match self {
            Names::Patterns(pats) => pats.iter().any(|p| wildcard(p, name)),
            Names::Exact(names) => names.contains(&name.to_lowercase()),
            Names::ByKind(kinds) => kinds.get(json_key(kind)).is_some_and(|names| names.contains(&name.to_lowercase())),
        }
    }

    /// `names` of the JSON form: absent or null is every name, text is patterns, a list is exact
    /// names, and an object lists exact names per kind.
    fn from_json(v: Option<&Value>) -> Result<Names> {
        let not_names = || bad("purge", "`names` must be text, a list of names or names by kind");
        let exact = |v: &Value, room: &mut usize| -> Result<HashSet<String>> {
            let a = v.as_array().ok_or_else(not_names)?;
            *room = room.checked_sub(a.len()).ok_or_else(|| bad("purge", format!("at most {MAX_EXACT_NAMES} names")))?;
            a.iter().map(|n| n.as_str().map(str::to_lowercase).ok_or_else(not_names)).collect()
        };
        let mut room = MAX_EXACT_NAMES;
        Ok(match v {
            None | Some(Value::Null) => Names::patterns("*"),
            Some(Value::String(t)) => Names::patterns(t),
            Some(a @ Value::Array(_)) => Names::Exact(exact(a, &mut room)?),
            Some(Value::Object(m)) => {
                let mut kinds = HashMap::new();
                for (k, list) in m {
                    let kind = kind_of(k).map(|(_, kind)| kind).filter(|kind| ALL.contains(kind));
                    let kind = kind.ok_or_else(|| bad("purge", format!("`names`: `{k}` is not a kind of named object")))?;
                    kinds.entry(json_key(kind)).or_insert_with(HashSet::new).extend(exact(list, &mut room)?);
                }
                Names::ByKind(kinds)
            }
            Some(_) => return Err(not_names()),
        })
    }
}

fn matching(d: &Drawing, kind: Kind, names: &Names) -> Vec<String> {
    candidates(d, kind).into_iter().filter(|n| names.matches(kind, n)).collect()
}

/// PURGE.LIST: what PURGE would remove now, by kind, without removing anything.
pub(in crate::cmd) fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let named: Map<String, Value> = ALL.iter().map(|k| (json_key(*k).to_string(), json!(candidates(d, *k)))).collect();
    Ok(json!({
        "named": named,
        "zeroLength": degenerate(d, Kind::ZeroLength).len(),
        "emptyText": degenerate(d, Kind::EmptyText).len(),
    }))
}

/// Run a command body directly: the purge records a single undo step.
fn call(s: &mut Session, id: &str, p: &Value) -> Result<Value> {
    let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
    (spec.run)(s, p)
}

/// Purge one named object. `false` when it is in use after all (or gone).
fn purge_one(s: &mut Session, kind: Kind, name: &str) -> Result<bool> {
    let d = s.doc_mut()?;
    let n = name.to_string();
    let gone = match kind {
        Kind::Layers => return Ok(call(s, "layer.delete", &json!({ "name": n })).is_ok()),
        Kind::TextStyles => return Ok(call(s, "style.delete", &json!({ "name": n })).is_ok()),
        Kind::Blocks => {
            let key = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned();
            key.is_some_and(|k| d.blocks.remove(&k).is_some())
        }
        Kind::Linetypes => retain(&mut d.linetypes, |t| &t.name, name),
        Kind::Dimstyles => retain(&mut d.dim_styles, |t| &t.name, name),
        Kind::MleaderStyles => retain(&mut d.mleader_styles, |t| &t.name, name),
        Kind::MlineStyles => retain(&mut d.mline_styles, |t| &t.name, name),
        Kind::TableStyles => retain(&mut d.table_styles, |t| &t.name, name),
        Kind::Groups => retain(&mut d.groups, |g| &g.name, name),
        Kind::ZeroLength | Kind::EmptyText | Kind::Absent | Kind::All => false,
    };
    Ok(gone)
}

fn retain<T>(v: &mut Vec<T>, name: impl Fn(&T) -> &String, n: &str) -> bool {
    let before = v.len();
    v.retain(|t| !name(t).eq_ignore_ascii_case(n));
    v.len() != before
}

/// Model-space and layout objects with no length, or text with no characters.
fn degenerate(d: &Drawing, kind: Kind) -> Vec<Handle> {
    let zero = |e: &EntityKind| match e {
        EntityKind::Line(l) => l.a == l.b,
        EntityKind::Circle(c) => c.radius == 0.0,
        EntityKind::Arc(a) => a.radius == 0.0,
        EntityKind::LwPolyline(p) => p.vertices.first().is_some_and(|f| p.vertices.iter().all(|v| v.p == f.p)),
        EntityKind::Polyline3d(p) => p.points.first().is_some_and(|f| p.points.iter().all(|v| v == f)),
        _ => false,
    };
    let empty = |e: &EntityKind| match e {
        EntityKind::Text(t) => t.value.trim().is_empty(),
        EntityKind::MText(t) => t.contents.trim().is_empty(),
        _ => false,
    };
    d.model
        .iter()
        .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
        .filter(|e| if kind == Kind::ZeroLength { zero(&e.kind) } else { empty(&e.kind) })
        .map(|e| e.handle)
        .collect()
}

/// Purges and the messages it produced, collected for the JSON result.
#[derive(Default)]
struct Report {
    purged: Map<String, Value>,
    lines: Vec<String>,
}

impl Report {
    fn add(&mut self, kind: Kind, name: &str) {
        self.lines.push(format!("Deleting {} \"{name}\".", noun(kind)));
        if let Value::Array(a) = self.purged.entry(json_key(kind)).or_insert_with(|| Value::Array(Vec::new())) {
            a.push(json!(name));
        }
    }
    fn count(&self, kind: Kind) -> usize {
        self.purged.get(json_key(kind)).and_then(Value::as_array).map_or(0, Vec::len)
    }
    fn summary(&mut self, kind: Kind) {
        let n = self.count(kind);
        let noun = noun(kind);
        self.lines.push(match n {
            0 => format!("No unreferenced {noun}s found."),
            1 => format!("1 {noun} deleted."),
            n => format!("{n} {noun}s deleted."),
        });
    }
    fn json(self) -> Value {
        let msg = self.lines.join("\n");
        let count = |k: &str| self.purged.get(k).and_then(Value::as_array).map_or(0, Vec::len);
        json!({
            "blocks": count("blocks"), "layers": count("layers"), "linetypes": count("linetypes"),
            "purged": Value::Object(self.purged.clone()), "message": msg,
        })
    }
}

/// Purge everything matching `names` for one kind (zero-length and empty text ignore names).
fn purge_kind(s: &mut Session, kind: Kind, names: &Names, r: &mut Report) -> Result<()> {
    match kind {
        Kind::All => {
            for k in ALL {
                purge_kind(s, *k, names, r)?;
            }
            return Ok(());
        }
        Kind::Absent => {
            r.lines.push("No unreferenced objects of this type found.".into());
            return Ok(());
        }
        Kind::ZeroLength | Kind::EmptyText => {
            let d = s.doc_mut()?;
            for h in degenerate(d, kind) {
                if d.remove_entity(h).is_some() {
                    r.add(kind, &h.hex());
                }
            }
        }
        _ => {
            for n in matching(s.doc()?, kind, names) {
                if purge_one(s, kind, &n)? {
                    r.add(kind, &n);
                }
            }
        }
    }
    r.summary(kind);
    Ok(())
}

pub(in crate::cmd) fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let ty = str_param(p, "type").unwrap_or("all");
    let (_, kind) = kind_of(ty).ok_or_else(|| bad("purge", format!("unknown type `{ty}`")))?;
    let names = Names::from_json(p.get("names"))?;
    let unnamed = match p.get("unnamed") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| bad("purge", "`unnamed` must be true or false"))?,
    };
    let mut r = Report::default();
    purge_kind(s, kind, &names, &mut r)?;
    // Zero-length geometry and empty text in the same step (the Purge dialog's option).
    if unnamed && !matches!(kind, Kind::ZeroLength | Kind::EmptyText) {
        for k in [Kind::ZeroLength, Kind::EmptyText] {
            purge_kind(s, k, &names, &mut r)?;
        }
    }
    s.set_selection(Vec::new());
    Ok(r.json())
}

// ---------------- -PURGE prompts ----------------

/// Keyword-only: spaces separate inputs.
const KW: Accept = Accept { point: false, number: false, text: false, select: false, enter: true };
/// A name list: Space ends the input.
const NAME: Accept = Accept { number: true, ..Accept::TEXT };

#[derive(Default)]
enum Stage {
    #[default]
    Type,
    Names(Kind),
    Verify(Kind, String),
    /// Confirm each name: the kind being asked about, its remaining names, the kinds still to
    /// come (for All) and the name pattern.
    Confirm {
        kind: Kind,
        queue: VecDeque<String>,
        rest: VecDeque<Kind>,
        names: String,
    },
}

#[derive(Default)]
pub(in crate::cmd) struct PurgeM {
    stage: Stage,
    report: Report,
}

impl PurgeM {
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        for l in std::mem::take(&mut self.report.lines) {
            s.echo(l);
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }

    /// Move to the next kind with candidates, echoing a summary for each finished one.
    fn next_confirm(&mut self, s: &mut Session, mut rest: VecDeque<Kind>, names: String) -> Result<Step> {
        while let Some(kind) = rest.pop_front() {
            let queue: VecDeque<String> = matching(s.doc()?, kind, &Names::patterns(&names)).into();
            if queue.is_empty() {
                self.report.summary(kind);
                continue;
            }
            self.stage = Stage::Confirm { kind, queue, rest, names };
            for l in std::mem::take(&mut self.report.lines) {
                s.echo(l);
            }
            return Ok(Step::Continue);
        }
        self.finish(s)
    }
}

impl Interactive for PurgeM {
    fn name(&self) -> &'static str {
        "-PURGE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match &self.stage {
            Stage::Type => {
                let kws: Vec<&str> = KINDS.iter().map(|(k, _, _)| *k).collect();
                Prompt::new("Enter type of unused objects to purge", KW).kw(&kws)
            }
            Stage::Names(_) => Prompt::new("Enter name(s) to purge", NAME).default("*"),
            Stage::Verify(..) => Prompt::new("Verify each name to be purged?", KW).kw(&["Yes", "No"]).default("Y"),
            Stage::Confirm { kind, queue, .. } => {
                let n = queue.front().map(String::as_str).unwrap_or("");
                Prompt::new(format!("Purge {} \"{n}\"?", noun(*kind)), KW).kw(&["Yes", "No"]).default("N")
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if i == Input::Cancel {
            return Ok(Step::Cancel);
        }
        match std::mem::take(&mut self.stage) {
            Stage::Type => {
                let Input::Keyword(k) = &i else {
                    if i == Input::Enter {
                        return Ok(Step::Done);
                    }
                    return Err(EngineError::Other("Invalid option keyword.".into()));
                };
                let Some((_, kind)) = kind_of(k) else { return Err(EngineError::Other("Invalid option keyword.".into())) };
                // Zero-length geometry, empty text and orphaned data take no names.
                match kind {
                    Kind::ZeroLength | Kind::EmptyText => {
                        purge_kind(s, kind, &Names::patterns("*"), &mut self.report)?;
                        self.finish(s)
                    }
                    Kind::Absent if k == "Orphaned data" => {
                        purge_kind(s, kind, &Names::patterns("*"), &mut self.report)?;
                        self.finish(s)
                    }
                    _ => {
                        self.stage = Stage::Names(kind);
                        Ok(Step::Continue)
                    }
                }
            }
            Stage::Names(kind) => {
                let names = match &i {
                    Input::Text(t) | Input::Keyword(t) if !t.trim().is_empty() => t.trim().to_string(),
                    _ => "*".into(),
                };
                self.stage = Stage::Verify(kind, names);
                Ok(Step::Continue)
            }
            Stage::Verify(kind, names) => match &i {
                Input::Keyword(k) if k == "No" => {
                    purge_kind(s, kind, &Names::patterns(&names), &mut self.report)?;
                    self.finish(s)
                }
                Input::Keyword(_) | Input::Enter => {
                    let rest: VecDeque<Kind> = if kind == Kind::All { ALL.iter().copied().collect() } else { VecDeque::from([kind]) };
                    self.next_confirm(s, rest, names)
                }
                _ => {
                    self.stage = Stage::Verify(kind, names);
                    Err(EngineError::Other("Enter Yes or No.".into()))
                }
            },
            Stage::Confirm { kind, mut queue, rest, names } => {
                let yes = matches!(&i, Input::Keyword(k) if k == "Yes");
                if !yes && !matches!(&i, Input::Keyword(_) | Input::Enter) {
                    self.stage = Stage::Confirm { kind, queue, rest, names };
                    return Err(EngineError::Other("Enter Yes or No.".into()));
                }
                if let Some(n) = queue.pop_front()
                    && yes
                    && purge_one(s, kind, &n)?
                {
                    self.report.add(kind, &n);
                    if let Some(l) = self.report.lines.pop() {
                        s.echo(l);
                    }
                }
                if queue.is_empty() {
                    self.report.summary(kind);
                    return self.next_confirm(s, rest, names);
                }
                self.stage = Stage::Confirm { kind, queue, rest, names };
                Ok(Step::Continue)
            }
        }
    }
}
