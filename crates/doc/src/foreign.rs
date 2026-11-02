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
    /// The `$ACADVER` of the file all this was read from: its groups are in that version's form.
    /// The drawing's own `$ACADVER` is the version it is saved as, which SAVEAS changes, so a
    /// later save must not take it for this one (#2471).
    pub version: Option<String>,
}

impl Foreign {
    /// The class of the file's CLASSES a proxy's group 91 names (see [`class_number`]), if any.
    /// Saving as DWG keeps a proxy with that class and leaves out one whose class it can't name.
    pub fn proxy_class(&self, tags: &[RawTag]) -> Option<&str> {
        class_index(class_number(tags.iter().map(|r| (r.code, r.value.as_str())))).and_then(|i| self.class_order.get(i)).map(String::as_str)
    }

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

    /// The entities the data store has records for (their 320 handles).
    pub fn data_store_entities(&self) -> std::collections::BTreeSet<Handle> {
        self.data_store.iter().filter(|t| t.code == 320).filter_map(|t| Handle::parse_hex(t.value.trim())).collect()
    }
}

/// Where a proxy's class number is: its first group 91 outside the `102 {NAME` … `102 }` groups
/// (an application's can hold a 91 of its own) and before any xdata.
pub fn class_group<S: AsRef<str>>(groups: impl IntoIterator<Item = (i32, S)>) -> Option<usize> {
    let mut in_group = false;
    for (i, (code, value)) in groups.into_iter().enumerate() {
        match code {
            1000.. => return None,
            102 => in_group = value.as_ref().trim().starts_with('{'),
            91 if !in_group => return Some(i),
            _ => {}
        }
    }
    None
}

/// A proxy's class number (see [`class_group`]).
pub fn class_number<S: AsRef<str>>(groups: impl IntoIterator<Item = (i32, S)> + Clone) -> Option<i64> {
    let i = class_group(groups.clone())?;
    groups.into_iter().nth(i).and_then(|(_, v)| v.as_ref().trim().parse().ok())
}

/// The place in CLASSES a class number names (500 is the first class).
pub fn class_index(number: Option<i64>) -> Option<usize> {
    usize::try_from(number?.checked_sub(500)?).ok()
}
