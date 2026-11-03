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


#[test]
fn real_dxf_delivery_failure_preserves_the_owned_drawing_and_later_edits_through_retry() {
    let mut app = CadApp::new(Session::new(), crate::Services::default());
    app.run("line", json!({"points":[[0,0],[10,10]]})).unwrap();
    let host = app.file_providers.clone();
    host.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
    host.connect("test".into(), "Test storage".into(), "account".into()).unwrap();
    let snapshot = app.session.state().unwrap().clone();
    let uid = snapshot.uid;
    let checkpoint = snapshot.saved.clone();
    // Use the actual native drawing codec without mutating process-global engine hooks.
    let bytes = cadcraft_io::write(&snapshot.doc, "Owned.dxf").unwrap();
    assert_eq!(cadcraft_io::read(&bytes, "Owned.dxf").unwrap().model.len(), snapshot.doc.model.len());
    let request = host.begin(vec![snapshot.clone()], uid, snapshot.revision, "Owned.dxf".into(), bytes, false).unwrap();
    host.take_events();
    host.resolve_dialog(&request, "provider", "Owned.dxf").unwrap();
    app.run("line", json!({"points":[[20,20],[30,30]]})).unwrap();
    let later_revision = app.session.state().unwrap().revision;
    app.session.new_drawing(false);
    let active = app.session.state().unwrap().uid;
    assert!(host.publish_events_with(
        |metadata, bytes| {
            assert_eq!(metadata["type"], "save");
            assert_eq!(metadata["requestId"], request);
            assert!(bytes.is_some_and(|bytes| !bytes.is_empty()));
            Err("synthetic native delivery exception")
        },
        |_| {},
    ));
    assert!(host.pending_for(uid));
    tick(&mut app);
    let owned = app.session.docs.iter().find(|doc| doc.uid == uid).unwrap();
    assert_eq!(owned.revision, later_revision);
    assert!(owned.is_dirty() && std::sync::Arc::ptr_eq(&owned.saved, &checkpoint));
    assert_eq!(app.session.state().unwrap().uid, active);
    assert!(app.status.as_ref().unwrap().0.contains("File provider save failed"));
    assert!(host.result(request.clone(), "saved".into(), "late".into()).is_err());
    let mut failed = 0;
    assert!(!host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["type"], "saveFailed");
            assert_eq!(metadata["requestId"], request);
            assert_eq!(metadata["documentId"], uid.to_string());
            failed += 1;
            Ok::<(), &str>(())
        },
        |_| {},
    ));
    assert_eq!(failed, 1);
    assert!(!host.busy());
    let retry_snapshot = app.session.docs.iter().find(|doc| doc.uid == uid).unwrap().clone();
    let bytes = cadcraft_io::write(&retry_snapshot.doc, "Owned.dxf").unwrap();
    let retry = host.begin(vec![retry_snapshot.clone()], uid, retry_snapshot.revision, "Owned.dxf".into(), bytes, false).unwrap();
    host.take_events();
    host.resolve_dialog(&retry, "provider", "Owned.dxf").unwrap();
    assert!(!host.publish_events_with(
        |metadata, bytes| {
            assert_eq!(metadata["requestId"], retry);
            assert_eq!(cadcraft_io::read(&bytes.unwrap(), "Owned.dxf").unwrap().model.len(), retry_snapshot.doc.model.len());
            Ok::<(), &str>(())
        },
        |_| {},
    ));
    let index = app.session.docs.iter().position(|doc| doc.uid == uid).unwrap();
    app.run("document.switch", json!({"index":index})).unwrap();
    app.run("line", json!({"points":[[40,40],[50,50]]})).unwrap();
    let current_revision = app.session.state().unwrap().revision;
    let back = app.session.docs.iter().position(|doc| doc.uid == active).unwrap();
    app.run("document.switch", json!({"index":back})).unwrap();
    host.result(retry.clone(), "saved".into(), "file-1".into()).unwrap();
    tick(&mut app);
    let owned = app.session.docs.iter().find(|doc| doc.uid == uid).unwrap();
    assert_eq!(owned.revision, current_revision);
    assert!(owned.is_dirty() && std::sync::Arc::ptr_eq(&owned.saved, &retry_snapshot.doc));
    assert_eq!(app.session.state().unwrap().uid, active);
    assert_eq!(host.bound_document("file-1"), Some(uid));
    assert_eq!(host.take_events()[0].metadata["type"], "saved");
    assert!(!host.busy());
}
