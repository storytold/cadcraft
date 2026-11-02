//! COUNT, COUNTLIST and COUNTTABLE: block references counted in model space, in the layouts and
//! nested inside other blocks (multiplied by the references holding them), or in a selection; and
//! the objects shaped like a picked one.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, Insert, MAX_BLOCK_DEPTH, Space};
use cadcraft_geom::{Mat3, Vec2};
use serde_json::{Value, json};

use super::super::{bad, bool_or, point_req, str_param, targets};

use super::SelfCheck;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Merges done while expanding nested references; a hostile file stops here (and says so).
const MAX_WORK: u64 = 5_000_000;
/// Top-level objects reported (and highlighted) per count.
const MAX_HANDLES: usize = 100_000;

/// COUNT's JSON parameters.
pub(super) const PARAMS: &str = "{block?, object?: handle (count objects like this one), space?: current|model|all|<layout> (default current), \
     handles?: [...] (count in these objects), nested?: bool (default true), highlight?: bool} → \
     {blocks: {name: n}, nested: {name: n}, spaces: {space: {name: n}}, total} | {block, count, nested, handles}, plus `message` and \
     `selfCheck` (found/analyzed/skipped counts with reasons, and the caps hit: work, handles, blockDepth)";

/// COUNT's interactive command-line prompt machine.
pub(super) fn machine() -> Box<dyn Interactive> {
    Box::new(CountM)
}

/// COUNTTABLE's interactive command-line prompt machine (it asks for the insertion point).
pub(super) fn table_machine() -> Box<dyn Interactive> {
    Box::new(CountTableM)
}

/// Where COUNT looks.
#[derive(Clone, Debug, PartialEq)]
enum Scope {
    Space(Space),
    All,
    /// These top-level objects (a selection).
    Objects(Vec<Handle>),
}

impl Scope {
    fn parse(s: &Session, cmd: &str, p: &Value, default_all: bool) -> Result<Scope> {
        if p.get("handles").is_some_and(|v| !v.is_null()) || p.get("handle").is_some_and(|v| !v.is_null()) {
            return Ok(Scope::Objects(targets(s, p)?));
        }
        let given = match p.get("space") {
            None | Some(Value::Null) => None,
            Some(Value::String(t)) => Some(t.trim().to_string()),
            Some(v) => return Err(bad(cmd, format!("`space` must be current, model, all or a layout name, got {v}"))),
        };
        match given.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None if default_all => Ok(Scope::All),
            None | Some("current") => Ok(Scope::Space(s.space())),
            Some("all") => Ok(Scope::All),
            Some("model") => Ok(Scope::Space(Space::Model)),
            Some(_) => {
                let name = given.unwrap_or_default();
                let d = s.doc()?;
                let l =
                    d.layouts.iter().find(|l| l.name.eq_ignore_ascii_case(&name)).ok_or_else(|| bad(cmd, format!("no layout named \"{name}\"")))?;
                Ok(Scope::Space(Space::Paper(l.name.clone())))
            }
        }
    }
    /// "model space", "layout Layout1", "all spaces", "the selection".
    fn label(&self) -> String {
        match self {
            Scope::Space(Space::Model) => "model space".into(),
            Scope::Space(Space::Paper(n)) => format!("layout {n}"),
            Scope::All => "all spaces".into(),
            Scope::Objects(_) => "the selection".into(),
        }
    }
    /// The top-level objects in scope, each with the name of its space.
    fn objects<'a>(&self, d: &'a Drawing) -> Vec<(String, &'a std::sync::Arc<Entity>)> {
        let name = |sp: &Space| sp.tab_name().to_string();
        let mut out = Vec::new();
        let mut add = |sp: &Space| {
            if let Some(store) = d.space(sp) {
                out.extend(store.iter().map(|e| (name(sp), e)));
            }
        };
        match self {
            Scope::Space(sp) => add(sp),
            Scope::All => {
                add(&Space::Model);
                for l in &d.layouts {
                    add(&Space::Paper(l.name.clone()));
                }
            }
            Scope::Objects(hs) => {
                return hs.iter().filter_map(|h| Some((d.space_of(*h).map_or_else(|| "Model".into(), |sp| name(&sp)), d.entity(*h)?))).collect();
            }
        }
        out
    }
}

/// Copies one reference places (a MINSERT's rows × columns).
fn copies(i: &Insert) -> u64 {
    u64::from(i.cols.max(1)).saturating_mul(u64::from(i.rows.max(1)))
}

/// Nested block references per block definition, memoised by (block, levels left) so shared and
/// cyclic definitions don't expand exponentially.
struct Expander<'a> {
    d: &'a Drawing,
    memo: HashMap<(String, usize), Rc<BTreeMap<String, u64>>>,
    work: u64,
    truncated: bool,
    /// A block at the end of the depth cap still held references.
    depth_capped: bool,
}

impl<'a> Expander<'a> {
    /// An expander that has not traversed any block yet.
    fn new(d: &'a Drawing) -> Self {
        Expander { d, memo: HashMap::new(), work: 0, truncated: false, depth_capped: false }
    }
    /// The definition's own name for a referenced block (names are case-insensitive).
    fn canonical(&self, name: &str) -> String {
        self.d.block(name).map_or_else(|| name.to_string(), |b| b.name.clone())
    }
    /// References inside block `name` and its nested blocks, `levels` deep: name → copies.
    fn expand(&mut self, name: &str, levels: usize) -> Rc<BTreeMap<String, u64>> {
        let key = (name.to_ascii_lowercase(), levels);
        if let Some(m) = self.memo.get(&key) {
            return m.clone();
        }
        if levels == 0 {
            // More references than MAX_BLOCK_DEPTH covers: say so rather than silently stop.
            if self.d.block(name).is_some_and(|b| b.entities.iter().any(|e| matches!(&e.kind, EntityKind::Insert(_)))) {
                self.depth_capped = true;
            }
            let out = Rc::new(BTreeMap::new());
            self.memo.insert(key, out.clone());
            return out;
        }
        let mut out: BTreeMap<String, u64> = BTreeMap::new();
        if let Some(b) = self.d.block(name) {
            let b = b.clone();
            for e in b.entities.iter() {
                let EntityKind::Insert(i) = &e.kind else { continue };
                if self.work >= MAX_WORK {
                    self.truncated = true;
                    break;
                }
                let m = copies(i);
                let inner = self.canonical(&i.block);
                let sub = self.expand(&inner, levels - 1);
                self.work = self.work.saturating_add(1).saturating_add(sub.len() as u64);
                add(&mut out, &inner, m);
                for (k, v) in sub.iter() {
                    add(&mut out, k, m.saturating_mul(*v));
                }
            }
        }
        let out = Rc::new(out);
        self.memo.insert(key, out.clone());
        out
    }
}

fn add(m: &mut BTreeMap<String, u64>, k: &str, n: u64) {
    let v = m.entry(k.to_string()).or_default();
    *v = v.saturating_add(n);
}

/// Anonymous blocks (`*U3`: arrays, dynamic block copies) are traversed but not reported.
fn reported(name: &str) -> bool {
    !name.starts_with('*')
}

#[derive(Default)]
struct Tally {
    /// Every reference: top-level and nested.
    blocks: BTreeMap<String, u64>,
    /// The nested part of `blocks`.
    nested: BTreeMap<String, u64>,
    /// Per space (Model or a layout): name → every reference there.
    spaces: BTreeMap<String, BTreeMap<String, u64>>,
    /// Top-level objects holding `want` (when one block is asked for).
    handles: Vec<Handle>,
    truncated: bool,
    /// Top-level objects scanned, and the block references among them.
    found: u64,
    analyzed: u64,
    /// Top-level objects that are not block references, by type.
    skipped: BTreeMap<&'static str, u64>,
    /// More top-level objects held the block than `MAX_HANDLES`.
    handles_capped: bool,
    depth_capped: bool,
}

/// Count block references in `scope`; with `want`, also collect the top-level objects holding it.
fn tally(d: &Drawing, scope: &Scope, nested: bool, want: Option<&str>) -> Tally {
    let mut x = Expander::new(d);
    let mut t = Tally::default();
    for (space, e) in scope.objects(d) {
        t.found = t.found.saturating_add(1);
        let EntityKind::Insert(i) = &e.kind else {
            let n = t.skipped.entry(e.kind.type_name()).or_default();
            *n = n.saturating_add(1);
            continue;
        };
        t.analyzed = t.analyzed.saturating_add(1);
        let name = x.canonical(&i.block);
        let m = copies(i);
        let mut holds = want.is_some_and(|w| w.eq_ignore_ascii_case(&name));
        let per = t.spaces.entry(space).or_default();
        if reported(&name) {
            add(&mut t.blocks, &name, m);
            add(per, &name, m);
        }
        if nested {
            let sub = x.expand(&name, MAX_BLOCK_DEPTH.saturating_sub(1));
            for (k, v) in sub.iter().filter(|(k, _)| reported(k)) {
                let n = m.saturating_mul(*v);
                add(&mut t.blocks, k, n);
                add(&mut t.nested, k, n);
                add(per, k, n);
                holds |= want.is_some_and(|w| w.eq_ignore_ascii_case(k));
            }
        }
        if holds {
            if t.handles.len() < MAX_HANDLES {
                t.handles.push(e.handle);
            } else {
                t.handles_capped = true;
            }
        }
    }
    t.spaces.retain(|_, m| !m.is_empty());
    t.truncated = x.truncated;
    t.depth_capped = x.depth_capped;
    t
}

/// The self-check for a COUNT-style result: what was scanned, counted and skipped, and the caps
/// that stopped it.
fn self_check(t: &Tally) -> Value {
    let mut check = SelfCheck::new();
    check.found(t.found);
    check.analyzed(t.analyzed);
    for (ty, n) in &t.skipped {
        check.skip(ty, "not a block reference", *n);
    }
    if t.truncated {
        check.cap("work", MAX_WORK);
    }
    if t.handles_capped {
        check.cap("handles", u64::try_from(MAX_HANDLES).unwrap_or(u64::MAX));
    }
    if t.depth_capped {
        check.cap("blockDepth", u64::try_from(MAX_BLOCK_DEPTH).unwrap_or(u64::MAX));
    }
    check.to_json()
}

/// A translation-free signature of an object's geometry: objects with equal keys are copies of
/// each other (moved, not rotated or scaled). Coordinates are rounded to 1e-6.
fn shape_key(k: &EntityKind) -> Option<String> {
    let origin = k.grips().first().copied().filter(|p| p.is_finite())?;
    let mut k = k.clone();
    k.transform(&Mat3::translate(Vec2::ZERO - origin));
    let mut v = serde_json::to_value(&k).ok()?;
    round(&mut v, 0);
    Some(v.to_string())
}

fn round(v: &mut Value, depth: usize) {
    if depth > 64 {
        return;
    }
    match v {
        Value::Number(n) if n.is_f64() => {
            if let Some(f) = n.as_f64() {
                let r = (f * 1e6).round() / 1e6;
                *v = json!(if r == 0.0 { 0.0 } else { r });
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| round(x, depth + 1)),
        Value::Object(o) => o.values_mut().for_each(|x| round(x, depth + 1)),
        _ => {}
    }
}

fn hex(hs: &[Handle]) -> Vec<String> {
    hs.iter().map(|h| h.hex()).collect()
}

fn count_line(name: &str, n: u64, nested: u64) -> String {
    if nested > 0 { format!("  {name}: {n} ({nested} nested)") } else { format!("  {name}: {n}") }
}

/// The per-block list as JSON plus the command-line text.
fn list(t: &Tally, scope: &Scope) -> Value {
    let wh = scope.label();
    let message = if t.blocks.is_empty() {
        format!("No block references in {wh}.")
    } else {
        let mut m = format!("Block references in {wh}:");
        for (name, n) in &t.blocks {
            m.push('\n');
            m.push_str(&count_line(name, *n, t.nested.get(name).copied().unwrap_or(0)));
        }
        m.push_str(&format!("\n  Total: {}", t.blocks.values().fold(0u64, |a, b| a.saturating_add(*b))));
        if t.truncated {
            m.push_str("\n  (nesting too large: counts are partial)");
        }
        m
    };
    let total = t.blocks.values().fold(0u64, |a, b| a.saturating_add(*b));
    json!({
        "blocks": t.blocks,
        "nested": t.nested,
        "spaces": t.spaces,
        "total": total,
        "truncated": t.truncated,
        "selfCheck": self_check(t),
        "message": message,
    })
}

/// Count block `b` (its references, nested ones included) in `scope`.
fn count_block(d: &Drawing, scope: &Scope, nested: bool, b: &str) -> Value {
    let t = tally(d, scope, nested, Some(b));
    let name = d.block(b).map_or_else(|| b.to_string(), |blk| blk.name.clone());
    let n = t.blocks.get(&name).copied().unwrap_or(0);
    let k = t.nested.get(&name).copied().unwrap_or(0);
    let wh = scope.label();
    let message = if k > 0 { format!("Block {name}: {n} in {wh} ({k} nested).") } else { format!("Block {name}: {n} in {wh}.") };
    json!({
        "block": name,
        "count": n,
        "nested": k,
        "handles": hex(&t.handles),
        "truncated": t.truncated,
        "selfCheck": self_check(&t),
        "message": message,
    })
}

/// Count the objects in `scope` shaped like `h` (a block reference: references of its block).
fn count_like(d: &Drawing, scope: &Scope, nested: bool, h: Handle) -> Result<Value> {
    let e = d.entity(h).ok_or_else(|| bad("count", format!("no object with handle {}", h.hex())))?;
    if let EntityKind::Insert(i) = &e.kind {
        return Ok(count_block(d, scope, nested, &i.block));
    }
    let ty = e.kind.type_name();
    let key = shape_key(&e.kind).ok_or_else(|| bad("count", format!("a {ty} can't be counted by shape")))?;
    let mut check = SelfCheck::new();
    let mut hs: Vec<Handle> = Vec::new();
    let mut capped = false;
    for (_, o) in scope.objects(d) {
        check.found(1);
        if o.kind.type_name() != ty {
            check.skip(o.kind.type_name(), "a different type", 1);
            continue;
        }
        check.analyzed(1);
        if shape_key(&o.kind).as_deref() == Some(key.as_str()) {
            if hs.len() < MAX_HANDLES {
                hs.push(o.handle);
            } else {
                capped = true;
            }
        }
    }
    if capped {
        check.cap("handles", u64::try_from(MAX_HANDLES).unwrap_or(u64::MAX));
    }
    let n = hs.len();
    let message = format!("{ty}: {n} like the selected one in {}.", scope.label());
    Ok(json!({ "object": ty, "count": n, "handles": hex(&hs), "selfCheck": check.to_json(), "message": message }))
}

/// The body of COUNT and COUNTLIST: list the blocks in scope, count one block, or count the
/// objects shaped like a picked one. `default_all` makes an omitted `space` mean every space
/// (COUNTLIST) instead of the current one (COUNT).
fn run(s: &mut Session, cmd: &str, p: &Value, default_all: bool) -> Result<Value> {
    let scope = Scope::parse(s, cmd, p, default_all)?;
    let nested = bool_or(p, "nested", true);
    let d = s.doc()?;
    let r = if let Some(b) = str_param(p, "block") {
        count_block(d, &scope, nested, b)
    } else if let Some(v) = p.get("object").filter(|v| !v.is_null()) {
        let h = v
            .as_str()
            .and_then(Handle::parse_hex)
            .or_else(|| v.as_u64().map(Handle))
            .ok_or_else(|| bad(cmd, format!("`object`: {v} is not an object handle")))?;
        count_like(d, &scope, nested, h)?
    } else {
        list(&tally(d, &scope, nested, None), &scope)
    };
    if bool_or(p, "highlight", false)
        && let Some(hs) = r.get("handles").and_then(Value::as_array)
    {
        let hs = hs.iter().filter_map(|v| v.as_str().and_then(Handle::parse_hex)).collect();
        s.set_selection(hs);
    }
    Ok(r)
}

pub(super) fn run_count(s: &mut Session, p: &Value) -> Result<Value> {
    run(s, "count", p, false)
}

pub(super) fn run_countlist(s: &mut Session, p: &Value) -> Result<Value> {
    run(s, "countlist", p, true)
}

/// The current table style (CTABLESTYLE, else the first).
fn table_style(s: &Session) -> cadcraft_doc::TableStyle {
    let Ok(d) = s.doc() else { return Default::default() };
    let cur = d.header.str("CTABLESTYLE", "Standard");
    d.table_styles.iter().find(|t| t.name.eq_ignore_ascii_case(&cur)).or(d.table_styles.first()).cloned().unwrap_or_default()
}

/// COUNTTABLE: insert a table of the block counts in scope at `at`, and report what the tally
/// covered (and refused: no block references in scope is an error, not an empty table).
pub(super) fn run_counttable(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "counttable";
    let at = point_req(CMD, p, "at")?;
    let scope = Scope::parse(s, CMD, p, false)?;
    let t = tally(s.doc()?, &scope, bool_or(p, "nested", true), None);
    if t.blocks.is_empty() {
        return Err(bad(CMD, format!("No block references in {}.", scope.label())));
    }
    let cells: Vec<Vec<String>> = t.blocks.iter().map(|(name, n)| vec![name.clone(), n.to_string()]).collect();
    let header = Some(vec!["Name".to_string(), "Count".to_string()]);
    let table = super::super::table::make_table(&table_style(s), at, cells.len(), 2, None, None, Some("Block Count"), header, &cells);
    let h = s.add_entity(EntityKind::Table(table))?;
    Ok(json!({
        "handle": h.hex(),
        "rows": cells.len(),
        "selfCheck": self_check(&t),
        "message": format!("Table of {} block(s) inserted.", cells.len()),
    }))
}

/// Echo a result's `message` at the command line, one line at a time.
fn echo_lines(s: &mut Session, r: &Value) {
    if let Some(m) = r.get("message").and_then(Value::as_str) {
        for l in m.lines() {
            s.echo(l.to_string());
        }
    }
}

/// COUNT at the command line: pick an object to count the ones like it (highlighted), select
/// several to list the blocks among them, or list the blocks in this space or in all of them.
struct CountM;

impl Interactive for CountM {
    fn name(&self) -> &'static str {
        "COUNT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Select object to count or [List/Spaces]", Accept::SELECT).kw(&["List", "Spaces"]).default("List")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let p = match i {
            Input::Pick(hs) => match hs.as_slice() {
                [] => return Ok(Step::Continue),
                [h] => json!({ "object": h.hex(), "highlight": true }),
                many => json!({ "handles": hex(many) }),
            },
            Input::Keyword(k) if k == "Spaces" => json!({ "space": "all" }),
            Input::Keyword(_) | Input::Enter => json!({}),
            _ => return Ok(Step::Continue),
        };
        let r = run(s, "count", &p, false)?;
        echo_lines(s, &r);
        Ok(Step::Done)
    }
}

struct CountTableM;

impl Interactive for CountTableM {
    fn name(&self) -> &'static str {
        "COUNTTABLE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        Prompt::new("Specify insertion point", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(at) => {
                let r = run_counttable(s, &json!({ "at": [at.x, at.y] }))?;
                echo_lines(s, &r);
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}
