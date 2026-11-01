#![allow(clippy::unwrap_used)]
use super::*;
use cadcraft_engine::Session;
fn pending() -> (CadApp, String, u64) {
    let app = CadApp::new(Session::new(), crate::Services::default());
    app.file_providers.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
    app.file_providers.connect("test".into(), "Test storage".into(), "account".into()).unwrap();
    let doc = app.session.state().unwrap().clone();
    let uid = doc.uid;
    // Lifecycle fixture: a fake capture is isolated from the engine's process-global format hooks.
    let request = app.file_providers.begin(vec![doc.clone()], uid, doc.revision, "Drawing.dxf".into(), vec![1], false).unwrap();
    (app, request, uid)
}
#[test]
fn rejected_provider_save_is_reported_without_consuming_the_capture() {
    let (mut app, _, uid) = pending();
    let error = save(&mut app, false).unwrap_err();
    assert!(error.contains("already pending"));
    assert_eq!(app.status.as_ref().map(|(text, _)| text.as_str()), Some(error.as_str()));
    assert_eq!(app.session.log.last(), Some(&error));
    assert!(app.file_providers.pending_for(uid));
}
#[test]
fn ack_owns_its_document_and_does_not_clean_newer_edits() {
    let (mut app, request, uid) = pending();
    app.file_providers.resolve_dialog(&request, "provider", "Drawing.dxf").unwrap();
    let doc = app.session.docs.iter_mut().find(|d| d.uid == uid).unwrap();
    std::sync::Arc::make_mut(&mut doc.doc).handseed += 1;
    app.session.new_drawing(false);
    app.file_providers.result(request, "saved".into(), "drawing-1".into()).unwrap();
    tick(&mut app);
    assert!(app.session.docs.iter().find(|d| d.uid == uid).unwrap().is_dirty());
    assert_eq!(app.file_providers.bound_document("drawing-1"), Some(uid));
}
#[test]
fn device_result_is_terminal_without_claiming_durable_storage() {
    let (mut app, request, uid) = pending();
    let doc = app.session.docs.iter_mut().find(|d| d.uid == uid).unwrap();
    std::sync::Arc::make_mut(&mut doc.doc).handseed += 1;
    app.file_providers.resolve_dialog(&request, "device", "Drawing.dxf").unwrap();
    app.file_providers.result(request, "download".into(), String::new()).unwrap();
    tick(&mut app);
    assert!(app.session.docs.iter().find(|d| d.uid == uid).unwrap().is_dirty());
    assert!(app.file_providers.bound_document("drawing-1").is_none());
}
