//! The drawing's tables of named entries (layers, blocks, linetypes, text styles, dimension styles,
//! table styles and layouts), whose names are looked up without case through an index instead of a
//! scan once a table is large and looked up often.
//!
//! [`Drawing::layer`](crate::Drawing::layer), [`Drawing::block`](crate::Drawing::block) and the
//! other lookups by name run for every entity while a drawing is read, measured and drawn, and
//! every layout is looked up on each change. Scanning the table on each made a file of N layers,
//! or of N blocks, linetypes or styles and N objects naming ones it does not have, cost N² name
//! comparisons: tens of seconds to open or draw at a few tens of thousands (#1924, F-04).
//!
//! Each table keeps an index from a name with its ASCII letters lower-cased to the table's first
//! entry of that name, so a lookup returns what a scan in table order returns. The index is ordered
//! by those bytes, not hashed: a lookup costs a logarithmic number of name comparisons whatever
//! the names are, also where a file could predict a hash's keys (the web build's `RandomState`).
//! Every change to a table goes through `DerefMut`, which drops the index, so it can never be
//! stale. The index is built only once a table has been scanned [`SCANS_BEFORE_INDEX`] times since
//! it last changed: a table changed between every few hundred lookups costs at most about 2.5 times
//! what scanning it always did, and one looked up many times in between is indexed once.

use std::collections::{BTreeMap, btree_map};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

/// Tables with fewer entries than this are always scanned: a search of the index costs about what a
/// scan of 100 names does (measured), so a smaller table gains nothing from one.
const MIN_INDEXED: usize = 128;
/// Lookups that scan a table after it changed before its index is built. A build costs about 100
/// to 350 scans (measured on tables of 100 to 40 000 names), so a table changed after every few
/// hundred lookups costs at most about 2.5 times what scanning it always did.
const SCANS_BEFORE_INDEX: u32 = 256;

/// From a name with its ASCII letters lower-cased to the first entry, in table order, of that name.
struct Index<V>(BTreeMap<Box<[u8]>, V>);

impl<V> Index<V> {
    fn build<'a>(entries: impl Iterator<Item = (&'a str, V)>) -> Self {
        let mut map = BTreeMap::new();
        for (name, v) in entries {
            map.entry(name.to_ascii_lowercase().into_bytes().into_boxed_slice()).or_insert(v);
        }
        Index(map)
    }

    /// The first entry named `name`, without case.
    fn get(&self, name: &str) -> Option<&V> {
        with_folded(name, |folded| self.0.get(folded))
    }
}

/// `f` of the bytes of `name` with its ASCII letters lower-cased, which are equal for the names
/// `eq_ignore_ascii_case` finds equal. A short name is folded on the stack.
fn with_folded<R>(name: &str, f: impl FnOnce(&[u8]) -> R) -> R {
    let mut buf = [0u8; 64];
    match buf.get_mut(..name.len()) {
        Some(short) => {
            short.copy_from_slice(name.as_bytes());
            short.make_ascii_lowercase();
            f(short)
        }
        None => f(name.to_ascii_lowercase().as_bytes()),
    }
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

/// Entries named by their `name` field.
macro_rules! named_by_field {
    ($($t:ty),*) => {$(
        impl Named for $t {
            fn name(&self) -> &str {
                &self.name
            }
        }
    )*};
}
named_by_field!(crate::Layer, crate::Linetype, crate::TextStyle, crate::DimStyle, crate::TableStyle, crate::Layout);

/// A list of named entries in their own order (the drawing's layers, linetypes, text styles,
/// dimension styles, table styles and layouts). It reads and changes as a `Vec`;
/// [`NamedList::by_name`] finds the first entry of a name without case.
pub struct NamedList<T> {
    list: Vec<T>,
    index: Lazy<usize>,
}

impl<T: Named> NamedList<T> {
    /// The position of the first entry named `name`, without case.
    pub fn position_by_name(&self, name: &str) -> Option<usize> {
        match self.index.get(self.list.len(), || Index::build(self.list.iter().enumerate().map(|(i, t)| (t.name(), i)))) {
            Some(ix) => ix.get(name).copied(),
            None => self.list.iter().position(|t| t.name().eq_ignore_ascii_case(name)),
        }
    }

    /// The first entry named `name`, without case.
    pub fn by_name(&self, name: &str) -> Option<&T> {
        self.position_by_name(name).and_then(|i| self.list.get(i))
    }

    /// The first entry named `name`, without case, to change (which drops the index).
    pub fn by_name_mut(&mut self, name: &str) -> Option<&mut T> {
        let i = self.position_by_name(name)?;
        self.get_mut(i)
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
        match self.index.get(self.map.len(), || Index::build(self.map.keys().map(|k| (k.as_str(), Box::from(k.as_str()))))) {
            Some(ix) => ix.get(name).and_then(|k| self.map.get(&**k)),
            None => self.map.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v),
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

    use super::{MIN_INDEXED, Named, NamedList};
    use crate::{Block, DimStyle, Drawing, Layer, Layout, Linetype, TextStyle};

    /// Names to look up: each given name as it is, upper-cased, lower-cased and with a character
    /// added (a miss).
    fn probes<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
        names.flat_map(|n| [n.to_string(), n.to_ascii_uppercase(), n.to_ascii_lowercase(), format!("{n}?")]).collect()
    }

    fn layers_indexed(d: &Drawing) -> bool {
        d.layers.index.index.get().is_some()
    }

    fn indexed<T>(list: &NamedList<T>) -> bool {
        list.index.index.get().is_some()
    }

    /// Every lookup through `find` finds what a scan of `list` in table order finds (the first of a
    /// name without case), by pointer, for the list's names and `extra`.
    fn check_list<'a, T: Named>(list: &'a NamedList<T>, find: impl Fn(&str) -> Option<&'a T>, extra: &[&str]) {
        let names: Vec<String> = list.iter().map(|t| t.name().to_string()).collect();
        for p in probes(names.iter().map(String::as_str).chain(extra.iter().copied())) {
            let want = list.iter().find(|t| t.name().eq_ignore_ascii_case(&p)).map(std::ptr::from_ref);
            assert_eq!(find(&p).map(std::ptr::from_ref), want, "{p}");
        }
    }

    /// The linetype, text style, dimension style and layout lookups each find what a scan in table
    /// order finds (the first of a name without case): while they scan, once the table is indexed,
    /// and after `by_name_mut` (RENAME, STYLE, DIMSTYLE, a .lin file reloaded, a layout's UCS)
    /// changed the first of a name and dropped the index.
    #[test]
    fn linetype_style_and_layout_lookups_find_the_first_of_a_name() {
        let names: Vec<String> = (0..150).map(|i| format!("N{i}")).chain(["n7", "Dash", "DASH"].map(String::from)).collect();
        let mut d = Drawing::new_imperial();
        d.linetypes = names.iter().map(|n| Linetype::simple(n, "", &[0.5, -0.25])).collect();
        d.text_styles = names.iter().map(|n| TextStyle { name: n.clone(), ..TextStyle::default() }).collect();
        d.dim_styles = names.iter().map(|n| DimStyle { name: n.clone(), ..DimStyle::default() }).collect();
        d.layouts = names.iter().zip(1..).map(|(n, tab)| Layout::new(n, tab)).collect();
        let check = |d: &Drawing, extra: &[&str]| {
            check_list(&d.linetypes, |n| d.linetype(n), extra);
            check_list(&d.text_styles, |n| d.text_style(n), extra);
            check_list(&d.dim_styles, |n| d.dim_style(n), extra);
            check_list(&d.layouts, |n| d.layout(n), extra);
        };
        let all_indexed = |d: &Drawing| indexed(&d.linetypes) && indexed(&d.text_styles) && indexed(&d.dim_styles) && indexed(&d.layouts);
        // The first round scans until the index is built; the second reads only the index.
        check(&d, &["dash", "missing"]);
        assert!(all_indexed(&d));
        check(&d, &["dash", "missing"]);

        // The first of a name changes, and the later one of that name is found after it.
        d.linetypes.by_name_mut("n7").unwrap().name = "Seven".into();
        d.text_styles.by_name_mut("n7").unwrap().name = "Seven".into();
        d.dim_styles.by_name_mut("n7").unwrap().name = "Seven".into();
        d.layouts.by_name_mut("n7").unwrap().name = "Seven".into();
        assert!(!indexed(&d.linetypes) && !indexed(&d.text_styles) && !indexed(&d.dim_styles) && !indexed(&d.layouts));
        assert_eq!(d.linetype("N7").map(|l| l.name.as_str()), Some("n7"));
        assert_eq!(d.text_style("N7").map(|s| s.name.as_str()), Some("n7"));
        assert_eq!(d.dim_style("N7").map(|s| s.name.as_str()), Some("n7"));
        assert_eq!(d.layout("N7").map(|l| (l.name.as_str(), l.tab_order)), Some(("n7", 151)));
        assert_eq!(d.linetypes.get(7).map(|l| l.name.as_str()), Some("Seven"));
        check(&d, &["SEVEN", "n7"]);
        assert!(all_indexed(&d));
        // A miss changes nothing and keeps the index.
        assert!(d.dim_styles.by_name_mut("none").is_none());
        assert!(indexed(&d.dim_styles));
        check(&d, &["dash"]);
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
    /// else the first in name order of that name without case. Only names that miss an exact name
    /// count towards building the index, so the names are looked up twice.
    fn check_blocks(d: &Drawing, extra: &[&str]) {
        let names: Vec<String> = d.blocks.keys().cloned().collect();
        for p in probes(names.iter().chain(&names).map(String::as_str).chain(extra.iter().copied())) {
            let want = d.blocks.get(&p).or_else(|| d.blocks.iter().find(|(k, _)| k.eq_ignore_ascii_case(&p)).map(|(_, v)| v));
            assert_eq!(d.block(&p).map(std::ptr::from_ref), want.map(std::ptr::from_ref), "{p}");
        }
        assert_eq!(blocks_indexed(d), d.blocks.len() >= MIN_INDEXED);
    }

    /// The index answers by itself, with no scan to fall back on: every name finds the first entry
    /// of its name without case, also names that differ only after a long shared prefix, names
    /// that fill the stack buffer or are longer, and names with non-ASCII letters (which keep their
    /// case). Names are compared, not hashed, so no two names can share a slot.
    #[test]
    fn the_index_alone_finds_the_first_entry_of_every_name() {
        let long = "x".repeat(100);
        let names: Vec<String> = vec![
            "Door".into(),
            "DOOR".into(),
            "door2".into(),
            format!("{long}A"),
            format!("{long}a"),
            format!("{long}B"),
            format!("{}b", "P".repeat(63)),
            format!("{}B", "p".repeat(63)),
            "Ärea".into(),
            "ärea".into(),
            "ÄREA".into(),
            String::new(),
        ];
        let ix = super::Index::build(names.iter().enumerate().map(|(i, n)| (n.as_str(), i)));
        for p in names.iter().flat_map(|n| [n.clone(), n.to_ascii_uppercase(), n.to_ascii_lowercase(), format!("{n}?")]) {
            let want = names.iter().position(|n| n.eq_ignore_ascii_case(&p));
            assert_eq!(ix.get(&p).copied(), want, "{p:?}");
        }
        assert_eq!(ix.get(&format!("{long}a")), Some(&3));
        assert_eq!(ix.get("ärea"), Some(&9));
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
        for name in (0..150).map(|i| format!("B{i}")).chain(["Door", "DOOR", "door"].map(String::from)) {
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
