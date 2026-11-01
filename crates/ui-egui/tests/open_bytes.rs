//! Opening a drawing from its contents, where the file has no path (the web's File > Open… picker
//! and files dropped on the page, #55).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services};

#[test]
fn open_bytes_opens_the_named_drawing_and_reports_failures() {
    // A reader that only accepts what a DXF starts with: proves the bytes arrive unchanged.
    set_io(IoHooks {
        read: |b, _| if b.starts_with(b"0\nSECTION\n") { Ok(Drawing::new_imperial()) } else { Err("not a drawing".into()) },
        write: |_, _| Ok(Vec::new()),
        plot: None,
    });
    let mut app = CadApp::new(Session::empty(), Services::default());
    app.ui.start_tab = true;

    app.open_bytes("Bracket.dwg", b"0\nSECTION\n2\nHEADER\n");
    assert_eq!(app.session.docs.len(), 1);
    assert_eq!(app.session.state().map(|s| s.title.clone()).ok().as_deref(), Some("Bracket.dwg"));
    assert!(!app.ui.start_tab && app.canvas.zoom_pending, "shown and zoomed like a drawing opened from a path");

    // A file the reader rejects: nothing opens; the error reaches the command line and status bar.
    app.open_bytes("junk.dxf", b"junk");
    assert_eq!(app.session.docs.len(), 1);
    assert!(app.session.log.last().is_some_and(|l| l.contains("not a drawing")), "{:?}", app.session.log.last());
    assert!(app.status.as_ref().is_some_and(|(s, _)| s.contains("not a drawing")));

    // Too large to hold in memory: refused before anything is read, with the limit named.
    assert!(CadApp::open_size_error("big.dwg", CadApp::MAX_OPEN_BYTES).is_none());
    let e = CadApp::open_size_error("huge.dwg", CadApp::MAX_OPEN_BYTES * 2);
    assert_eq!(e.as_deref(), Some("Open: huge.dwg is 512.0 MB, larger than the 256 MB CADCraft opens in the browser"));
    app.open_failed(e.unwrap_or_default());
    assert_eq!(app.session.log.last().map(String::as_str), Some("Open: huge.dwg is 512.0 MB, larger than the 256 MB CADCraft opens in the browser"));
    assert_eq!(app.session.docs.len(), 1);
}

/// A shell driven from outside -- the web build in a page that sent the drawing -- reports a
/// failed open to whoever sent it (#1603); the status bar and command line are no use to them.
/// The error is readable once, so one failed open is reported once.
#[test]
fn a_failed_open_is_reported_once() {
    set_io(IoHooks {
        read: |b, _| if b.starts_with(b"0\nSECTION\n") { Ok(Drawing::new_imperial()) } else { Err("not a drawing".into()) },
        write: |_, _| Ok(Vec::new()),
        plot: None,
    });
    let mut app = CadApp::new(Session::empty(), Services::default());
    assert_eq!(app.take_open_error(), None, "nothing has been opened yet");

    app.open_bytes("junk.dxf", b"junk");
    assert!(app.take_open_error().is_some_and(|e| e.contains("not a drawing")));
    assert_eq!(app.take_open_error(), None, "reported once, not on every frame after");

    // Refused before anything is read: also reported.
    app.open_failed("Open: huge.dwg is too large");
    assert_eq!(app.take_open_error().as_deref(), Some("Open: huge.dwg is too large"));

    // A drawing that opens leaves nothing to report.
    app.open_bytes("Bracket.dxf", b"0\nSECTION\n2\nHEADER\n");
    assert_eq!(app.session.docs.len(), 1);
    assert_eq!(app.take_open_error(), None);
}
