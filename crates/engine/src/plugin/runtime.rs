//! The session's plug-ins.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use craft_plugin::fetch::Fetch;
use craft_plugin::{Manifest, Source};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::Pending;

/// An installed plug-in.
#[derive(Clone, Debug)]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    /// The SHA-256 of its bundle (lower-case hex).
    pub sha256: String,
    /// Where it came from.
    pub source: Source,
    /// Kept in the user's `Plug-ins` folder; otherwise for this session only (the CLI, MCP).
    pub stored: bool,
    pub(crate) wasm: Arc<Vec<u8>>,
}

impl InstalledPlugin {
    /// What `plugin.list` says about it.
    pub fn describe(&self) -> Value {
        let mut v = serde_json::to_value(&self.manifest).unwrap_or_else(|_| json!({"id": self.manifest.id}));
        v["kind"] = json!(self.manifest.kind);
        v["sha256"] = json!(self.sha256);
        v["source"] = serde_json::to_value(&self.source).unwrap_or(Value::Null);
        v["stored"] = json!(self.stored);
        v
    }
}

/// The plug-ins' part of the app's preferences: each plug-in's own settings and the catalogs added.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginPrefs {
    /// Each plug-in's settings, by plug-in id (`plugin::MAX_SETTINGS_BYTES` each).
    pub settings: BTreeMap<String, Map<String, Value>>,
    /// Plug-in catalogs added with `plugin.catalog.add` (`index.json` URLs).
    pub catalogs: Vec<String>,
}

/// The session's plug-ins: the installed ones, where they are kept, their settings and catalogs,
/// and the HTTPS client installs download with. The app opens the store
/// ([`Runtime::open_store`]) and sets the client ([`Runtime::set_fetch`]); the CLI and MCP keep
/// plug-ins for the session only.
#[derive(Default)]
pub struct Runtime {
    /// The user's `Plug-ins` folder.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) store: Option<craft_plugin::store::Store>,
    plugins: Vec<InstalledPlugin>,
    /// Plug-in folders that couldn't be read, and plug-ins this version can't run.
    problems: Vec<String>,
    /// HTTPS for catalogs and GitHub releases (without one, only files and `file://` catalogs
    /// install).
    pub(crate) fetch: Mutex<Option<Box<dyn Fetch + Send>>>,
    pub(crate) pending: Option<Pending>,
    /// The plug-ins' settings and catalogs, kept with the app's preferences.
    pub prefs: PluginPrefs,
    /// The last run (plug-in id, command id, params).
    pub last: Option<(String, String, Value)>,
}

impl Runtime {
    /// Keep plug-ins in `<settings_dir>/Plug-ins` and read the ones installed there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_store(&mut self, settings_dir: &std::path::Path) {
        let store = craft_plugin::store::Store::new(craft_plugin::store::plugins_dir(settings_dir));
        let listing = store.list_for(&super::APP);
        self.plugins.retain(|p| !p.stored);
        self.problems = listing.problems.into_iter().map(|p| format!("{}: {}", p.dir.display(), p.error)).collect();
        // The store keeps whatever was installed; only plug-ins for this CADCraft are offered
        self.problems.extend(listing.not_runnable.into_iter().map(|(i, why)| format!("{}: {why}", i.manifest.name)));
        for (i, _) in listing.runnable {
            let plugin = InstalledPlugin {
                manifest: i.manifest,
                sha256: i.record.sha256,
                source: i.record.source,
                stored: true,
                wasm: Arc::new(i.bundle.wasm),
            };
            self.keep(plugin);
        }
        self.store = Some(store);
    }

    /// Download catalogs and releases with `client`.
    pub fn set_fetch(&mut self, client: Option<Box<dyn Fetch + Send>>) {
        *self.fetch.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner) = client;
    }

    /// The installed plug-ins.
    pub fn list(&self) -> &[InstalledPlugin] {
        &self.plugins
    }

    /// Why plug-in folders couldn't be used.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// Plug-in `id` (ignoring case, as the store does).
    pub fn get(&self, id: &str) -> Option<&InstalledPlugin> {
        self.plugins.iter().find(|p| p.manifest.id.eq_ignore_ascii_case(id))
    }

    /// Whether plug-ins are kept in the user's folder (else for this session only).
    pub fn has_store(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.store.is_some();
        #[cfg(target_arch = "wasm32")]
        false
    }

    /// Put `plugin` in the list, in place of one with the same id.
    pub(crate) fn keep(&mut self, plugin: InstalledPlugin) -> InstalledPlugin {
        match self.plugins.iter_mut().find(|p| p.manifest.id.eq_ignore_ascii_case(&plugin.manifest.id)) {
            Some(slot) => *slot = plugin.clone(),
            None => self.plugins.push(plugin.clone()),
        }
        plugin
    }

    /// Drop plug-in `id` from the list, with its settings.
    pub(crate) fn forget(&mut self, id: &str) {
        self.plugins.retain(|p| !p.manifest.id.eq_ignore_ascii_case(id));
        self.prefs.settings.retain(|k, _| !k.eq_ignore_ascii_case(id));
        if self.last.as_ref().is_some_and(|(last, ..)| last.eq_ignore_ascii_case(id)) {
            self.last = None;
        }
    }
}
