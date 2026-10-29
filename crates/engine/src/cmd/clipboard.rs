//! The clipboard every open drawing shares (COPYCLIP, CUTCLIP, COPYBASE; PASTECLIP, PASTEORIG,
//! PASTEBLOCK).
//!
//! Like AutoCAD's, it holds the copied objects together with what they refer to in their drawing:
//! layers, linetypes, text, dimension, multileader and table styles, and the blocks they reference
//! (nested ones too). Pasting adds the ones the target drawing doesn't have; a name the target
//! already has keeps the target's definition, and a generated (anonymous `*…`) block whose name is
//! taken gets a fresh one. Pasted objects get new handles.
//!
//! The clipboard can also travel as DXF text ([`system_text`], [`load_system_text`]), so a front
//! end can put it on the system clipboard and paste it into another CADCraft window.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Block, Drawing, Entity, EntityKind, HVal, Handle};
use cadcraft_geom::{Mat3, Vec2};

use super::{bad, has_doc};
use crate::{Result, Session};

/// More objects than this aren't mirrored to (or read from) the system clipboard.
const MAX_SYSTEM_OBJECTS: usize = 50_000;
/// Bounds the walk through nested block references.
const MAX_WALK: usize = 5_000_000;

/// Where the clipboard's objects came from, and the definitions they need.
#[derive(Clone, Debug)]
pub struct ClipSource {
    /// `DocState::uid` of the drawing they were copied from; `None` when they came from the system
    /// clipboard.
    pub uid: Option<u64>,
    /// The layers, linetypes, styles and blocks the objects reference (and the source's header).
    pub defs: Arc<Drawing>,
    /// Counts copies, so a front end mirrors each new clipboard to the system clipboard once.
    pub serial: u64,
}

/// A raster image payload held on the clipboard.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipboardImage {
    /// PNG or raw image bytes.
    pub bytes: Arc<Vec<u8>>,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
}

/// Put copies of `ents` from the active drawing on the clipboard, with `base` as base point.
pub(crate) fn copy(s: &mut Session, ents: Vec<Entity>, base: Vec2) -> Result<()> {
    let uid = s.state()?.uid;
    let defs = defs_for(s.doc()?, &ents);
    set(s, ents, base, defs, Some(uid));
    Ok(())
}

fn set(s: &mut Session, ents: Vec<Entity>, base: Vec2, defs: Drawing, uid: Option<u64>) {
    let serial = s.clipboard_source.as_ref().map_or(0, |c| c.serial).wrapping_add(1);
    s.clipboard = ents;
    s.clipboard_base = base;
    s.clipboard_image = None;
    s.clipboard_source = Some(ClipSource { uid, defs: Arc::new(defs), serial });
}

/// Enabled check of PASTECLIP and PASTEBLOCK: something to paste.
pub(crate) fn has_clip(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.clipboard.is_empty() && s.clipboard_image.is_none() { Err("the clipboard is empty".into()) } else { Ok(()) }
}

/// Default scale for a pasted clipboard image in `s`'s current space.
pub(crate) fn default_image_scale(s: &Session, width: u32, height: u32) -> f64 {
    let w = (width.max(1)) as f64;
    let h = (height.max(1)) as f64;
    match s.space() {
        cadcraft_doc::Space::Paper(_) => 0.25_f64.min(180.0 / w).min(125.0 / h).max(1e-6),
        cadcraft_doc::Space::Model => 1.0,
    }
}

/// Paste a clipboard image at insertion point `at`.
pub(crate) fn paste_image(s: &mut Session, at: Vec2, scale: Option<f64>) -> Result<Handle> {
    let img = s.clipboard_image.clone().ok_or_else(|| bad("pasteclip", "no image on clipboard"))?;
    use std::hash::{DefaultHasher, Hasher};
    let mut hasher = DefaultHasher::new();
    hasher.write(&img.bytes);
    let hash = hasher.finish();
    let key = format!("paste_{hash:016x}.png");

    let default_scale = default_image_scale(s, img.width, img.height);
    let space = s.space();
    let d = s.doc_mut()?;
    d.store_image_asset(&key, (*img.bytes).clone());
    let scale = scale.unwrap_or(default_scale).max(1e-9);
    let image_ent = cadcraft_doc::Image {
        insert: at.to3(0.0),
        u: Vec2::new(scale, 0.0).to3(0.0),
        v: Vec2::new(0.0, scale).to3(0.0),
        size: Vec2::new(img.width as f64, img.height as f64),
        path: key.clone(),
        name: key,
        ..cadcraft_doc::Image::default()
    };
    let h = d.new_handle();
    let e = Entity::new(h, EntityKind::Image(image_ent));
    d.ensure_layer(&e.common.layer);
    if let Some(st) = d.space_mut(&space) {
        st.push(e);
    }
    s.set_selection(vec![h]);
    Ok(h)
}

/// Enabled check of PASTEORIG: as in AutoCAD, it pastes objects copied from another drawing.
pub(crate) fn can_paste_orig(s: &Session) -> std::result::Result<(), String> {
    has_clip(s)?;
    if from_active(s) {
        return Err("the clipboard holds objects from this drawing; Paste to Original Coordinates pastes into another drawing".into());
    }
    Ok(())
}

/// Whether the clipboard was copied from the active drawing.
pub fn from_active(s: &Session) -> bool {
    let active = s.state().ok().map(|st| st.uid);
    s.clipboard_source.as_ref().is_some_and(|c| c.uid.is_some() && c.uid == active)
}

/// Paste the clipboard into the active drawing's current space, moved by `m`; selects and
/// returns the new objects.
pub(crate) fn paste(s: &mut Session, m: &Mat3) -> Result<Vec<Handle>> {
    let ents = s.clipboard.clone();
    let defs = s.clipboard_source.as_ref().map(|c| c.defs.clone());
    let space = s.space();
    let d = s.doc_mut()?;
    let renames = defs.map(|x| merge(d, &x)).unwrap_or_default();
    let mut out = Vec::new();
    for e in &ents {
        let mut e = adopt(d, e, &renames);
        e.kind.transform(m);
        d.ensure_layer(&e.common.layer);
        out.push(e.handle);
        if let Some(st) = d.space_mut(&space) {
            st.push(e);
        }
    }
    s.set_selection(out.clone());
    Ok(out)
}

/// PASTEBLOCK: the clipboard becomes a new block (named `A$C…`, base point = the clipboard's), and
/// a reference to it is inserted at `at`. Returns the block reference.
pub(crate) fn paste_block(s: &mut Session, at: Vec2) -> Result<Handle> {
    let ents = s.clipboard.clone();
    if ents.is_empty() {
        return Err(bad("pasteblock", "the clipboard is empty"));
    }
    let defs = s.clipboard_source.as_ref().map(|c| c.defs.clone());
    let base = s.clipboard_base;
    let d = s.doc_mut()?;
    let renames = defs.map(|x| merge(d, &x)).unwrap_or_default();
    let seed = d.handseed;
    let name = (0..1_000_000u64)
        .map(|i| format!("A$C{:08X}", seed.wrapping_add(i) & 0xFFFF_FFFF))
        .find(|n| d.block(n).is_none())
        .ok_or_else(|| bad("pasteblock", "no free block name"))?;
    let mut b = Block::new(&name);
    b.base = base.to3(0.0);
    for e in &ents {
        let e = adopt(d, e, &renames);
        d.ensure_layer(&e.common.layer);
        b.entities.push(e);
    }
    d.blocks.insert(name.clone(), Arc::new(b));
    let h = super::blocks::insert(s, &name, at, 1.0, 0.0, &serde_json::Map::new())?;
    s.set_selection(vec![h]);
    Ok(h)
}

/// The clipboard as DXF text (base point in `$INSBASE`), for the system clipboard. `None` when
/// it is empty, very large, or this build has no DXF writer.
pub fn system_text(s: &Session) -> Option<String> {
    if s.clipboard.is_empty() || s.clipboard.len() > MAX_SYSTEM_OBJECTS {
        return None;
    }
    let hooks = super::file::io()?;
    let mut d = Drawing::new_imperial();
    let renames = match &s.clipboard_source {
        Some(c) => {
            d.header = c.defs.header.clone();
            merge(&mut d, &c.defs)
        }
        None => BTreeMap::new(),
    };
    d.header.set("INSBASE", HVal::Point(s.clipboard_base.to3(0.0)));
    for e in &s.clipboard {
        let e = adopt(&mut d, e, &renames);
        d.ensure_layer(&e.common.layer);
        d.model.push(e);
    }
    let bytes = (hooks.write)(&d, "clipboard.dxf").ok()?;
    String::from_utf8(bytes).ok()
}

/// Make DXF text or tab-separated spreadsheet cells (from the system clipboard) the clipboard:
/// its model space objects with what they reference, the base point from `$INSBASE`. `false`
/// (clipboard unchanged) when the text isn't a DXF drawing or spreadsheet range.
pub fn load_system_text(s: &mut Session, text: &str) -> bool {
    let mut lines = text.lines().map(str::trim);
    if lines.next() != Some("0") || lines.next() != Some("SECTION") || text.len() > 256 << 20 {
        return load_tsv_table(s, text);
    }
    let Some(hooks) = super::file::io() else { return false };
    let Ok(src) = (hooks.read)(text.as_bytes(), "clipboard.dxf") else { return false };
    if src.model.is_empty() || src.model.len() > MAX_SYSTEM_OBJECTS {
        return false;
    }
    let ents: Vec<Entity> = src.model.iter().map(|e| (**e).clone()).collect();
    let base = src.header.point("INSBASE").map(|p| p.xy()).filter(|p| p.is_finite()).unwrap_or(Vec2::ZERO);
    let defs = defs_for(&src, &ents);
    set(s, ents, base, defs, None);
    true
}

/// Convert a tab-separated spreadsheet range (copied from Excel / Calc) into a formatted
/// [`EntityKind::Table`] on the clipboard so `Ctrl+V` pastes it directly.
fn load_tsv_table(s: &mut Session, text: &str) -> bool {
    if !text.contains('\t') || text.len() > 4 << 20 {
        return false;
    }
    let raw_rows: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim_end_matches('\r').split('\t').collect())
        .collect();
    if raw_rows.is_empty() {
        return false;
    }
    let ncols = raw_rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if !(2..=64).contains(&ncols) || raw_rows.len() > 500 {
        return false;
    }
    let nrows = raw_rows.len();
    let text_h = 2.0;
    let margin_h = 1.2;
    let margin_v = 0.8;
    let row_h = 5.0;

    let mut col_widths = vec![12.0_f64; ncols];
    let mut col_center = vec![true; ncols];
    for (r_idx, row) in raw_rows.iter().enumerate() {
        for (c_idx, cell) in row.iter().enumerate() {
            let trimmed = cell.trim().trim_matches('"');
            let chars = trimmed.chars().count() as f64;
            let needed = (chars * text_h * 0.68 + margin_h * 2.4).clamp(10.0, 95.0);
            if needed > col_widths[c_idx] {
                col_widths[c_idx] = needed;
            }
            if r_idx > 0 && !trimmed.is_empty() {
                let is_short_or_num = chars <= 4.0
                    || trimmed
                        .chars()
                        .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | '-' | '+' | '/' | ' '));
                if !is_short_or_num {
                    col_center[c_idx] = false;
                }
            }
        }
    }
    let total_w: f64 = col_widths.iter().sum();
    if matches!(s.space(), cadcraft_doc::Space::Paper(_)) && total_w > 225.0 {
        let factor = 225.0 / total_w;
        for w in &mut col_widths {
            *w *= factor;
        }
    }

    let cells: Vec<Vec<cadcraft_doc::TableCell>> = raw_rows
        .iter()
        .enumerate()
        .map(|(r_idx, row)| {
            (0..ncols)
                .map(|c_idx| {
                    let raw = row.get(c_idx).copied().unwrap_or("").trim().trim_matches('"');
                    let style = if r_idx == 0 {
                        String::new()
                    } else if col_center[c_idx] {
                        "DataCenter".into()
                    } else {
                        String::new()
                    };
                    cadcraft_doc::TableCell {
                        text: raw.into(),
                        merged: None,
                        style,
                        formula: None,
                        text_height: None,
                    }
                })
                .collect()
        })
        .collect();

    let border = cadcraft_doc::CellBorder {
        visible: true,
        color: cadcraft_color::Color::True(cadcraft_color::Rgb(0, 0, 0)),
        lineweight: cadcraft_doc::Lineweight::Mm100(18),
    };
    let borders = cadcraft_doc::CellBorders {
        top: border,
        inside_h: border,
        bottom: border,
        left: border,
        inside_v: border,
        right: border,
    };
    let header_cs = cadcraft_doc::CellStyle {
        name: cadcraft_doc::CellStyle::HEADER.into(),
        text_style: "EXCEL_ISO".into(),
        text_height: text_h,
        text_color: cadcraft_color::Color::True(cadcraft_color::Rgb(0, 0, 0)),
        fill: Some(cadcraft_color::Color::True(cadcraft_color::Rgb(183, 222, 232))),
        align: 5,
        margin_h,
        margin_v,
        borders,
        format: cadcraft_doc::CellFormat::Text,
        precision: 2,
        merge: false,
    };
    let data_cs = cadcraft_doc::CellStyle {
        name: cadcraft_doc::CellStyle::DATA.into(),
        text_style: "EXCEL_ISO".into(),
        text_height: text_h,
        text_color: cadcraft_color::Color::True(cadcraft_color::Rgb(0, 0, 0)),
        fill: Some(cadcraft_color::Color::True(cadcraft_color::Rgb(255, 255, 255))),
        align: 4,
        margin_h,
        margin_v,
        borders,
        format: cadcraft_doc::CellFormat::Text,
        precision: 2,
        merge: false,
    };
    let data_center_cs = cadcraft_doc::CellStyle {
        name: "DataCenter".into(),
        align: 5,
        ..data_cs.clone()
    };
    let ts = cadcraft_doc::TableStyle {
        name: "EXCEL_TABLE".into(),
        text_height: text_h,
        margin: margin_v,
        title: false,
        header: nrows > 1,
        cell_styles: vec![header_cs, data_cs, data_center_cs],
    };

    let mut defs = Drawing::new_metric();
    defs.text_styles.push(cadcraft_doc::TextStyle {
        name: "EXCEL_ISO".into(),
        font: "isocp.shx".into(),
        ..cadcraft_doc::TextStyle::default()
    });
    defs.table_styles.push(ts);

    let table = cadcraft_doc::Table {
        insert: Vec2::ZERO.to3(0.0),
        col_widths,
        row_heights: vec![row_h; nrows],
        cells,
        style: "EXCEL_TABLE".into(),
        text_height: text_h,
        title: false,
        header: nrows > 1,
        row_styles: Vec::new(),
        breaks: None,
        data_link: None,
    };
    let mut ent = Entity::new(Handle(1), EntityKind::Table(table));
    if let Ok(d) = s.doc() {
        ent.common.layer = d.header.str("CLAYER", "0").to_string();
    }
    set(s, vec![ent], Vec2::ZERO, defs, None);
    true
}

/// Upper-case names of the table records and blocks some objects use.
#[derive(Default)]
struct Used {
    layers: HashSet<String>,
    linetypes: HashSet<String>,
    text_styles: HashSet<String>,
    dim_styles: HashSet<String>,
    mleader_styles: HashSet<String>,
    table_styles: HashSet<String>,
    blocks: HashSet<String>,
}

fn up(n: &str) -> String {
    n.to_ascii_uppercase()
}

impl Used {
    /// Note what `e` uses; returns the blocks it references.
    fn note(&mut self, e: &Entity) -> Vec<String> {
        self.layers.insert(up(&e.common.layer));
        self.linetypes.insert(up(&e.common.linetype));
        let mut blocks = Vec::new();
        match &e.kind {
            EntityKind::Text(t) => {
                self.text_styles.insert(up(&t.style));
            }
            EntityKind::MText(t) => {
                self.text_styles.insert(up(&t.style));
            }
            EntityKind::AttDef(a) => {
                self.text_styles.insert(up(&a.text.style));
            }
            EntityKind::Insert(i) => {
                self.text_styles.extend(i.attribs.iter().map(|a| up(&a.text.style)));
                blocks.push(i.block.clone());
            }
            EntityKind::Dimension(dm) => {
                self.dim_styles.insert(up(&dm.style));
                blocks.extend(dm.block.clone());
            }
            EntityKind::Leader(l) => {
                self.dim_styles.insert(up(&l.style));
            }
            EntityKind::Tolerance(t) => {
                self.dim_styles.insert(up(&t.style));
            }
            EntityKind::MLeader(m) => {
                self.mleader_styles.insert(up(&m.style));
                self.text_styles.extend(m.text.iter().map(|t| up(&t.style)));
                blocks.extend(m.block.as_ref().map(|b| b.block.clone()));
            }
            EntityKind::Table(t) => {
                self.table_styles.insert(up(&t.style));
            }
            _ => {}
        }
        blocks
    }
}

/// The definitions `ents` need from `src`: the layers, linetypes, styles and blocks they (and the
/// blocks they reference, nested ones too) use, and `src`'s header.
fn defs_for(src: &Drawing, ents: &[Entity]) -> Drawing {
    let mut used = Used::default();
    let mut frontier: Vec<String> = ents.iter().flat_map(|e| used.note(e)).collect();
    let mut walked = 0usize;
    while let Some(name) = frontier.pop() {
        if !used.blocks.insert(up(&name)) {
            continue;
        }
        let Some(b) = src.block(&name) else { continue };
        for e in b.entities.iter() {
            walked += 1;
            if walked > MAX_WALK {
                break;
            }
            frontier.extend(used.note(e));
        }
    }
    // Records referenced by other records.
    for l in src.layers.iter().filter(|l| used.layers.contains(&up(&l.name))) {
        used.linetypes.insert(up(&l.linetype));
    }
    for t in src.linetypes.iter().filter(|t| used.linetypes.contains(&up(&t.name))) {
        used.text_styles.extend(t.pattern.iter().filter_map(|p| p.style.as_deref()).map(up));
    }
    for ds in src.dim_styles.iter().filter(|t| used.dim_styles.contains(&up(&t.name))) {
        used.text_styles.insert(up(&ds.text_style));
    }
    for ms in src.mleader_styles.iter().filter(|t| used.mleader_styles.contains(&up(&t.name))) {
        used.text_styles.insert(up(&ms.text_style));
    }
    for ts in src.table_styles.iter().filter(|t| used.table_styles.contains(&up(&t.name))) {
        used.text_styles.extend(ts.cell_styles.iter().map(|c| up(&c.text_style)));
    }
    fn pick<T: Clone>(all: &[T], used: &HashSet<String>, name: fn(&T) -> &str) -> Vec<T> {
        all.iter().filter(|t| used.contains(&up(name(t)))).cloned().collect()
    }
    let mut d = Drawing::new_imperial();
    d.header = src.header.clone();
    d.layers = pick(&src.layers, &used.layers, |l| &l.name);
    d.linetypes = pick(&src.linetypes, &used.linetypes, |l| &l.name);
    d.text_styles = pick(&src.text_styles, &used.text_styles, |t| &t.name);
    d.dim_styles = pick(&src.dim_styles, &used.dim_styles, |t| &t.name);
    d.mleader_styles = pick(&src.mleader_styles, &used.mleader_styles, |t| &t.name);
    d.table_styles = pick(&src.table_styles, &used.table_styles, |t| &t.name);
    d.blocks = src.blocks.iter().filter(|(k, _)| used.blocks.contains(&up(k))).map(|(k, b)| (k.clone(), b.clone())).collect();
    d.layouts.clear();
    d
}

/// Add `defs`' table records and blocks that `d` doesn't have. Returns the blocks copied, from
/// their name in `defs` to their name in `d`.
fn merge(d: &mut Drawing, defs: &Drawing) -> BTreeMap<String, String> {
    fn add<T: Clone>(ours: &mut Vec<T>, theirs: &[T], name: fn(&T) -> &str) {
        for t in theirs {
            if !ours.iter().any(|o| name(o).eq_ignore_ascii_case(name(t))) {
                ours.push(t.clone());
            }
        }
    }
    add(&mut d.layers, &defs.layers, |l| &l.name);
    add(&mut d.linetypes, &defs.linetypes, |l| &l.name);
    add(&mut d.text_styles, &defs.text_styles, |t| &t.name);
    add(&mut d.dim_styles, &defs.dim_styles, |t| &t.name);
    add(&mut d.mleader_styles, &defs.mleader_styles, |t| &t.name);
    add(&mut d.table_styles, &defs.table_styles, |t| &t.name);
    let renames = block_names(d, defs);
    for (from, to) in &renames {
        let Some(b) = defs.blocks.get(from) else { continue };
        let mut nb = (**b).clone();
        nb.name = to.clone();
        nb.entities = Default::default();
        for e in b.entities.iter() {
            let e = adopt(d, e, &renames);
            d.ensure_layer(&e.common.layer);
            nb.entities.push(e);
        }
        d.blocks.insert(to.clone(), Arc::new(nb));
    }
    renames
}

/// Which blocks of `defs` to copy, and under which name: a new name keeps its own, a generated
/// (anonymous `*…`) block whose name is taken gets a fresh generated name, and a named block `d`
/// already has keeps `d`'s definition (not copied).
fn block_names(d: &Drawing, defs: &Drawing) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut taken: BTreeSet<String> = d.blocks.keys().map(|k| up(k)).collect();
    for (name, b) in &defs.blocks {
        let upper = up(name);
        if upper.starts_with("*MODEL_SPACE") || upper.starts_with("*PAPER_SPACE") {
            continue;
        }
        let to = if !taken.contains(&upper) {
            name.clone()
        } else if b.anonymous || name.starts_with('*') {
            let prefix: String = name.chars().take_while(|c| !c.is_ascii_digit()).collect();
            match (1..=1_000_000).map(|i| format!("{prefix}{i}")).find(|n| !taken.contains(&up(n))) {
                Some(n) => n,
                None => continue,
            }
        } else {
            continue;
        };
        taken.insert(up(&to));
        out.insert(name.clone(), to);
    }
    out
}

/// A copy of a clipboard object for `d`: a fresh handle, block references renamed, and links to
/// objects of the source drawing (associative dimension points) dropped.
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
