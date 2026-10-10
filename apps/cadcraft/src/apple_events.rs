//! macOS open-documents and quit Apple events.
//!
//! A Finder double-click, Open With, a drop on the Dock icon or `open -a CADCraft file.dxf` don't
//! put the path in argv: LaunchServices sends the app a `kAEOpenDocuments` ('odoc') Apple event,
//! which winit 0.30 doesn't handle, so macOS reported that CADCraft "cannot open files in the
//! Drawing Exchange Format format". Handling the event needs Objective-C class declarations
//! (`unsafe`, forbidden in this workspace); `fmv-macos-events` does that behind a safe main-thread
//! API, leaving winit's application delegate alone. Registering the 'odoc' handler also takes over
//! 'quit', which is handled like Cmd+Q.

use cadcraft_ui_egui::OsEvent;
use fmv_macos_events::{Event, Inbox, Registration};

/// Keeps the handlers registered; hold it until the event loop returns.
pub struct AppleEvents {
    _registration: Registration,
    inbox: Inbox,
}

impl AppleEvents {
    /// Register the handlers. Call on the main thread before the event loop starts, so the event
    /// that launched the app is caught too.
    pub fn install() -> Self {
        let (registration, inbox) = Registration::install();
        AppleEvents { _registration: registration, inbox }
    }

    /// The `Services::os_events` poll. Events that arrive between frames repaint `ctx`.
    pub fn poll(&self, ctx: &egui::Context) -> Box<dyn Fn() -> Vec<OsEvent>> {
        let ctx = ctx.clone();
        self.inbox.set_wake(move || ctx.request_repaint());
        let inbox = self.inbox.clone();
        Box::new(move || {
            inbox
                .drain()
                .into_iter()
                .map(|e| match e {
                    Event::Open(paths) => OsEvent::Open(paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect()),
                    Event::Quit => OsEvent::Quit,
                })
                .collect()
        })
    }
}
