//! Extension slot: data another crate keeps on a [`crate::Drawing`] (an add-on's own data, an
//! analysis result…) without the document crate knowing its type. Values are shared (`Arc`), so cloning a
//! drawing for an undo snapshot stays cheap; a change replaces the value (copy-on-write).
//!
//! Persistence: a value that returns JSON from [`ExtData::to_json`] is written to the file by the
//! io crate under its key and read back through the loader registered with [`register_loader`].

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

/// A value stored in [`Extensions`]. Unwind-safe, so a [`crate::Drawing`] stays unwind-safe.
pub trait ExtData: Any + Send + Sync + std::panic::RefUnwindSafe + std::panic::UnwindSafe + std::fmt::Debug {
    fn as_any(&self) -> &dyn Any;
    /// Equality across the type-erased boundary (used by `Drawing: PartialEq`).
    fn dyn_eq(&self, other: &dyn ExtData) -> bool;
    /// The value for files; `None` keeps it in memory only.
    fn to_json(&self) -> Option<serde_json::Value> {
        None
    }
}

/// Rebuilds an extension value from its file JSON.
pub type Loader = fn(serde_json::Value) -> Option<Arc<dyn ExtData>>;

fn loaders() -> &'static Mutex<BTreeMap<&'static str, Loader>> {
    static L: OnceLock<Mutex<BTreeMap<&'static str, Loader>>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Registers how a key's JSON becomes a value again (once per key, at startup).
pub fn register_loader(key: &'static str, loader: Loader) {
    if let Ok(mut l) = loaders().lock() {
        l.insert(key, loader);
    }
}

/// The loader registered for a key, if any.
pub fn loader_for(key: &str) -> Option<Loader> {
    loaders().lock().ok().and_then(|l| l.get(key).copied())
}

/// Every key a loader is registered for, in order.
pub fn registered_keys() -> Vec<&'static str> {
    loaders().lock().map(|l| l.keys().copied().collect()).unwrap_or_default()
}

/// Type-erased, cheaply cloneable extension values keyed by name.
#[derive(Clone, Debug, Default)]
pub struct Extensions {
    values: BTreeMap<&'static str, Arc<dyn ExtData>>,
}

impl PartialEq for Extensions {
    fn eq(&self, other: &Self) -> bool {
        self.values.len() == other.values.len()
            && self.values.iter().all(|(k, v)| other.values.get(k).is_some_and(|o| Arc::ptr_eq(v, o) || v.dyn_eq(o.as_ref())))
    }
}

impl Extensions {
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.values.keys().copied()
    }
    pub fn get<T: ExtData>(&self, key: &str) -> Option<&T> {
        self.values.get(key).and_then(|v| v.as_any().downcast_ref::<T>())
    }
    pub fn get_arc(&self, key: &str) -> Option<&Arc<dyn ExtData>> {
        self.values.get(key)
    }
    pub fn set<T: ExtData>(&mut self, key: &'static str, value: T) {
        self.values.insert(key, Arc::new(value));
    }
    pub fn set_arc(&mut self, key: &'static str, value: Arc<dyn ExtData>) {
        self.values.insert(key, value);
    }
    pub fn remove(&mut self, key: &str) -> Option<Arc<dyn ExtData>> {
        self.values.remove(key)
    }
    /// Copy-on-write edit: clones the value, applies `f`, stores the result.
    pub fn with<T: ExtData + Clone, R>(&mut self, key: &'static str, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut v: T = self.get::<T>(key)?.clone();
        let r = f(&mut v);
        self.set(key, v);
        Some(r)
    }
    /// Same pointer identity for every key (nothing changed since `other`).
    pub fn same_as(&self, other: &Extensions) -> bool {
        self.values.len() == other.values.len() && self.values.iter().all(|(k, v)| other.values.get(k).is_some_and(|o| Arc::ptr_eq(v, o)))
    }
    /// JSON of every persisted value, by key.
    pub fn to_json(&self) -> Vec<(&'static str, serde_json::Value)> {
        self.values.iter().filter_map(|(k, v)| v.to_json().map(|j| (*k, j))).collect()
    }
}
