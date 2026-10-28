//! Drawing → DXF (AC1015 / R2000 structure, readable by every DXF consumer since 2000). Later
//! versions keep that structure (the groups they add are optional, DXF Reference) and differ in
//! `$ACADVER`, from 2004 in the newer DIMSTYLE groups, and from 2007 in UTF-8 text (the caller
//! picks the text encoding) and MULTILEADER entities (older files get LEADER + MTEXT).

use std::collections::{HashMap, HashSet};

use cadcraft_color::Color;
use cadcraft_doc::{
    AssocSnap, Attrib, Block, Common, DimAssoc, DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, EntityStore, Gradient, Group, HAlign,
    HVal, Handle, Hatch, Insert, Layout, Leader, Line, Lineweight, LwPolyline, MLeader, MLeaderStyle, MText, PageSetup, Point, Solid, Space, Table,
    TableStyle, Text, Transparency, VAlign, VportConfig, VportTile,
};
use cadcraft_dxf::{Tag, Version};
use cadcraft_geom::{Bounds2, Vec2, Vec3};

use crate::dxf_ext::{self, DimVal, K};

mod assoc;
mod blocks;
mod common;
mod ctx;
mod entity;
mod header;
mod layout;
mod objects;
mod sections;

#[cfg(test)]
pub(crate) use assoc::polyline_vertex_ref;
pub(crate) use blocks::is_generated_arrow;
#[cfg(test)]
pub(crate) use blocks::table_block_entities;
use ctx::Ctx;
use entity::entity;
use sections::tags_for;

struct W {
    t: Vec<Tag>,
    next: u64,
    /// Upper-case plot style name → its `ACDBPLACEHOLDER` handle (group 390 of layers and objects).
    pstyles: HashMap<String, String>,
    /// Upper-case layer name → its LAYER table record handle (group 341 of viewports).
    layers: HashMap<String, String>,
    /// Persistent reactors of boundary objects: each source object of an associative hatch →
    /// the hatches (groups `{ACAD_REACTORS 330 …}`), from [`hatch_reactors`].
    reactors: HashMap<Handle, Vec<String>>,
    /// Handles already given in the file being written (entities' own, and every one handed out).
    used: HashSet<u64>,
    /// Handles kept from the file the drawing was read from: new handles
    /// skip them, so each goes back to the object that had it.
    reserved: HashSet<u64>,
    /// Role → the handle it had when read ([`cadcraft_doc::foreign::Foreign::handles`]).
    roles: HashMap<(String, String), Handle>,
}

impl W {
    /// A writer for groups only: no handles are kept or reserved.
    fn scratch(next: u64) -> W {
        W {
            t: Vec::new(),
            next,
            pstyles: HashMap::new(),
            layers: HashMap::new(),
            reactors: HashMap::new(),
            used: HashSet::new(),
            reserved: HashSet::new(),
            roles: HashMap::new(),
        }
    }
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
    /// A new handle, never one that is used or kept for another object.
    fn h(&mut self) -> String {
        // A hostile file can put handles at the top of the range: past it, look for a free one
        // from the bottom. Among `taken + 1` values one is free, so this ends.
        while self.next == 0 || self.next == u64::MAX || self.used.contains(&self.next) || self.reserved.contains(&self.next) {
            self.next = if self.next >= u64::MAX - 1 { 1 } else { self.next + 1 };
        }
        let n = self.next;
        self.used.insert(n);
        self.next = self.next.saturating_add(1);
        format!("{n:X}")
    }
    /// The handle `kept` (an object's own, else the one its role had when read), if it is set
    /// and not given yet; otherwise a new one.
    fn keep(&mut self, kept: impl Into<Handle>, role: (&str, &str)) -> String {
        let kept: Handle = kept.into();
        let role_handle = || self.roles.get(&(role.0.to_string(), role.1.to_ascii_uppercase())).copied();
        let h = if kept.is_none() { role_handle().unwrap_or(Handle(0)) } else { kept };
        if !h.is_none() && !self.used.contains(&h.0) {
            self.used.insert(h.0);
            return h.hex();
        }
        self.h()
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
    cadcraft_dxf::write_ascii(&tags(d, Version::R2000))
}

/// The drawing's DXF tags, declaring `version` in `$ACADVER`.
pub fn tags(d: &Drawing, version: Version) -> Vec<Tag> {
    tags_for(d, version, version)
}

/// The R2000 DXF the DWG bridge converts to a `dwg` version DWG: its entity types are the DWG
/// version's (MULTILEADER from 2007).
pub(crate) fn dwg_bridge(d: &Drawing, dwg: Version) -> String {
    let mut tags = tags_for(d, Version::R2000, dwg);
    bridge_markers(&mut tags);
    cadcraft_dxf::write_ascii(&tags)
}

/// Subclass markers the DWG bridge knows by another spelling (it matches them exactly):
/// AutoCAD writes a dynamic block lookup parameter's as `AcDbBlockLookUpParameter`, the bridge
/// reads `AcDbBlockLookupParameter`; otherwise the parameter loses its name and its lookup
/// action in the DWG, and AutoCAD's lookups stop working.
fn bridge_markers(tags: &mut [Tag]) {
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
    let mut w = W::scratch(d.handseed.max(0x100).saturating_add(0x1000));
    let cx = Ctx { target: Version::R2018, ..Ctx::default() };
    let paper = d.space_of(e.handle).is_some_and(|s| !matches!(s, cadcraft_doc::Space::Model));
    entity(&mut w, d, e, "0", paper, &cx);
    let mut tags = w.t.into_iter();
    let kind = tags.next().filter(|t| t.code == 0).map(|t| t.value.as_str()).unwrap_or_default();
    (kind, tags.take_while(|t| t.code != 0).filter(|t| t.code != 330).collect())
}
