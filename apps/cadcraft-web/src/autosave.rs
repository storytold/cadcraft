//! Autosave in the browser: the engine's autosave store ([`cadcraft_engine::autosave::store`]) is
//! the page's `localStorage`, so a tab that is closed, reloaded or crashes with unsaved changes
//! leaves its last autosave there, and the next start offers it in Drawing Recovery.
//!
//! A tab remembers its app instance in `sessionStorage` (which survives a reload or a crash of the
//! tab): at start, the instance it showed before is known to be gone, so its autosaves are offered
//! at once rather than after its lock goes stale; a page being hidden for good (`pagehide`) drops
//! its lock the same way. No storage (private browsing, storage turned off) turns autosave off
//! with a message; a full storage is reported when an autosave fails.

use cadcraft_engine::autosave::store::StoreError;

/// The storage error a browser `DOMException` named `name` stands for.
pub(crate) fn classify(name: Option<&str>, message: String) -> StoreError {
    match name {
        // Firefox before 2020 used its own name for a full storage.
        Some("QuotaExceededError" | "NS_ERROR_DOM_QUOTA_REACHED") => StoreError::Quota(message),
        // Storage turned off, or blocked for this page (third-party frames, sandboxing).
        Some("SecurityError" | "InvalidAccessError") => StoreError::Unavailable(message),
        _ => StoreError::Other(message),
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::start;

#[cfg(target_arch = "wasm32")]
mod web {
    use cadcraft_engine::autosave::store::{self, Store, StoreError};
    use cadcraft_ui_egui::CadApp;
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::JsValue;
    use wasm_bindgen::closure::Closure;
    use web_sys::js_sys;

    /// The `sessionStorage` key holding this tab's app instance.
    const TAB_INSTANCE_KEY: &str = "cadcraft.autosave.instance";
    /// The key written and removed at start to see whether storage works at all.
    const PROBE_KEY: &str = "cadcraft.autosave.probe";
    /// At most this many keys are listed.
    const MAX_KEYS: u32 = 10_000;

    /// The browser's `localStorage` as the engine's autosave store.
    struct LocalStorage(web_sys::Storage);

    impl Store for LocalStorage {
        fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
            self.0.get_item(key).map_err(|e| error(&e))
        }

        fn set(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
            self.0.set_item(key, value).map_err(|e| error(&e))
        }

        fn remove(&mut self, key: &str) -> Result<(), StoreError> {
            self.0.remove_item(key).map_err(|e| error(&e))
        }

        fn keys(&self) -> Result<Vec<String>, StoreError> {
            let n = self.0.length().map_err(|e| error(&e))?;
            Ok((0..n.min(MAX_KEYS)).filter_map(|i| self.0.key(i).ok().flatten()).collect())
        }

        fn now(&self) -> f64 {
            js_sys::Date::now() / 1000.0
        }
    }

    /// A thrown storage error (a `DOMException`) as a [`StoreError`].
    fn error(e: &JsValue) -> StoreError {
        let field = |k: &str| js_sys::Reflect::get(e, &JsValue::from_str(k)).ok().and_then(|v| v.as_string());
        let message = field("message").or_else(|| e.as_string()).unwrap_or_else(|| format!("{e:?}"));
        super::classify(field("name").as_deref(), message)
    }

    /// Turn autosave on in `app` with the page's `localStorage`, and list what tabs that stopped
    /// left there for Drawing Recovery. `Err` (for the status bar) when the browser has no
    /// storage for the page: autosave stays off.
    pub fn start(app: &mut CadApp) -> Result<(), String> {
        let window = web_sys::window().ok_or("no browser window")?;
        let storage = match window.local_storage() {
            Ok(Some(storage)) => storage,
            Ok(None) => return Err("Autosave is off: this browser keeps no storage for the page".into()),
            Err(e) => return Err(format!("Autosave is off: {}", error(&e))),
        };
        let mut storage = LocalStorage(storage);
        // Some browsers hand out a storage that refuses every write (older Safari in private mode).
        match storage.set(PROBE_KEY, "1").and_then(|()| storage.remove(PROBE_KEY)) {
            Err(e @ (StoreError::Unavailable(_) | StoreError::Other(_))) => return Err(format!("Autosave is off: {e}")),
            // A full storage still holds what is there; writes report it when they fail.
            Ok(()) | Err(StoreError::Quota(_)) => {}
        }
        store::install(Box::new(storage));
        // `as` saturates, so the random draw always fits.
        let instance = store::instance_id((js_sys::Math::random() * f64::from(u32::MAX)) as u32, js_sys::Date::now() as u64);
        // The page this tab showed before (reloaded, or crashed and reloaded) is gone.
        if let Ok(Some(tab)) = window.session_storage() {
            if let Ok(Some(previous)) = tab.get_item(TAB_INSTANCE_KEY) {
                store::with(|st| store::forget_instance(st, &previous));
            }
            let _ = tab.set_item(TAB_INSTANCE_KEY, &instance);
        }
        forget_on_pagehide(&window, &instance);
        app.enable_store_autosave(instance);
        Ok(())
    }

    /// When the page goes away (closed, reloaded, navigated from), drop this instance's lock so
    /// the next start offers its autosaves at once. The autosaves themselves stay.
    fn forget_on_pagehide(window: &web_sys::Window, instance: &str) {
        let instance = instance.to_string();
        let on_hide = Closure::<dyn FnMut()>::new(move || {
            store::with(|st| store::forget_instance(st, &instance));
        });
        if window.add_event_listener_with_callback("pagehide", on_hide.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_hide.forget();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::classify;
    use cadcraft_engine::autosave::store::StoreError;

    /// Guards the mapping of browser storage errors: a full storage is reported as such (the
    /// autosave is tried again later), blocked storage turns autosave off, anything else is
    /// passed on as is.
    #[test]
    fn browser_storage_errors_are_classified() {
        assert_eq!(classify(Some("QuotaExceededError"), "full".into()), StoreError::Quota("full".into()));
        assert_eq!(classify(Some("NS_ERROR_DOM_QUOTA_REACHED"), "full".into()), StoreError::Quota("full".into()));
        assert_eq!(classify(Some("SecurityError"), "denied".into()), StoreError::Unavailable("denied".into()));
        assert_eq!(classify(Some("TypeError"), "odd".into()), StoreError::Other("odd".into()));
        assert_eq!(classify(None, "odd".into()), StoreError::Other("odd".into()));
    }
}
