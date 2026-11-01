//! INSERT of a drawing file (`-INSERT C:\parts\bolt.dxf`, `bolt=C:\parts\bolt.dwg`, JSON `file`)
//! and exploded insertion (`*name`).
//!
//! A file becomes a block named after it (or after the part before `=`): its model space, with
//! the file's `$INSBASE` as the base point, plus the layers, linetypes, styles and blocks it needs
//! (names this drawing already has keep this drawing's definitions). The file is read through the
//! host's io hooks like OPEN, so the engine stays I/O-agnostic.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cadcraft_doc::{Block, Drawing, Entity, EntityKind, Handle};
use serde_json::{Value, json};

#[cfg(not(target_arch = "wasm32"))]
use super::super::file::MAX_DRAWING_BYTES;
use super::super::file::io;
use super::super::modify::explode_kind;
use super::purge::{Kind, candidates, is_arrow_override, override_arrows, scan_reads_everything};
use super::{referenced_block, references_block, valid_block_name};
use crate::cmd::{bad, str_param};
use crate::{EngineError, Result, Session};

/// A drawing file named at the block name prompt.
#[derive(Clone, Debug)]
pub(super) struct FileRef {
    /// The block to define.
    pub name: String,
    pub path: String,
    /// `name=path`: redefine the block without asking.
    explicit: bool,
}

impl FileRef {
    /// Whether to ask before redefining: a plain path whose block already exists.
    pub fn ask_redefine(&self, s: &Session) -> bool {
        !self.explicit && s.doc().is_ok_and(|d| d.block(&self.name).is_some())
    }

    /// Read the file and (re)define its block. Returns the block's name.
    pub fn define(&self, s: &mut Session) -> Result<String> {
        let src = read_drawing(&self.path)?;
        define_from(s, &src, &self.name, false)
    }
}

/// The file `t` names at the block name prompt: `name=path` (`name=` alone: the file `name`), or a
/// path (it has a folder separator or a .dwg/.dxf extension) that isn't the name of a block.
pub(super) fn file_ref(s: &Session, t: &str) -> Option<FileRef> {
    let t = t.trim();
    // A block of this drawing is that block, whatever its name looks like: a name read from a file
    // must not pass for `name=path` and make INSERT read another file (CWE-74).
    if s.doc().is_ok_and(|d| d.block(t).is_some()) {
        return None;
    }
    if let Some((name, path)) = t.split_once('=') {
        let name = name.trim();
        let path = if path.trim().is_empty() { name } else { path.trim() };
        return (!name.is_empty()).then(|| FileRef { name: name.to_string(), path: path.to_string(), explicit: true });
    }
    let lower = t.to_ascii_lowercase();
    let pathlike = t.contains(['/', '\\']) || lower.ends_with(".dwg") || lower.ends_with(".dxf");
    if !pathlike {
        return None;
    }
    Some(FileRef { name: stem(t), path: t.to_string(), explicit: false })
}

/// The file name without folders and extension (either separator, so Windows paths work anywhere).
pub(super) fn stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => file.to_string(),
    }
}

/// Read a drawing file through the io hooks; a path without extension tries .dwg, then .dxf.
pub(super) fn read_drawing(path: &str) -> Result<Drawing> {
    let hooks = io().ok_or_else(|| bad("insert", "file formats are not available in this build"))?;
    let has_ext = stem(path) != path.rsplit(['/', '\\']).next().unwrap_or(path);
    let candidates = if has_ext { vec![path.to_string()] } else { vec![path.to_string(), format!("{path}.dwg"), format!("{path}.dxf")] };
    let mut last = None;
    for p in candidates {
        match read_file(&p) {
            Ok(bytes) => return (hooks.read)(&bytes, &p).map_err(|e| bad("insert", format!("{p}: {e}"))),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| bad("insert", format!("{path}: file not found"))))
}

fn read_file(_path: &str) -> Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::capped_read::read_capped(std::path::Path::new(_path), MAX_DRAWING_BYTES).map_err(|e| bad("insert", format!("{_path}: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    Err(bad("insert", "paths are not available on the web"))
}

/// (Re)define block `name` from `src`'s model space with base point `$INSBASE`, adding the
/// layers, linetypes, styles and blocks of `src` this drawing doesn't have. With `only_needed`
/// the layers, linetypes and styles that nothing uses are left out again, as when a block is
/// taken from a library drawing: only what the block needs comes along. Nothing changes when the
/// definition fails (an invalid name, or a block that would contain itself).
pub(super) fn define_from(s: &mut Session, src: &Drawing, name: &str, only_needed: bool) -> Result<String> {
    if !valid_block_name(name) {
        return Err(bad("insert", format!("\"{name}\" is not a valid block name")));
    }
    let mut d = s.doc()?.clone();
    let before = TableNames::of(&d);
    // Redefining keeps the existing definition's name (block names are case-insensitive).
    let name = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| name.to_string());
    let own_dim_styles = names_of(&d, Kind::Dimstyles);
    merge_tables(&mut d, src);
    let mut renames = block_names(&d, src, &name);
    if only_needed {
        // The other blocks of the file are not copied at all.
        let need = needed_blocks(src);
        renames.retain(|from, _| need.contains(&from.to_ascii_uppercase()));
    }
    let renames = Renames::new(renames);
    rename_style_arrows(&mut d, &own_dim_styles, &renames);
    for (from, to) in &renames.exact {
        let Some(b) = src.blocks.get(from) else { continue };
        let mut nb = (**b).clone();
        nb.name = to.clone();
        nb.entities = Default::default();
        for e in b.entities.iter() {
            let e = adopt(&mut d, e, &renames);
            nb.entities.push(e);
        }
        d.blocks.insert(to.clone(), Arc::new(nb));
    }
    let mut b = Block::new(&name);
    b.base = src.header.point("INSBASE").filter(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()).unwrap_or_default();
    let mut ents = Vec::new();
    for e in src.model.iter() {
        let e = adopt(&mut d, e, &renames);
        d.ensure_layer(&e.common.layer);
        ents.push(e);
    }
    if references_block(&d, &ents, &name) {
        return Err(bad("insert", format!("Block \"{name}\" references itself.")));
    }
    for e in ents {
        b.entities.push(e);
    }
    d.blocks.insert(name.clone(), Arc::new(b));
    if only_needed {
        drop_unused_new(&mut d, &before, &name);
    }
    *s.doc_mut()? = d;
    Ok(name)
}

/// The tables a drawing merge adds to, in an order where removing the unused entries of one
/// frees the next (blocks free layers, layers free linetypes, dimension styles free text styles).
const MERGED: [Kind; 8] =
    [Kind::Blocks, Kind::Dimstyles, Kind::MleaderStyles, Kind::MlineStyles, Kind::TableStyles, Kind::Layers, Kind::Linetypes, Kind::TextStyles];

/// The names (lower case) in one of those tables.
fn names_of(d: &Drawing, kind: Kind) -> BTreeSet<String> {
    let lower = |it: &mut dyn Iterator<Item = &String>| it.map(|n| n.to_ascii_lowercase()).collect::<BTreeSet<String>>();
    match kind {
        Kind::Blocks => lower(&mut d.blocks.keys()),
        Kind::Dimstyles => lower(&mut d.dim_styles.iter().map(|t| &t.name)),
        Kind::MleaderStyles => lower(&mut d.mleader_styles.iter().map(|t| &t.name)),
        Kind::MlineStyles => lower(&mut d.mline_styles.iter().map(|t| &t.name)),
        Kind::TableStyles => lower(&mut d.table_styles.iter().map(|t| &t.name)),
        Kind::Layers => lower(&mut d.layers.iter().map(|t| &t.name)),
        Kind::Linetypes => lower(&mut d.linetypes.iter().map(|t| &t.name)),
        Kind::TextStyles => lower(&mut d.text_styles.iter().map(|t| &t.name)),
        _ => BTreeSet::new(),
    }
}

/// The names in the merged tables, by kind.
struct TableNames(Vec<(Kind, BTreeSet<String>)>);

impl TableNames {
    fn of(d: &Drawing) -> Self {
        TableNames(MERGED.iter().map(|k| (*k, names_of(d, *k))).collect())
    }
}

/// Upper-case names of the blocks `roots` and every block they need, through nested references,
/// dimension blocks, multileader block content and dimension arrowheads.
fn reachable(d: &Drawing, roots: Vec<String>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut todo = roots;
    // Each block is walked once, so this ends, cycles included.
    while let Some(n) = todo.pop() {
        if !seen.insert(n.to_ascii_uppercase()) {
            continue;
        }
        let Some(b) = d.block(&n) else { continue };
        for e in b.entities.iter() {
            todo.extend(referenced_block(e));
            if let EntityKind::Dimension(dm) = &e.kind {
                todo.extend(override_arrows(dm));
            }
        }
    }
    seen
}

/// Upper-case names of the blocks the model space of `src` needs: the ones it refers to, what
/// those need, and the arrowheads of its dimension styles.
fn needed_blocks(src: &Drawing) -> BTreeSet<String> {
    let mut roots: Vec<String> = Vec::new();
    for e in src.model.iter() {
        roots.extend(referenced_block(e));
        if let EntityKind::Dimension(dm) = &e.kind {
            roots.extend(override_arrows(dm));
        }
    }
    for st in &src.dim_styles {
        let arrows = [&st.arrow_block, &st.arrow_block1, &st.arrow_block2, &st.leader_arrow_block];
        roots.extend(arrows.into_iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()));
    }
    reachable(src, roots)
}

/// Remove the blocks, layers, linetypes and styles that were not in the drawing `before` and that
/// nothing uses (PURGE's scan), in an order where removing one frees the next. `keep` is the
/// block just defined: nothing refers to it yet, so it and the blocks it needs stay. Nothing goes
/// from a drawing too big for that scan: an unused entry stays, but a used one would be lost.
fn drop_unused_new(d: &mut Drawing, before: &TableNames, keep: &str) {
    if !scan_reads_everything(d) {
        return;
    }
    // Removing a dimension style frees its arrowhead block, which an earlier table already
    // scanned: scan again until a pass removes nothing (a chain is never longer than the tables).
    for _ in 0..MERGED.len() {
        if !drop_unused_pass(d, before, keep) {
            break;
        }
    }
}

/// One scan of the merged tables; `true` when it removed something.
fn drop_unused_pass(d: &mut Drawing, before: &TableNames, keep: &str) -> bool {
    let mut removed = false;
    for (kind, old) in &before.0 {
        // Nothing was added to this table: nothing to scan for.
        if names_of(d, *kind).is_subset(old) {
            continue;
        }
        let keep_blocks = if matches!(kind, Kind::Blocks) { reachable(d, vec![keep.to_string()]) } else { BTreeSet::new() };
        let fresh: BTreeSet<String> = candidates(d, *kind)
            .into_iter()
            .filter(|n| !old.contains(&n.to_ascii_lowercase()) && !keep_blocks.contains(&n.to_ascii_uppercase()))
            .map(|n| n.to_ascii_lowercase())
            .collect();
        let gone = |name: &str| fresh.contains(&name.to_ascii_lowercase());
        removed |= !fresh.is_empty();
        match kind {
            Kind::Blocks => d.blocks.retain(|k, _| !gone(k)),
            Kind::Dimstyles => d.dim_styles.retain(|t| !gone(&t.name)),
            Kind::MleaderStyles => d.mleader_styles.retain(|t| !gone(&t.name)),
            Kind::MlineStyles => d.mline_styles.retain(|t| !gone(&t.name)),
            Kind::TableStyles => d.table_styles.retain(|t| !gone(&t.name)),
            Kind::Layers => d.layers.retain(|t| !gone(&t.name)),
            Kind::Linetypes => d.linetypes.retain(|t| !gone(&t.name)),
            Kind::TextStyles => d.text_styles.retain(|t| !gone(&t.name)),
            _ => {}
        }
    }
    removed
}

/// Add the source's layers, linetypes and styles that this drawing doesn't have (a name that
/// exists keeps this drawing's definition).
fn merge_tables(d: &mut Drawing, src: &Drawing) {
    fn merge<T: Clone>(ours: &mut Vec<T>, theirs: &[T], name: fn(&T) -> &str) {
        for t in theirs {
            if !ours.iter().any(|o| name(o).eq_ignore_ascii_case(name(t))) {
                ours.push(t.clone());
            }
        }
    }
    merge(&mut d.layers, &src.layers, |l| &l.name);
    merge(&mut d.linetypes, &src.linetypes, |l| &l.name);
    merge(&mut d.text_styles, &src.text_styles, |t| &t.name);
    merge(&mut d.dim_styles, &src.dim_styles, |t| &t.name);
    merge(&mut d.mleader_styles, &src.mleader_styles, |t| &t.name);
    merge(&mut d.table_styles, &src.table_styles, |t| &t.name);
    merge(&mut d.mline_styles, &src.mline_styles, |t| &t.name);
}

/// Which source blocks to copy, and under which name: new names keep theirs, generated
/// (anonymous `*…`) blocks that collide get a fresh generated name, and a named block this drawing
/// already has (or the block being defined) keeps this drawing's definition (not copied).
fn block_names(d: &Drawing, src: &Drawing, defining: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut taken: BTreeSet<String> = d.blocks.keys().map(|k| k.to_ascii_uppercase()).collect();
    taken.insert(defining.to_ascii_uppercase());
    for (name, b) in &src.blocks {
        let upper = name.to_ascii_uppercase();
        if upper.starts_with("*MODEL_SPACE") || upper.starts_with("*PAPER_SPACE") {
            continue;
        }
        let to = if !taken.contains(&upper) {
            name.clone()
        } else if b.anonymous || name.starts_with('*') {
            let prefix: String = name.chars().take_while(|c| !c.is_ascii_digit()).collect();
            match (1..=1_000_000).map(|i| format!("{prefix}{i}")).find(|n| !taken.contains(&n.to_ascii_uppercase())) {
                Some(n) => n,
                None => continue,
            }
        } else {
            continue;
        };
        taken.insert(to.to_ascii_uppercase());
        out.insert(name.clone(), to);
    }
    out
}

/// The names the copied source blocks take, found as `Drawing::block` finds a block: a reference
/// may spell a name differently from its definition.
struct Renames {
    exact: BTreeMap<String, String>,
    upper: BTreeMap<String, String>,
}

impl Renames {
    fn new(exact: BTreeMap<String, String>) -> Self {
        let mut upper = BTreeMap::new();
        for (from, to) in &exact {
            upper.entry(from.to_ascii_uppercase()).or_insert_with(|| to.clone());
        }
        Renames { exact, upper }
    }

    fn get(&self, name: &str) -> Option<&String> {
        self.exact.get(name).or_else(|| self.upper.get(&name.to_ascii_uppercase()))
    }
}

/// Point the dimension styles the source brought at their renamed arrowhead blocks. The styles
/// this drawing already had (`own`) name this drawing's blocks.
fn rename_style_arrows(d: &mut Drawing, own: &BTreeSet<String>, renames: &Renames) {
    for st in d.dim_styles.iter_mut().filter(|t| !own.contains(&t.name.to_ascii_lowercase())) {
        for arrow in [&mut st.arrow_block, &mut st.arrow_block1, &mut st.arrow_block2, &mut st.leader_arrow_block] {
            if let Some(n) = renames.get(arrow.trim()) {
                arrow.clone_from(n);
            }
        }
    }
}

/// A copy of a source entity for this drawing: a fresh handle, block references renamed, and
/// links to other source objects (associative dimension points) dropped.
fn adopt(d: &mut Drawing, e: &Entity, renames: &Renames) -> Entity {
    let mut e = e.clone();
    e.handle = d.new_handle();
    match &mut e.kind {
        EntityKind::Insert(i) => {
            if let Some(n) = renames.get(&i.block) {
                i.block = n.clone();
            }
        }
        EntityKind::Dimension(dm) => {
            dm.assoc.clear();
            if let Some(n) = dm.block.as_ref().and_then(|b| renames.get(b)) {
                dm.block = Some(n.clone());
            }
            for (key, value) in dm.overrides.iter_mut() {
                if is_arrow_override(key)
                    && let Some(n) = value.as_str().and_then(|b| renames.get(b.trim()))
                {
                    *value = json!(n);
                }
            }
        }
        EntityKind::MLeader(m) => {
            if let Some(b) = &mut m.block
                && let Some(n) = renames.get(&b.block)
            {
                b.block = n.clone();
            }
        }
        _ => {}
    }
    e
}

/// Replace block reference `h` by the objects of its block (an exploded insertion).
pub(super) fn explode_insert(s: &mut Session, h: Handle) -> Result<Vec<Handle>> {
    let space = s.space();
    let e = s.doc()?.entity(h).map(|e| (**e).clone()).ok_or_else(|| EngineError::Other("no such object".into()))?;
    let parts = explode_kind(s.doc()?, &e).unwrap_or_default();
    let d = s.doc_mut()?;
    d.remove_entity(h);
    let mut out = Vec::new();
    for mut p in parts {
        p.handle = d.new_handle();
        out.push(p.handle);
        if let Some(st) = d.space_mut(&space) {
            st.push(p);
        }
    }
    Ok(out)
}

/// The JSON INSERT parameters with `file` defined as a block and `*name` as `explode: true`.
pub(super) fn json_params(s: &mut Session, p: &Value) -> Result<Value> {
    let mut p = p.clone();
    let mut name = str_param(&p, "name").map(str::trim).map(str::to_string);
    if let Some(n) = name.as_deref().and_then(|n| n.strip_prefix('*')) {
        name = Some(n.trim().to_string());
        p["explode"] = json!(true);
    }
    if let Some(path) = str_param(&p, "file").map(str::to_string) {
        let n = name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| stem(&path));
        name = Some(FileRef { name: n, path, explicit: true }.define(s)?);
    }
    if let Some(n) = name {
        p["name"] = json!(n);
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_become_block_names() {
        assert_eq!(stem("C:\\parts\\bolt.dxf"), "bolt");
        assert_eq!(stem("/parts/nut.v2.DWG"), "nut.v2");
        assert_eq!(stem("washer"), "washer");
        let s = Session::new();
        let f = file_ref(&s, "M8=parts/bolt.dwg").unwrap();
        assert_eq!((f.name.as_str(), f.path.as_str(), f.explicit), ("M8", "parts/bolt.dwg", true));
        let f = file_ref(&s, "bolt=").unwrap();
        assert_eq!((f.name.as_str(), f.path.as_str()), ("bolt", "bolt"));
        assert_eq!(file_ref(&s, "C:\\parts\\bolt.dxf").map(|f| f.name), Some("bolt".into()));
        assert!(file_ref(&s, "BOLT").is_none());
    }

    /// A block holding very many references still reaches every block it needs.
    #[test]
    fn a_block_of_many_references_keeps_all_it_needs() {
        let reference = |handle: u64, block: &str| {
            let insert: cadcraft_doc::Insert = serde_json::from_value(json!({ "block": block, "insert": [0, 0, 0] })).unwrap();
            Entity::new(Handle(handle), EntityKind::Insert(insert))
        };
        let mut big = Block::new("Big");
        big.entities.push(reference(0x100, "Deep"));
        for i in 0..100_001 {
            big.entities.push(reference(0x200 + i, "Leaf"));
        }
        let mut d = Drawing::default();
        d.blocks.insert("Big".into(), Arc::new(big));
        d.blocks.insert("Deep".into(), Arc::new(Block::new("Deep")));
        d.blocks.insert("Leaf".into(), Arc::new(Block::new("Leaf")));
        assert!(reachable(&d, vec!["Big".into()]).contains("DEEP"));
    }

    /// A drawing with very many references does not lose the arrowhead of a merged dimension
    /// style: the arrowheads are the last thing PURGE's walk looks at.
    #[test]
    fn many_references_keep_a_merged_arrowhead() {
        let mut d = Drawing::new_metric();
        d.blocks.insert("Leaf".into(), Arc::new(Block::new("Leaf")));
        for i in 0..100_001 {
            let insert: cadcraft_doc::Insert = serde_json::from_value(json!({ "block": "Leaf", "insert": [0, 0, 0] })).unwrap();
            d.model.push(Entity::new(Handle(0x200 + i), EntityKind::Insert(insert)));
        }
        let before = TableNames::of(&d);
        d.blocks.insert("Tip".into(), Arc::new(Block::new("Tip")));
        d.dim_styles.push(cadcraft_doc::DimStyle { name: "Late".into(), arrow_block: "Tip".into(), ..Default::default() });
        d.header.set_str("DIMSTYLE", "Late");
        d.blocks.insert("Merged".into(), Arc::new(Block::new("Merged")));
        drop_unused_new(&mut d, &before, "Merged");
        assert!(d.block("Tip").is_some());
    }

    /// Past the purge scan's limit nothing is called unused: a merged block's dimension style and
    /// layer stay even when the scan never reaches its objects.
    #[test]
    fn a_drawing_too_big_for_the_scan_keeps_what_was_merged() {
        let mut s = Session::new();
        s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 2] })).unwrap();
        let mut dim = (**s.doc().unwrap().model.iter().next().unwrap()).clone();
        dim.common.layer = "Late".into();
        if let EntityKind::Dimension(dm) = &mut dim.kind {
            dm.style = "Late".into();
        }
        // One definition under many names: millions of objects for the scan, thousands in memory.
        let mut filler = Block::new("Filler");
        for i in 0..10_000 {
            filler.entities.push(Entity::new(Handle(0x1000 + i), dim.kind.clone()));
        }
        let filler = Arc::new(filler);
        let mut d = Drawing::new_metric();
        for i in 0..501 {
            d.blocks.insert(format!("F{i:03}"), filler.clone());
        }
        let before = TableNames::of(&d);
        assert!(!scan_reads_everything(&d));
        d.layers.push(cadcraft_doc::Layer { name: "Late".into(), ..Default::default() });
        d.dim_styles.push(cadcraft_doc::DimStyle { name: "Late".into(), ..Default::default() });
        // Sorted after every filler, so past the limit.
        let mut merged = Block::new("Z");
        merged.entities.push(dim);
        d.blocks.insert("Z".into(), Arc::new(merged));
        drop_unused_new(&mut d, &before, "Z");
        assert!(d.layers.iter().any(|l| l.name == "Late"));
        assert!(d.dim_styles.iter().any(|t| t.name == "Late"));
    }

    #[test]
    fn inserted_file_brings_its_mline_styles() {
        let mut src = Drawing::default();
        src.mline_styles.push(cadcraft_doc::MLineStyle { name: "WALL".into(), ..Default::default() });
        let mut d = Drawing::default();
        merge_tables(&mut d, &src);
        assert!(d.mline_styles.iter().any(|m| m.name == "WALL"));
        // A style this drawing already has keeps its own definition.
        let before = d.mline_styles.len();
        merge_tables(&mut d, &src);
        assert_eq!(d.mline_styles.len(), before);
    }

    /// A block reference, for the fixtures below.
    fn reference(handle: u64, block: &str) -> Entity {
        let insert: cadcraft_doc::Insert = serde_json::from_value(json!({ "block": block, "insert": [0, 0, 0] })).unwrap();
        Entity::new(Handle(handle), EntityKind::Insert(insert))
    }

    /// An anonymous block `*U1` that its description tells apart.
    fn anonymous(description: &str) -> Arc<Block> {
        let mut b = Block::new("*U1");
        b.description = description.into();
        Arc::new(b)
    }

    /// Takes `model`, whose `*U1` is the file's, into a drawing with a `*U1` of its own, and says
    /// which of the two the one object of block "Part" refers to.
    fn colliding_block_after_import(model: Entity) -> String {
        let mut src = Drawing::new_metric();
        src.blocks.insert("*U1".into(), anonymous("from the file"));
        src.model.push(model);
        let mut s = Session::new();
        s.doc_mut().unwrap().blocks.insert("*U1".into(), anonymous("from the drawing"));
        define_from(&mut s, &src, "Part", true).unwrap();
        let d = s.doc().unwrap();
        let part = d.block("Part").unwrap();
        let to = part.entities.iter().next().and_then(|e| referenced_block(e)).unwrap();
        d.block(&to).unwrap().description.clone()
    }

    /// Block names are case-insensitive, so `*u1` is the file's `*U1` and takes its new name.
    #[test]
    fn a_renamed_block_is_found_whatever_the_case_of_the_reference() {
        assert_eq!(colliding_block_after_import(reference(0x500, "*u1")), "from the file");
    }

    /// The block content of a multileader takes the new name like any other reference.
    #[test]
    fn a_multileader_follows_its_renamed_block() {
        let EntityKind::Insert(content) = reference(0x500, "*U1").kind else { return };
        let mut m: cadcraft_doc::MLeader =
            serde_json::from_value(json!({ "leaders": [], "landing": [0, 0, 0], "dogleg": 0.0, "text": null })).unwrap();
        m.block = Some(content);
        assert_eq!(colliding_block_after_import(Entity::new(Handle(0x501), EntityKind::MLeader(m))), "from the file");
    }

    /// Arrowheads are references too: a dimension style that came along and a dimension's override
    /// follow the renamed block, and a style this drawing already had keeps its own.
    #[test]
    fn arrowheads_follow_a_renamed_block() {
        let mut s = Session::new();
        s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 2] })).unwrap();
        let mut dim = (**s.doc().unwrap().model.iter().next().unwrap()).clone();
        if let EntityKind::Dimension(dm) = &mut dim.kind {
            dm.style = "Came".into();
            dm.overrides.insert("DIMBLK1".into(), json!("*U1"));
        }
        let style = |name: &str| cadcraft_doc::DimStyle { name: name.into(), arrow_block: "*U1".into(), ..Default::default() };
        let mut src = Drawing::new_metric();
        src.blocks.insert("*U1".into(), anonymous("from the file"));
        src.dim_styles.extend([style("Came"), style("Own")]);
        src.model.push(dim);
        let mut t = Session::new();
        let d = t.doc_mut().unwrap();
        d.blocks.insert("*U1".into(), anonymous("from the drawing"));
        d.dim_styles.push(style("Own"));
        define_from(&mut t, &src, "Part", true).unwrap();

        let d = t.doc().unwrap();
        let of = |name: &str| d.block(name).unwrap().description.clone();
        let arrow = |name: &str| d.dim_styles.iter().find(|st| st.name == name).unwrap().arrow_block.clone();
        assert_eq!(of(&arrow("Came")), "from the file");
        assert_eq!(of(&arrow("Own")), "from the drawing");
        let e = d.block("Part").unwrap().entities.iter().next().unwrap().clone();
        let EntityKind::Dimension(dm) = &e.kind else { return };
        assert_eq!(of(dm.overrides["DIMBLK1"].as_str().unwrap()), "from the file");
    }

    /// A linetype of the file stays when a dimension style that came along names it, and stays out
    /// when only a style that did not come along does.
    #[test]
    fn a_linetype_follows_the_dimension_style_that_names_it() {
        let mut s = Session::new();
        s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [4, 0], "at": [2, 2] })).unwrap();
        let mut dim = (**s.doc().unwrap().model.iter().next().unwrap()).clone();
        if let EntityKind::Dimension(dm) = &mut dim.kind {
            dm.style = "Used".into();
        }
        let mut src = Drawing::new_metric();
        for name in ["Kept", "Left"] {
            let mut lt = src.linetypes[0].clone();
            lt.name = name.into();
            src.linetypes.push(lt);
        }
        src.dim_styles.push(cadcraft_doc::DimStyle { name: "Used".into(), dim_linetype: "Kept".into(), ..Default::default() });
        src.dim_styles.push(cadcraft_doc::DimStyle { name: "Spare".into(), ext_linetype1: "Left".into(), ..Default::default() });
        src.model.push(dim);
        let mut t = Session::new();
        define_from(&mut t, &src, "Part", true).unwrap();
        let d = t.doc().unwrap();
        assert!(d.linetypes.iter().any(|l| l.name == "Kept"));
        assert!(!d.dim_styles.iter().any(|st| st.name == "Spare"));
        assert!(!d.linetypes.iter().any(|l| l.name == "Left"));
    }
}
