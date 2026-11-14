//! Drawing → DXF (AC1015 / R2000 structure, readable by every DXF consumer since 2000). Later
//! versions keep that structure and differ in `$ACADVER`, from 2004 in CLASS instance counts and
//! the newer DIMSTYLE groups, and from 2007 in UTF-8 text (the caller picks the text encoding) and
//! MULTILEADER entities (older files get LEADER + MTEXT).

use std::collections::HashMap;

use cadcraft_color::Color;
use cadcraft_doc::{
    AssocSnap, Attrib, Block, Common, DimAssoc, DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, EntityStore, Gradient, Group, HAlign,
    HVal, Handle, Hatch, Insert, Layout, Leader, Line, Lineweight, LwPolyline, MLeader, MLeaderStyle, MText, PageSetup, Point, Solid, Space, Table,
    TableStyle, Text, Transparency, VAlign, VportConfig, VportTile,
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
    /// Upper-case plot style name → its `ACDBPLACEHOLDER` handle (group 390 of layers and objects).
    pstyles: HashMap<String, String>,
    /// Upper-case layer name → its LAYER table record handle (group 331/341 of viewports).
    layers: HashMap<String, String>,
    /// Persistent reactors of source geometry: the associative hatches and DIMASSOC objects
    /// that follow it (groups `{ACAD_REACTORS 330 …}`).
    reactors: HashMap<Handle, Vec<String>>,
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
    fn next_handle(&mut self) -> u64 {
        let h = self.next;
        self.next = self.next.saturating_add(1);
        h
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
    tags_for(d, version, version, version < Version::R2004)
}

/// The R2000 DXF the DWG bridge converts to a `dwg` version DWG: its entity types are the DWG
/// version's (MULTILEADER from 2007).
pub(crate) fn dwg_bridge(d: &Drawing, dwg: Version) -> String {
    let mut tags = tags_for(d, Version::R2000, dwg, false);
    bridge_markers(&mut tags);
    craft_dxf::write_ascii(&tags)
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
    let mut records = entity_records(d, e).into_iter();
    let (kind, tags) = records.next().unwrap_or_default();
    (kind, tags.into_iter().filter(|t| t.code != 330).collect())
}

/// The records an entity is written with after its own, each as its type name and groups
/// without its `0` group: an INSERT's ATTRIBs and SEQEND, a POLYLINE's VERTEXes and SEQEND.
/// Their handles (5) and owner (330) are left out: they have none of their own in the drawing.
/// AutoLISP's `entnext` and `entget` reach an INSERT's attributes through it.
pub fn entity_sub_groups(d: &Drawing, e: &Entity) -> Vec<(String, Vec<Tag>)> {
    entity_records(d, e).into_iter().skip(1).map(|(kind, tags)| (kind, tags.into_iter().filter(|t| t.code != 5 && t.code != 330).collect())).collect()
}

/// An entity's records as the writer stores them, split at their `0` groups.
fn entity_records(d: &Drawing, e: &Entity) -> Vec<(String, Vec<Tag>)> {
    let mut w = W {
        t: Vec::new(),
        next: d.handseed.max(0x100).saturating_add(0x1000),
        pstyles: HashMap::new(),
        layers: HashMap::new(),
        reactors: HashMap::new(),
    };
    let cx = Ctx { target: Version::R2018, legacy_columns: true, ..Ctx::default() };
    let paper = d.space_of(e.handle).is_some_and(|s| !matches!(s, cadcraft_doc::Space::Model));
    entity(&mut w, d, e, "0", paper, &cx);
    let mut records: Vec<(String, Vec<Tag>)> = Vec::new();
    for t in w.t {
        if t.code == 0 {
            records.push((t.value.as_str(), Vec::new()));
        } else if let Some((_, tags)) = records.last_mut() {
            tags.push(t);
        }
    }
    // Strings as AutoLISP sees them: `10^2`, not the file's `10^ 2`.
    for (kind, tags) in &mut records {
        crate::dxf_read::mtext_dxf::map_text_groups(kind, tags, crate::dxf_read::mtext_dxf::decode_line);
    }
    records
}
