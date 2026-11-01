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

use super::super::file::io;
use super::super::modify::explode_kind;
use super::{references_block, valid_block_name};
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
        define_from(s, &src, &self.name)
    }
}

/// The file `t` names at the block name prompt: `name=path` (`name=` alone: the file `name`), or a
/// path (it has a folder separator or a .dwg/.dxf extension) that isn't the name of a block.
pub(super) fn file_ref(s: &Session, t: &str) -> Option<FileRef> {
    let t = t.trim();
    if let Some((name, path)) = t.split_once('=') {
        let name = name.trim();
        let path = if path.trim().is_empty() { name } else { path.trim() };
        return (!name.is_empty()).then(|| FileRef { name: name.to_string(), path: path.to_string(), explicit: true });
    }
    let lower = t.to_ascii_lowercase();
    let pathlike = t.contains(['/', '\\']) || lower.ends_with(".dwg") || lower.ends_with(".dxf");
    if !pathlike || s.doc().is_ok_and(|d| d.block(t).is_some()) {
        return None;
    }
    Some(FileRef { name: stem(t), path: t.to_string(), explicit: false })
}

/// The file name without folders and extension (either separator, so Windows paths work anywhere).
fn stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => file.to_string(),
    }
}

/// Read a drawing file through the io hooks; a path without extension tries .dwg, then .dxf.
fn read_drawing(path: &str) -> Result<Drawing> {
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
        std::fs::read(_path).map_err(|e| bad("insert", format!("{_path}: {e}")))
    }
    #[cfg(target_arch = "wasm32")]
    Err(bad("insert", "paths are not available on the web"))
}

/// (Re)define block `name` from `src`'s model space with base point `$INSBASE`, adding the
/// layers, linetypes, styles and blocks of `src` this drawing doesn't have. Nothing changes when
/// the definition fails (an invalid name, or a block that would contain itself).
fn define_from(s: &mut Session, src: &Drawing, name: &str) -> Result<String> {
    if !valid_block_name(name) {
        return Err(bad("insert", format!("\"{name}\" is not a valid block name")));
    }
    let mut d = s.doc()?.clone();
    // Redefining keeps the existing definition's name (block names are case-insensitive).
    let name = d.blocks.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| name.to_string());
    merge_tables(&mut d, src);
    let renames = block_names(&d, src, &name);
    for (from, to) in &renames {
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
    *s.doc_mut()? = d;
    Ok(name)
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

/// A copy of a source entity for this drawing: a fresh handle, block references renamed, and
/// links to other source objects (associative dimension points) dropped.
fn adopt(d: &mut Drawing, e: &Entity, renames: &BTreeMap<String, String>) -> Entity {
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
}
