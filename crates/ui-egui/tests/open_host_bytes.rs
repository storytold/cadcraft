//! The document a hosting page sends (`cadcraft:open`) opens in the window.
//!
//! The cabinet sends the file's bytes with the session (INTEGRATION.md §5.1). They are how the
//! drawing reaches the browser, so they open locally; without this the editor stays on its start
//! tab and the shared session has no document to show.

use cadcraft_doc::Drawing;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_ui_egui::{CadApp, Services};

fn install_io() {
    set_io(IoHooks {
        read: |bytes, _| {
            if bytes.starts_with(b"0\nSECTION") { Ok(Drawing::new_metric()) } else { Err("not a drawing".to_string()) }
        },
        write: |_, _| Ok(b"0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nEOF\n".to_vec()),
        plot: None,
    });
}

/// The host's document replaces the start tab with a drawing named as the cabinet named it.
#[test]
fn a_host_document_opens_in_the_window() {
    install_io();
    let mut app = CadApp::new(cadcraft_engine::Session::empty(), Services::default());
    assert!(app.session.docs.is_empty(), "an embedded app starts on the start tab");

    app.open_host_bytes("acceptance.dxf", b"0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nEOF\n");

    let state = app.session.state().expect("the host's drawing opened");
    assert_eq!(state.title, "acceptance.dxf");
    assert!(!app.session.docs.is_empty(), "the window shows the drawing, not the start tab");
}

/// A document that is not a drawing is reported, not opened, and leaves the window where it was.
#[test]
fn a_host_document_that_is_not_a_drawing_is_reported() {
    install_io();
    let mut app = CadApp::new(cadcraft_engine::Session::empty(), Services::default());

    app.open_host_bytes("junk.dxf", b"junk");

    assert!(app.session.state().is_err(), "no drawing was opened");
    assert!(app.session.docs.is_empty(), "the window stays on the start tab");
}
