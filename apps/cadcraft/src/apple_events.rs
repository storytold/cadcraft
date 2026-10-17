//! macOS open-documents and quit Apple events (#158).
//!
//! A Finder double-click, Open With, a drop on the Dock icon and `open -a CADCraft plan.dxf` don't
//! put the path in argv: LaunchServices sends the running (or just-launched) app a
//! `kAEOpenDocuments` ('odoc') Apple event. winit 0.30 doesn't handle it and owns the
//! `NSApplicationDelegate`, so AppKit answered "CADCraft cannot open files in the Drawing Exchange
//! Format format". Handling it ourselves needs Objective-C class declarations, i.e. `unsafe`, which
//! this workspace forbids. The audited `fmv-macos-events` crate (also used by GridCraft, PdfCraft
//! and PhotoCraft) wraps exactly that, an `NSAppleEventManager` handler registered before Finder's
//! launch event that leaves winit's delegate alone, behind a safe main-thread API.

use std::path::PathBuf;

use cadcraft_ui_egui::CadApp;
use fmv_macos_events::{Event, Inbox, Registration};

/// Keeps the Apple-event handlers registered; hold it until the event loop returns.
pub struct AppleEvents {
    _registration: Registration,
    inbox: Inbox,
}

impl AppleEvents {
    /// Register the handlers. Call on the main thread before the event loop starts, so the event
    /// that launched the app (a Finder double-click) is caught too.
    pub fn install() -> Self {
        let (registration, inbox) = Registration::install();
        Self { _registration: registration, inbox }
    }

    /// The queue the app drains every frame ([`poll`]); events arriving later wake `ctx`.
    pub fn connect(&self, ctx: &egui::Context) -> Inbox {
        let ctx = ctx.clone();
        self.inbox.set_wake(move || ctx.request_repaint());
        self.inbox.clone()
    }
}

/// The paths to hand to OPEN: UTF-8 and non-empty, each once, in the order they arrived. A second
/// copy of the same path (Finder sends one per selected item, and a drop can repeat) would open
/// the drawing twice.
fn openable(paths: impl IntoIterator<Item = PathBuf>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in paths {
        if let Some(s) = p.to_str().filter(|s| !s.is_empty())
            && !out.iter().any(|o| o == s)
        {
            out.push(s.to_string());
        }
    }
    out
}

/// Open the documents that arrived since the last call, then report whether a quit was requested.
/// The same OPEN as File > Open, so a failure shows on the command line and in the status bar.
pub fn drain(inbox: &Inbox, app: &mut CadApp) -> bool {
    let mut quit = false;
    for e in inbox.drain() {
        match e {
            Event::Open(paths) => {
                for p in openable(paths) {
                    app.open_path(&p);
                }
            }
            Event::Quit => quit = true,
        }
    }
    quit
}

/// [`drain`] each frame; a quit closes the window like Cmd+Q, so unsaved drawings are asked about.
pub fn poll(inbox: &Inbox, app: &mut CadApp, ctx: &egui::Context) {
    if drain(inbox, app) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::openable;
    use std::path::PathBuf;

    #[test]
    fn keeps_order_and_drops_duplicates_and_empty_paths() {
        let paths = ["/a/plan.dxf", "", "/b/site.dwg", "/a/plan.dxf"].map(PathBuf::from);
        assert_eq!(openable(paths), ["/a/plan.dxf", "/b/site.dwg"]);
        assert!(openable(Vec::new()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn skips_paths_that_are_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let bad = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff, b'.', b'd', b'x', b'f']));
        assert_eq!(openable([bad, PathBuf::from("/ok.dxf")]), ["/ok.dxf"]);
    }
}
