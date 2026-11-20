//! REFEDIT, REFSET and REFCLOSE: edit a block reference or an xref in place.
//!
//! REFEDIT hides the picked reference and puts the block's objects (all, or the ones named) into
//! the drawing where the reference shows them: the *working set*, ordinary objects that every
//! command edits. The rest of the block stands there as faded copies, and everything else in the
//! drawing is locked ([`crate::cmd::curves::is_locked`], enforced by [`after_command`]) and drawn
//! faded (XFADECTL). Objects drawn while editing join the working set. REFSET adds objects to or
//! removes them from it. REFCLOSE writes the working set back into the block definition, so every
//! reference to the block shows the change, or discards the edit.
//!
//! A reference nested in another block is picked through its parents (`path`, or "Select nesting
//! level [OK/Next]" at the command line): its own block is edited, the parents' other objects
//! stand faded around it.
//!
//! An xref (or a block that came with one) is edited the same way, and REFCLOSE Save writes the
//! change into its source drawing the way SAVE writes a drawing: the previous file is kept as its
//! `.bak`, the new one replaces it atomically, and a symbolic link planted beside it or put at its
//! path during the edit never redirects the write. The save is refused when the file is
//! read-only, open in another program (or another drawing tab), or changed since it was loaded.
//! The xref is then loaded again from the file it wrote.
//!
//! The state is [`Drawing::refedit`]: it travels with undo, is never written to a file (files
//! are written from [`Drawing::committed`]) and ends with the drawing it belongs to.
//!
//! Not yet: annotative scale contexts, attribute synchronisation and xrefs nested in xrefs (open
//! those with XOPEN).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Block, Drawing, Entity, EntityKind, EntityStore, Handle, RefEdit, Transparency, WorkItem, XrefSource, XrefStatus};
use cadcraft_geom::Mat3;
use serde_json::{Value, json};

use super::super::curves::handle_value;
use super::super::machines::{SelOutcome, SelectPhase};
use super::references_block;
use super::xref::load::{self, Sym};
use crate::cmd::{CommandSpec, bad, bool_or, has_doc, str_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most nesting levels offered for one picked reference.
const MAX_LEVELS: usize = 256;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("refedit", "Edit Reference In-Place", run_refedit)
            .menu(&["Tools", "Xref and Block In-place Editing", "Edit Reference In-Place"])
            .alias(&["-refedit"])
            .params("{handle? | handles? | name? (a block or xref reference, default the selection), path?: [handles of nested block references, outermost first] | nested?: block name (the first nested reference to it), objects?: [block object handles] (default all)} → {block, insert, working: [handles], faded, path, xref?: {name, file}}. Editing an xref writes its file back on REFCLOSE Save.")
            .enabled(has_doc)
            .interactive(|_| Ok(Box::new(RefeditM::default()))),
        CommandSpec::new("refedit.levels", "Reference Nesting Levels", run_levels)
            .params("{handle (a block or xref reference)} → {levels: [{name, path: [handles], xref}]} (the picked reference first, then the references nested in it)")
            .enabled(has_doc)
            .noundo(),
        CommandSpec::new("refset", "Working Set", run_refset)
            .menu(&["Tools", "Xref and Block In-place Editing", "Add to Working Set"])
            .params("{add?: [handles], remove?: [handles]} (or {handles, mode?: \"add\"|\"remove\"}); with neither: lists the working set → {working: [handles], added, removed}")
            .enabled(editing)
            .interactive(|_| Ok(Box::new(RefsetM::default()))),
        CommandSpec::new("refclose", "Save Reference Edits", run_refclose)
            .menu(&["Tools", "Xref and Block In-place Editing", "Save Reference Edits"])
            .params("{option?: \"save\"|\"discard\" | save?: bool (default true)} → {block, saved, objects, file? (an xref's source drawing written), backup? (its previous file, as SAVE names it; null when none was kept)}")
            .enabled(editing)
            .interactive(|_| Ok(Box::new(RefcloseM))),
    ]
}

fn editing(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.doc().is_ok_and(|d| d.refedit.is_some()) { Ok(()) } else { Err("no block reference is being edited (REFEDIT)".into()) }
}

fn active(s: &Session, cmd: &str) -> Result<RefEdit> {
    s.doc()?.refedit.clone().ok_or_else(|| bad(cmd, "no block reference is being edited (REFEDIT)"))
}

/// How much the block's other objects fade (XFADECTL, 0–90 percent).
fn xfadectl(s: &Session) -> Result<u8> {
    Ok(u8::try_from(s.doc()?.header.i64("XFADECTL", 50).clamp(0, 90)).unwrap_or(50))
}

/// A block object as the drawing shows it through the edited reference; `fade` (XFADECTL, a
/// percentage) shows the block's other objects faded.
fn to_host(to_world: &Mat3, e: &Entity, h: Handle, fade: Option<u8>) -> Entity {
    let mut c = e.clone();
    c.handle = h;
    c.kind.transform(to_world);
    if let Some(f) = fade {
        c.common.transparency = Transparency::Percent(f.min(90));
    }
    c
}

/// A drawing object as it belongs in the block, with the block object's handle `h`.
fn to_block(inv: &Mat3, e: &Entity, h: Handle) -> Entity {
    let mut c = e.clone();
    c.handle = h;
    c.kind.transform(inv);
    c
}

fn put(d: &mut Drawing, r: &RefEdit, e: Entity) -> Result<()> {
    d.ensure_layer(&e.common.layer);
    d.space_mut(&r.space).ok_or_else(|| bad("refedit", "the space of the reference is gone"))?.push(e);
    Ok(())
}

fn hexes(hs: impl IntoIterator<Item = Handle>) -> Vec<String> {
    hs.into_iter().map(|h| h.hex()).collect()
}

// ---------------- nesting ----------------

/// One level of a picked reference: a block, how its objects show in the drawing, and the
/// reference inside it that leads to the next level (none at the last).
struct Level {
    block: Arc<Block>,
    to_world: Mat3,
    next: Option<Handle>,
}

/// Whether a reference with transform `m` can be edited in place (finite, not flat, scaled the
/// same in X and Y).
fn editable_transform(m: &Mat3, sx: f64, sy: f64) -> bool {
    let finite = [m.a, m.b, m.c, m.d, m.e, m.f].iter().all(|v| v.is_finite());
    finite && m.det().abs() >= 1e-12 && (sx.abs() - sy.abs()).abs() <= 1e-9 * sx.abs().max(sy.abs())
}

/// The levels from top reference `ins` down `path` (references inside the block definitions).
fn levels(d: &Drawing, ins: &cadcraft_doc::Insert, path: &[Handle]) -> Result<Vec<Level>> {
    let mut block = d.block(&ins.block).cloned().ok_or_else(|| bad("refedit", format!("block \"{}\" not found", ins.block)))?;
    let mut m = ins.transform(block.base.xy());
    if !editable_transform(&m, ins.scale.x, ins.scale.y) {
        return Err(bad("refedit", "the reference must be scaled equally in X and Y (and not flat) to be edited in place"));
    }
    if path.len() >= cadcraft_doc::MAX_BLOCK_DEPTH {
        return Err(bad("refedit", "the reference is nested too deeply"));
    }
    let mut out = Vec::with_capacity(path.len() + 1);
    for h in path {
        let e = block.entities.get(*h).ok_or_else(|| bad("refedit", format!("object {} is not a reference in block \"{}\"", h.hex(), block.name)))?;
        let EntityKind::Insert(child) = &e.kind else { return Err(bad("refedit", format!("object {} is not a block reference", h.hex()))) };
        let cb = d.block(&child.block).cloned().ok_or_else(|| bad("refedit", format!("block \"{}\" not found", child.block)))?;
        let cm = m.then_before(child.transform(cb.base.xy()));
        if !editable_transform(&cm, child.scale.x, child.scale.y) {
            return Err(bad("refedit", format!("the nested reference to \"{}\" must be scaled equally in X and Y", cb.name)));
        }
        out.push(Level { block, to_world: m, next: Some(*h) });
        block = cb;
        m = cm;
    }
    out.push(Level { block, to_world: m, next: None });
    Ok(out)
}

/// The references nested in block `name` (depth first): (path of handles, block name).
fn nested(d: &Drawing, name: &str, path: &mut Vec<Handle>, out: &mut Vec<(Vec<Handle>, String)>) {
    if path.len() + 1 >= cadcraft_doc::MAX_BLOCK_DEPTH || out.len() >= MAX_LEVELS {
        return;
    }
    let Some(b) = d.block(name) else { return };
    for e in b.entities.iter() {
        let EntityKind::Insert(i) = &e.kind else { continue };
        // Generated geometry (dimensions, arrays) is not offered.
        if i.block.starts_with('*') || d.block(&i.block).is_none_or(|c| c.array.is_some()) || out.len() >= MAX_LEVELS {
            continue;
        }
        path.push(e.handle);
        out.push((path.clone(), i.block.clone()));
        nested(d, &i.block, path, out);
        path.pop();
    }
}

/// The nesting levels of reference `ih`: the reference itself, then every reference nested in it.
fn level_list(d: &Drawing, ih: Handle) -> Result<Vec<(Vec<Handle>, String)>> {
    let Some(EntityKind::Insert(ins)) = d.entity(ih).map(|e| &e.kind) else { return Err(bad("refedit", "select a block reference")) };
    let mut out = vec![(Vec::new(), ins.block.clone())];
    nested(d, &ins.block, &mut Vec::new(), &mut out);
    Ok(out)
}

fn run_levels(s: &mut Session, p: &Value) -> Result<Value> {
    let ih = targets(s, p)?.into_iter().next().ok_or_else(|| bad("refedit.levels", "select a block reference"))?;
    let d = s.doc()?;
    let levels: Vec<Value> = level_list(d, ih)?
        .into_iter()
        .map(|(path, name)| json!({ "name": name, "path": hexes(path), "xref": xref_of(d, &name).is_some() }))
        .collect();
    Ok(json!({ "levels": levels }))
}

// ---------------- xrefs ----------------

/// The xref block `name` belongs to: (the xref's name, the block of its source drawing or `None`
/// for its model space). `None` for a block of this drawing.
fn xref_of(d: &Drawing, name: &str) -> Option<(String, Option<String>)> {
    let b = d.block(name)?;
    if b.xref_path.is_some() {
        return Some((b.name.clone(), None));
    }
    let (root, rest) = name.split_once('|')?;
    let x = d.block(root).filter(|x| x.xref_path.is_some())?;
    Some((x.name.clone(), Some(rest.to_string())))
}

/// The source file of xref `name`, checked for an edit in place: found, not open in another
/// drawing tab, writable, not changed since it was loaded.
fn xref_file(s: &Session, name: &str) -> Result<std::path::PathBuf> {
    if cfg!(target_arch = "wasm32") {
        return Err(bad("refedit", "editing xrefs in place is not available on the web; open the xref's drawing instead"));
    }
    let d = s.doc()?;
    let b = d.block(name).ok_or_else(|| bad("refedit", format!("no xref \"{name}\"")))?;
    if b.xref_status != XrefStatus::Loaded {
        return Err(bad("refedit", format!("xref \"{}\" is not loaded ({}); reload it first", b.name, b.xref_status.label())));
    }
    let path = b.xref_path.clone().unwrap_or_default();
    let host = s.state().ok().and_then(|st| st.path.clone());
    let file = load::find(&path, host.as_deref().map(std::path::Path::new), false).map_err(|_| bad("refedit", format!("{path}: file not found")))?;
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let me = canon(&file);
    for (i, st) in s.docs.iter().enumerate() {
        if i != s.active && st.path.as_deref().is_some_and(|p| canon(std::path::Path::new(p)) == me) {
            return Err(bad("refedit", format!("{} is open in another drawing tab: edit it there, or close it first", file.display())));
        }
    }
    writable(&file).map_err(|e| bad("refedit", e))?;
    if let (Some(then), Some(now)) = (load::loaded_stamp(&file), load::stamp(&file))
        && then != now
    {
        return Err(bad("refedit", format!("{} changed since xref \"{}\" was loaded; reload it first", file.display(), b.name)));
    }
    Ok(file)
}

/// Whether `file` may be written: not read-only and not open in another program (AutoCAD's
/// `.dwl` lock file beside it, or a file that can't be opened for writing).
fn writable(file: &std::path::Path) -> std::result::Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = file;
        Err("files are not available on the web".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let meta = std::fs::metadata(file).map_err(|e| format!("{}: {e}", file.display()))?;
        if meta.permissions().readonly() {
            return Err(format!("{} is read-only", file.display()));
        }
        let lock = file.with_extension("dwl");
        if lock.is_file() {
            return Err(format!("{} is open in another program (lock file {})", file.display(), lock.display()));
        }
        std::fs::OpenOptions::new().append(true).open(file).map(|_| ()).map_err(|e| format!("{} can't be written: {e}", file.display()))
    }
}

/// The objects of the source drawing a block of xref `x` came from, matched one to one with the
/// block's objects (the order loading keeps). Refused when they no longer match.
fn source_handles(blk: &Block, src: &Drawing, inner: Option<&str>) -> Result<Vec<(Handle, Handle)>> {
    let store = match inner {
        None => &src.model,
        Some(n) => {
            let b = src.block(n).ok_or_else(|| bad("refedit", format!("block \"{n}\" is no longer in the xref's drawing; reload the xref")))?;
            if b.xref_path.is_some() {
                return Err(bad("refedit", format!("\"{n}\" is an xref of the xref's drawing: open it with XOPEN to edit it")));
            }
            &b.entities
        }
    };
    let kind = |e: &Entity| std::mem::discriminant(&e.kind);
    if store.len() != blk.entities.len() || store.iter().zip(blk.entities.iter()).any(|(a, b)| kind(a) != kind(b)) {
        return Err(bad("refedit", "the xref's drawing no longer matches what was loaded; reload the xref first"));
    }
    Ok(blk.entities.iter().zip(store.iter()).map(|(a, b)| (a.handle, b.handle)).collect())
}

// ---------------- REFEDIT ----------------

fn run_refedit(s: &mut Session, p: &Value) -> Result<Value> {
    if s.doc()?.refedit.is_some() {
        return Err(bad("refedit", "a reference is already being edited; finish with REFCLOSE"));
    }
    let d = s.doc()?;
    let picked = match str_param(p, "name") {
        Some(n) => d
            .model
            .iter()
            .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
            .find(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block.eq_ignore_ascii_case(n)))
            .map(|e| e.handle),
        None => targets(s, p)?.into_iter().find(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Insert(_)))),
    };
    let ih = picked.ok_or_else(|| bad("refedit", "select a block reference"))?;
    let (ins, space) = match d.entity(ih).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => (i.clone(), d.space_of(ih).ok_or_else(|| bad("refedit", "the reference is in no space"))?),
        _ => return Err(bad("refedit", "select a block reference")),
    };
    let path: Vec<Handle> = match (p.get("path"), str_param(p, "nested")) {
        (Some(Value::Array(a)), _) => {
            a.iter().map(|v| handle_value(v).ok_or_else(|| bad("refedit", format!("`path`: {v} is not an object handle")))).collect::<Result<_>>()?
        }
        (Some(Value::Null) | None, Some(n)) => level_list(d, ih)?
            .into_iter()
            .find(|(path, name)| !path.is_empty() && name.eq_ignore_ascii_case(n))
            .map(|(path, _)| path)
            .ok_or_else(|| bad("refedit", format!("no reference to \"{n}\" is nested in this one")))?,
        (Some(Value::Null) | None, None) => Vec::new(),
        (Some(v), _) => return Err(bad("refedit", format!("`path` must be an array of object handles, got {v}"))),
    };
    let levels = levels(d, &ins, &path)?;
    let Some(edit) = levels.last() else { return Err(bad("refedit", "select a block reference")) };
    let blk = edit.block.clone();
    let to_world = edit.to_world;
    if blk.name.starts_with('*') || blk.array.is_some() {
        return Err(bad("refedit", format!("\"{}\" is generated geometry and can't be edited in place", blk.name)));
    }
    // An xref's block: where it is written back.
    let xref = match xref_of(d, &blk.name) {
        Some((x, inner)) => {
            let file = xref_file(s, &x)?;
            let src = load::read_file(&file).map_err(|e| bad("refedit", format!("{}: {e}", file.display())))?;
            let sources = source_handles(&blk, &src, inner.as_deref())?;
            let real = std::fs::canonicalize(&file).map_err(|e| bad("refedit", format!("{}: {e}", file.display())))?;
            Some(XrefSource {
                xref: x,
                file: file.to_string_lossy().into_owned(),
                real: real.to_string_lossy().into_owned(),
                block: inner,
                stamp: load::stamp(&file),
                sources,
            })
        }
        None => None,
    };
    let d = s.doc()?;
    let chosen: Option<HashSet<Handle>> = match p.get("objects") {
        None | Some(Value::Null) => None,
        Some(Value::String(a)) if a.eq_ignore_ascii_case("all") => None,
        Some(Value::Array(a)) => {
            let mut set = HashSet::new();
            for v in a {
                let h = handle_value(v).ok_or_else(|| bad("refedit", format!("`objects`: {v} is not an object handle")))?;
                if blk.entities.get(h).is_none() {
                    return Err(bad("refedit", format!("object {} is not in block \"{}\"", h.hex(), blk.name)));
                }
                set.insert(h);
            }
            if set.is_empty() {
                return Err(bad("refedit", "`objects` is empty"));
            }
            Some(set)
        }
        Some(v) => return Err(bad("refedit", format!("`objects` must be \"all\" or an array of handles, got {v}"))),
    };
    let was_visible = d.entity(ih).is_some_and(|e| e.common.visible);
    let mut r = RefEdit {
        block: blk.name.clone(),
        insert: ih,
        insert_was_visible: was_visible,
        space,
        to_world,
        items: Vec::new(),
        ghosts: Vec::new(),
        released: Vec::new(),
        path: path.clone(),
        xref,
    };
    let fade = xfadectl(s)?;
    let doc = s.doc_mut()?;
    for e in blk.entities.iter() {
        let working = chosen.as_ref().is_none_or(|c| c.contains(&e.handle));
        let h = doc.new_handle();
        put(doc, &r, to_host(&to_world, e, h, (!working).then_some(fade)))?;
        if working {
            r.items.push(WorkItem { copy: h, source: Some(e.handle) });
        } else {
            r.ghosts.push((h, e.handle));
        }
    }
    // The parents' other objects stand faded around a nested block.
    for level in levels.iter().filter(|l| l.next.is_some()) {
        for e in level.block.entities.iter().filter(|e| Some(e.handle) != level.next && !matches!(e.kind, EntityKind::AttDef(_))) {
            let h = doc.new_handle();
            put(doc, &r, to_host(&level.to_world, e, h, Some(fade)))?;
            r.ghosts.push((h, e.handle));
        }
    }
    doc.modify_entity(ih, |e| e.common.visible = false).map_err(|e| bad("refedit", e.to_string()))?;
    let out = json!({
        "block": r.block,
        "insert": ih.hex(),
        "working": hexes(r.items.iter().map(|i| i.copy)),
        "faded": r.ghosts.len(),
        "path": hexes(r.path.iter().copied()),
        "xref": r.xref.as_ref().map(|x| json!({ "name": x.xref, "file": x.file })),
    });
    let sel: Vec<Handle> = r.items.iter().map(|i| i.copy).collect();
    doc.refedit = Some(r);
    s.set_selection(sel);
    Ok(out)
}

// ---------------- REFSET ----------------

fn handle_list(p: &Value, key: &str) -> Result<Vec<Handle>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => {
            a.iter().map(|v| handle_value(v).ok_or_else(|| bad("refset", format!("`{key}`: {v} is not an object handle")))).collect()
        }
        Some(v) => Err(bad("refset", format!("`{key}` must be an array of object handles, got {v}"))),
    }
}

fn run_refset(s: &mut Session, p: &Value) -> Result<Value> {
    let (mut add, mut remove) = (handle_list(p, "add")?, handle_list(p, "remove")?);
    let hs = handle_list(p, "handles")?;
    if str_param(p, "mode").is_some_and(|m| m.to_ascii_lowercase().starts_with('r')) {
        remove.extend(hs)
    } else {
        add.extend(hs)
    }
    apply_refset(s, &add, &remove)
}

fn apply_refset(s: &mut Session, add: &[Handle], remove: &[Handle]) -> Result<Value> {
    let mut r = active(s, "refset")?;
    let inv = r.to_world.inverse().ok_or_else(|| bad("refset", "the reference can't be inverted"))?;
    let d = s.doc()?;
    for h in add.iter().chain(remove) {
        if d.entity(*h).is_none() {
            return Err(bad("refset", format!("no object {}", h.hex())));
        }
    }
    if let Some(h) = remove.iter().find(|h| !r.is_working(**h)) {
        return Err(bad("refset", format!("object {} is not in the working set", h.hex())));
    }
    let blk = d.block(&r.block).cloned().ok_or_else(|| bad("refset", format!("block \"{}\" is gone; REFCLOSE Discard", r.block)))?;
    let (mut added, mut removed) = (0usize, 0usize);
    let fade = xfadectl(s)?;
    let doc = s.doc_mut()?;
    for h in add {
        if *h == r.insert || r.is_working(*h) {
            continue;
        }
        if let Some(pos) = r.ghosts.iter().position(|(g, _)| g == h) {
            // A faded object of the edited block joins the working set (with the edits it had
            // before); a parent's faded object (nested edits) can't.
            let src = r.ghosts.get(pos).map(|g| g.1);
            let Some(src) = src.filter(|src| blk.entities.get(*src).is_some()) else { continue };
            r.ghosts.remove(pos);
            doc.remove_entity(*h);
            let cur = match r.released.iter().position(|e| e.handle == src) {
                Some(i) => Some(r.released.remove(i)),
                None => blk.entities.get(src).map(|e| (**e).clone()),
            };
            if let Some(cur) = cur {
                let c = doc.new_handle();
                put(doc, &r, to_host(&r.to_world, &cur, c, None))?;
                r.items.push(WorkItem { copy: c, source: Some(src) });
                added += 1;
            }
        } else {
            r.items.push(WorkItem { copy: *h, source: None });
            added += 1;
        }
    }
    for h in remove {
        let Some(pos) = r.items.iter().position(|i| i.copy == *h) else { continue };
        let item = r.items.remove(pos);
        if let Some(src) = item.source
            && let Some(e) = doc.entity(*h).map(|e| (**e).clone())
        {
            // Back to the block, edits kept, shown faded.
            let kept = to_block(&inv, &e, src);
            doc.remove_entity(*h);
            let g = doc.new_handle();
            put(doc, &r, to_host(&r.to_world, &kept, g, Some(fade)))?;
            r.ghosts.push((g, src));
            r.released.push(kept);
        }
        removed += 1;
    }
    let working = hexes(r.items.iter().map(|i| i.copy));
    doc.refedit = Some(r);
    Ok(json!({ "working": working, "added": added, "removed": removed }))
}

// ---------------- REFCLOSE ----------------

fn run_refclose(s: &mut Session, p: &Value) -> Result<Value> {
    let save = match str_param(p, "option") {
        Some(o) if o.to_ascii_lowercase().starts_with('d') => false,
        Some(o) if o.to_ascii_lowercase().starts_with('s') => true,
        Some(o) => return Err(bad("refclose", format!("unknown option \"{o}\" (save or discard)"))),
        None => bool_or(p, "save", true),
    };
    close(s, save)
}

fn close(s: &mut Session, save: bool) -> Result<Value> {
    let r = active(s, "refclose")?;
    // Work on a copy: the drawing changes only when everything went through.
    let mut nd: Drawing = s.doc()?.clone();
    let mut objects = 0;
    let mut written = None;
    if save {
        objects = commit_block(&mut nd, &r)?;
        if let Some(x) = &r.xref {
            // The file first: when it can't be written the edit stays open (Discard ends it).
            written = Some(write_back(&nd, &r, x)?);
        }
    }
    nd.end_refedit_discard(&r);
    let mut notes = Vec::new();
    if let (Some(x), Some(_)) = (&r.xref, &written) {
        let host = s.state().ok().and_then(|st| st.path.clone());
        notes = load::load(&mut nd, &x.xref, host.as_deref(), false);
    }
    *s.doc_mut()? = nd;
    // The objects the edit managed were its own: OOPS doesn't bring them back after it.
    s.state_mut()?.erased.forget(|h| r.owns(h));
    s.set_selection(Vec::new());
    for o in notes {
        s.echo(o.message());
    }
    let mut out = json!({ "block": r.block, "saved": save, "objects": objects });
    if let (Some(x), Some(backup)) = (&r.xref, written) {
        out["file"] = json!(x.file);
        // `null` when the file didn't look like a whole drawing and the `.bak` there was kept.
        out["backup"] = json!(backup);
    }
    Ok(out)
}

/// Replace the block's objects by the working set (and move the drawing objects added to it
/// into the block). Returns the number of objects the block has now.
fn commit_block(nd: &mut Drawing, r: &RefEdit) -> Result<usize> {
    let blk = nd.block(&r.block).cloned().ok_or_else(|| bad("refclose", format!("block \"{}\" is gone; use REFCLOSE Discard", r.block)))?;
    let inv = r.to_world.inverse().ok_or_else(|| bad("refclose", "the reference can't be inverted; use REFCLOSE Discard"))?;
    let mut ents: Vec<Entity> = Vec::new();
    for e in blk.entities.iter() {
        match r.items.iter().find(|i| i.source == Some(e.handle)) {
            // A working object the user erased leaves the block.
            Some(i) => {
                if let Some(c) = nd.entity(i.copy) {
                    ents.push(to_block(&inv, c, e.handle));
                }
            }
            None => match r.released.iter().find(|x| x.handle == e.handle) {
                Some(x) => ents.push(x.clone()),
                None => ents.push((**e).clone()),
            },
        }
    }
    let mut moved = Vec::new();
    for i in r.items.iter().filter(|i| i.source.is_none()) {
        if let Some(c) = nd.entity(i.copy).map(|c| (**c).clone()) {
            let h = nd.new_handle();
            ents.push(to_block(&inv, &c, h));
            moved.push(i.copy);
        }
    }
    if references_block(nd, &ents, &r.block) {
        return Err(bad("refclose", format!("Block \"{}\" would reference itself; remove that reference or use REFCLOSE Discard.", r.block)));
    }
    let n = ents.len();
    let mut store = EntityStore::new();
    for e in ents {
        store.push(e);
    }
    let key = nd.blocks.keys().find(|k| k.eq_ignore_ascii_case(&r.block)).cloned().unwrap_or_else(|| r.block.clone());
    let mut nb = (*blk).clone();
    nb.entities = store;
    nd.blocks.insert(key, Arc::new(nb));
    for h in moved {
        nd.remove_entity(h);
    }
    Ok(n)
}

// ---------------- writing an xref back ----------------

/// Write the edited block of `nd` (committed) into xref source `x`. Returns the backup's path
/// (`None` when none was kept).
fn write_back(nd: &Drawing, r: &RefEdit, x: &XrefSource) -> Result<Option<String>> {
    let file = std::path::Path::new(&x.file);
    let real = std::path::Path::new(&x.real);
    writable(file).map_err(|e| bad("refclose", format!("{e}; the edit stays open (REFCLOSE Discard ends it)")))?;
    still_leads_to(file, real).map_err(|e| bad("refclose", format!("{e}; the edit stays open (REFCLOSE Discard ends it)")))?;
    if load::stamp(file) != x.stamp {
        return Err(bad(
            "refclose",
            format!("{} changed since the edit began; it is not overwritten (REFCLOSE Discard ends the edit)", file.display()),
        ));
    }
    let hooks = crate::cmd::file::io().ok_or_else(|| bad("refclose", "file formats are not available in this build"))?;
    let mut src = load::read_file(file).map_err(|e| bad("refclose", format!("{}: {e}", file.display())))?;
    let blk = nd.block(&r.block).ok_or_else(|| bad("refclose", format!("block \"{}\" is gone", r.block)))?;
    let prefix = format!("{}|", x.xref);
    let map: HashMap<Handle, Handle> = x.sources.iter().copied().collect();
    let mut store = EntityStore::new();
    let mut wanted: Vec<(Sym, String)> = Vec::new();
    for e in blk.entities.iter() {
        let mut e = (**e).clone();
        e.handle = match map.get(&e.handle) {
            Some(h) => *h,
            None => src.new_handle(),
        };
        rename_into_source(&mut e, &prefix, &mut wanted);
        store.push(e);
    }
    bring_symbols(nd, &mut src, &prefix, wanted);
    match &x.block {
        None => src.model = store,
        Some(name) => {
            let key = src
                .blocks
                .keys()
                .find(|k| k.eq_ignore_ascii_case(name))
                .cloned()
                .ok_or_else(|| bad("refclose", format!("block \"{name}\" is gone from {}", file.display())))?;
            if let Some(b) = src.blocks.get_mut(&key) {
                Arc::make_mut(b).entities = store;
            }
        }
    }
    let bytes = (hooks.write)(&src, &x.file).map_err(|e| bad("refclose", format!("writing {}: {e}", file.display())))?;
    replace_file(file, real, &bytes).map_err(|e| bad("refclose", e))
}

/// Refuse a save when `file` no longer leads to `real` (the file the edit began with), or `real`
/// is no longer a plain file: a symbolic link put at either since would send the edit to another
/// file.
fn still_leads_to(file: &std::path::Path, real: &std::path::Path) -> std::result::Result<(), String> {
    let now = std::fs::canonicalize(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let plain = std::fs::symlink_metadata(real).is_ok_and(|m| m.is_file());
    if now != real || !plain {
        return Err(format!("{} leads to another file than when the edit began (a symbolic link was put there); it is not overwritten", file.display()));
    }
    Ok(())
}

/// Symbol names of a host object as the xref's drawing has them: `xref|name` loses its prefix;
/// the host's own symbols keep their names and are noted in `wanted` (copied into the drawing).
fn rename_into_source(e: &mut Entity, prefix: &str, wanted: &mut Vec<(Sym, String)>) {
    let found = std::cell::RefCell::new(Vec::new());
    load::rename_refs(e, &|sym, n| {
        if load::has_prefix(n, prefix) {
            return n.get(prefix.len()..).map(str::to_string);
        }
        found.borrow_mut().push((sym, n.to_string()));
        None
    });
    wanted.extend(found.into_inner());
}

/// Copy the host's symbols that objects written into `src` use and `src` lacks (layers,
/// linetypes, text and dimension styles, and blocks with what they use in turn).
fn bring_symbols(host: &Drawing, src: &mut Drawing, prefix: &str, mut wanted: Vec<(Sym, String)>) {
    let mut seen: HashSet<(Sym, String)> = HashSet::new();
    let mut guard = 0usize;
    while let Some((sym, name)) = wanted.pop() {
        guard += 1;
        if guard > 100_000 || name.is_empty() || !seen.insert((sym, name.to_ascii_uppercase())) {
            continue;
        }
        match sym {
            Sym::Layer => {
                if name != "0"
                    && src.layer(&name).is_none()
                    && let Some(l) = host.layer(&name)
                {
                    wanted.push((Sym::Linetype, l.linetype.clone()));
                    src.layers.push(l.clone());
                }
            }
            Sym::Linetype => {
                let builtin = ["ByLayer", "ByBlock", "Continuous"].iter().any(|n| n.eq_ignore_ascii_case(&name));
                if !builtin
                    && src.linetype(&name).is_none()
                    && let Some(t) = host.linetype(&name)
                {
                    src.linetypes.push(t.clone());
                }
            }
            Sym::Style => {
                if src.text_style(&name).is_none()
                    && let Some(t) = host.text_style(&name)
                {
                    src.text_styles.push(t.clone());
                }
            }
            Sym::DimStyle => {
                if src.dim_style(&name).is_none()
                    && let Some(t) = host.dim_style(&name)
                {
                    wanted.push((Sym::Style, t.text_style.clone()));
                    src.dim_styles.push(t.clone());
                }
            }
            Sym::Block => {
                if src.block(&name).is_none()
                    && let Some(b) = host.block(&name)
                    && b.xref_path.is_none()
                {
                    let mut nb = (**b).clone();
                    let mut store = EntityStore::new();
                    for e in b.entities.iter() {
                        let mut e = (**e).clone();
                        e.handle = src.new_handle();
                        rename_into_source(&mut e, prefix, &mut wanted);
                        store.push(e);
                    }
                    nb.entities = store;
                    src.blocks.insert(nb.name.clone(), Arc::new(nb));
                }
            }
            Sym::MLeaderStyle | Sym::TableStyle => {}
        }
    }
}

/// Replace the xref's file (`file` as the xref names it, `real` the file it leads to) by `bytes`,
/// the way SAVE replaces a drawing: the old file is kept as its `.bak`
/// ([`crate::cmd::file::backup_path`]: `site.dwg` as `site.bak`, `site.dxf` as `site.dxf.bak`),
/// and the new one goes into a fresh, exclusively created temporary file that is renamed over
/// `real` ([`crate::atomic_write`]). A symbolic link planted at the temporary or backup path is
/// never written through, and a link at `real` is replaced, not followed. Returns the backup's
/// path (`None` when none was kept).
fn replace_file(file: &std::path::Path, real: &std::path::Path, bytes: &[u8]) -> std::result::Result<Option<String>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (file, real, bytes);
        Err("files are not available on the web".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let kept = crate::cmd::file::backup(real)
            .map_err(|e| format!("the backup copy of {} could not be written ({e}); it is unchanged", file.display()))?;
        let permissions = std::fs::symlink_metadata(real).ok().filter(std::fs::Metadata::is_file).map(|m| m.permissions());
        crate::atomic_write::write_atomic_with(real, permissions, |f| std::io::Write::write_all(f, bytes))
            .map_err(|e| format!("{} can't be replaced: {e}; it is unchanged", file.display()))?;
        // Named beside the file as the xref names it, unless that name is itself a link.
        let shown = if std::fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_symlink()) { real } else { file };
        Ok(kept.then(|| crate::cmd::file::backup_path(shown).to_string_lossy().into_owned()))
    }
}

// ---------------- while editing ----------------

/// After a command ran: objects it created join the working set, and a command that changed
/// anything outside the working set is undone (returns the reason).
pub(crate) fn after_command(s: &mut Session, before: &Arc<Drawing>, id: &str) -> Option<String> {
    if matches!(id, "refedit" | "refset" | "refclose") || !crate::cmd::find_command(id).is_some_and(|c| c.undoable) {
        return None;
    }
    let rb = before.refedit.as_ref()?;
    let st = s.state_mut().ok()?;
    if Arc::ptr_eq(before, &st.doc) {
        return None;
    }
    let ra = st.doc.refedit.as_ref().filter(|ra| ra.insert == rb.insert)?;
    let owned: HashSet<Handle> = rb.items.iter().map(|i| i.copy).chain(rb.ghosts.iter().map(|g| g.0)).chain([rb.insert]).collect();
    let stores = |d: &Drawing| std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).cloned().collect::<Vec<_>>();
    let touched = stores(before)
        .iter()
        .any(|store| store.iter().any(|e| !owned.contains(&e.handle) && !st.doc.entity(e.handle).is_some_and(|a| Arc::ptr_eq(a, e))));
    if touched {
        st.doc = before.clone();
        return Some("Only the working set can be edited during REFEDIT (REFSET adds objects, REFCLOSE ends the edit).".into());
    }
    let seed = before.handseed;
    let mut new: Vec<Handle> = Vec::new();
    for store in stores(&st.doc) {
        new.extend(store.iter().map(|e| e.handle).filter(|h| h.0 >= seed && !ra.owns(*h)));
    }
    if !new.is_empty()
        && let Some(r) = Arc::make_mut(&mut st.doc).refedit.as_mut()
    {
        r.items.extend(new.into_iter().map(|copy| WorkItem { copy, source: None }));
    }
    None
}

/// Whether `h` may not be edited because a reference is being edited and it is not in the working set.
pub(crate) fn locked(d: &Drawing, h: Handle) -> bool {
    d.refedit.as_ref().is_some_and(|r| !r.is_working(h))
}

// ---------------- interactive ----------------

/// REFEDIT at the command line: "Select reference", then "Select nesting level [OK/Next]" when
/// the reference has others nested in it.
#[derive(Default)]
struct RefeditM {
    sel: SelectPhase,
    picked: Option<Handle>,
    levels: Vec<(Vec<Handle>, String)>,
    at: usize,
}

impl RefeditM {
    fn picked(&mut self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let d = s.doc()?;
        let Some(h) = hs.iter().copied().find(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Insert(_)))) else {
            s.echo("Select a block or xref reference.");
            self.sel = SelectPhase::single();
            return Ok(Step::Continue);
        };
        self.levels = level_list(d, h)?;
        self.picked = Some(h);
        if self.levels.len() > 1 {
            self.echo_level(s);
            return Ok(Step::Continue);
        }
        self.finish(s)
    }
    fn echo_level(&self, s: &mut Session) {
        if let Some((path, name)) = self.levels.get(self.at) {
            s.echo(format!("Reference name: {name}{}", if path.is_empty() { String::new() } else { format!(" (nested {} deep)", path.len()) }));
        }
    }
    fn finish(&mut self, s: &mut Session) -> Result<Step> {
        let Some(h) = self.picked else { return Ok(Step::Cancel) };
        let path = self.levels.get(self.at).map(|(p, _)| hexes(p.iter().copied())).unwrap_or_default();
        let r = run_refedit(s, &json!({ "handle": h.hex(), "path": path }))?;
        let n = r["working"].as_array().map_or(0, Vec::len);
        s.set_selection(Vec::new());
        s.echo(format!("{n} object(s) in the working set. Use REFCLOSE to save or discard the changes."));
        Ok(Step::Done)
    }
}

impl Interactive for RefeditM {
    fn name(&self) -> &'static str {
        "REFEDIT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        self.sel.single = true;
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.picked(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.picked.is_none() {
            return Prompt::new("Select reference", Accept::SELECT);
        }
        Prompt::new("Select nesting level", Accept::TEXT).kw(&["OK", "Next"]).default("Next")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if matches!(i, Input::Cancel) {
            return Ok(Step::Cancel);
        }
        if self.picked.is_none() {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.picked(s, &hs),
            };
        }
        let k = match &i {
            Input::Keyword(k) | Input::Text(k) => k.trim().to_ascii_lowercase(),
            Input::Enter => "next".into(),
            _ => return Ok(Step::Continue),
        };
        if k.starts_with('o') {
            return self.finish(s);
        }
        if k.starts_with('n') {
            self.at = (self.at + 1) % self.levels.len().max(1);
            self.echo_level(s);
        }
        Ok(Step::Continue)
    }
}

#[derive(Default)]
enum RStage {
    #[default]
    Mode,
    Pick(bool),
}

#[derive(Default)]
struct RefsetM {
    stage: RStage,
    sel: SelectPhase,
}

impl Interactive for RefsetM {
    fn name(&self) -> &'static str {
        "REFSET"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        active(s, "refset").map(|_| Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.stage {
            RStage::Mode => {
                let mut p = Prompt::new("Enter an option [Add/Remove] <Add>", Accept::TEXT);
                p.keywords = vec!["Add".into(), "Remove".into()];
                p.default = Some("Add".into());
                p
            }
            RStage::Pick(remove) => {
                let mut p = self.sel.prompt();
                p.message = format!("Select objects to {}", if remove { "remove" } else { "add" });
                p
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.stage {
            RStage::Mode => {
                let remove = match i {
                    Input::Keyword(k) | Input::Text(k) => k.to_ascii_lowercase().starts_with('r'),
                    Input::Enter => false,
                    _ => return Ok(Step::Cancel),
                };
                self.sel = SelectPhase::begin(s);
                self.stage = RStage::Pick(remove);
                if self.sel.done {
                    let hs = self.sel.picked.clone();
                    return self.finish(s, remove, &hs);
                }
                Ok(Step::Continue)
            }
            RStage::Pick(remove) => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.finish(s, remove, &hs),
            },
        }
    }
}

impl RefsetM {
    fn finish(&mut self, s: &mut Session, remove: bool, hs: &[Handle]) -> Result<Step> {
        let r = if remove { apply_refset(s, &[], hs) } else { apply_refset(s, hs, &[]) }?;
        s.echo(format!("{} object(s) in the working set.", r.get("working").and_then(Value::as_array).map_or(0, Vec::len)));
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

struct RefcloseM;

impl Interactive for RefcloseM {
    fn name(&self) -> &'static str {
        "REFCLOSE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let mut p = Prompt::new("Enter option [Save/Discard reference changes] <Save>", Accept::TEXT);
        p.keywords = vec!["Save".into(), "Discard".into()];
        p.default = Some("Save".into());
        p
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let save = match i {
            Input::Keyword(k) | Input::Text(k) => !k.to_ascii_lowercase().starts_with('d'),
            Input::Enter => true,
            _ => return Ok(Step::Cancel),
        };
        let r = close(s, save)?;
        s.echo(match (save, r.get("file").and_then(Value::as_str)) {
            (true, Some(f)) => match r["backup"].as_str() {
                Some(b) => format!("Reference changes saved to {f} (the previous file is kept as {b})."),
                None => format!("Reference changes saved to {f}."),
            },
            (true, None) => "Reference changes saved.".to_string(),
            (false, _) => "Reference changes discarded.".to_string(),
        });
        Ok(Step::Done)
    }
}
