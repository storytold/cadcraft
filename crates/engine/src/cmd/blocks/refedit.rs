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
//! change into its source drawing the way SAVE writes a drawing: when ISAVEBAK is on, the previous
//! file is kept as its `.bak` (with it off, no backup is kept); the new one replaces it
//! atomically, and a symbolic link planted beside it or put at its path during the edit never
//! redirects the write. The save is refused when the file is read-only, open in another program
//! (or another drawing tab), or changed since it was loaded. The xref is then loaded again from
//! the file it wrote.
//!
//! The state is [`Drawing::refedit`]: it travels with undo, is never written to a file (files
//! are written from [`Drawing::committed`]) and ends with the drawing it belongs to.
//!
//! Not yet: annotative scale contexts, attribute synchronisation and xrefs nested in xrefs (open
//! those with XOPEN).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Block, Drawing, Entity, EntityKind, EntityStore, Handle, RefEdit, WorkItem, XrefSource, XrefStatus};
use cadcraft_geom::Mat3;
use serde_json::{Value, json};

use super::super::curves::handle_value;
use super::super::machines::{SelOutcome, SelectPhase};
use super::references_block;
use super::xref::load::{self, Sym};
use crate::cmd::file::Version;
use crate::cmd::{CommandSpec, bad, bool_or, has_doc, str_param, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most nesting levels offered for one picked reference.
const MAX_LEVELS: usize = 256;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("refedit", "Edit Reference In-Place", run_refedit)
            .menu(&["Tools", "Xref and Block In-place Editing", "Edit Reference In-Place"])
            .alias(&["-refedit"])
            .params("{handle? | handles? | name? (a block or xref reference, default the selection), path?: [handles of nested block references, outermost first] | nested?: block name (the first nested reference to it), objects?: [block object handles] (default all)} → {block, insert, working: [handles], faded, path, xref?: {name, file, version: its $ACADVER, binary}}. Editing an xref writes its file back on REFCLOSE Save, in the version and form it was read in (refused for versions CADCraft does not write, such as R12).")
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
            .params("{option?: \"save\"|\"discard\" | save?: bool (default true)} → {block, saved, objects, file? (an xref's source drawing written), backup? (its previous file, as SAVE names it; null when none was kept), notSaved?: {type: count} (what the file couldn't keep, also echoed)}")
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

/// A block object as the drawing shows it through the edited reference. The copy keeps the
/// object's own transparency: the renderer fades what is outside the working set by the current
/// XFADECTL, so changing XFADECTL during the edit shows at once.
fn to_host(to_world: &Mat3, e: &Entity, h: Handle) -> Entity {
    let mut c = e.clone();
    c.handle = h;
    c.kind.transform(to_world);
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

fn unequal_scale() -> crate::EngineError {
    bad("refedit", "the reference must be scaled equally in X and Y (and not flat) to be edited in place")
}

/// The levels from top reference `ins` down `path` (references inside the block definitions).
fn levels(d: &Drawing, ins: &cadcraft_doc::Insert, path: &[Handle]) -> Result<Vec<Level>> {
    let mut block = d.block(&ins.block).cloned().ok_or_else(|| bad("refedit", format!("block \"{}\" not found", ins.block)))?;
    let mut m = ins.transform(block.base.xy());
    if !editable_transform(&m, ins.scale.x, ins.scale.y) {
        return Err(unequal_scale());
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

/// The references nested in block `name` (depth first): (path of handles, block name). `m` is how
/// the block shows in the drawing: a reference is checked with everything above it, as [`levels`]
/// checks it.
fn nested(d: &Drawing, name: &str, m: &Mat3, path: &mut Vec<Handle>, out: &mut Vec<(Vec<Handle>, String)>) {
    if path.len() + 1 >= cadcraft_doc::MAX_BLOCK_DEPTH || out.len() >= MAX_LEVELS {
        return;
    }
    let Some(b) = d.block(name) else { return };
    for e in b.entities.iter() {
        let EntityKind::Insert(i) = &e.kind else { continue };
        // Generated geometry (dimensions, arrays) is not offered, nor a reference `levels` would
        // refuse (scaled differently in X and Y, flat): nothing below it can be edited either.
        let Some(c) = d.block(&i.block) else { continue };
        let cm = m.then_before(i.transform(c.base.xy()));
        let editable = editable_transform(&cm, i.scale.x, i.scale.y);
        if i.block.starts_with('*') || c.array.is_some() || out.len() >= MAX_LEVELS || !editable {
            continue;
        }
        path.push(e.handle);
        out.push((path.clone(), i.block.clone()));
        nested(d, &i.block, &cm, path, out);
        path.pop();
    }
}

/// The nesting levels of reference `ih`: the reference itself, then every reference nested in it.
fn level_list(d: &Drawing, ih: Handle) -> Result<Vec<(Vec<Handle>, String)>> {
    let Some(EntityKind::Insert(ins)) = d.entity(ih).map(|e| &e.kind) else { return Err(bad("refedit", "select a block reference")) };
    let m = d.block(&ins.block).map_or(Mat3::IDENTITY, |b| ins.transform(b.base.xy()));
    // As [`levels`] checks it: a reference it would refuse offers nothing nested in it either.
    if !editable_transform(&m, ins.scale.x, ins.scale.y) {
        return Err(unequal_scale());
    }
    let mut out = vec![(Vec::new(), ins.block.clone())];
    nested(d, &ins.block, &m, &mut Vec::new(), &mut out);
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

/// Whether a drawing tab other than the active one has `file` open.
fn open_in_another_tab(s: &Session, file: &std::path::Path) -> bool {
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let me = canon(file);
    s.docs.iter().enumerate().any(|(i, st)| i != s.active && st.path.as_deref().is_some_and(|p| canon(std::path::Path::new(p)) == me))
}

/// The source file of xref `name`, checked for an edit in place: found, not open in another
/// drawing tab, writable, not changed since this drawing loaded it. Also returns the
/// [`load::Stamp`] it had then (this drawing's own baseline, [`Block::xref_stamp`]).
fn xref_file(s: &Session, name: &str) -> Result<(std::path::PathBuf, load::Stamp)> {
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
    // Under the automation roots: the file is read for the edit, and written by REFCLOSE Save, so
    // an edit that couldn't be saved doesn't begin.
    crate::file_access::check_read(&file).map_err(|e| bad("refedit", e))?;
    crate::file_access::check_write(&file).map_err(|e| bad("refedit", format!("{e}; open it with XOPEN instead")))?;
    if open_in_another_tab(s, &file) {
        return Err(bad("refedit", format!("{} is open in another drawing tab: edit it there, or close it first", file.display())));
    }
    writable(&file).map_err(|e| bad("refedit", e))?;
    // Unknown (the system can't tell, or the file changed while it was read) counts as changed.
    match (b.xref_stamp, load::stamp(&file)) {
        (Some(then), Some(now)) if then == now => Ok((file, then)),
        _ => Err(bad("refedit", format!("{} changed since xref \"{}\" was loaded; reload it first", file.display(), b.name))),
    }
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

/// The version (`$ACADVER` code) and form (binary DXF or not) of the xref's file `file`, read as
/// `src`, so it is written back as it was: a DWG's version is its first bytes, a DXF's the
/// `$ACADVER` it was read with. Refused when CADCraft can't write that version (an R12 DXF,
/// AC1009): saving would change it, and the xref is often someone else's file.
fn file_form(file: &std::path::Path, src: &Drawing) -> Result<(Option<String>, bool)> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(file).and_then(|f| f.take(4096).read_to_end(&mut head)).map_err(|e| bad("refedit", format!("{}: {e}", file.display())))?;
    let dwg = head
        .get(..6)
        .filter(|t| t.starts_with(b"AC") && t.get(2..).is_some_and(|d| d.iter().all(u8::is_ascii_digit)))
        .map(|t| String::from_utf8_lossy(t).into_owned());
    let declared = || src.foreign.version.clone().or_else(|| src.header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).map(str::to_string));
    let version = dwg.or_else(declared).map(|v| v.trim().to_ascii_uppercase());
    if let Some(code) = &version
        && Version::parse(code).is_none_or(|v| v.code() != code)
    {
        return Err(bad(
            "refedit",
            format!(
                "{} is an {code} file, and CADCraft writes only 2000 to 2018 files, so the edit couldn't be saved in the file's own version; open it with XOPEN instead",
                file.display()
            ),
        ));
    }
    let binary = craft_dxf::is_binary(&head);
    if (version.is_some() || binary) && crate::cmd::file::write_as().is_none() {
        return Err(bad("refedit", "saving an xref in its own version is not available in this build; open it with XOPEN instead"));
    }
    Ok((version, binary))
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
            let (file, baseline) = xref_file(s, &x)?;
            let (src, read_as) = load::read_file(&file).map_err(|e| bad("refedit", format!("{}: {e}", file.display())))?;
            // The handles, version and stamp all come from this one read, which must be of the
            // file this drawing loaded.
            if read_as != Some(baseline) {
                return Err(bad("refedit", format!("{} changed since xref \"{x}\" was loaded; reload it first", file.display())));
            }
            let sources = source_handles(&blk, &src, inner.as_deref())?;
            let real = std::fs::canonicalize(&file).map_err(|e| bad("refedit", format!("{}: {e}", file.display())))?;
            let (version, binary) = file_form(&file, &src)?;
            Some(XrefSource {
                xref: x,
                file: file.to_string_lossy().into_owned(),
                real: real.to_string_lossy().into_owned(),
                version,
                binary,
                block: inner,
                stamp: read_as,
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
    let doc = s.doc_mut()?;
    for e in blk.entities.iter() {
        let working = chosen.as_ref().is_none_or(|c| c.contains(&e.handle));
        let h = doc.new_handle();
        put(doc, &r, to_host(&to_world, e, h))?;
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
            put(doc, &r, to_host(&level.to_world, e, h))?;
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
        "xref": r.xref.as_ref().map(|x| json!({ "name": x.xref, "file": x.file, "version": x.version, "binary": x.binary })),
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
                put(doc, &r, to_host(&r.to_world, &cur, c))?;
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
            put(doc, &r, to_host(&r.to_world, &kept, g))?;
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
            // Opened in another tab since the edit began: that tab would hold a stale drawing.
            if open_in_another_tab(s, std::path::Path::new(&x.file)) {
                return Err(bad(
                    "refclose",
                    format!("{} is open in another drawing tab: close it there first; the edit stays open (REFCLOSE Discard ends it)", x.file),
                ));
            }
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
    if let (Some(x), Some(w)) = (&r.xref, written) {
        out["file"] = json!(x.file);
        // `null` when the file didn't look like a whole drawing and the `.bak` there was kept.
        out["backup"] = json!(w.backup);
        // What the file couldn't keep, said as SAVE says it (nothing is dropped silently).
        let kept = w.backup.as_ref().map(|b| format!(" The previous file, with them, is kept as {b}.")).unwrap_or_default();
        let mut not_saved = w.lost.clone();
        if let Some(msg) = crate::cmd::file::lost_message(&x.file, w.version, &w.lost) {
            s.echo(format!("{}: {msg}.{kept}", x.file));
        }
        if w.associations > 0 {
            s.echo(format!(
                "{}: Not saved in the file: the dimension associations of {} dimension(s), which tie them to the objects they measure (an xref edited in place doesn't keep them yet).{kept}",
                x.file, w.associations
            ));
            not_saved.insert("dimension associations".into(), w.associations);
        }
        if !not_saved.is_empty() {
            out["notSaved"] = json!(not_saved);
        }
    }
    Ok(out)
}

/// What writing an xref back did: the backup it kept, the version it was written at, what the
/// file couldn't keep ([`crate::cmd::file::lost_writing`]), and how many dimensions were written
/// without their associations ([`associations_dropped`]).
struct Written {
    backup: Option<String>,
    version: Version,
    lost: std::collections::BTreeMap<String, usize>,
    associations: usize,
}

/// The dimensions of the part of `src` (the xref's drawing as read) that the write-back replaces
/// which are attached to objects: loading the xref dropped those associations, so the file is
/// written without them.
fn associations_dropped(src: &Drawing, x: &XrefSource) -> usize {
    let store = match &x.block {
        None => Some(&src.model),
        Some(n) => src.block(n).map(|b| &b.entities),
    };
    store.map_or(0, |st| st.iter().filter(|e| matches!(&e.kind, EntityKind::Dimension(dm) if !dm.assoc.is_empty())).count())
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

/// Write the edited block of `nd` (committed) into xref source `x`.
fn write_back(nd: &Drawing, r: &RefEdit, x: &XrefSource) -> Result<Written> {
    let file = std::path::Path::new(&x.file);
    let real = std::path::Path::new(&x.real);
    // Under the automation roots, the file written and its backup, before anything is read or
    // written: roots put in force since the edit began hold too.
    for target in [real.to_path_buf(), crate::cmd::file::backup_path(real)] {
        crate::file_access::check_write(&target).map_err(|e| bad("refclose", format!("{e}; the edit stays open (REFCLOSE Discard ends it)")))?;
    }
    writable(file).map_err(|e| bad("refclose", format!("{e}; the edit stays open (REFCLOSE Discard ends it)")))?;
    still_leads_to(file, real).map_err(|e| bad("refclose", format!("{e}; the edit stays open (REFCLOSE Discard ends it)")))?;
    unchanged(file, x, load::stamp(file))?;
    let hooks = crate::cmd::file::io().ok_or_else(|| bad("refclose", "file formats are not available in this build"))?;
    let (mut src, read_as) = load::read_file(file).map_err(|e| bad("refclose", format!("{}: {e}", file.display())))?;
    // What is written over is what was just read, and that is the file the edit began with.
    unchanged(file, x, read_as)?;
    let dropped = associations_dropped(&src, x);
    let cleared = put_edit(nd, r, x, &mut src)?;
    // The xref is loaded again from the file: one it could no longer load is not written.
    let objects = load::object_count(&src);
    if objects > load::MAX_XREF_OBJECTS {
        return Err(bad(
            "refclose",
            format!(
                "{} would hold {objects} objects, more than an xref loads ({}); it is not written (REFCLOSE Discard ends the edit)",
                file.display(),
                load::MAX_XREF_OBJECTS
            ),
        ));
    }
    // In the version and form the file was read in, not the writer's default (a DWG at 2004 or
    // later): a version change can lock out whoever made the file.
    let version = x.version.as_deref().and_then(Version::parse);
    let bytes = if version.is_none() && !x.binary {
        (hooks.write)(&src, &x.file)
    } else {
        let w = crate::cmd::file::write_as().ok_or_else(|| bad("refclose", "saving an xref in its own version is not available in this build"))?;
        w(&src, &x.file, &crate::cmd::file::SaveAs { version, binary: x.binary, portable_fonts: false })
    };
    let bytes = bytes.map_err(|e| bad("refclose", format!("writing {}: {e}", file.display())))?;
    let as_version =
        version.unwrap_or_else(|| src.header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).and_then(Version::for_acadver).unwrap_or_default());
    let lost = crate::cmd::file::lost_writing(&src, &x.file, as_version);
    // Once more just before the file is replaced: another program may have saved it while the
    // new file was made. (Without a lock every writer honours, this narrows the window; it
    // can't close it.)
    unchanged(file, x, load::stamp(file))?;
    let backup = replace_file(file, real, &bytes).map_err(|e| bad("refclose", e))?;
    Ok(Written { backup, version: as_version, lost, associations: dropped.saturating_add(cleared) })
}

/// Refuse when `now` (the xref's file as it is, or as just read) is not the file the edit began
/// with; an unknown stamp counts as changed.
fn unchanged(file: &std::path::Path, x: &XrefSource, now: Option<load::Stamp>) -> Result<()> {
    if x.stamp.is_none() || now != x.stamp {
        return Err(bad(
            "refclose",
            format!("{} changed since the edit began; it is not overwritten (REFCLOSE Discard ends the edit)", file.display()),
        ));
    }
    Ok(())
}

/// Put the edited block of `nd` (committed) into `src`, the xref's drawing as read: its objects
/// with their source handles and names, and the host symbols they use. A dimension attached to
/// objects of the host (drawn during the edit) is written without those associations: the
/// host's handles mean nothing in the xref's file. Returns how many were.
fn put_edit(nd: &Drawing, r: &RefEdit, x: &XrefSource, src: &mut Drawing) -> Result<usize> {
    let file = std::path::Path::new(&x.file);
    let blk = nd.block(&r.block).ok_or_else(|| bad("refclose", format!("block \"{}\" is gone", r.block)))?;
    let prefix = format!("{}|", x.xref);
    let map: HashMap<Handle, Handle> = x.sources.iter().copied().collect();
    let mut store = EntityStore::new();
    let mut wanted: Vec<(Sym, String)> = Vec::new();
    let mut cleared = 0usize;
    for e in blk.entities.iter() {
        let mut e = (**e).clone();
        e.handle = match map.get(&e.handle) {
            Some(h) => *h,
            None => src.new_handle(),
        };
        rename_into_source(&mut e, &prefix, &mut wanted);
        if let EntityKind::Dimension(dm) = &mut e.kind
            && !dm.assoc.is_empty()
        {
            dm.assoc.clear();
            cleared = cleared.saturating_add(1);
        }
        store.push(e);
    }
    bring_symbols(nd, src, &prefix, wanted, MAX_SYMBOLS)?;
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
    Ok(cleared)
}

/// Refuse a save when `file` no longer leads to `real` (the file the edit began with), or `real`
/// is no longer a plain file: a symbolic link put at either since would send the edit to another
/// file.
fn still_leads_to(file: &std::path::Path, real: &std::path::Path) -> std::result::Result<(), String> {
    let now = std::fs::canonicalize(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let plain = std::fs::symlink_metadata(real).is_ok_and(|m| m.is_file());
    if now != real || !plain {
        return Err(format!(
            "{} leads to another file than when the edit began (a symbolic link was put there); it is not overwritten",
            file.display()
        ));
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

/// Most distinct symbols [`bring_symbols`] looks at for one save (hostile drawings: blocks naming
/// blocks naming blocks…).
const MAX_SYMBOLS: usize = 100_000;

/// Copy the host's symbols that objects written into `src` use and `src` lacks (layers,
/// linetypes, text, dimension, multileader and table styles, and blocks with what they use in
/// turn). Only distinct names count against `cap`; past it the save is refused rather than
/// written with symbols missing.
fn bring_symbols(host: &Drawing, src: &mut Drawing, prefix: &str, mut wanted: Vec<(Sym, String)>, cap: usize) -> Result<()> {
    let mut seen: HashSet<(Sym, String)> = HashSet::new();
    let mut blocks: Vec<String> = Vec::new();
    while let Some((sym, name)) = wanted.pop() {
        if name.is_empty() || !seen.insert((sym, name.to_ascii_uppercase())) {
            continue;
        }
        if seen.len() > cap {
            return Err(bad("refclose", format!("the edit uses more than {cap} symbols; the xref is not written (REFCLOSE Discard ends the edit)")));
        }
        if sym == Sym::Block {
            blocks.push(name.clone());
        }
        bring_symbol(host, src, prefix, sym, &name, &mut wanted);
    }
    // A block the written objects name that the drawing still lacks (an xref of the host, added
    // with REFSET) would leave a reference to nothing in the file: refused, nothing written.
    if let Some(name) = blocks.iter().find(|n| src.block(n).is_none()) {
        return Err(bad(
            "refclose",
            format!(
                "the edit uses block \"{name}\", which can't be written into the xref's drawing (an xref of this drawing, or a block it doesn't define); remove it from the working set (REFSET) or use REFCLOSE Discard"
            ),
        ));
    }
    Ok(())
}

/// The blocks dimension style `t` of `host` draws its arrowheads with: the names `host` defines as
/// blocks (a built-in arrowhead name is drawn in code, not a block), as the xref's drawing names
/// them.
fn arrow_blocks(host: &Drawing, t: &cadcraft_doc::DimStyle, prefix: &str) -> Vec<(Sym, String)> {
    [&t.arrow_block, &t.arrow_block1, &t.arrow_block2, &t.leader_arrow_block]
        .into_iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty() && host.block(n).is_some())
        .map(|n| (Sym::Block, if load::has_prefix(n, prefix) { n.get(prefix.len()..).unwrap_or(n).to_string() } else { n.to_string() }))
        .collect()
}

/// Copy host symbol `name` of kind `sym` into `src` when `src` lacks it, queueing what it uses.
fn bring_symbol(host: &Drawing, src: &mut Drawing, prefix: &str, sym: Sym, name: &str, wanted: &mut Vec<(Sym, String)>) {
    let name = name.to_string();
    {
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
                    wanted.extend(arrow_blocks(host, t, prefix));
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
                        rename_into_source(&mut e, prefix, wanted);
                        store.push(e);
                    }
                    nb.entities = store;
                    src.blocks.insert(nb.name.clone(), Arc::new(nb));
                }
            }
            Sym::MLeaderStyle => {
                let have = src.mleader_styles.iter().any(|t| t.name.eq_ignore_ascii_case(&name));
                if !have && let Some(t) = host.mleader_styles.iter().find(|t| t.name.eq_ignore_ascii_case(&name)) {
                    wanted.push((Sym::Style, t.text_style.clone()));
                    src.mleader_styles.push(t.clone());
                }
            }
            Sym::TableStyle => {
                let have = src.table_styles.iter().any(|t| t.name.eq_ignore_ascii_case(&name));
                if !have && let Some(t) = host.table_styles.iter().find(|t| t.name.eq_ignore_ascii_case(&name)) {
                    wanted.extend(t.cell_styles.iter().map(|c| (Sym::Style, c.text_style.clone())));
                    src.table_styles.push(t.clone());
                }
            }
        }
    }
}

/// Replace the xref's file (`file` as the xref names it, `real` the file it leads to) by `bytes`,
/// the way SAVE replaces a drawing: the old file is kept as its `.bak` (when ISAVEBAK is on)
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
        // As SAVE: ISAVEBAK off keeps no backup.
        let kept = if crate::autosave::isavebak() {
            crate::cmd::file::backup(real)
                .map_err(|e| format!("the backup copy of {} could not be written ({e}); it is unchanged", file.display()))?
        } else {
            false
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Review of #2355: the budget counted every queued reference, so a host-only layer queued
    /// first was skipped once enough repeated ones followed it, and the file was written without
    /// it. Only distinct names count now, and running out refuses the save.
    #[test]
    fn repeated_symbols_do_not_use_up_the_budget() {
        let mut host = Drawing::new_metric();
        host.layers.push(cadcraft_doc::Layer { name: "Added".into(), ..cadcraft_doc::Layer::default() });
        let mut wanted = vec![(Sym::Layer, "Added".to_string())];
        for _ in 0..1000 {
            wanted.push((Sym::Layer, "0".into()));
            wanted.push((Sym::Linetype, "ByLayer".into()));
        }
        // Distinct: ByLayer, 0, Added and its linetype.
        let mut src = Drawing::new_metric();
        bring_symbols(&host, &mut src, "x|", wanted.clone(), 4).unwrap();
        assert!(src.layer("Added").is_some(), "the layer queued first is copied");
        let mut src = Drawing::new_metric();
        assert!(bring_symbols(&host, &mut src, "x|", wanted, 2).is_err(), "past the budget the save is refused");
    }
}
