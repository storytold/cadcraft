//! COMPAREEXPORT: the comparison as a new drawing (a snapshot). The drawings are compared again
//! when the command runs, so edits made since COMPARE count. The new drawing's model space holds
//! copies of the other drawing's objects that differ, then copies of the current drawing's
//! objects in the compared tab, as two groups, `COMPARE_COMPARED` and `COMPARE_CURRENT`. Objects
//! only in this drawing or changed in it are drawn in the overlay's green, objects only in the
//! other drawing or changed there in its red, and objects in both in gray (color 8). A block
//! reference, dimension or multileader refers to a copy of its block whose objects all have that
//! color (`COMPARE_CURRENT_<name>`, `COMPARE_COMPARED_<name>`, `COMPARE_COMMON_<name>`; for a
//! dimension's `*D` block, another `*D` block), and a table or tolerance uses a recolored copy
//! of its style; xrefs keep their own colors. The new
//! drawing has this drawing's layers, styles and blocks, plus the layers and styles of the other
//! drawing that this one lacks and the blocks its copied objects refer to; an anonymous block of
//! the other drawing whose name this one uses is copied under a free name. Relative xref, image
//! and data link paths become absolute paths from the folder of the drawing they came from. It
//! opens untitled as `compare_<this> vs <other>.dwg`, with unsaved changes; saving it asks for a
//! file name.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cadcraft_doc::color::{Color, Rgb};
use cadcraft_doc::{Block, CellStyle, DimStyle, Drawing, Entity, EntityKind, EntityStore, Group, Handle, MAX_BLOCK_DEPTH, TableStyle};
use serde_json::{Value, json};

use super::super::super::{CommandSpec, bad};
use super::{Comparison, MAX_OBJECTS};
use crate::{Result, Session};

/// Only in this drawing, or changed in it (the overlay's color).
const THIS: Color = Color::True(Rgb(0x4c, 0xd9, 0x7b));
/// Only in the other drawing, or changed there.
const OTHER: Color = Color::True(Rgb(0xff, 0x5c, 0x5c));
/// In both drawings.
const COMMON: Color = Color::Index(8);
/// Block definitions copied from the other drawing (the ones they reference included), and
/// recolored copies of blocks and styles, per export.
const MAX_BLOCKS: usize = 10_000;
/// Objects copied into block definitions (the other drawing's blocks and the recolored
/// copies), per export.
const MAX_COPIED: usize = 1_000_000;

pub(super) fn spec() -> CommandSpec {
    CommandSpec::new("compareexport", "Export Comparison", run)
        .params(
            "{} → {index, title, current, compared: group names, counts: {current, compared}, truncated, message}: the comparison \
             as a new drawing, opened untitled",
        )
        .noundo()
}

/// What [`snapshot`] made: the drawing, the two group names, how many objects each holds,
/// whether a drawing had more objects than were compared, whether the other drawing had more
/// blocks to copy than [`MAX_BLOCKS`], and whether a block was left in its own colors.
struct Snapshot {
    drawing: Drawing,
    current: String,
    compared: String,
    counts: (usize, usize),
    truncated: bool,
    blocks_left_out: bool,
    uncolored: bool,
}

fn run(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.state()?;
    let (uid, cur, title, cur_folder) = (st.uid, st.doc.clone(), st.title.clone(), folder_of(st.path.as_deref()));
    let c = s.compare.as_ref().filter(|c| c.uid == uid).ok_or_else(|| bad("compareexport", "no comparison: run COMPARE first"))?;
    let name = format!("compare_{} vs {}.dwg", stem(&title), stem(&c.other_name));
    let folders = Folders { here: cur_folder, there: folder_of(c.other_path.as_deref()) };
    let Snapshot { drawing, current, compared, counts, truncated, blocks_left_out, uncolored } = snapshot(&cur, c, &folders);
    let i = s.open_drawing(drawing, &name, None);
    // Not saved yet: closing it asks to save it under a name.
    let st = s.state_mut()?;
    st.saved = Arc::new((*st.doc).clone());
    let mut notes = Vec::new();
    if truncated {
        notes.push(format!("only the first {MAX_OBJECTS} objects of each drawing were compared"));
    }
    if blocks_left_out {
        notes.push("some blocks of the other drawing were too many or too large to copy".to_string());
    }
    if uncolored {
        notes.push("some blocks keep their own colors".to_string());
    }
    let notes = if notes.is_empty() { String::new() } else { format!(" ({})", notes.join("; ")) };
    Ok(json!({
        "index": i,
        "title": name,
        "current": current,
        "compared": compared,
        "counts": { "current": counts.0, "compared": counts.1 },
        "truncated": truncated || blocks_left_out || uncolored,
        "message": format!("The comparison was exported to the new drawing {name}{notes}."),
    }))
}

/// The file of the drawing COMPARE compares with: the `path` it was read from, or the open
/// drawing's own file. `None` for `{data}` and for an open drawing never saved.
pub(super) fn source_path(s: &Session, p: &Value, other: &Arc<Drawing>) -> Option<String> {
    match p.get("path").and_then(Value::as_str) {
        Some(path) if p.get("document").is_none_or(Value::is_null) => Some(path.to_string()),
        _ => s.docs.iter().find(|d| Arc::ptr_eq(&d.doc, other)).and_then(|d| d.path.clone()),
    }
}

/// The folders of the two drawings' files. The snapshot has no folder of its own, so the
/// relative file paths it copies (xrefs, images, table data links) become absolute paths from
/// the folder of the drawing they came from.
struct Folders {
    here: Option<PathBuf>,
    there: Option<PathBuf>,
}

/// The absolute folder of the file at `path`.
fn folder_of(path: Option<&str>) -> Option<PathBuf> {
    let parent = Path::new(path?).parent()?;
    let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
    Some(std::path::absolute(parent).unwrap_or_else(|_| parent.to_path_buf()))
}

/// `p` as an absolute path from `folder` when it is relative.
pub(super) fn rebase_path(p: &mut String, folder: Option<&Path>) {
    if let Some(folder) = folder
        && !p.trim().is_empty()
        && Path::new(p.as_str()).is_relative()
    {
        let joined = folder.join(p.as_str());
        *p = std::path::absolute(&joined).unwrap_or(joined).to_string_lossy().to_string();
    }
}

/// Rebase the file an object refers to: an image's file, a table's data link.
pub(super) fn rebase(k: &mut EntityKind, folder: Option<&Path>) {
    match k {
        EntityKind::Image(i) => rebase_path(&mut i.path, folder),
        EntityKind::Table(t) => {
            if let Some(l) = t.data_link.as_mut() {
                rebase_path(&mut l.path, folder);
            }
        }
        _ => {}
    }
}

/// Rebase a block's xref path and the files its objects refer to.
fn rebase_block(b: &mut Block, folder: Option<&Path>) {
    if let Some(x) = b.xref_path.as_mut() {
        rebase_path(x, folder);
    }
    let mut store = EntityStore::new();
    for e in b.entities.iter() {
        let mut e = (**e).clone();
        rebase(&mut e.kind, folder);
        store.push(e);
    }
    b.entities = store;
}

/// Whether a block holds a relative file path that [`rebase_block`] would change.
fn has_relative_paths(b: &Block) -> bool {
    let rel = |p: &str| !p.trim().is_empty() && Path::new(p).is_relative();
    b.xref_path.as_deref().is_some_and(rel)
        || b.entities.iter().any(|e| match &e.kind {
            EntityKind::Image(i) => rel(&i.path),
            EntityKind::Table(t) => t.data_link.as_ref().is_some_and(|l| rel(&l.path)),
            _ => false,
        })
}

/// A drawing title without its file extension.
fn stem(title: &str) -> String {
    std::path::Path::new(title).file_stem().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| title.to_string())
}

fn snapshot(cur: &Drawing, c: &Comparison, folders: &Folders) -> Snapshot {
    let other: &Drawing = &c.other;
    let mut d = cur.clone();
    if folders.here.is_some() {
        for b in d.blocks.values_mut().filter(|b| has_relative_paths(b)) {
            rebase_block(Arc::make_mut(b), folders.here.as_deref());
        }
    }
    d.model = EntityStore::new();
    for l in &mut d.layouts {
        l.entities = EntityStore::new();
    }
    d.foreign = Arc::default();
    d.groups.clear();
    d.constraints.clear();
    // The per-object constraint bar and dimension exceptions name objects the copy doesn't have.
    d.parametric.settings.bar_exceptions.clear();
    d.parametric.settings.dim_exceptions.clear();
    d.hidden_objects.clear();
    d.read_notes.clear();
    d.refedit = None;
    d.skipped_records.clear();
    d.handseed = d.handseed.max(other.handseed);

    merge(&mut d.layers, &other.layers, |t| &t.name);
    merge(&mut d.linetypes, &other.linetypes, |t| &t.name);
    merge(&mut d.text_styles, &other.text_styles, |t| &t.name);
    merge(&mut d.dim_styles, &other.dim_styles, |t| &t.name);
    merge(&mut d.mleader_styles, &other.mleader_styles, |t| &t.name);
    merge(&mut d.table_styles, &other.table_styles, |t| &t.name);
    merge(&mut d.mline_styles, &other.mline_styles, |t| &t.name);

    // Compared again, so edits made since COMPARE ran are classified as they are now.
    let diff = super::diff(cur, other, &c.space);
    let ours: HashSet<Handle> = diff.added.iter().copied().chain(diff.changed.iter().map(|(a, ..)| *a)).collect();
    let here: Vec<(&Entity, Color)> = cur
        .space(&c.space)
        .into_iter()
        .flat_map(EntityStore::iter)
        .filter(|e| !matches!(e.kind, EntityKind::Viewport(_)))
        .take(MAX_OBJECTS)
        .map(|e| (e.as_ref(), if ours.contains(&e.handle) { THIS } else { COMMON }))
        .collect();
    // In the other drawing's draw order.
    let theirs: HashSet<Handle> = diff.removed.iter().copied().chain(diff.changed.iter().map(|(_, b, ..)| *b)).collect();
    let there: Vec<(&Entity, Color)> = other
        .space(&c.space)
        .into_iter()
        .flat_map(EntityStore::iter)
        .filter(|e| theirs.contains(&e.handle) && !matches!(e.kind, EntityKind::Viewport(_)))
        .map(|e| (e.as_ref(), OTHER))
        .collect();
    let mut tinted = Tinted::default();
    let (renames, blocks_left_out) = copy_blocks(&mut d, other, there.iter().map(|(e, _)| *e), folders.there.as_deref(), &mut tinted);

    // The other drawing's objects first, so this drawing's are drawn over them.
    let compared = add_group(&mut d, "COMPARE_COMPARED", &there, &renames, folders.there.as_deref(), &mut tinted);
    let current = add_group(&mut d, "COMPARE_CURRENT", &here, &Renames::new(), folders.here.as_deref(), &mut tinted);
    let uncolored = tinted.uncolored;
    Snapshot { drawing: d, current, compared, counts: (here.len(), there.len()), truncated: diff.truncated, blocks_left_out, uncolored }
}

/// Add the entries of `from` whose names `to` lacks (compared without letter case).
fn merge<T: Clone>(to: &mut Vec<T>, from: &[T], name: impl Fn(&T) -> &String) {
    for t in from {
        if !to.iter().any(|x| name(x).eq_ignore_ascii_case(name(t))) {
            to.push(t.clone());
        }
    }
}

/// Add copies of `objs` to model space with new handles, their block references renamed by
/// `renames` and their files rebased on `folder`, each painted in its comparison color, and
/// group them as `name` (or `name_2`… when that group name is taken). Returns the group's name.
fn add_group(d: &mut Drawing, name: &str, objs: &[(&Entity, Color)], renames: &Renames, folder: Option<&Path>, tinted: &mut Tinted) -> String {
    let mut members = Vec::new();
    for (e, color) in objs {
        d.ensure_layer(&e.common.layer);
        let h = d.new_handle();
        let mut copy = Entity { handle: h, ..(*e).clone() };
        detach(&mut copy.kind);
        rebase(&mut copy.kind, folder);
        rename_refs(&mut copy.kind, renames);
        paint(d, &mut copy, *color, tinted, 0);
        d.model.push(copy);
        members.push(h);
    }
    let name = free_name(name, d.groups.len(), |n| d.groups.iter().any(|g| g.name.eq_ignore_ascii_case(n)));
    if !members.is_empty() {
        d.groups.push(Group { name: name.clone(), description: String::new(), selectable: true, members });
    }
    name
}

/// Drop the links of an associative dimension or hatch to the objects it follows: the copies
/// have new handles, so the links would name objects the snapshot does not have.
fn detach(k: &mut EntityKind) {
    match k {
        EntityKind::Dimension(dm) => dm.assoc.clear(),
        EntityKind::Hatch(h) => {
            h.associative = false;
            h.loops.iter_mut().for_each(|l| l.sources.clear());
        }
        _ => {}
    }
}

/// The block names an object refers to.
fn block_refs(k: &EntityKind) -> Vec<String> {
    match k {
        EntityKind::Insert(i) => vec![i.block.clone()],
        EntityKind::Dimension(dm) => dm.block.iter().cloned().collect(),
        EntityKind::MLeader(m) => m.block.iter().map(|b| b.block.clone()).collect(),
        _ => Vec::new(),
    }
}

/// Call `f` on each block name an object refers to.
fn each_ref(k: &mut EntityKind, mut f: impl FnMut(&mut String)) {
    match k {
        EntityKind::Insert(i) => f(&mut i.block),
        EntityKind::Dimension(dm) => {
            if let Some(n) = dm.block.as_mut() {
                f(n);
            }
        }
        EntityKind::MLeader(m) => {
            if let Some(b) = m.block.as_mut() {
                f(&mut b.block);
            }
        }
        _ => {}
    }
}

/// Point the block references of an object at the names its blocks were copied under.
fn rename_refs(k: &mut EntityKind, renames: &Renames) {
    each_ref(k, |n| {
        if let Some(new) = renames.get(&n.to_ascii_uppercase()) {
            n.clone_from(new);
        }
    });
}

/// Recolored copies of blocks and styles made so far, and whether a block was left in its own
/// colors because the nesting or copy limit was reached.
#[derive(Default)]
struct Tinted {
    /// (comparison tag, "block", "table style" or "dimension style", upper-case name) → the
    /// copy's name.
    names: HashMap<(&'static str, &'static str, String), String>,
    uncolored: bool,
    /// Objects copied into block definitions so far, counted against [`MAX_COPIED`].
    copied: usize,
    next: NextFree,
    /// The drawing's block names in upper case, filled on the first block copy. Only
    /// [`tinted_block`] adds blocks after that, and it adds their names here too.
    blocks: Option<HashSet<String>>,
}

/// The next index to try for each name base. Names are only added while a snapshot is built,
/// so an index found taken stays taken, and each search starts where the last one for the same
/// base stopped: a name costs O(1) amortised instead of a scan from 1.
#[derive(Default)]
struct NextFree(HashMap<(&'static str, String), usize>);

impl NextFree {
    /// The first of `name(i)` for `i` from 1 that `taken` doesn't hold, skipping the indices
    /// earlier searches for `base` in namespace `kind` passed. `n` is how many names there are,
    /// so one of `name(1)`…`name(n + 1)` is free. The caller adds the returned name.
    fn find(&mut self, kind: &'static str, base: &str, n: usize, name: impl Fn(usize) -> String, taken: impl Fn(&str) -> bool) -> String {
        let key = (kind, base.to_ascii_uppercase());
        let start = self.0.get(&key).copied().unwrap_or(1);
        let i = (start..=n.saturating_add(1)).find(|&i| !taken(&name(i))).unwrap_or_else(|| n.saturating_add(2).max(start));
        self.0.insert(key, i.saturating_add(1));
        name(i)
    }
}

/// `base` for 1, `base_2`, `base_3`… after it.
fn suffixed(base: &str, i: usize) -> String {
    if i == 1 { base.to_string() } else { format!("{base}_{i}") }
}

/// The word a comparison color adds to the names of its recolored copies.
fn tag(color: Color) -> &'static str {
    if color == THIS {
        "CURRENT"
    } else if color == OTHER {
        "COMPARED"
    } else {
        "COMMON"
    }
}

/// `base`, or `base_2`, `base_3`… when `taken` holds that name. `n` is how many names there are.
fn free_name(base: &str, n: usize, taken: impl Fn(&str) -> bool) -> String {
    (1..=n + 1).map(|i| suffixed(base, i)).find(|c| !taken(c)).unwrap_or_else(|| suffixed(base, n + 2))
}

/// Give an object its comparison color: its own color, its attributes' colors, and, through
/// recolored copies of the blocks and styles it refers to, the color of everything they draw.
fn paint(d: &mut Drawing, e: &mut Entity, color: Color, tinted: &mut Tinted, depth: usize) {
    e.common.color = color;
    let attribs = match &mut e.kind {
        EntityKind::Insert(i) => Some(&mut i.attribs),
        EntityKind::MLeader(m) => m.block.as_mut().map(|b| &mut b.attribs),
        _ => None,
    };
    for a in attribs.into_iter().flatten() {
        a.props.color = Some(color);
        if let Some(ml) = a.multiline.as_mut() {
            ml.mtext.contents = without_color_codes(&ml.mtext.contents);
        }
    }
    // Colors an object carries apart from its own: inline MTEXT colors (in multiline attributes
    // too), a dimension style's DIMCLRD, DIMCLRE and DIMCLRT, gradient colors, multiline element
    // colors, the style colors of a table or a tolerance (through a recolored copy of the style)
    // and inline cell colors. Hatch backgrounds, multiline fills and table cell fills are left out.
    match &mut e.kind {
        EntityKind::MText(t) => t.contents = without_color_codes(&t.contents),
        EntityKind::AttDef(a) => {
            if let Some(ml) = a.multiline.as_mut() {
                ml.mtext.contents = without_color_codes(&ml.mtext.contents);
            }
        }
        EntityKind::MLeader(m) => {
            if let Some(t) = m.text.as_mut() {
                t.contents = without_color_codes(&t.contents);
            }
        }
        EntityKind::Dimension(dm) => {
            if let Ok(by_block) = serde_json::to_value(Color::ByBlock) {
                for var in ["DIMCLRD", "DIMCLRE", "DIMCLRT"] {
                    dm.overrides.retain(|k, _| DimStyle::field_name(k) != DimStyle::field_name(var));
                    dm.overrides.insert(var.into(), by_block.clone());
                }
            }
        }
        EntityKind::Tolerance(t) => {
            if let Some(style) = tinted_dim_style(d, &t.style, color, tinted) {
                t.style = style;
            }
        }
        EntityKind::Hatch(h) => {
            if let Some(g) = h.gradient.as_mut() {
                (g.color1, g.color2) = (color, color);
            }
            h.background = None;
        }
        EntityKind::MLine(m) => {
            for el in &mut m.def.elements {
                el.color = Color::ByBlock;
            }
            m.def.fill = None;
        }
        EntityKind::Table(t) => {
            for cell in t.cells.iter_mut().flatten() {
                cell.text = without_color_codes(&cell.text);
            }
            if let Some(style) = tinted_table_style(d, &t.style, color, tinted) {
                t.style = style;
            }
        }
        _ => {}
    }
    if let EntityKind::Dimension(dm) = &mut e.kind {
        // A dimension whose block can't be copied (past the limits) is drawn again from its
        // geometry: its DIMCLR* overrides are ByBlock, so the original block would keep its colors.
        dm.block = dm.block.take().and_then(|n| tinted_block(d, &n, color, tinted, depth));
        return;
    }
    each_ref(&mut e.kind, |n| {
        if let Some(copy) = tinted_block(d, n, color, tinted, depth) {
            *n = copy;
        }
    });
}

/// The name of a copy of dimension style `name` that draws lines and text in the object's
/// color, made on first use. `None` when the drawing has no such style.
fn tinted_dim_style(d: &mut Drawing, name: &str, color: Color, tinted: &mut Tinted) -> Option<String> {
    let key = (tag(color), "dimension style", name.to_ascii_uppercase());
    if let Some(n) = tinted.names.get(&key) {
        return Some(n.clone());
    }
    let st = d.dim_styles.iter().find(|s| s.name.eq_ignore_ascii_case(name))?.clone();
    let base = format!("COMPARE_{}_{}", tag(color), st.name);
    let new = tinted.next.find(
        "dimension style",
        &base,
        d.dim_styles.len(),
        |i| suffixed(&base, i),
        |n| d.dim_styles.iter().any(|s| s.name.eq_ignore_ascii_case(n)),
    );
    d.dim_styles.push(DimStyle {
        name: new.clone(),
        dim_line_color: Color::ByBlock,
        ext_line_color: Color::ByBlock,
        text_color: Color::ByBlock,
        ..st
    });
    tinted.names.insert(key, new.clone());
    Some(new)
}

/// The name of a copy of table style `name` whose cell styles draw text and borders in the
/// table's color and fill no cells, made on first use. `None` when the drawing has no such style.
fn tinted_table_style(d: &mut Drawing, name: &str, color: Color, tinted: &mut Tinted) -> Option<String> {
    let key = (tag(color), "table style", name.to_ascii_uppercase());
    if let Some(n) = tinted.names.get(&key) {
        return Some(n.clone());
    }
    let st = d.table_styles.iter().find(|s| s.name.eq_ignore_ascii_case(name))?.clone();
    let base = format!("COMPARE_{}_{}", tag(color), st.name);
    let new = tinted.next.find(
        "table style",
        &base,
        d.table_styles.len(),
        |i| suffixed(&base, i),
        |n| d.table_styles.iter().any(|s| s.name.eq_ignore_ascii_case(n)),
    );
    let names =
        [CellStyle::TITLE, CellStyle::HEADER, CellStyle::DATA].into_iter().map(str::to_string).chain(st.cell_styles.iter().map(|c| c.name.clone()));
    let mut cell_styles: Vec<CellStyle> = Vec::new();
    for n in names {
        if cell_styles.iter().any(|c| c.name.eq_ignore_ascii_case(&n)) {
            continue;
        }
        let mut cs = st.cell_style(&n);
        cs.name = n;
        cs.text_color = Color::ByBlock;
        cs.fill = None;
        let b = &mut cs.borders;
        for border in [&mut b.top, &mut b.inside_h, &mut b.bottom, &mut b.left, &mut b.inside_v, &mut b.right] {
            border.color = Color::ByBlock;
        }
        cell_styles.push(cs);
    }
    d.table_styles.push(TableStyle { name: new.clone(), cell_styles, ..st });
    tinted.names.insert(key, new.clone());
    Some(new)
}

/// MTEXT contents without inline color codes (`\C1;`, `\c255;`), so the text takes the
/// object's color. An escaped backslash (`\\`) stays.
fn without_color_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.peek().copied() {
            Some('C' | 'c') => {
                for d in it.by_ref() {
                    if d == ';' {
                        break;
                    }
                }
            }
            Some(n) => {
                out.push('\\');
                out.push(n);
                it.next();
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The name of a copy of block `name` whose objects all have `color`, made on first use. `None`
/// (the reference keeps the original block) for an xref, a missing block, or past the nesting
/// and copy limits.
fn tinted_block(d: &mut Drawing, name: &str, color: Color, tinted: &mut Tinted, depth: usize) -> Option<String> {
    let key = (tag(color), "block", name.to_ascii_uppercase());
    if let Some(n) = tinted.names.get(&key) {
        return Some(n.clone());
    }
    let b = d.block(name)?.clone();
    if b.xref_path.is_some() {
        return None;
    }
    if depth >= MAX_BLOCK_DEPTH || tinted.names.len() >= MAX_BLOCKS || tinted.copied.saturating_add(b.entities.len()) > MAX_COPIED {
        tinted.uncolored = true;
        return None;
    }
    tinted.copied += b.entities.len();
    // A dimension's block copy stays a `*D` block, so saving keeps its geometry instead of
    // drawing the dimension again.
    let dim_block = b.anonymous && b.name.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("*D"));
    let Tinted { next, blocks, .. } = tinted;
    let taken = blocks.get_or_insert_with(|| d.blocks.keys().map(|k| k.to_ascii_uppercase()).collect());
    let is_taken = |n: &str| taken.contains(&n.to_ascii_uppercase());
    let new = if dim_block {
        next.find("block", "*D", d.blocks.len(), |i| format!("*D{i}"), is_taken)
    } else {
        let base = format!("COMPARE_{}_{}", tag(color), b.name.trim_start_matches('*'));
        next.find("block", &base, d.blocks.len(), |i| suffixed(&base, i), is_taken)
    };
    taken.insert(new.to_ascii_uppercase());
    tinted.names.insert(key, new.clone());
    // Hold the name while the nested copies pick theirs.
    d.blocks.insert(new.clone(), Arc::new(Block::new(&new)));
    let mut copy = Block { name: new.clone(), anonymous: dim_block, entities: EntityStore::new(), ..(*b).clone() };
    for e in b.entities.iter() {
        let h = d.new_handle();
        let mut e = Entity { handle: h, ..(**e).clone() };
        detach(&mut e.kind);
        paint(d, &mut e, color, tinted, depth + 1);
        copy.entities.push(e);
    }
    d.blocks.insert(new.clone(), Arc::new(copy));
    Some(new)
}

/// The name each block copied from the other drawing has in the snapshot, by its upper-case
/// name there.
type Renames = HashMap<String, String>;

/// Copy into `d` the blocks of `other` that `objs` refer to, directly or through other blocks.
/// A named block `d` already has keeps `d`'s definition when it draws the same thing there (the
/// same objects, base point, units, flags and xref file, in it and in the blocks it refers to);
/// otherwise the other drawing's block is copied as `PART_2` (or `PART_3`…). An anonymous block
/// (`*D3`, `*U7`…) whose name `d` already uses is copied under the next free name with the same
/// prefix. Copied
/// objects get new handles, and relative file paths are rebased on `folder`. The flag is true
/// when more than [`MAX_BLOCKS`] blocks or [`MAX_COPIED`] objects were needed and the rest were
/// left out.
fn copy_blocks<'a>(
    d: &mut Drawing,
    other: &Drawing,
    objs: impl Iterator<Item = &'a Entity>,
    folder: Option<&Path>,
    tinted: &mut Tinted,
) -> (Renames, bool) {
    let mut frontier: Vec<String> = objs.flat_map(|e| block_refs(&e.kind)).collect();
    let mut seen = HashSet::new();
    let mut needed: Vec<Arc<Block>> = Vec::new();
    let mut left_out = false;
    // `d`'s relative paths were made absolute from its own folder before the copy.
    let (mut mine, mut theirs) = (super::Signer::with_frames(d, None), super::Signer::with_frames(other, folder));
    // Whether `d`'s block `n` draws what `other`'s block `b` draws. Past the signers' work limit
    // the signatures no longer cover every object, so the blocks count as different.
    let mut same = |n: &str, b: &Block| {
        !b.anonymous
            && d.block(n).is_some()
            && mine.block(n, 0) == theirs.block(n, 0)
            && mine.work < super::MAX_BLOCK_WORK
            && theirs.work < super::MAX_BLOCK_WORK
    };
    while let Some(n) = frontier.pop() {
        let key = n.to_ascii_uppercase();
        if seen.contains(&key) {
            continue;
        }
        if seen.len() >= MAX_BLOCKS {
            left_out |= other.block(&n).is_some_and(|b| !same(&n, b));
            continue;
        }
        seen.insert(key);
        let Some(b) = other.block(&n) else { continue };
        if same(&n, b) {
            continue;
        }
        if tinted.copied.saturating_add(b.entities.len()) > MAX_COPIED {
            left_out = true;
            continue;
        }
        tinted.copied += b.entities.len();
        frontier.extend(b.entities.iter().flat_map(|e| block_refs(&e.kind)));
        needed.push(b.clone());
    }
    // Names `d` does not use stay; the others get free names, avoiding the kept ones.
    let mut renames = Renames::new();
    let mut taken: HashSet<String> = d.blocks.keys().map(|k| k.to_ascii_uppercase()).collect();
    let mut next = NextFree::default();
    for b in needed.iter().filter(|b| d.block(&b.name).is_none()) {
        taken.insert(b.name.to_ascii_uppercase());
        renames.insert(b.name.to_ascii_uppercase(), b.name.clone());
    }
    for b in needed.iter().filter(|b| d.block(&b.name).is_some()) {
        let is_taken = |n: &str| taken.contains(&n.to_ascii_uppercase());
        let free = if b.anonymous {
            let prefix: String = b.name.chars().take_while(|c| *c == '*' || c.is_ascii_alphabetic()).collect();
            next.find("anonymous block", &prefix, taken.len(), |i| format!("{prefix}{i}"), is_taken)
        } else {
            next.find("block", &b.name, taken.len(), |i| suffixed(&b.name, i), is_taken)
        };
        taken.insert(free.to_ascii_uppercase());
        renames.insert(b.name.to_ascii_uppercase(), free);
    }
    for b in needed {
        let Some(name) = renames.get(&b.name.to_ascii_uppercase()).cloned() else { continue };
        let mut copy = Block { name: name.clone(), entities: EntityStore::new(), ..(*b).clone() };
        for e in b.entities.iter() {
            let h = d.new_handle();
            let mut e = Entity { handle: h, ..(**e).clone() };
            detach(&mut e.kind);
            rename_refs(&mut e.kind, &renames);
            copy.entities.push(e);
        }
        rebase_block(&mut copy, folder);
        d.blocks.insert(name, Arc::new(copy));
    }
    (renames, left_out)
}
