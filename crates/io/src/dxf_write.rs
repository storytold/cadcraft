//! Drawing → DXF (AC1015 / R2000 structure, readable by every DXF consumer since 2000). Later
//! versions keep that structure and differ in `$ACADVER`, from 2004 in CLASS instance counts and
//! the newer DIMSTYLE groups, and from 2007 in UTF-8 text (the caller picks the text encoding) and
//! MULTILEADER entities (older files get LEADER + MTEXT).

use std::collections::{HashMap, HashSet};

use cadcraft_color::Color;
use cadcraft_doc::{
    AssocSnap, Attrib, Block, Common, DimAssoc, DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, EntityStore, Gradient, Group, HAlign,
    HVal, Handle, Hatch, Insert, Layout, Leader, Line, Lineweight, LwPolyline, MLeader, MLeaderStyle, MText, PageSetup, Point, Solid, Space, Table,
    TableStyle, Text, Transparency, Ucs, VAlign, VportConfig, VportTile,
};
use cadcraft_geom::{Bounds2, Vec2, Vec3};
use craft_dxf::{Tag, Version};

use crate::dxf_ext::{self, DimVal, K};

mod assoc;
mod blocks;
mod common;
mod ctx;
mod entity;
mod header;
mod layout;
mod objects;
mod proxy;
pub use proxy::proxies_lost;
mod sections;

#[cfg(test)]
pub(crate) use assoc::polyline_vertex_ref;
pub(crate) use blocks::is_generated_arrow;
#[cfg(test)]
pub(crate) use blocks::table_block_entities;
use ctx::Ctx;
use entity::entity;
pub(crate) use header::code_page;
use sections::tags_for;

struct W {
    t: Vec<Tag>,
    next: u64,
    /// Handles the drawing's objects already hold: structural records skip them, so writing a
    /// file whose own handles sit near the top of the range never repeats a handle (#913).
    used: HashSet<u64>,
    /// Upper-case plot style name → its `ACDBPLACEHOLDER` handle (group 390 of layers and objects).
    pstyles: HashMap<String, String>,
    /// Upper-case layer name → its LAYER table record handle (group 331/341 of viewports).
    layers: HashMap<String, String>,
    /// Upper-case UCS name → its UCS table record handle (group 345 of layouts and viewports).
    ucss: HashMap<String, String>,
    /// Active VPORT group-345 tags (their index in `t`) waiting for the handle of the named UCS
    /// (upper case) they refer to, filled in after the UCS table is written.
    vport_ucs: Vec<(usize, String)>,
    /// Persistent reactors of source geometry: the associative hatches and DIMASSOC objects
    /// that follow it (groups `{ACAD_REACTORS 330 …}`).
    reactors: HashMap<Handle, Vec<String>>,
}

/// Where the writer's own records (tables, objects, dictionaries) start numbering: above every
/// object handle and the drawing's seed, with headroom for handles the drawing allocates next.
fn structural_start(d: &Drawing) -> u64 {
    let highest = entity_handles(d).max().unwrap_or(0);
    d.handseed.max(0x100).max(highest.saturating_add(1)).saturating_add(0x1000)
}

/// Every handle an object holds, plus the drawing's next one.
fn entity_handles(d: &Drawing) -> impl Iterator<Item = u64> + '_ {
    let model = d.model.iter().map(|e| e.handle.0);
    let layouts = d.layouts.iter().flat_map(|l| l.entities.iter()).map(|e| e.handle.0);
    let blocks = d.blocks.values().flat_map(|b| b.entities.iter()).map(|e| e.handle.0);
    model.chain(layouts).chain(blocks).chain(std::iter::once(d.handseed))
}

impl W {
    fn s(&mut self, c: i32, v: impl Into<String>) {
        self.t.push(Tag::s(c, v));
    }
    fn f(&mut self, c: i32, v: f64) {
        self.t.push(Tag::f(c, v));
    }
    fn i(&mut self, c: i32, v: i64) {
        self.t.push(Tag::i(c, v));
    }
    fn p(&mut self, c: i32, p: Vec3) {
        self.f(c, p.x);
        self.f(c + 10, p.y);
        self.f(c + 20, p.z);
    }
    fn p2(&mut self, c: i32, p: Vec2) {
        self.f(c, p.x);
        self.f(c + 10, p.y);
    }
    fn h(&mut self) -> String {
        format!("{:X}", self.next_handle())
    }
    /// A new handle for a record the drawing doesn't hold (as [`W::h`]).
    ///
    /// Skips every handle in use, so a file object carrying a handle above the seed (the
    /// reserved band) is not handed out again, and wraps into that band if the writer runs off
    /// the top of the range. Each handle skipped is one in use, so the walk is bounded by their
    /// count.
    fn next_handle(&mut self) -> u64 {
        for _ in 0..self.used.len().saturating_add(1) {
            let h = self.next;
            self.next = h.checked_add(1).unwrap_or(cadcraft_doc::HANDLE_LIMIT);
            if self.used.insert(h) {
                return h;
            }
        }
        self.next
    }
    /// A `102 {NAME` … `102 }` group of handles.
    fn group(&mut self, name: &str, code: i32, handles: &[&str]) {
        self.s(102, format!("{{{name}"));
        for h in handles {
            self.s(code, *h);
        }
        self.s(102, "}");
    }
    /// A dimension variable as a group (DIMSTYLE records) or as an xdata value (DSTYLE).
    fn dimval(&mut self, code: i32, v: DimVal, xdata: bool) {
        match v {
            DimVal::Real(x) => self.f(if xdata { 1040 } else { code }, x),
            DimVal::Int(i) => self.i(if xdata { 1070 } else { code }, i),
            DimVal::Str(t) => {
                if xdata || !t.is_empty() {
                    self.s(if xdata { 1000 } else { code }, t);
                }
            }
            DimVal::Handle(h) => {
                if xdata {
                    self.s(1005, h);
                } else if h != "0" {
                    self.s(code, h);
                }
            }
        }
    }
    fn xdata(&mut self, tags: Vec<Tag>) {
        self.t.extend(tags);
    }
}

/// Write a drawing as ASCII DXF.
/// The drawing as R2000 ASCII DXF.
pub fn write(d: &Drawing) -> String {
    craft_dxf::write_ascii(&tags(d, Version::R2000))
}

/// The drawing's DXF tags, declaring `version` in `$ACADVER`.
pub fn tags(d: &Drawing, version: Version) -> Vec<Tag> {
    tags_kept(d, version).0
}

/// [`tags`], with the foreign objects the file keeps, by their handles in the drawing.
fn tags_kept(d: &Drawing, version: Version) -> (Vec<Tag>, std::collections::HashSet<cadcraft_doc::Handle>) {
    let (mut tags, kept) = sections::tags_kept_for(d, version, version, version < Version::R2004, false);
    // Kept proxies in this version's layout (#3229). Not for the DWG bridge, whose writer reads
    // them in the layout of the DWG they came from.
    proxy::proxy_layout(&mut tags, version);
    (tags, kept)
}

/// The R2000 DXF the DWG bridge converts to a `dwg` version DWG: its entity types are the DWG
/// version's (MULTILEADER from 2007).
pub(crate) fn dwg_bridge(d: &Drawing, dwg: Version) -> (String, craft_dwg::Unwritten) {
    let (mut tags, kept) = sections::tags_kept_for(d, Version::R2000, dwg, false, true);
    let mut unwritten = craft_dwg::Unwritten::new();
    if dwg == Version::R2007 {
        let omitted = legacy_visualstyles_kept(d, &kept);
        if omitted != 0 {
            let mut source = d.clone();
            std::sync::Arc::make_mut(&mut source.foreign).objects.retain(|object| !legacy_visual_style(object));
            tags = tags_for(&source, Version::R2000, dwg, false);
            unwritten.insert("VISUALSTYLE".into(), omitted);
        }
    }
    bridge_markers(&mut tags);
    (craft_dxf::write_ascii(&tags), unwritten)
}

/// Version-specific DWG compatibility omissions, counted from the actual saved view.
/// Hosts use this before saving and when reporting saves through their byte-only I/O hooks.
pub fn dwg_compat_lost(d: &Drawing, version: Version) -> craft_dwg::Unwritten {
    let mut lost = craft_dwg::Unwritten::new();
    if version == Version::R2007 && d.foreign.objects.iter().any(legacy_visual_style) {
        let (_, kept) = sections::tags_kept_for(d, Version::R2000, version, false, true);
        let omitted = legacy_visualstyles_kept(d, &kept);
        if omitted != 0 {
            lost.insert("VISUALSTYLE".into(), omitted);
        }
    }
    lost
}

fn legacy_visualstyles_kept(d: &Drawing, kept: &HashSet<cadcraft_doc::Handle>) -> usize {
    d.foreign.objects.iter().filter(|object| kept.contains(&object.handle) && legacy_visual_style(object)).count()
}

/// The fixed-property VISUALSTYLE layout in the 2007/2010 DXF Reference has no 176/177
/// property pairs. The bridge cannot faithfully save this layout as a 2007 DWG.
fn legacy_visual_style(object: &cadcraft_doc::foreign::ForeignObject) -> bool {
    if !object.kind.trim().eq_ignore_ascii_case("VISUALSTYLE") {
        return false;
    }
    let mut properties = object.tags.iter().skip_while(|tag| tag.code != 100 || tag.value.trim() != "AcDbVisualStyle");
    properties.next();
    let mut fixed = false;
    for tag in properties.take_while(|tag| tag.code != 100 && tag.code < 1000) {
        if matches!(tag.code, 176 | 177) {
            return false;
        }
        fixed |= matches!(tag.code, 40 | 71);
    }
    fixed
}

/// Proxy type names and lookup-parameter subclass names must match the bridge reader's spelling.
/// Ordinary DXF output retains source spellings.
fn bridge_markers(tags: &mut [Tag]) {
    for tag in tags.iter_mut().filter(|tag| tag.code == 0) {
        for name in ["ACAD_PROXY_ENTITY", "ACAD_PROXY_OBJECT"] {
            if tag.str().trim().eq_ignore_ascii_case(name) {
                *tag = Tag::s(0, name);
                break;
            }
        }
    }
    for t in tags.iter_mut().filter(|t| t.code == 100) {
        if t.str().trim().eq_ignore_ascii_case("AcDbBlockLookupParameter") {
            *t = Tag::s(100, "AcDbBlockLookupParameter");
        }
    }
}

/// One entity's DXF groups, as the writer stores them, without its `0` group: its type name
/// and the groups of its record (an entity written as several records, a POLYLINE and its
/// VERTEXes, gives its first). Handles of objects outside the entity (owner, styles) are left
/// out. AutoLISP's `entget` is built on it.
pub fn entity_groups(d: &Drawing, e: &Entity) -> (String, Vec<Tag>) {
    // A handle the writer makes here goes only on the entity's later records (VERTEX, ATTRIB,
    // SEQEND), which are dropped below, so no walk over the drawing's handles is needed: AutoLISP
    // calls this once per entity, and that walk would make a loop over a drawing quadratic.
    let mut w = W {
        t: Vec::new(),
        next: d.handseed.max(0x100).saturating_add(0x1000),
        used: HashSet::new(),
        pstyles: HashMap::new(),
        layers: HashMap::new(),
        ucss: HashMap::new(),
        vport_ucs: Vec::new(),
        reactors: HashMap::new(),
    };
    let cx = Ctx { target: Version::R2018, legacy_columns: true, ..Ctx::default() };
    let paper = d.space_of(e.handle).is_some_and(|s| !matches!(s, cadcraft_doc::Space::Model));
    entity(&mut w, d, e, "0", paper, &cx);
    let mut tags = w.t.into_iter();
    let kind = tags.next().filter(|t| t.code == 0).map(|t| t.value.as_str()).unwrap_or_default();
    let mut tags: Vec<Tag> = tags.take_while(|t| t.code != 0).filter(|t| t.code != 330).collect();
    // Strings as AutoLISP sees them: `10^2`, not the file's `10^ 2`.
    crate::dxf_read::mtext_dxf::map_text_groups(&kind, &mut tags, crate::dxf_read::mtext_dxf::decode_line);
    (kind, tags)
}
