//! Data from a drawing file that CADCraft does not model, kept so that saving the drawing writes
//! it back: other programs' objects, extension dictionaries, xdata and properties (#528).
//!
//! It is kept as the file's own DXF groups, with the file's handles. The file reader fills it and
//! the writer puts it back, pointing references at whatever now has the referenced role (the
//! named object dictionary, a table record, a block, an entity); references to objects that are
//! gone are dropped. Commands don't edit it.

use std::collections::BTreeMap;

use crate::{Handle, RawTag};

/// What a file handle stood for when the object it named is one CADCraft writes itself (under a
/// new handle on save): a kind and an upper-case name, e.g. `("LAYER", "WALLS")`,
/// `("TABLE", "LAYER")`, `("BLOCK", "DOOR")`, `("NOD", "")` for the named object dictionary and
/// `("NOD", "ACAD_GROUP")` for one of its entries.
pub type ForeignKey = (String, String);

/// A non-graphical object of a type or role CADCraft does not model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForeignObject {
    /// Its handle in the file it came from (kept on save).
    pub handle: Handle,
    /// Its DXF type (group 0), e.g. `XRECORD`.
    pub kind: String,
    /// Its groups after the type, as read.
    pub tags: Vec<RawTag>,
}

/// What an entity, table record or block carries beyond what CADCraft models.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForeignExtra {
    /// Its extension dictionary (one of [`Foreign::objects`]).
    pub xdict: Option<Handle>,
    /// Objects of [`Foreign::objects`] that react to it (persistent reactors).
    pub reactors: Vec<Handle>,
    /// Other applications' extended data of a table record or block: groups from `1001` on. An
    /// entity's is part of the entity ([`crate::Common::xdata`]), so it follows the entity's copies.
    pub xdata: Vec<RawTag>,
    /// The colour book colour (group 430) and the true colour (group 420) it was read with: it is
    /// written back while the colour is unchanged.
    pub color_book: Option<(String, String)>,
    /// Other unmodelled property groups (material 347, shadow mode 284).
    pub props: Vec<RawTag>,
}

impl ForeignExtra {
    pub fn is_empty(&self) -> bool {
        self.xdict.is_none() && self.reactors.is_empty() && self.xdata.is_empty() && self.color_book.is_none() && self.props.is_empty()
    }
}

/// Everything of a file CADCraft keeps without modelling it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Foreign {
    /// CLASS records (groups after `0 CLASS`) of the kept objects' and entities' types.
    pub classes: Vec<Vec<RawTag>>,
    /// Kept objects, in file order.
    pub objects: Vec<ForeignObject>,
    /// Entries of the named object dictionary that CADCraft doesn't write itself: (key, object).
    pub dictionary: Vec<(String, Handle)>,
    /// Extras of entities, by handle.
    pub entities: BTreeMap<Handle, ForeignExtra>,
    /// Extras of table records (`("LAYER", "WALLS")`), tables (`("TABLE", "LAYER")`) and blocks
    /// (`("BLOCK", "DOOR")`), by [`ForeignKey`].
    pub records: BTreeMap<ForeignKey, ForeignExtra>,
    /// The roles of the file handles the kept data refers to, for objects CADCraft rewrites.
    pub targets: BTreeMap<Handle, ForeignKey>,
    /// The upper-case names of the file's classes in its CLASSES order, kept when the file has
    /// proxy objects or entities: a proxy's class number (group 91) is 500 plus its class's place
    /// here, and saving renumbers it to the written CLASSES (#1812).
    pub class_order: Vec<String>,
    /// The file's data store section (ACDSDATA, AutoCAD 2013 and later), as read: its groups
    /// after the section name. It holds the ACIS geometry of 3D solids, regions, bodies and
    /// surfaces, which from 2013 on those entities don't carry themselves: each `ACDSRECORD`
    /// names its entity with a 320 handle (see [`Foreign::data_store_entities`]).
    pub data_store: Vec<RawTag>,
}

impl Foreign {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
            && self.dictionary.is_empty()
            && self.entities.is_empty()
            && self.records.is_empty()
            && self.classes.is_empty()
            && self.targets.is_empty()
            && self.class_order.is_empty()
            && self.data_store.is_empty()
    }

    /// The entities with a geometry record in the data store: an `ACDSRECORD` naming the entity
    /// (320) that holds its `ASM_Data`. A 320 without one doesn't keep an entity's geometry.
    pub fn data_store_entities(&self) -> std::collections::BTreeSet<Handle> {
        let mut out = std::collections::BTreeSet::new();
        for rec in data_store_records(&self.data_store) {
            let is_record = rec.first().is_some_and(|t| t.value.trim().eq_ignore_ascii_case("ACDSRECORD"));
            if is_record && rec.iter().any(|t| t.code == 2 && t.value.trim().eq_ignore_ascii_case("ASM_Data")) {
                out.extend(data_store_owner(rec).and_then(|t| Handle::parse_hex(t.value.trim())));
            }
        }
        out
    }
}

/// Whether a solid modeler entity (3DSOLID, REGION, BODY) holds its ACIS geometry as SAT text
/// (groups 1 and 3), which DXF 2013 and later no longer has: those files keep it in the data
/// store section, so an entity with SAT text and no record there can't be written to one (#2334).
pub fn sat_text_acis(u: &crate::Unknown, handle: Handle, stored: &std::collections::BTreeSet<Handle>) -> bool {
    ["3DSOLID", "REGION", "BODY"].iter().any(|t| u.dxf_type.eq_ignore_ascii_case(t)) && !stored.contains(&handle) && has_sat_text(&u.tags)
}

/// The entities a DXF of 2013 or later leaves out because they hold SAT-text ACIS data
/// ([`sat_text_acis`]), counted by DXF type, across model space, layouts and block definitions.
pub fn sat_text_dropped(d: &crate::Drawing) -> BTreeMap<String, usize> {
    let stored = d.foreign.data_store_entities();
    let mut out = BTreeMap::new();
    // The writer saves xrefs as references: their blocks (and `xref|` ones) hold nothing in the file.
    let own_blocks = d.blocks.values().filter(|b| b.xref_path.is_none() && !xref_dependent(&b.name));
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(own_blocks.map(|b| &b.entities));
    for e in stores.flat_map(|s| s.iter()) {
        if let crate::EntityKind::Unknown(u) = &e.kind
            && sat_text_acis(u, e.handle, &stored)
        {
            let n: &mut usize = out.entry(u.dxf_type.to_ascii_uppercase()).or_default();
            *n = n.saturating_add(1);
        }
    }
    out
}

/// Whether a symbol name belongs to an attached xref (`xref|name`): the DXF writer leaves such
/// symbols out, and the contents of xref blocks too ([`Block::xref_path`](crate::Block::xref_path)).
pub fn xref_dependent(name: &str) -> bool {
    name.contains('|')
}

/// The records of a data store section's groups: each starts at a `0` group and runs to the next
/// (the section's own groups before the first record are not part of one).
pub fn data_store_records(tags: &[RawTag]) -> Vec<&[RawTag]> {
    let mut out = Vec::new();
    let mut start = tags.iter().position(|t| t.code == 0).unwrap_or(tags.len());
    while start < tags.len() {
        let end = tags.iter().skip(start + 1).position(|t| t.code == 0).map_or(tags.len(), |k| start + 1 + k);
        out.push(tags.get(start..end).unwrap_or(&[]));
        start = end;
    }
    out
}

/// Whether an entity's groups hold SAT text: groups 1 and 3 inside its `AcDbModelerGeometry`
/// subclass. A file of a version before the subclass markers has no `100` groups: all of its
/// groups (before the extended data) are the entity's.
pub fn has_sat_text(tags: &[RawTag]) -> bool {
    let own = tags.iter().take_while(|r| r.code < 1000);
    let marked = tags.iter().take_while(|r| r.code < 1000).any(|r| r.code == 100);
    let mut in_modeler = !marked;
    for r in own {
        if r.code == 100 {
            in_modeler = r.value.trim().eq_ignore_ascii_case("AcDbModelerGeometry");
        } else if in_modeler && (r.code == 1 || r.code == 3) {
            return true;
        }
    }
    false
}

/// The group naming the entity a data store record belongs to: its first `320`. A record is kept
/// or dropped by this handle alone, in the writer and wherever the store is read.
pub fn data_store_owner(record: &[RawTag]) -> Option<&RawTag> {
    record.iter().find(|t| t.code == 320)
}
