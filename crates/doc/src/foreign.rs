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
            && self.data_store.is_empty()
    }

    /// Re-key what is kept for the record named `from` under each of `kinds` (e.g. `["LAYER"]`, or
    /// `["BLOCK_RECORD", "BLOCK", "ENDBLK"]` for a block) to the name `to`, after a rename: its
    /// extras ([`Foreign::records`]) and the roles of its file handles ([`Foreign::targets`]), so
    /// saving writes its xdata, extension dictionary and reactors on the renamed record. Names
    /// compare trimmed and upper-case, as the reader keys them, so a rename that only changes case
    /// changes nothing.
    pub fn rename(&mut self, kinds: &[&str], from: &str, to: &str) {
        let (from, to) = (from.trim().to_ascii_uppercase(), to.trim().to_ascii_uppercase());
        if from == to {
            return;
        }
        for kind in kinds {
            let old: ForeignKey = ((*kind).to_string(), from.clone());
            let new: ForeignKey = ((*kind).to_string(), to.clone());
            if let Some(extra) = self.records.remove(&old) {
                self.records.insert(new.clone(), extra);
            }
            self.targets.values_mut().filter(|k| **k == old).for_each(|k| *k = new.clone());
        }
    }

    /// Whether anything is kept for the record named `from` under one of `kinds`: what
    /// [`Foreign::rename`] would re-key.
    pub fn names(&self, kinds: &[&str], from: &str) -> bool {
        let from = from.trim().to_ascii_uppercase();
        let hit = |k: &ForeignKey| k.1 == from && kinds.contains(&k.0.as_str());
        self.records.keys().any(hit) || self.targets.values().any(hit)
    }

    /// [`Foreign::rename`] on a drawing's shared copy, which is cloned only when it keeps
    /// something for the renamed record and the name really changes (not only its case).
    pub fn rename_shared(this: &mut std::sync::Arc<Foreign>, kinds: &[&str], from: &str, to: &str) {
        if !from.trim().eq_ignore_ascii_case(to.trim()) && this.names(kinds, from) {
            std::sync::Arc::make_mut(this).rename(kinds, from, to);
        }
    }

    /// The entities the data store has records for (their 320 handles).
    pub fn data_store_entities(&self) -> std::collections::BTreeSet<Handle> {
        self.data_store.iter().filter(|t| t.code == 320).filter_map(|t| Handle::parse_hex(t.value.trim())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str, n: &str) -> ForeignKey {
        (k.to_string(), n.to_string())
    }

    #[test]
    fn rename_rekeys_records_and_targets_of_the_given_kinds_only() {
        let mut f = Foreign::default();
        let xdict = ForeignExtra { xdict: Some(Handle(0x40)), ..ForeignExtra::default() };
        f.records.insert(key("BLOCK_RECORD", "DOOR"), xdict.clone());
        f.records.insert(key("LAYER", "DOOR"), ForeignExtra { reactors: vec![Handle(0x41)], ..ForeignExtra::default() });
        f.targets.insert(Handle(0x10), key("BLOCK_RECORD", "DOOR"));
        f.targets.insert(Handle(0x11), key("BLOCK", "DOOR"));
        f.targets.insert(Handle(0x12), key("LAYER", "DOOR"));
        f.rename(&["BLOCK_RECORD", "BLOCK", "ENDBLK"], "Door", "Gate");
        assert_eq!(f.records.get(&key("BLOCK_RECORD", "GATE")), Some(&xdict));
        assert!(!f.records.contains_key(&key("BLOCK_RECORD", "DOOR")));
        assert!(f.records.contains_key(&key("LAYER", "DOOR")), "another kind with the same name stays");
        assert_eq!(f.targets.get(&Handle(0x10)), Some(&key("BLOCK_RECORD", "GATE")));
        assert_eq!(f.targets.get(&Handle(0x11)), Some(&key("BLOCK", "GATE")));
        assert_eq!(f.targets.get(&Handle(0x12)), Some(&key("LAYER", "DOOR")));
    }

    #[test]
    fn rename_shared_leaves_an_unrelated_drawing_shared() {
        let mut f = Foreign::default();
        f.records.insert(key("LAYER", "A"), ForeignExtra { xdict: Some(Handle(0x40)), ..ForeignExtra::default() });
        let mut a = std::sync::Arc::new(f);
        let snapshot = a.clone();
        Foreign::rename_shared(&mut a, &["LAYER"], "B", "C");
        assert!(std::sync::Arc::ptr_eq(&a, &snapshot));
        Foreign::rename_shared(&mut a, &["LAYER"], "a", "c");
        assert!(a.records.contains_key(&key("LAYER", "C")) && snapshot.records.contains_key(&key("LAYER", "A")));
    }
}
