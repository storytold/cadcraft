//! Finder's Open Documents Apple event, independent of winit's application delegate.
//!
//! This module contains the Open Documents Objective-C boundary. Unsafe code is denied
//! elsewhere in the application and forbidden in the document/engine/render crates.
#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

use cadcraft_ui_egui::CadApp;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{
    MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, ns_string,
};

const OPEN_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const FILE_URL: u32 = u32::from_be_bytes(*b"furl");
const MAX_PENDING: usize = 256;

#[derive(Default)]
struct Pending {
    paths: VecDeque<PathBuf>,
    error: Option<String>,
    context: Option<egui::Context>,
}

impl Pending {
    fn next_path(&mut self, busy: bool) -> Option<PathBuf> {
        if busy { None } else { self.paths.pop_front() }
    }
}

define_class!(
    // SAFETY: NSObject has no subclass requirements; the handler is main-thread-only, has
    // no custom Drop, and objc2 owns and destroys its Rust ivars.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "CADCraftOpenDocumentsHandler"]
    #[ivars = RefCell<Pending>]
    struct Handler;

    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for Handler {}

    impl Handler {
        // SAFETY: NSNotificationCenter passes its notification to this one-argument selector.
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &NSNotification) {
            self.register();
        }
        // SAFETY: NSAppleEventManager invokes this selector with two event descriptors.
        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let Some(list) = event.paramDescriptorForKeyword(DIRECT_OBJECT) else { return };
            log::debug!("macOS Open Documents: {} items", list.numberOfItems());
            let mut pending = self.ivars().borrow_mut();
            for i in 1..=list.numberOfItems().min(MAX_PENDING as isize) {
                let path = list.descriptorAtIndex(i)
                    .and_then(|d| d.coerceToDescriptorType(FILE_URL))
                    .and_then(|d| {
                        // A file-URL descriptor contains UTF-8 bytes, not a Cocoa text
                        // descriptor. stringValue can return nil even for a valid file URL.
                        let data = d.data();
                        (data.len() <= 65_536).then(|| data.to_vec())
                    })
                    .and_then(|data| String::from_utf8(data).ok())
                    .and_then(|s| file_path(&s));
                match path {
                    Some(path) if pending.paths.len() < MAX_PENDING => {
                        if !pending.paths.contains(&path) {
                            pending.paths.push_back(path);
                        }
                    }
                    _ => pending.error = Some("macOS: could not receive the drawing's file path".into()),
                }
            }
            if let Some(ctx) = &pending.context {
                ctx.request_repaint();
            }
        }
    }
);

impl Handler {
    fn register(&self) {
        let manager = NSAppleEventManager::sharedAppleEventManager();
        // SAFETY: The main-thread handler implements exactly the required two-descriptor
        // selector, and NativeOpen retains it while the run loop is alive.
        unsafe {
            manager.setEventHandler_andSelector_forEventClass_andEventID(self, sel!(handleOpenDocuments:withReplyEvent:), OPEN_CLASS, OPEN_DOCUMENTS);
        }
    }
}

fn file_path(s: &str) -> Option<PathBuf> {
    let path = url::Url::parse(s).ok()?.to_file_path().ok()?;
    // Open Documents is a drawing entry point, not APPLOAD: open_path also executes LISP.
    let ext = path.extension()?.to_str()?;
    (ext.eq_ignore_ascii_case("dxf") || ext.eq_ignore_ascii_case("dwg")).then_some(path)
}

/// Retains the event handler for the lifetime of the application.
pub struct NativeOpen {
    handler: Option<Retained<Handler>>,
}

impl NativeOpen {
    pub fn install() -> Self {
        let Some(mtm) = MainThreadMarker::new() else { return Self { handler: None } };
        let this = Handler::alloc(mtm).set_ivars(RefCell::new(Pending::default()));
        // SAFETY: NSObject's init returns the allocated handler with the standard signature.
        let handler: Retained<Handler> = unsafe { msg_send![super(this), init] };
        // AppKit installs its default Apple-event handlers during startup. Register ours at
        // WillFinishLaunching, before the launch's document events, and again when eframe is
        // ready. Never replace the application delegate winit uses for its event loop.
        // SAFETY: The retained handler implements this notification selector with its required
        // one-NSNotification signature. NativeOpen removes it before releasing the handler.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &handler,
                sel!(applicationWillFinishLaunching:),
                Some(ns_string!("NSApplicationWillFinishLaunchingNotification")),
                None,
            );
        }
        Self { handler: Some(handler) }
    }

    pub fn set_context(&self, ctx: egui::Context) {
        if let Some(handler) = &self.handler {
            handler.register();
            handler.ivars().borrow_mut().context = Some(ctx);
        }
    }

    pub fn poll(&self, app: &mut CadApp) {
        let Some(handler) = &self.handler else { return };
        // Admit only one native import at a time, including files already opening through
        // another entry point. Keep the bounded queue intact while the worker is busy.
        let (path, error) = {
            let mut pending = handler.ivars().borrow_mut();
            (pending.next_path(app.opening.busy()), pending.error.take())
        };
        if let Some(path) = path {
            let path = path.canonicalize().unwrap_or(path);
            let existing = app.session.docs.iter().position(|d| {
                d.path.as_deref().is_some_and(|p| {
                    let p = std::path::Path::new(p);
                    p.canonicalize().unwrap_or_else(|_| p.to_path_buf()) == path
                })
            });
            if let Some(index) = existing {
                // Finder can deliver the same drawing again, including one restored at
                // launch. Select it without re-importing over any unsaved work.
                let _ = app.run("document.switch", serde_json::json!({ "index": index }));
                app.ui.start_tab = false;
            } else {
                app.open_path(&path);
            }
            // Existing documents need no worker to keep frames coming for the next path.
            if let Some(ctx) = &handler.ivars().borrow().context {
                ctx.request_repaint();
            }
        }
        if let Some(error) = error {
            log::warn!("{error}");
            app.session.echo(error.clone());
            app.set_status(error);
        }
    }
}

impl Drop for NativeOpen {
    fn drop(&mut self) {
        if let Some(handler) = &self.handler {
            // SAFETY: The observer is the same retained handler registered in install.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(handler) };
            NSAppleEventManager::sharedAppleEventManager().removeEventHandlerForEventClass_andEventID(OPEN_CLASS, OPEN_DOCUMENTS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Pending, file_path};
    use std::path::PathBuf;

    #[test]
    fn finder_urls_preserve_spaces_accents_and_literal_percent() {
        assert_eq!(file_path("file:///tmp/Plano%20Isome%CC%81trico%2520.dwg"), Some(PathBuf::from("/tmp/Plano Isome\u{301}trico%20.dwg")));
        assert_eq!(file_path("https://example.com/plan.dwg"), None);
        assert_eq!(file_path("not a url"), None);
        assert_eq!(file_path("file:///tmp/Drawing.DXF"), Some(PathBuf::from("/tmp/Drawing.DXF")));
        for name in ["routine.lsp", "routine.LSP", "drawing.dxf.lsp", "drawing.txt", "drawing"] {
            assert_eq!(file_path(&format!("file:///tmp/{name}")), None);
        }
    }

    #[test]
    fn pending_drawings_wait_for_the_active_import_and_keep_their_order() {
        let mut pending = Pending::default();
        pending.paths.extend([PathBuf::from("first.dxf"), PathBuf::from("second.dwg")]);
        assert_eq!(pending.next_path(true), None);
        assert_eq!(pending.paths.len(), 2);
        assert_eq!(pending.next_path(false), Some(PathBuf::from("first.dxf")));
        assert_eq!(pending.next_path(true), None);
        assert_eq!(pending.next_path(false), Some(PathBuf::from("second.dwg")));
        assert_eq!(pending.next_path(false), None);
    }
}
