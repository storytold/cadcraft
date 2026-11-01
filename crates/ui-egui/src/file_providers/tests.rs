#![allow(clippy::unwrap_used)]
use super::*;
use cadcraft_engine::Session;
fn host() -> Host {
    let h = Host::default();
    h.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
    h.connect("test".into(), "Test storage".into(), "account-a".into()).unwrap();
    h
}
fn capture(h: &Host, save_as: bool) -> (String, u64) {
    let mut session = Session::new();
    session.execute("new", &json!({})).unwrap();
    let doc = session.state().unwrap().clone();
    let uid = doc.uid;
    (h.begin(vec![doc.clone()], uid, doc.revision, "example.dxf".into(), vec![1, 2, 3], save_as).unwrap(), uid)
}
#[test]
fn off_until_explicit_registration_and_session_is_single_use() {
    let h = Host::default();
    assert!(!h.enabled());
    assert!(!h.connected());
    assert!(h.connect("test".into(), "Test".into(), "a".into()).is_err());
    assert!(host().set_session("0123456789abcdef".into()).is_err());
}
#[test]
fn each_save_has_a_choice_and_cancel_never_acknowledges_a_document() {
    let h = host();
    let (request, uid) = capture(&h, false);
    assert_eq!(h.take_events()[0].metadata["type"], "saveDialog");
    assert!(h.pending_for(uid));
    h.resolve_dialog(&request, "cancel", "").unwrap();
    let actions = h.take_actions();
    assert!(matches!(&actions[..],[Action::Save { status, .. }] if status == "cancel"));
    assert!(h.state().bindings.is_empty());
    assert!(h.result(request, "saved".into(), "file".into()).is_err());
}
#[test]
fn durable_ack_owns_its_capture_and_is_single_use() {
    let h = host();
    let (request, uid) = capture(&h, false);
    h.take_events();
    assert!(h.disconnect().is_err());
    h.resolve_dialog(&request, "provider", "durable.dxf").unwrap();
    let event = h.take_events().remove(0);
    assert_eq!(event.metadata["type"], "save");
    assert_eq!(event.bytes, Some(vec![1, 2, 3]));
    h.result(request.clone(), "saved".into(), "file-1".into()).unwrap();
    assert!(h.result(request, "saved".into(), "file-1".into()).is_err());
    assert!(matches!(h.take_actions().as_slice(),[Action::Save { capture,resource,.. }] if capture.doc==uid && resource=="file-1"));
}
#[test]
fn replies_cannot_cross_the_chosen_destination_or_an_open_resource_alias() {
    let h = host();
    let (request, uid) = capture(&h, false);
    h.resolve_dialog(&request, "device", "local.dxf").unwrap();
    assert!(h.result(request.clone(), "saved".into(), "remote".into()).is_err());
    h.result(request, "download".into(), String::new()).unwrap();
    h.take_actions();
    h.take_events();
    let (request, _) = capture(&h, false);
    h.state().bindings.insert(uid, "already-open".into());
    h.resolve_dialog(&request, "provider", "new.dxf").unwrap();
    assert!(h.result(request, "saved".into(), "already-open".into()).is_err());
}
#[test]
fn unregister_clears_cached_account_documents_before_reconnect() {
    let h = host();
    h.state().bindings.insert(42, "account-a-resource".into());
    h.state().names.insert(42, "Account A.dxf".into());
    h.refresh(vec![(42, 1, "Account A.dxf".into(), false)]);
    assert_eq!(h.snapshot()["documents"][0]["resourceId"], "account-a-resource");
    h.disconnect().unwrap();
    let disconnected = h.snapshot();
    assert!(disconnected["documents"].is_null());
    assert!(disconnected["resources"].as_array().unwrap().is_empty());
    assert!(h.state().bindings.is_empty());
    assert!(h.state().names.is_empty());
    h.connect("test".into(), "Test storage".into(), "account-b".into()).unwrap();
    assert!(h.snapshot()["documents"].is_null());
}
#[test]
fn resource_reads_and_lists_are_scoped_to_current_account_connection() {
    let h = host();
    let connection = h.snapshot()["connectionId"].as_str().unwrap().to_string();
    let rows = vec![Resource { id: "file-1".into(), name: "remote.dxf".into(), updated_at: String::new() }];
    assert!(h.set_resources("another-connection", rows.clone()).is_err());
    h.set_resources(&connection, rows).unwrap();
    assert!(h.request_open("unknown").is_err());
    let request = h.request_open("file-1").unwrap();
    assert!(h.open("forged".into(), "remote.dxf".into(), vec![1]).is_err());
    h.open(request.clone(), "remote.dxf".into(), vec![1]).unwrap();
    assert!(h.open(request.clone(), "remote.dxf".into(), vec![1]).is_err());
    assert!(h.cancel_open(request.clone(), String::new()).is_err());
    assert!(h.disconnect().is_err());
    h.take_actions();
    h.finish_open(&request, "file-1", "remote.dxf", Err("invalid archive".into()));
    h.disconnect().unwrap();
    let next = h.connect("test".into(), "Test storage".into(), "account-b".into()).unwrap();
    assert_ne!(connection, next);
    assert!(h.set_resources(&connection, Vec::new()).is_err());
    assert!(h.snapshot()["resources"].as_array().unwrap().is_empty());
}
#[test]
fn names_are_native_filenames_and_resource_ids_are_opaque() {
    assert!(valid_native_name("with accents áé.dxf"));
    assert!(!valid_native_name("../project.dxf"));
    assert!(!valid_native_name("project.svg"));
    assert!(!valid_id("../../project"));
    assert!(!valid_native_name("project.dxf\0"));
}
#[test]
fn disabled_provider_does_not_discard_a_capture_already_awaiting_ack() {
    let h = host();
    let (request, uid) = capture(&h, false);
    h.set_enabled(false).unwrap();
    assert!(!h.enabled());
    assert!(h.pending_for(uid));
    h.resolve_dialog(&request, "provider", "in-flight.dxf").unwrap();
    h.result(request, "saved".into(), "file-1".into()).unwrap();
    assert!(matches!(h.take_actions().as_slice(),[Action::Save { status,.. }] if status=="saved"));
}

fn fill_transport_event_queue(h: &Host) {
    let mut state = h.state();
    for ordinal in state.events.len()..MAX_EVENTS {
        state.event(json!({"type":"queued","ordinal":ordinal}), None);
    }
}

fn terminal_resource(h: &Host) -> String {
    let connection = h.snapshot()["connectionId"].as_str().unwrap().to_string();
    let filename = format!("Remote{EXTENSION}");
    h.set_resources(&connection, vec![Resource { id: "file-1".into(), name: filename.clone(), updated_at: String::new() }]).unwrap();
    filename
}

#[test]
fn full_event_queue_retains_open_terminals_until_ordered_drain() {
    let h = host();
    let filename = terminal_resource(&h);
    let connection = h.snapshot()["connectionId"].as_str().unwrap().to_string();
    let success = h.request_open("file-1").unwrap();
    h.open(success.clone(), filename.clone(), vec![1]).unwrap();
    let failure = h.request_open("file-1").unwrap();
    h.open(failure.clone(), filename.clone(), vec![1]).unwrap();
    h.take_actions();
    fill_transport_event_queue(&h);
    h.finish_open(&success, "file-1", &filename, Ok(42));
    h.finish_open(&failure, "file-1", &filename, Err("é\n".repeat(1000)));
    h.finish_open(&success, "file-1", &filename, Err("duplicate".into()));
    h.finish_open("unowned", "file-1", &filename, Ok(99));
    assert_eq!(h.state().events.len(), MAX_EVENTS);
    assert_eq!(h.state().finished.len(), 2);
    assert!(h.disconnect().is_err());
    let first = h.take_events();
    assert_eq!(first.len(), MAX_EVENTS);
    assert!(!first.iter().any(|event| matches!(event.metadata["type"].as_str(), Some("opened" | "openFailed"))));
    let terminals = h.take_events();
    assert_eq!(terminals.len(), 2);
    assert_eq!(terminals[0].metadata["type"], "opened");
    assert_eq!(terminals[1].metadata["type"], "openFailed");
    assert_eq!(terminals.iter().map(|event| event.metadata["requestId"].as_str().unwrap()).collect::<Vec<_>>(), [success.as_str(), failure.as_str()]);
    for event in &terminals {
        assert_eq!(event.metadata["connectionId"], connection);
        assert_eq!(event.metadata["namespace"], "account-a");
    }
    let error = terminals[1].metadata["error"].as_str().unwrap();
    assert!(error.len() <= 1024 && error.chars().all(|ch| !ch.is_control()));
    assert_eq!(h.bound_document("file-1"), Some(42));
    assert!(!h.busy());
    assert!(h.take_events().is_empty());
}

#[test]
fn full_event_queue_retains_save_failure_without_duplicate_or_unowned_terminals() {
    let h = host();
    let (request, uid) = capture(&h, false);
    h.resolve_dialog(&request, "provider", &format!("Failed{EXTENSION}")).unwrap();
    h.result(request.clone(), "failed".into(), String::new()).unwrap();
    let Action::Save { capture, .. } = h.take_actions().remove(0) else { panic!("expected save action") };
    fill_transport_event_queue(&h);
    h.finish_save(&request, &capture, "failed", "", true);
    h.finish_save(&request, &capture, "failed", "", true);
    h.finish_save("unowned", &capture, "failed", "", true);
    assert_eq!(h.state().events.len(), MAX_EVENTS);
    assert_eq!(h.state().finished.len(), 1);
    assert!(h.pending_for(uid));
    assert_eq!(h.take_events().len(), MAX_EVENTS);
    let terminals = h.take_events();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].metadata["type"], "saveFailed");
    assert_eq!(terminals[0].metadata["requestId"], request);
    assert!(!h.pending_for(uid));
    assert!(!h.busy());
    assert!(h.take_events().is_empty());
}

#[test]
fn retained_terminals_are_bounded_by_admitted_reads_and_saves() {
    let h = host();
    let filename = terminal_resource(&h);
    let connection = h.snapshot()["connectionId"].as_str().unwrap().to_string();
    for _ in 0..MAX_PENDING {
        let (request, _) = capture(&h, false);
        h.resolve_dialog(&request, "provider", &format!("Failed{EXTENSION}")).unwrap();
        h.result(request, "failed".into(), String::new()).unwrap();
    }
    let saves: Vec<_> = h
        .take_actions()
        .into_iter()
        .map(|action| match action {
            Action::Save { request, capture, .. } => (request, capture),
            _ => panic!("expected save action"),
        })
        .collect();
    assert!(h.state().events.len() < MAX_EVENTS);
    let mut extra = saves[0].1.snapshots.clone();
    extra[0].uid = u64::MAX;
    assert!(h.begin(extra, u64::MAX, 1, format!("Extra{EXTENSION}"), vec![1], false).is_err());
    let mut reads = Vec::new();
    for _ in 0..MAX_PENDING {
        let request = h.request_open("file-1").unwrap();
        h.open(request.clone(), filename.clone(), vec![1]).unwrap();
        reads.push(request);
    }
    h.take_actions();
    fill_transport_event_queue(&h);
    for (request, capture) in &saves {
        h.finish_save(request, capture, "failed", "", true);
    }
    for request in &reads {
        h.finish_open(request, "file-1", &filename, Err("read failed".into()));
    }
    assert_eq!(h.state().finished.len(), 2 * MAX_PENDING);
    assert_eq!(h.state().finishing_saves.len(), MAX_PENDING);
    assert_eq!(h.state().opens.len(), MAX_PENDING);
    assert!(h.request_open("file-1").is_err());
    assert!(h.disconnect().is_err());
    assert_eq!(h.take_events().len(), MAX_EVENTS);
    let terminals = h.take_events();
    assert_eq!(terminals.len(), 2 * MAX_PENDING);
    let expected: Vec<_> = saves.iter().map(|(request, _)| request.as_str()).chain(reads.iter().map(String::as_str)).collect();
    assert_eq!(terminals.iter().map(|event| event.metadata["requestId"].as_str().unwrap()).collect::<Vec<_>>(), expected);
    for event in &terminals {
        assert_eq!(event.metadata["connectionId"], connection);
        assert_eq!(event.metadata["namespace"], "account-a");
    }
    assert!(!h.busy());
    assert!(h.take_events().is_empty());
}

#[test]
fn capture_target_must_belong_to_its_snapshot_aliases() {
    let h = host();
    let (request, _) = capture(&h, false);
    let snapshots = h.state().dialogs.get(&request).unwrap().snapshots.clone();
    assert!(h.begin(snapshots, u64::MAX, 1, format!("Other{EXTENSION}"), vec![1], false).is_err());
    assert!(!h.pending_for(u64::MAX));
}
