//! The layer and block tables, whose names are looked up without case in about constant time.
//!
//! [`Drawing::layer`](crate::Drawing::layer) and [`Drawing::block`](crate::Drawing::block) run for
//! every entity while a drawing is read, measured and drawn. Scanning the table on each made a file
//! of N layers, or of N blocks and N references to blocks it does not have, cost N² name
//! comparisons: tens of seconds to open at a few tens of thousands (#1924, F-04).
//!
//! Each table keeps an index from the hash of a name with its ASCII letters lower-cased to the
//! table's first entry of that hash, so a lookup returns what a scan in table order returns.
//! Every change to a table goes through `DerefMut`, which drops the index, so it can never be
//! stale. The index is built only once a table has been scanned [`SCANS_BEFORE_INDEX`] times since
//! it last changed: a table changed between every few lookups costs what scanning it always did,
//! and one looked up many times in between is indexed once.

use std::collections::{BTreeMap, HashMap, btree_map};
use std::hash::{BuildHasher, Hasher};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

/// Tables with fewer entries than this are always scanned: comparing a few names costs less than
/// hashing one.
const MIN_INDEXED: usize = 64;
/// Lookups that scan a table after it changed before its index is built. A build costs about as
/// much as some tens of scans.
const SCANS_BEFORE_INDEX: u32 = 32;

/// From the hash of a name without case to the first entry, in table order, of a name with that
/// hash. The hash is keyed per index (the map's `RandomState`), so a file cannot choose names that
/// collide.
struct Index<V>(HashMap<u64, V>);

impl<V> Index<V> {
    fn build<'a>(entries: impl Iterator<Item = (&'a str, V)>) -> Self {
        let mut map = HashMap::new();
        for (name, v) in entries {
            map.entry(folded_hash(map.hasher(), name)).or_insert(v);
        }
        Index(map)
    }

    /// The first entry whose name hashes as `name` does: the first named `name` without case when
    /// its name is that, else another name with the same hash, and the caller scans.
    fn get(&self, name: &str) -> Option<&V> {
        self.0.get(&folded_hash(self.0.hasher(), name))
    }
}

/// The hash of `name` with its ASCII letters lower-cased: names that `eq_ignore_ascii_case` finds
/// equal hash the same.
fn folded_hash(state: &impl BuildHasher, name: &str) -> u64 {
    let mut h = state.build_hasher();
    let mut buf = [0u8; 64];
    for chunk in name.as_bytes().chunks(buf.len()) {
        if let Some(part) = buf.get_mut(..chunk.len()) {
            part.copy_from_slice(chunk);
            part.make_ascii_lowercase();
            h.write(part);
        }
    }
    h.write_usize(name.len());
    h.finish()
}

/// A table's index: built on demand, shared by copies of the unchanged table (an undo snapshot),
/// dropped when the table changes.
struct Lazy<V> {
    index: OnceLock<Arc<Index<V>>>,
    /// Lookups that scanned the table since it last changed.
    scans: AtomicU32,
}

impl<V> Lazy<V> {
    /// The index of a table of `len` entries, built with `build` once the table has been scanned
    /// often enough since it changed; `None` while it is to be scanned.
    fn get(&self, len: usize, build: impl FnOnce() -> Index<V>) -> Option<&Index<V>> {
        if let Some(ix) = self.index.get() {
            return Some(ix);
        }
        if len < MIN_INDEXED || self.scans.fetch_add(1, Ordering::Relaxed) < SCANS_BEFORE_INDEX {
            return None;
        }
        Some(self.index.get_or_init(|| Arc::new(build())))
    }

    /// Forget the index: the table may change.
    fn clear(&mut self) {
        self.index = OnceLock::new();
        *self.scans.get_mut() = 0;
    }
}

impl<V> Default for Lazy<V> {
    fn default() -> Self {
        Lazy { index: OnceLock::new(), scans: AtomicU32::new(0) }
    }
}

impl<V> Clone for Lazy<V> {
    fn clone(&self) -> Self {
        Lazy { index: self.index.clone(), scans: AtomicU32::new(0) }
    }
}

/// An entry of a [`NamedList`]: what it is looked up by.
pub trait Named {
    fn name(&self) -> &str;
}

impl Named for crate::Layer {
    fn name(&self) -> &str {
        &self.name
    }
}

/// A list of named entries in their own order (the drawing's layers). It reads and changes as a
/// `Vec`; [`NamedList::by_name`] finds the first entry of a name without case.
pub struct NamedList<T> {
    list: Vec<T>,
    index: Lazy<usize>,
}

impl<T: Named> NamedList<T> {
    /// The position of the first entry named `name`, without case.
    pub fn position_by_name(&self, name: &str) -> Option<usize> {
        let scan = || self.list.iter().position(|t| t.name().eq_ignore_ascii_case(name));
        let Some(ix) = self.index.get(self.list.len(), || Index::build(self.list.iter().enumerate().map(|(i, t)| (t.name(), i)))) else {
            return scan();
        };
        match ix.get(name) {
            None => None,
            Some(&i) if self.list.get(i).is_some_and(|t| t.name().eq_ignore_ascii_case(name)) => Some(i),
            Some(_) => scan(),
        }
    }

    /// The first entry named `name`, without case.
    pub fn by_name(&self, name: &str) -> Option<&T> {
        self.position_by_name(name).and_then(|i| self.list.get(i))
    }
}

impl<T> Deref for NamedList<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.list
    }
}

impl<T> DerefMut for NamedList<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        self.index.clear();
        &mut self.list
    }
}

impl<T> Default for NamedList<T> {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl<T: Clone> Clone for NamedList<T> {
    fn clone(&self) -> Self {
        NamedList { list: self.list.clone(), index: self.index.clone() }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for NamedList<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.list.fmt(f)
    }
}

impl<T: PartialEq> PartialEq for NamedList<T> {
    fn eq(&self, other: &Self) -> bool {
        self.list == other.list
    }
}

impl<T> From<Vec<T>> for NamedList<T> {
    fn from(list: Vec<T>) -> Self {
        NamedList { list, index: Lazy::default() }
    }
}

impl<T> From<NamedList<T>> for Vec<T> {
    fn from(l: NamedList<T>) -> Self {
        l.list
    }
}

impl<T: serde::Serialize> serde::Serialize for NamedList<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.list.serialize(s)
    }
}

impl<T> FromIterator<T> for NamedList<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Vec::from_iter(iter).into()
    }
}

impl<T> IntoIterator for NamedList<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a NamedList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut NamedList<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.deref_mut().iter_mut()
    }
}

/// Entries by name, in name order (the drawing's block definitions). It reads and changes as a
/// `BTreeMap`; [`NamedMap::by_name`] finds an entry by its exact name, else the first in name
/// order whose name matches without case.
pub struct NamedMap<V> {
    map: BTreeMap<String, V>,
    index: Lazy<Box<str>>,
}

impl<V> NamedMap<V> {
    /// The entry named exactly `name`, else the first in name order named `name` without case.
    pub fn by_name(&self, name: &str) -> Option<&V> {
        if let Some(v) = self.map.get(name) {
            return Some(v);
        }
        let scan = || self.map.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v);
        let Some(ix) = self.index.get(self.map.len(), || Index::build(self.map.keys().map(|k| (k.as_str(), Box::from(k.as_str()))))) else {
            return scan();
        };
        match ix.get(name) {
            None => None,
            Some(k) if k.eq_ignore_ascii_case(name) => self.map.get(&**k).or_else(scan),
            Some(_) => scan(),
        }
    }
}

impl<V> Deref for NamedMap<V> {
    type Target = BTreeMap<String, V>;
    fn deref(&self) -> &BTreeMap<String, V> {
        &self.map
    }
}

impl<V> DerefMut for NamedMap<V> {
    fn deref_mut(&mut self) -> &mut BTreeMap<String, V> {
        self.index.clear();
        &mut self.map
    }
}

impl<V> Default for NamedMap<V> {
    fn default() -> Self {
        BTreeMap::new().into()
    }
}

impl<V: Clone> Clone for NamedMap<V> {
    fn clone(&self) -> Self {
        NamedMap { map: self.map.clone(), index: self.index.clone() }
    }
}

impl<V: std::fmt::Debug> std::fmt::Debug for NamedMap<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.map.fmt(f)
    }
}

impl<V: PartialEq> PartialEq for NamedMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl<V> From<BTreeMap<String, V>> for NamedMap<V> {
    fn from(map: BTreeMap<String, V>) -> Self {
        NamedMap { map, index: Lazy::default() }
    }
}

impl<V> FromIterator<(String, V)> for NamedMap<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
        BTreeMap::from_iter(iter).into()
    }
}

impl<V> IntoIterator for NamedMap<V> {
    type Item = (String, V);
    type IntoIter = btree_map::IntoIter<String, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a, V> IntoIterator for &'a NamedMap<V> {
    type Item = (&'a String, &'a V);
    type IntoIter = btree_map::Iter<'a, String, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

impl<'a, V> IntoIterator for &'a mut NamedMap<V> {
    type Item = (&'a String, &'a mut V);
    type IntoIter = btree_map::IterMut<'a, String, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.deref_mut().iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::MIN_INDEXED;
    use crate::{Block, Drawing, Layer};

    /// Names to look up: each given name as it is, upper-cased, lower-cased and with a character
    /// added (a miss).
    fn probes<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
        names.flat_map(|n| [n.to_string(), n.to_ascii_uppercase(), n.to_ascii_lowercase(), format!("{n}?")]).collect()
    }

    fn layers_indexed(d: &Drawing) -> bool {
        d.layers.index.index.get().is_some()
    }

    fn blocks_indexed(d: &Drawing) -> bool {
        d.blocks.index.index.get().is_some()
    }

    /// `layer_mut` finds the first layer of a name and drops the index, as the table may change
    /// through it. Then every layer lookup finds what a scan in table order finds (the first of a
    /// name without case), and a large table looked up this often is left indexed.
    fn check_layers(d: &mut Drawing, extra: &[&str]) {
        let p = extra.first().copied().unwrap_or("L1");
        let want = d.layers.iter().position(|l| l.name.eq_ignore_ascii_case(p)).and_then(|i| d.layers.get(i)).map(std::ptr::from_ref);
        assert_eq!(d.layer_mut(p).map(|l| std::ptr::from_ref(&*l)), want, "{p}");
        assert!(want.is_none() || !layers_indexed(d));
        let names: Vec<String> = d.layers.iter().map(|l| l.name.clone()).collect();
        for p in probes(names.iter().map(String::as_str).chain(extra.iter().copied())) {
            let want = d.layers.iter().position(|l| l.name.eq_ignore_ascii_case(&p));
            assert_eq!(d.layers.position_by_name(&p), want, "{p}");
            let at = want.and_then(|i| d.layers.get(i)).map(std::ptr::from_ref);
            assert_eq!(d.layer(&p).map(std::ptr::from_ref), at, "{p}");
        }
        assert_eq!(layers_indexed(d), d.layers.len() >= MIN_INDEXED);
    }

    /// Every block lookup finds what `Drawing::block` always found: the block of that exact name,
    /// else the first in name order of that name without case.
    fn check_blocks(d: &Drawing, extra: &[&str]) {
        let names: Vec<String> = d.blocks.keys().cloned().collect();
        for p in probes(names.iter().map(String::as_str).chain(extra.iter().copied())) {
            let want = d.blocks.get(&p).or_else(|| d.blocks.iter().find(|(k, _)| k.eq_ignore_ascii_case(&p)).map(|(_, v)| v));
            assert_eq!(d.block(&p).map(std::ptr::from_ref), want.map(std::ptr::from_ref), "{p}");
        }
        assert_eq!(blocks_indexed(d), d.blocks.len() >= MIN_INDEXED);
    }

    #[test]
    fn layer_lookups_find_the_first_of_a_name_through_every_change() {
        let mut d = Drawing::new_imperial();
        d.layers = (0..200).map(|i| Layer::new(&format!("L{i}"))).chain([Layer::new("l5"), Layer::new("Doors")]).collect();
        check_layers(&mut d, &["l5", "missing"]);

        // Renamed through layer_mut (RENAME).
        d.layer_mut("l7").unwrap().name = "Seven".into();
        assert!(!layers_indexed(&d));
        assert!(d.layer("L7").is_none());
        check_layers(&mut d, &["SEVEN", "L7"]);
        // Added (LAYER New, or a file's later record of a name it already has).
        d.layers.push(Layer::new("SEVEN"));
        assert!(!layers_indexed(&d));
        check_layers(&mut d, &["seven"]);
        // Removed (PURGE, LAYDEL): the later `l5` becomes the first of that name.
        d.layers.retain(|l| l.name != "L5");
        assert!(!layers_indexed(&d));
        assert_eq!(d.layer("L5").map(|l| l.name.as_str()), Some("l5"));
        check_layers(&mut d, &["L5"]);
        // Inserted before an earlier layer of the same name.
        d.layers.insert(0, Layer::new("l9"));
        assert!(!layers_indexed(&d));
        assert_eq!(d.layer("L9").map(|l| l.name.as_str()), Some("l9"));
        check_layers(&mut d, &["L9"]);
        // Changed by position, and by a loop over all of them.
        d.layers[3].name = "Third".into();
        assert!(!layers_indexed(&d));
        check_layers(&mut d, &["third"]);
        for l in &mut d.layers {
            l.name.make_ascii_uppercase();
        }
        assert!(!layers_indexed(&d));
        check_layers(&mut d, &["doors"]);

        // Undo: a snapshot keeps the table it was taken with, and its index.
        let names = |d: &Drawing| d.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        assert!(layers_indexed(&d));
        let (snap, before) = (d.clone(), names(&d));
        d.layers.clear();
        d.layers.push(Layer::new("Only"));
        check_layers(&mut d, &["only", "L1"]);
        d = snap;
        assert!(layers_indexed(&d));
        assert_eq!(names(&d), before);
        check_layers(&mut d, &["THIRD"]);
        // The whole table replaced (LAYERP, LAYUNISO).
        d.layers = vec![Layer::new("0"), Layer::new("A"), Layer::new("a")].into();
        check_layers(&mut d, &["a"]);
    }

    #[test]
    fn block_lookups_find_an_exact_name_else_the_first_without_case_through_every_change() {
        let block = |name: &str| Arc::new(Block::new(name));
        let mut d = Drawing::new_imperial();
        for name in (0..100).map(|i| format!("B{i}")).chain(["Door", "DOOR", "door"].map(String::from)) {
            d.blocks.insert(name.clone(), block(&name));
        }
        check_blocks(&d, &["dOOR", "missing"]);
        // An exact name wins; otherwise the first in name order ("DOOR" < "Door" < "door").
        assert_eq!(d.block("door").map(|b| b.name.as_str()), Some("door"));
        assert_eq!(d.block("dOOR").map(|b| b.name.as_str()), Some("DOOR"));

        // Removed (PURGE, a definition replaced under another name).
        d.blocks.remove("DOOR");
        assert!(!blocks_indexed(&d));
        assert_eq!(d.block("dOOR").map(|b| b.name.as_str()), Some("Door"));
        check_blocks(&d, &["dOOR"]);
        // Added before the first of that name.
        d.blocks.insert("DoOr".into(), block("DoOr"));
        assert!(!blocks_indexed(&d));
        assert_eq!(d.block("dOOR").map(|b| b.name.as_str()), Some("DoOr"));
        check_blocks(&d, &["dOOR"]);
        // Renamed (RENAME takes the definition out and puts it back under the new name).
        let b1 = d.blocks.remove("B1").unwrap();
        d.blocks.insert("Renamed".into(), b1);
        assert!(d.block("b1").is_none() && d.block("RENAMED").is_some());
        check_blocks(&d, &["b1", "RENAMED"]);
        // Redefined in place, and changed through get_mut: the names stay.
        d.blocks.insert("Door".into(), block("Door"));
        assert!(!blocks_indexed(&d));
        check_blocks(&d, &["DOOR"]);
        Arc::make_mut(d.blocks.get_mut("B2").unwrap()).description = "changed".into();
        assert!(!blocks_indexed(&d));
        check_blocks(&d, &["b2"]);
        // Many removed at once.
        d.blocks.retain(|k, _| !k.starts_with("B9"));
        assert!(!blocks_indexed(&d));
        check_blocks(&d, &["b9", "b90"]);
        // A loop over all of them.
        for (_, b) in &mut d.blocks {
            Arc::make_mut(b).description = "looped".into();
        }
        assert!(!blocks_indexed(&d));
        check_blocks(&d, &["b3"]);

        // Undo: a snapshot keeps the table it was taken with, and its index.
        let snap = d.clone();
        d.blocks.clear();
        d.blocks.insert("Only".into(), block("Only"));
        check_blocks(&d, &["only", "door"]);
        d = snap;
        assert!(blocks_indexed(&d));
        check_blocks(&d, &["DOOR", "only"]);
    }
}
