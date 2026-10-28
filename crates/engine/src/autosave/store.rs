//! Autosave into a key-value store instead of files: the web build keeps its autosaves in the
//! browser's `localStorage`, which the host supplies through [`Store`] ([`install`]). This crate
//! never touches the browser itself, so all of it runs (and is tested) natively too.
//!
//! Each autosave is one record under `cadcraft.autosave/<file name>` (the same name an autosave
//! file would have, [`super::file_name`]), holding a one-line header and the drawing as DXF (see
//! [`encode`]). A running app keeps `cadcraft.autosave.lock/<instance>` set to the time it last
//! checked in, so other tabs of the same site don't offer its records for recovery while it runs;
//! records whose app stopped checking in [`super::STALE_LOCK_SECS`] ago (or whose tab was reloaded
//! or closed, see [`forget_instance`]) are offered by Drawing Recovery.
//!
//! Browser storage is small (about 5 MB per site, shared with the app's preferences) and may be
//! missing (private browsing, storage turned off) or full: autosaves together are kept under
//! [`MAX_TOTAL_BYTES`], and every storage error is reported to the caller, never a panic.

use std::cell::RefCell;

use super::{Recoverable, STALE_LOCK_SECS, is_instance_id, parse_file_name};

/// The key prefix of autosave records.
pub const KEY_PREFIX: &str = "cadcraft.autosave/";
/// The key prefix of the lock records running apps keep fresh.
pub const LOCK_PREFIX: &str = "cadcraft.autosave.lock/";
/// The most all autosave records together may take, in bytes of text (browsers keep about 5 MB
/// per site; the rest stays free for the app's preferences).
pub const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
/// At most this many keys are looked at (the store is shared with other code on the site).
const MAX_KEYS: usize = 10_000;
/// The first word of every record: the format and its version.
const MAGIC: &str = "CCAS1";

/// What can go wrong with the store.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// No storage (private browsing, storage turned off, a sandboxed frame).
    #[error("browser storage is not available ({0})")]
    Unavailable(String),
    /// The storage is full.
    #[error("browser storage is full ({0})")]
    Quota(String),
    #[error("{0}")]
    Other(String),
}

/// A string key-value store such as the browser's `localStorage`.
pub trait Store {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError>;
    fn set(&mut self, key: &str, value: &str) -> Result<(), StoreError>;
    fn remove(&mut self, key: &str) -> Result<(), StoreError>;
    fn keys(&self) -> Result<Vec<String>, StoreError>;
    /// The wall clock, in seconds since 1970.
    fn now(&self) -> f64;
}

thread_local! {
    /// The store the app autosaves to (`None`: autosave goes to files, or nowhere).
    static STORE: RefCell<Option<Box<dyn Store>>> = const { RefCell::new(None) };
}

/// Make `store` the place autosaves go to and Drawing Recovery reads from (the web build installs
/// the browser's `localStorage` at start). Per thread: the app runs on one.
pub fn install(store: Box<dyn Store>) {
    STORE.with(|s| *s.borrow_mut() = Some(store));
}

/// Remove the installed store (tests; a host that loses its storage).
pub fn uninstall() -> Option<Box<dyn Store>> {
    STORE.with(|s| s.borrow_mut().take())
}

/// Whether a store is installed.
pub fn installed() -> bool {
    STORE.with(|s| s.try_borrow().is_ok_and(|s| s.is_some()))
}

/// Run `f` on the installed store; `None` when there is none (or it is in use further up the
/// stack, which never happens in practice and is not worth a panic).
pub fn with<R>(f: impl FnOnce(&mut dyn Store) -> R) -> Option<R> {
    STORE.with(|s| {
        let mut s = s.try_borrow_mut().ok()?;
        let store = s.as_mut()?;
        Some(f(store.as_mut()))
    })
}

/// The record key of autosave file name `name`.
pub fn key(name: &str) -> String {
    format!("{KEY_PREFIX}{name}")
}

/// The lock key of app instance `instance`.
pub fn lock_key(instance: &str) -> String {
    format!("{LOCK_PREFIX}{instance}")
}

/// A record key's parts (the drawing's title, the instance and the drawing uid), if it is one of
/// ours.
pub fn parse_key(key: &str) -> Option<(&str, &str, u64)> {
    parse_file_name(key.strip_prefix(KEY_PREFIX)?)
}

/// An instance id for a browser tab: `<random number>-<start time in ms, hex>` (the form
/// [`is_instance_id`] accepts; there is no process id in a browser).
pub fn instance_id(random: u32, millis: u64) -> String {
    format!("{random}-{millis:x}")
}

/// A record: header `CCAS1 <modified> <encoding> <length>` and the data on the next line. The
/// encoding is `t` (the DXF text as is) or `b` (base64, for bytes that aren't UTF-8).
pub fn encode(modified: u64, bytes: &[u8]) -> String {
    let (enc, data) = match std::str::from_utf8(bytes) {
        Ok(text) => ("t", std::borrow::Cow::Borrowed(text)),
        Err(_) => ("b", std::borrow::Cow::Owned(crate::cmd::file::base64_encode(bytes))),
    };
    format!("{MAGIC} {modified} {enc} {}\n{data}", data.len())
}

/// The header of a record: when it was written and how many bytes its data has. Errors name what
/// is damaged.
pub fn header(record: &str) -> Result<(u64, usize), String> {
    let (head, data) = record.split_once('\n').ok_or("no header")?;
    let mut words = head.split(' ');
    if words.next() != Some(MAGIC) {
        return Err("not a CADCraft autosave".into());
    }
    let modified = words.next().and_then(|w| w.parse::<u64>().ok()).ok_or("bad time")?;
    words.next().filter(|e| *e == "t" || *e == "b").ok_or("bad encoding")?;
    let len = words.next().and_then(|w| w.parse::<usize>().ok()).ok_or("bad length")?;
    if words.next().is_some() {
        return Err("bad header".into());
    }
    if data.len() != len {
        return Err(format!("{} bytes where the header says {len}", data.len()));
    }
    Ok((modified, len))
}

/// The drawing bytes a record holds.
pub fn decode(record: &str) -> Result<Vec<u8>, String> {
    header(record)?;
    let (head, data) = record.split_once('\n').ok_or("no header")?;
    if head.split(' ').nth(2) == Some("b") {
        crate::cmd::file::base64_decode(data).ok_or_else(|| "bad base64 data".to_string())
    } else {
        Ok(data.as_bytes().to_vec())
    }
}

/// Our record and lock keys in the store (at most [`MAX_KEYS`] keys are looked at).
fn our_keys(store: &dyn Store) -> Vec<String> {
    match store.keys() {
        Ok(keys) => keys.into_iter().take(MAX_KEYS).filter(|k| k.starts_with(KEY_PREFIX) || k.starts_with(LOCK_PREFIX)).collect(),
        Err(e) => {
            log::warn!("autosave: {e}");
            Vec::new()
        }
    }
}

/// Whether app instance `instance` checked in less than [`STALE_LOCK_SECS`] before `now` (a
/// time in the future, after a clock change, counts as fresh; a damaged lock as stale).
pub fn instance_alive(store: &dyn Store, instance: &str, now: f64) -> bool {
    let Ok(Some(v)) = store.get(&lock_key(instance)) else { return false };
    v.trim().parse::<f64>().ok().filter(|t| t.is_finite()).is_some_and(|t| now - t < STALE_LOCK_SECS as f64)
}

/// The records left behind by apps that are no longer running (never those of `own`), oldest
/// first. A damaged record is listed too (time 0), so it can be discarded.
pub fn recoverable(store: &dyn Store, own: &str) -> Vec<Recoverable> {
    let now = store.now();
    let mut alive = std::collections::HashMap::new();
    let mut out = Vec::new();
    for k in our_keys(store) {
        let Some((title, instance, _)) = parse_key(&k) else { continue };
        if instance == own || *alive.entry(instance.to_string()).or_insert_with(|| instance_alive(store, instance, now)) {
            continue;
        }
        let Ok(Some(record)) = store.get(&k) else { continue };
        let (modified, bytes) = header(&record).unwrap_or((0, record.len()));
        out.push(Recoverable { path: k.clone(), drawing: title.to_string(), modified, bytes: bytes as u64 });
    }
    out.sort_by(|a, b| a.modified.cmp(&b.modified).then_with(|| a.path.cmp(&b.path)));
    out
}

/// The drawing bytes of record `key`.
pub fn read(store: &dyn Store, key: &str) -> Result<Vec<u8>, String> {
    if parse_key(key).is_none() {
        return Err(format!("{key} is not an autosave"));
    }
    let record = store.get(key).map_err(|e| e.to_string())?.ok_or_else(|| format!("{key} is no longer there"))?;
    decode(&record).map_err(|e| format!("the autosave is damaged: {e}"))
}

/// Store `bytes` (drawing `key`'s DXF) written at `now`, as long as all autosaves together stay
/// within [`MAX_TOTAL_BYTES`].
pub fn write(store: &mut dyn Store, key: &str, bytes: &[u8], now: f64) -> Result<(), StoreError> {
    let record = encode(now.max(0.0) as u64, bytes);
    let others: usize = our_keys(store)
        .iter()
        .filter(|k| k.as_str() != key && k.starts_with(KEY_PREFIX))
        .filter_map(|k| store.get(k).ok().flatten())
        .map(|r| r.len())
        .sum();
    let total = others.saturating_add(record.len());
    if total > MAX_TOTAL_BYTES {
        return Err(StoreError::Quota(format!("autosaves would take {} KB, at most {} KB are kept", total.div_ceil(1024), MAX_TOTAL_BYTES / 1024)));
    }
    store.set(key, &record)
}

/// Delete record `key` (discarded, or its drawing was saved or closed), and the locks of stopped
/// apps left without records. Only our record keys are deleted.
pub fn discard(store: &mut dyn Store, key: &str) {
    if parse_key(key).is_none() {
        log::warn!("autosave: not deleting {key}: not an autosave");
        return;
    }
    if let Err(e) = store.remove(key) {
        log::warn!("autosave: can't delete {key}: {e}");
    }
    prune_locks(store);
}

/// Delete the locks of apps that are no longer running and left no records.
pub fn prune_locks(store: &mut dyn Store) {
    let now = store.now();
    let keys = our_keys(store);
    for k in &keys {
        let Some(instance) = k.strip_prefix(LOCK_PREFIX).filter(|i| is_instance_id(i)) else { continue };
        let has_records = keys.iter().any(|r| parse_key(r).is_some_and(|(_, i, _)| i == instance));
        if !has_records && !instance_alive(store, instance, now) {
            let _ = store.remove(k);
        }
    }
}

/// The app instance `instance` stopped (its tab was reloaded or closed): drop its lock, so its
/// records are offered for recovery at once instead of [`super::STALE_LOCK_SECS`] later.
pub fn forget_instance(store: &mut dyn Store, instance: &str) {
    if is_instance_id(instance) {
        let _ = store.remove(&lock_key(instance));
    }
}

/// A store in memory: for tests, and a stand-in where the host has no storage.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    pub map: std::collections::BTreeMap<String, String>,
    /// The time [`Store::now`] reports.
    pub clock: f64,
    /// Refuse writes that would make all values together longer than this (a browser's quota).
    pub quota: Option<usize>,
    /// Fail every call, as storage turned off does.
    pub unavailable: bool,
}

impl MemoryStore {
    fn check(&self) -> Result<(), StoreError> {
        if self.unavailable { Err(StoreError::Unavailable("storage is turned off".into())) } else { Ok(()) }
    }
}

impl Store for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.check()?;
        Ok(self.map.get(key).cloned())
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
        self.check()?;
        let used: usize = self.map.iter().filter(|(k, _)| k.as_str() != key).map(|(k, v)| k.len() + v.len()).sum();
        if self.quota.is_some_and(|q| used + key.len() + value.len() > q) {
            return Err(StoreError::Quota("the quota is exceeded".into()));
        }
        self.map.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<(), StoreError> {
        self.check()?;
        self.map.remove(key);
        Ok(())
    }

    fn keys(&self) -> Result<Vec<String>, StoreError> {
        self.check()?;
        Ok(self.map.keys().cloned().collect())
    }

    fn now(&self) -> f64 {
        self.clock
    }
}
