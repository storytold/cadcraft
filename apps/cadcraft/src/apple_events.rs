//! macOS open-documents and quit Apple events (#158).
//! Based on the approach in <https://github.com/storytold/cadcraft/pull/1347>.
//!
//! A Finder double-click, Open With, a drop on the Dock icon and `open -a CADCraft plan.dxf` don't
//! put the path in argv: LaunchServices sends the running (or just-launched) app a
//! `kAEOpenDocuments` ('odoc') Apple event. winit 0.30 doesn't handle it and owns the
//! `NSApplicationDelegate`, so AppKit answered "CADCraft cannot open files in the Drawing Exchange
//! Format format". Handling it ourselves needs Objective-C class declarations, i.e. `unsafe`, which
//! this workspace forbids. The pinned `fmv-macos-events` crate wraps exactly that, an
//! `NSAppleEventManager` handler registered before Finder's
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

/// Dispatch a batch through the regular opening and unsaved-changes paths. Returns whether the
/// window may close now; a queued quit must not bypass the Save / Don't Save / Cancel prompt.
fn dispatch(events: impl IntoIterator<Item = Event>, app: &mut CadApp) -> bool {
    let mut quit = false;
    for e in events {
        match e {
            Event::Open(paths) => {
                for p in openable(paths) {
                    app.open_path(std::path::Path::new(&p));
                }
            }
            Event::Quit => quit = true,
        }
    }
    quit && !app.request_quit()
}

/// Drain the inbox each frame; a quit closes the window like Cmd+Q, prompting for unsaved drawings.
pub fn poll(inbox: &Inbox, app: &mut CadApp, ctx: &egui::Context) {
    if dispatch(inbox.drain(), app) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, openable};
    use cadcraft_engine::Session;
    use cadcraft_ui_egui::closing::Choice;
    use cadcraft_ui_egui::{CadApp, Services};
    use fmv_macos_events::Event;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("cadcraft-apple-events-{}-{}", std::process::id(), NEXT_DIR.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn app() -> CadApp {
        crate::install_io();
        CadApp::new(Session::new(), Services::default())
    }

    fn settle(app: &mut CadApp) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.opening.busy() && Instant::now() < deadline {
            app.finish_opening();
            std::thread::sleep(Duration::from_millis(5));
        }
        app.finish_opening();
        assert!(!app.opening.busy(), "background opening completed");
    }

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

    #[test]
    fn opens_dwg_and_dxf_without_replacing_an_unsaved_drawing() {
        let dir = TestDir::new();
        let dxf = dir.0.join("planta com espaços.dxf");
        let dwg = dir.0.join("Documentac\u{327}a\u{303}o.dwg");
        let bytes = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n10\n0\nENDSEC\n0\nEOF\n";
        let drawing = cadcraft_io::read(bytes, "synthetic.dxf").unwrap();
        std::fs::write(&dxf, bytes).unwrap();
        std::fs::write(&dwg, cadcraft_io::write(&drawing, "synthetic.dwg").unwrap()).unwrap();

        let mut app = app();
        app.run("line", serde_json::json!({"points": [[0, 0], [1, 1]]})).unwrap();
        let original = app.session.docs[0].uid;
        assert!(app.session.docs[0].is_dirty());
        assert!(!dispatch([Event::Open(vec![dxf.clone(), dwg.clone(), dxf.clone(), PathBuf::new()])], &mut app));
        settle(&mut app);

        assert_eq!(app.session.docs.len(), 3, "each file is opened once alongside the unsaved drawing");
        assert_eq!(app.session.docs[0].uid, original);
        assert!(app.session.docs[0].is_dirty());
        for path in [&dxf, &dwg] {
            assert!(app.session.docs.iter().any(|d| d.path.as_deref() == path.to_str()));
        }
    }

    #[test]
    fn a_failed_open_reports_the_path_and_keeps_existing_drawings() {
        let dir = TestDir::new();
        let missing = dir.0.join("missing drawing.dwg");
        let mut app = app();
        let original = app.session.docs[0].uid;
        assert!(!dispatch([Event::Open(vec![missing.clone()])], &mut app));
        settle(&mut app);
        assert_eq!(app.session.docs.len(), 1);
        assert_eq!(app.session.docs[0].uid, original);
        assert!(app.session.log.iter().any(|line| line.contains(missing.to_str().unwrap())));
    }

    #[test]
    fn a_quit_event_asks_before_closing_unsaved_work_and_can_be_cancelled() {
        let mut app = app();
        app.run("line", serde_json::json!({"points": [[0, 0], [1, 1]]})).unwrap();
        assert!(!dispatch([Event::Quit], &mut app), "must not issue a window close before the answer");
        assert!(app.close_prompt().is_some());
        assert!(!app.quit_confirmed);
        assert!(!app.quit_requested);
        app.answer_close(Choice::Cancel);
        assert!(app.close_prompt().is_none());
        assert!(app.session.docs[0].is_dirty());
        assert!(!app.quit_requested);
    }

    #[test]
    fn a_quit_event_can_close_a_clean_session() {
        let mut app = app();
        assert!(dispatch([Event::Quit], &mut app));
        assert!(app.close_prompt().is_none());
    }
}
