//! USAGE: a read-only cross-reference of the named objects a drawing uses — for each layer,
//! block, text style and linetype, how many objects use it, whether they are only inside block
//! definitions, and which are empty or unused. The unused lists are PURGE's scan (see
//! `cmd::blocks::purge`); nothing here changes or deletes anything.

use std::collections::{HashMap, HashSet};

use cadcraft_doc::{Drawing, Entity, EntityKind};
use serde_json::{Value, json};

use super::super::bad;
use super::super::blocks::purge;

use crate::{Result, Session};

const CMD: &str = "usage";
/// Names listed per kind in `unused`; the counts are complete unless the scan was cut short
/// (`incomplete`).
const MAX_NAMES: usize = 5_000;
/// The most objects one scan reads, across model space, the layouts and the block definitions. A
/// drawing with more is reported as `incomplete` instead of with counts that look complete. This
/// is USAGE's own limit; PURGE's scan limit is separate and unchanged.
const MAX_SCAN: usize = 5_000_000;

pub const PARAMS: &str = "{type?: layers|blocks|textstyles|linetypes|all (default all), limit? (default 200), offset?} → \
    {schema: 1, count (all items of the type), returned, offset, truncated, incomplete (true when the drawing has more \
    objects than one scan reads, so every count, status and summary is partial), incompleteReason? (why), \
    items: [{type: layer|block|textStyle|linetype, name, entities (objects in model space and layouts that use it), \
    blockEntities (objects in block definitions that use it), references (named objects that use it: a layer's linetype, \
    a text style's dimension and multileader styles), used (for a block: reachable from model space, a layout or a dimension), \
    insideBlocksOnly (used, but by block definitions only), empty (nothing refers to it), unused (PURGE would offer it), \
    current? (the current layer, text style or linetype)}], \
    summary: {<kind>: {total, used, insideBlocksOnly, empty, unused}}, \
    unused: {<kind>: {count, returned, truncated, names}} (PURGE's scan, in table order)}";

/// The four kinds of named object USAGE reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Layer,
    Block,
    TextStyle,
    Linetype,
}

impl Category {
    const ALL: [Category; 4] = [Category::Layer, Category::Block, Category::TextStyle, Category::Linetype];

    /// The singular JSON `type` value of this kind (`layer`, `block`, `textStyle`, `linetype`).
    fn key(self) -> &'static str {
        match self {
            Category::Layer => "layer",
            Category::Block => "block",
            Category::TextStyle => "textStyle",
            Category::Linetype => "linetype",
        }
    }
    /// The JSON key under which this kind's objects are grouped (`layers`, `blocks`,
    /// `textStyles`, `linetypes`).
    fn plural(self) -> &'static str {
        match self {
            Category::Layer => "layers",
            Category::Block => "blocks",
            Category::TextStyle => "textStyles",
            Category::Linetype => "linetypes",
        }
    }
    /// The capitalised plural noun used in the human-readable message.
    fn label(self) -> &'static str {
        match self {
            Category::Layer => "Layers",
            Category::Block => "Blocks",
            Category::TextStyle => "Text styles",
            Category::Linetype => "Linetypes",
        }
    }
    /// The PURGE kind whose scan answers whether this kind's objects are unused.
    fn purge(self) -> purge::Kind {
        match self {
            Category::Layer => purge::Kind::Layers,
            Category::Block => purge::Kind::Blocks,
            Category::TextStyle => purge::Kind::TextStyles,
            Category::Linetype => purge::Kind::Linetypes,
        }
    }
    /// `None` for an unknown name, `Some(None)` for all kinds.
    fn parse(s: &str) -> Option<Option<Category>> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "all" => Some(None),
            "layer" | "layers" => Some(Some(Category::Layer)),
            "block" | "blocks" => Some(Some(Category::Block)),
            "style" | "styles" | "textstyle" | "textstyles" | "text_style" | "text_styles" => Some(Some(Category::TextStyle)),
            "linetype" | "linetypes" | "ltype" | "ltypes" => Some(Some(Category::Linetype)),
            _ => None,
        }
    }
}

/// Objects in model space and the layouts (`false`), and in block definitions (`true`), up to
/// `limit` — the USAGE scan limit, past which later objects are not read.
fn each_entity(d: &Drawing, limit: usize) -> impl Iterator<Item = (&Entity, bool)> {
    let top = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())).map(|e| (&**e, false));
    let blocks = d.blocks.values().flat_map(|b| b.entities.iter()).map(|e| (&**e, true));
    top.chain(blocks).take(limit)
}

/// Objects in model space, the layouts and the block definitions — everything one scan reads.
fn entity_total(d: &Drawing) -> usize {
    d.blocks.values().fold(d.entity_count(), |n, b| n.saturating_add(b.entities.len()))
}

/// A scan that stopped at its limit: the drawing holds `total` objects but only the first
/// `scanned` were read, so objects after the limit are missing from the counts, verdicts and
/// summary. Present in the report as `incomplete`/`incompleteReason`, never silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Incomplete {
    scanned: usize,
    total: usize,
}

impl Incomplete {
    /// Why the report is partial, naming both counts so a caller can judge it.
    fn reason(self) -> String {
        format!(
            "USAGE read the first {} of {} objects (model space, layouts and block definitions); \
             references after the limit are missing, so counts, statuses and summaries are partial",
            self.scanned, self.total
        )
    }
}

/// Uses per lower-case name: (in model space and layouts, in block definitions).
type Uses = HashMap<String, (usize, usize)>;

/// Counts one use of a named object, separately for model space and the layouts (`in_block`
/// false) and for block definitions (`in_block` true); names compare case-insensitively.
fn bump(map: &mut Uses, name: &str, in_block: bool) {
    let slot = map.entry(name.to_ascii_lowercase()).or_default();
    if in_block {
        slot.1 = slot.1.saturating_add(1);
    } else {
        slot.0 = slot.0.saturating_add(1);
    }
}

/// Counts one named table entry that refers to `name`: a layer's linetype, or a dimension or
/// multileader style's text style. Names compare case-insensitively.
fn bump_reference(map: &mut HashMap<String, usize>, name: &str) {
    let slot = map.entry(name.to_ascii_lowercase()).or_default();
    *slot = slot.saturating_add(1);
}

/// `ByLayer` and `ByBlock` name a value, not the linetype table's entries.
fn linetype_value(name: &str) -> bool {
    name.eq_ignore_ascii_case("ByLayer") || name.eq_ignore_ascii_case("ByBlock")
}

/// Every use of the drawing's layers, blocks, text styles and linetypes, in one pass over the
/// objects (plus the block reachability PURGE already computes).
#[derive(Default)]
struct Scan {
    layers: Uses,
    blocks: Uses,
    styles: Uses,
    linetypes: Uses,
    /// Dimension and multileader styles that name a text style.
    style_refs: HashMap<String, usize>,
    /// Layers that name a linetype.
    linetype_refs: HashMap<String, usize>,
    /// Upper-case names of blocks reachable from model space, the layouts and dimensions.
    used_blocks: HashSet<String>,
    /// Set when the scan stopped at its limit, so every count and verdict is partial.
    incomplete: Option<Incomplete>,
}

impl Scan {
    /// Scans the drawing once, reading at most `limit` objects: counts each entity's layer,
    /// explicit linetype, block, text style and attribute style, plus the table references, and
    /// borrows block reachability from PURGE. Records whether the limit cut the drawing short.
    fn new(d: &Drawing, limit: usize) -> Scan {
        let total = entity_total(d);
        let scanned = total.min(limit);
        let mut sc = Scan { used_blocks: purge::used_blocks(d), ..Scan::default() };
        sc.incomplete = (scanned < total).then_some(Incomplete { scanned, total });
        for (e, in_block) in each_entity(d, scanned) {
            bump(&mut sc.layers, &e.common.layer, in_block);
            if !linetype_value(&e.common.linetype) {
                bump(&mut sc.linetypes, &e.common.linetype, in_block);
            }
            match &e.kind {
                EntityKind::Insert(i) => {
                    bump(&mut sc.blocks, &i.block, in_block);
                    for a in &i.attribs {
                        bump(&mut sc.styles, &a.text.style, in_block);
                    }
                }
                EntityKind::Dimension(dm) => {
                    if let Some(b) = &dm.block {
                        bump(&mut sc.blocks, b, in_block);
                    }
                }
                EntityKind::MLeader(m) => {
                    if let Some(b) = &m.block {
                        bump(&mut sc.blocks, &b.block, in_block);
                    }
                    if let Some(t) = &m.text {
                        bump(&mut sc.styles, &t.style, in_block);
                    }
                }
                EntityKind::Text(t) => bump(&mut sc.styles, &t.style, in_block),
                EntityKind::MText(t) => bump(&mut sc.styles, &t.style, in_block),
                EntityKind::AttDef(a) => bump(&mut sc.styles, &a.text.style, in_block),
                _ => {}
            }
        }
        for st in &d.dim_styles {
            bump_reference(&mut sc.style_refs, &st.text_style);
        }
        for st in &d.mleader_styles {
            bump_reference(&mut sc.style_refs, &st.text_style);
        }
        for l in &d.layers {
            if !linetype_value(&l.linetype) {
                bump_reference(&mut sc.linetype_refs, &l.linetype);
            }
        }
        sc
    }
}

/// PURGE's unused lists, kept once so each item can say whether PURGE would offer it.
struct Unused {
    lists: Vec<(Category, Vec<String>, HashSet<String>)>,
}

impl Unused {
    /// Runs PURGE's unused scan for all four kinds, keeping each list in table order plus a
    /// lower-case set for fast membership checks.
    fn new(d: &Drawing) -> Unused {
        let lists = Category::ALL
            .into_iter()
            .map(|c| {
                let names = purge::candidates(d, c.purge());
                let lower = names.iter().map(|n| n.to_ascii_lowercase()).collect();
                (c, names, lower)
            })
            .collect();
        Unused { lists }
    }
    /// Whether PURGE would offer `name` of kind `c` as unused (case-insensitive).
    fn has(&self, c: Category, name: &str) -> bool {
        self.lists.iter().find(|(k, ..)| *k == c).is_some_and(|(_, _, lower)| lower.contains(&name.to_ascii_lowercase()))
    }
    /// How many objects of kind `c` PURGE would offer — the full count, even when the name list
    /// in the JSON output is truncated.
    fn count(&self, c: Category) -> usize {
        self.lists.iter().find(|(k, ..)| *k == c).map_or(0, |(_, names, _)| names.len())
    }
    /// The `unused` member: per kind, the total count, the returned page of names capped at
    /// `MAX_NAMES`, and whether that page is truncated.
    fn json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for (c, names, _) in &self.lists {
            let page: Vec<&String> = names.iter().take(MAX_NAMES).collect();
            out.insert(
                c.plural().into(),
                json!({
                    "count": names.len(),
                    "returned": page.len(),
                    "truncated": page.len() < names.len(),
                    "names": page,
                }),
            );
        }
        Value::Object(out)
    }
}

/// One report item: the use counts, the caller's `used`/`empty` verdicts, `insideBlocksOnly`
/// derived from those counts and PURGE's `unused` answer. `current` is emitted only for the kinds
/// that have a current entry (layers, text styles and linetypes).
fn item_json(
    c: Category,
    name: &str,
    (entities, block_entities): (usize, usize),
    references: usize,
    (used, empty): (bool, bool),
    current: Option<bool>,
    unused: &Unused,
) -> Value {
    let mut v = json!({
        "type": c.key(),
        "name": name,
        "entities": entities,
        "blockEntities": block_entities,
        "references": references,
        "used": used,
        "insideBlocksOnly": used && entities == 0 && block_entities > 0 && references == 0,
        "empty": empty,
        "unused": unused.has(c, name),
    });
    if let (Some(cur), Some(o)) = (current, v.as_object_mut()) {
        o.insert("current".into(), json!(cur));
    }
    v
}

/// Builds one item per named object in table order — layers, blocks, text styles, linetypes —
/// reading the scanned use counts and adding the current-object flag where the drawing has one.
fn items(scan: &Scan, unused: &Unused, d: &Drawing) -> Vec<Value> {
    let mut out = Vec::new();
    let current_layer = d.header.str("CLAYER", "0");
    for l in &d.layers {
        let (e, b) = scan.layers.get(&l.name.to_ascii_lowercase()).copied().unwrap_or_default();
        let uses = e.saturating_add(b);
        let cur = l.name.eq_ignore_ascii_case(&current_layer);
        out.push(item_json(Category::Layer, &l.name, (e, b), 0, (uses > 0, uses == 0), Some(cur), unused));
    }
    for b in d.blocks.values() {
        let (e, n) = scan.blocks.get(&b.name.to_ascii_lowercase()).copied().unwrap_or_default();
        // A definition is used when a reference to it is reachable, as PURGE counts it.
        let used = scan.used_blocks.contains(&b.name.to_ascii_uppercase());
        out.push(item_json(Category::Block, &b.name, (e, n), 0, (used, e.saturating_add(n) == 0), None, unused));
    }
    let current_style = d.header.str("TEXTSTYLE", "Standard");
    for t in &d.text_styles {
        let (e, b) = scan.styles.get(&t.name.to_ascii_lowercase()).copied().unwrap_or_default();
        let r = scan.style_refs.get(&t.name.to_ascii_lowercase()).copied().unwrap_or(0);
        let uses = e.saturating_add(b).saturating_add(r);
        let cur = t.name.eq_ignore_ascii_case(&current_style);
        out.push(item_json(Category::TextStyle, &t.name, (e, b), r, (uses > 0, uses == 0), Some(cur), unused));
    }
    let current_linetype = d.header.str("CELTYPE", "ByLayer");
    for t in &d.linetypes {
        let (e, b) = scan.linetypes.get(&t.name.to_ascii_lowercase()).copied().unwrap_or_default();
        let r = scan.linetype_refs.get(&t.name.to_ascii_lowercase()).copied().unwrap_or(0);
        let uses = e.saturating_add(b).saturating_add(r);
        let cur = t.name.eq_ignore_ascii_case(&current_linetype);
        out.push(item_json(Category::Linetype, &t.name, (e, b), r, (uses > 0, uses == 0), Some(cur), unused));
    }
    out
}

/// Totals per kind over every item — total, used, inside blocks only, empty — plus how many
/// objects of that kind PURGE would offer.
fn summary(items: &[Value], unused: &Unused) -> Value {
    let mut out = serde_json::Map::new();
    for c in Category::ALL {
        let mut total = 0usize;
        let mut used = 0usize;
        let mut inside = 0usize;
        let mut empty = 0usize;
        for it in items.iter().filter(|i| i["type"] == c.key()) {
            total = total.saturating_add(1);
            used = used.saturating_add(usize::from(it["used"].as_bool() == Some(true)));
            inside = inside.saturating_add(usize::from(it["insideBlocksOnly"].as_bool() == Some(true)));
            empty = empty.saturating_add(usize::from(it["empty"].as_bool() == Some(true)));
        }
        out.insert(c.plural().into(), json!({ "total": total, "used": used, "insideBlocksOnly": inside, "empty": empty, "unused": unused.count(c) }));
    }
    Value::Object(out)
}

/// The human-readable summary, one line per kind, plus a note when the filter matched no named
/// objects at all and a note when the scan was cut short.
fn message(items: &[&Value], summary: &Value, incomplete: Option<Incomplete>) -> String {
    let mut lines = Vec::new();
    for c in Category::ALL {
        let s = &summary[c.plural()];
        lines.push(format!(
            "{}: {} total, {} used, {} inside blocks only, {} empty, {} unused.",
            c.label(),
            s["total"].as_u64().unwrap_or(0),
            s["used"].as_u64().unwrap_or(0),
            s["insideBlocksOnly"].as_u64().unwrap_or(0),
            s["empty"].as_u64().unwrap_or(0),
            s["unused"].as_u64().unwrap_or(0),
        ));
    }
    if items.is_empty() {
        lines.push("There are no named objects of this type.".into());
    }
    if let Some(inc) = incomplete {
        lines.push(format!("Scan incomplete: read the first {} of {} objects; the counts above are partial.", inc.scanned, inc.total));
    }
    lines.join("\n")
}

/// Runs `USAGE`: validates `type`, `limit` and `offset` (rejecting bad input without touching the
/// drawing), scans read-only, and returns the paged items with the per-kind summary, PURGE's
/// unused lists and the message.
pub fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let count = |k: &str, default: usize| -> Result<usize> {
        given(k).map_or(Ok(default), |v| {
            v.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX)).ok_or_else(|| bad(CMD, format!("`{k}` must be a whole number, 0 or more")))
        })
    };
    let ty = match given("type") {
        None => None,
        Some(Value::String(t)) => {
            Category::parse(t).ok_or_else(|| bad(CMD, format!("`type` must be layers, blocks, textstyles, linetypes or all, got \"{t}\"")))?
        }
        Some(v) => return Err(bad(CMD, format!("`type` must be a string, got {v}"))),
    };
    let limit = count("limit", 200)?;
    let offset = count("offset", 0)?;
    let st = s.state()?;
    let d = &st.doc;
    let scan = Scan::new(d, MAX_SCAN);
    let unused = Unused::new(d);
    let all = items(&scan, &unused, d);
    let matched: Vec<&Value> = all.iter().filter(|i| ty.is_none_or(|c| i["type"] == c.key())).collect();
    let total = matched.len();
    let page: Vec<&Value> = matched.iter().skip(offset).take(limit).copied().collect();
    let summary = summary(&all, &unused);
    let mut out = json!({
        "schema": 1,
        "count": total,
        "returned": page.len(),
        "offset": offset,
        "truncated": page.len() < total.saturating_sub(offset),
        "incomplete": scan.incomplete.is_some(),
        "items": page,
        "summary": summary,
        "unused": unused.json(),
        "message": message(&matched, &summary, scan.incomplete),
    });
    if let (Some(inc), Some(o)) = (scan.incomplete, out.as_object_mut()) {
        o.insert("incompleteReason".into(), json!(inc.reason()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
