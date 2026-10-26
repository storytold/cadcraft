//! Host modes: a page that runs CADCraft opens drawings in it and stores what it saves instead
//! of the browser downloading.
//!
//! ## `?host=parent` — an `<iframe>` (such as a Nextcloud app)
//!
//! The framing page and CADCraft talk over `postMessage` between same-origin windows. Messages
//! are plain objects with a `type`:
//!
//! - `cadcraft:ready` (to the parent): `{ version }`, the app is listening; send a drawing now.
//! - `cadcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a file picked with File › Open or dropped on the window.
//! - `cadcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file CADCraft would otherwise download (SAVE, SAVEAS, exports): it replaces the engine's
//!   delivery hook. The parent stores the file and reports failures itself.
//! - `cadcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `cadcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//!
//! Only the parent window on the page's own origin is heard or answered.
//!
//! ## `data-host="page"` — the page's own document
//!
//! A page that loads the module into its own document (its own `<canvas id="cadcraft_canvas">`
//! with `data-host="page"`, no iframe) gets the same behaviour over DOM `CustomEvent`s on the
//! canvas instead of `postMessage`: the same events outward (`ready`, `save`, `dirty`, `failed`,
//! with the message fields as the event's `detail`), and the page dispatches `cadcraft:open` on
//! the canvas with `detail: { name, bytes }` to open a drawing. Events don't bubble, so two
//! editors on one page can't hear each other, and the canvas is the only target CADCraft
//! listens on.
//!
//! In both host modes the app starts on the Start tab rather than an empty Drawing1, and the
//! host guards leaving the page.

use std::cell::{Cell, RefCell};

use cadcraft_ui_egui::CadApp;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use web_sys::js_sys;

/// Longest file name taken from the host page.
const MAX_NAME_CHARS: usize = 255;

thread_local! {
    /// The bridge [`deliver`] posts through (the engine's hook is a plain `fn`).
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
}

/// How a host page is reached. The modes behave the same; they differ only in how an event goes
/// out ([`Transport::send`]) and how an open request comes in ([`Bridge::listen`]).
#[derive(Clone)]
enum Transport {
    /// `?host=parent`: `postMessage` between same-origin windows.
    Parent { window: web_sys::Window, parent: web_sys::Window, origin: String },
    /// `data-host="page"`: DOM `CustomEvent`s on the app's own canvas.
    Page { canvas: web_sys::HtmlCanvasElement },
}

impl Transport {
    /// Hand an event to the host. In parent mode this is a `postMessage` with a `type` field; in
    /// page mode a `CustomEvent` of that name on the canvas, whose `detail` holds the fields and
    /// which never bubbles (two editors on one page must not hear each other). `transfer` hands
    /// the save buffer to the parent instead of copying it (the parent mode only; the page mode
    /// cannot transfer, so its `cadcraft:save` detail carries a copy).
    fn send(&self, name: &str, fields: &[(&str, JsValue)], transfer: Option<&js_sys::ArrayBuffer>) -> Result<(), String> {
        match self {
            Transport::Parent { parent, origin, .. } => {
                let mut message_fields = Vec::with_capacity(fields.len() + 1);
                message_fields.push(("type", JsValue::from_str(name)));
                message_fields.extend_from_slice(fields);
                let message = message(&message_fields).ok_or_else(|| "couldn't build the message".to_string())?;
                let sent = match transfer {
                    Some(buffer) => parent.post_message_with_transfer(&message, origin, &js_sys::Array::of1(buffer)),
                    None => parent.post_message(&message, origin),
                };
                sent.map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
            }
            Transport::Page { canvas } => {
                let detail = message(fields).ok_or_else(|| "couldn't build the event detail".to_string())?;
                let init = web_sys::CustomEventInit::new();
                // The builder methods (`bubbles(..)`, `detail(..)`) are deprecated in web-sys
                // 0.3.106 and fail clippy -D warnings; use the setters (they take `&self`).
                init.set_bubbles(false);
                init.set_detail(&detail);
                let event =
                    web_sys::CustomEvent::new_with_event_init_dict(name, &init).map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
                // The bool says whether a listener called `preventDefault`; not a send failure.
                canvas.dispatch_event(&event).map(|_| ()).map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
                Ok(())
            }
        }
    }
}

#[derive(Clone)]
pub struct Bridge {
    transport: Transport,
}

/// The unsaved-changes state last reported to the host.
#[derive(Default)]
pub struct DirtyReport(Cell<Option<bool>>);

impl Bridge {
    /// The parent bridge for `?host=parent`; `None` when the page isn't framed (there is no
    /// parent to talk to), so opening that URL directly stays the normal app with downloads.
    pub fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let origin = window.location().origin().ok()?;
        Some(Self { transport: Transport::Parent { window, parent, origin } })
    }

    /// The page bridge for `data-host="page"`: the host page runs CADCraft in its own document
    /// and hears the app's DOM `CustomEvent`s on this canvas.
    pub fn for_page(canvas: &web_sys::HtmlCanvasElement) -> Self {
        Self { transport: Transport::Page { canvas: canvas.clone() } }
    }

    /// Make this the bridge [`deliver`] uses.
    pub fn install(&self) {
        BRIDGE.with(|b| *b.borrow_mut() = Some(self.clone()));
    }

    pub fn post_ready(&self) {
        let _ = self.transport.send("cadcraft:ready", &[("version", env!("CARGO_PKG_VERSION").into())], None);
    }

    pub fn post_failed(&self, error: &str) {
        let _ = self.transport.send("cadcraft:failed", &[("error", error.into())], None);
    }

    /// Hear drawings from the host; they arrive through the upload inbox.
    pub fn listen(&self, inbox: crate::upload::Inbox) {
        match &self.transport {
            Transport::Parent { window, parent, origin } => {
                let parent = parent.clone();
                let origin = origin.clone();
                let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                    if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                        return;
                    }
                    let data = event.data();
                    if string(&data, "type").as_deref() != Some("cadcraft:open") {
                        return;
                    }
                    receive_open(&data, &inbox);
                });
                if window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
                    // The listener lives as long as the page.
                    on_message.forget();
                }
            }
            Transport::Page { canvas } => {
                let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                    let Some(open) = event.dyn_ref::<web_sys::CustomEvent>() else { return };
                    let detail = open.detail();
                    receive_open(&detail, &inbox);
                });
                if canvas.add_event_listener_with_callback("cadcraft:open", on_open.as_ref().unchecked_ref()).is_ok() {
                    // The listener lives as long as the page.
                    on_open.forget();
                }
            }
        }
    }

    /// After each logic callback (including a hidden tab's): tell the host when unsaved changes
    /// appear or go away. `DirtyReport` suppresses duplicates, so this is idempotent.
    pub fn report_dirty(&self, app: &CadApp, reported: &DirtyReport) {
        let dirty = app.session.docs.iter().any(|d| d.is_dirty());
        if reported.0.get() != Some(dirty) && self.transport.send("cadcraft:dirty", &[("dirty", dirty.into())], None).is_ok() {
            reported.0.set(Some(dirty));
        }
    }

    fn post_save(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        // `cadcraft:save` carries the bytes as an `ArrayBuffer`; the parent bridge transfers it.
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        self.transport.send("cadcraft:save", &[("name", name.into()), ("bytes", buffer.clone().into())], Some(&buffer))
    }
}

/// The engine's delivery hook in host mode ([`cadcraft_engine::cmd::file::set_deliver`]): hand
/// the file to the host page. `Ok` once it was sent; the engine then counts a drawing as saved.
pub fn deliver(name: &str, _mime: &str, bytes: &[u8]) -> Result<(), String> {
    let bridge = BRIDGE.with(|b| b.borrow().clone()).ok_or("the page that embeds CADCraft is gone")?;
    bridge.post_save(name, bytes)
}

/// The `cadcraft:open` payload, `{ name, bytes }` where `bytes` is an `ArrayBuffer` or
/// `Uint8Array` holding a drawing: it opens like a file picked with File › Open or dropped on
/// the window. Shared by both host modes (the parent's message object and the page event's
/// `detail` carry the same fields). A blank or over-long name falls back to `Drawing.dxf`
/// (trimmed to [`MAX_NAME_CHARS`]); an oversized drawing is refused before it is copied.
fn receive_open(data: &JsValue, inbox: &crate::upload::Inbox) {
    let name = string(data, "name")
        .filter(|n| !n.trim().is_empty())
        .map(|n| n.chars().take(MAX_NAME_CHARS).collect())
        .unwrap_or_else(|| "Drawing.dxf".to_string());
    // Refuse an oversized drawing before copying it into the app's memory.
    let Some(array) = byte_array(data) else { return };
    if let Some(e) = CadApp::open_size_error(&name, u64::from(array.length())) {
        inbox.refuse(e);
        return;
    }
    inbox.receive(name, array.to_vec());
}

fn message(fields: &[(&str, JsValue)]) -> Option<js_sys::Object> {
    let m = js_sys::Object::new();
    for (key, value) in fields {
        js_sys::Reflect::set(&m, &JsValue::from_str(key), value).ok()?;
    }
    Some(m)
}

fn string(data: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_string()
}

/// `bytes` (an `ArrayBuffer` or `Uint8Array`) as a view, not yet copied.
fn byte_array(data: &JsValue) -> Option<js_sys::Uint8Array> {
    let bytes = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
    if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
        Some(array.clone())
    } else if bytes.is_instance_of::<js_sys::ArrayBuffer>() {
        Some(js_sys::Uint8Array::new(&bytes))
    } else {
        None
    }
}
