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

fn recent_provider_app() -> CadApp {
    let app = CadApp::new(Session::new(), crate::Services::default());
    app.file_providers.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
    let connection = app.file_providers.connect("test".into(), "Test storage".into(), "account".into()).unwrap();
    app.file_providers
        .set_resources(
            &connection,
            vec![
                crate::file_providers::Resource { id: "owned".into(), name: "Owned.dxf".into(), updated_at: String::new() },
                crate::file_providers::Resource { id: "other".into(), name: "Other.dxf".into(), updated_at: String::new() },
            ],
        )
        .unwrap();
    app.file_providers.take_events();
    app
}

#[test]
fn provider_recent_command_queues_only_the_selected_resource_without_retargeting_the_drawing() {
    let mut app = recent_provider_app();
    let before = app.session.state().unwrap().clone();
    let local = app.ui.recent_files.clone();
    assert!(crate::menus::is_ui_command(crate::menus::PROVIDER_RECENT_OPEN));
    let result = app.run(crate::menus::PROVIDER_RECENT_OPEN, json!({"resource":"owned"})).unwrap();
    assert_eq!(result["pending"], "fileProvider");
    let events = app.file_providers.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].metadata["type"], "openRequested");
    assert_eq!(events[0].metadata["resourceId"], "owned");
    assert_eq!(events[0].metadata["requestId"], result["requestId"]);
    let after = app.session.state().unwrap();
    assert_eq!((after.uid, after.revision), (before.uid, before.revision));
    assert!(std::sync::Arc::ptr_eq(&after.doc, &before.doc));
    assert_eq!(after.path, before.path);
    assert_eq!(app.ui.recent_files, local);
}

#[test]
fn provider_recent_command_backpressure_remains_in_status_and_command_history() {
    let mut app = recent_provider_app();
    let mut full = false;
    for _ in 0..128 {
        if app.file_providers.request_open("owned").is_err() {
            full = true;
            break;
        }
        app.file_providers.take_events();
    }
    assert!(full, "the fixture must reach the actual bounded read capacity");
    let before = app.session.state().unwrap().clone();
    let error = app.run(crate::menus::PROVIDER_RECENT_OPEN, json!({"resource":"owned"})).unwrap_err();
    assert!(app.file_providers.take_events().is_empty(), "a refused command cannot publish a partial read");
    let ctx = egui::Context::default();
    let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| app.logic(ctx));
    output.textures_delta.clear();
    assert_eq!(app.status.as_ref().map(|(text, _)| text.as_str()), Some(error.as_str()));
    assert_eq!(app.session.log.last(), Some(&error));
    assert_eq!((app.session.state().unwrap().uid, app.session.state().unwrap().revision), (before.uid, before.revision));
}

#[test]
fn provider_recent_command_rejects_malformed_and_disabled_requests_without_opening_files() {
    let mut app = CadApp::new(Session::new(), crate::Services::default());
    let before = app.session.state().unwrap().clone();
    for params in [Value::Null, json!({}), json!({"resource":42}), json!({"resource":""}), json!({"resource":"owned"})] {
        assert!(app.run(crate::menus::PROVIDER_RECENT_OPEN, params).is_err());
        assert!(!app.file_providers.enabled());
        assert!(app.file_providers.take_events().is_empty());
        assert_eq!((app.session.state().unwrap().uid, app.session.state().unwrap().revision), (before.uid, before.revision));
        assert!(std::sync::Arc::ptr_eq(&app.session.state().unwrap().doc, &before.doc));
    }

    let mut connected = recent_provider_app();
    let before = connected.session.state().unwrap().clone();
    let connection = connected.file_providers.snapshot()["connectionId"].clone();
    connected.file_providers.set_enabled(false).unwrap();
    connected.file_providers.take_events();
    let error = connected.run(crate::menus::PROVIDER_RECENT_OPEN, json!({"resource":"owned"})).unwrap_err();
    assert!(connected.file_providers.connected() && !connected.file_providers.enabled());
    assert_eq!(connected.file_providers.snapshot()["connectionId"], connection);
    assert!(connected.file_providers.take_events().is_empty());
    assert!(std::sync::Arc::ptr_eq(&connected.session.state().unwrap().doc, &before.doc));
    assert_eq!(connected.session.log.last(), Some(&error));
}
