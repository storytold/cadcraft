//! Original native DXF/attachment fixtures exercise the actual CadApp commands and consumers.
#![cfg(not(target_arch = "wasm32"))]
use cadcraft_engine::{
    DocState,
    linked_files::{Bytes, Input, ResourceSet},
};
use cadcraft_ui_egui::{
    CadApp,
    file_providers::{Event, Host, LinkedPayload, Resource},
};
use serde_json::{Value, json};
use std::sync::Arc;
#[path = "support/linked_files.rs"]
mod fixture;

fn tick(app: &mut CadApp) {
    egui::Context::default().run_ui(Default::default(), |ui| app.logic(ui.ctx())).drop_without_applying_deltas();
}
fn connected() -> Result<(CadApp, String), String> {
    let app = fixture::app();
    let host = &app.file_providers;
    host.set_session("0123456789abcdef0123456789abcdef".into())?;
    let connection = host.connect("fixture".into(), "Linked fixture".into(), "account-one".into())?;
    host.configure_linked_files(&connection, 1)?;
    host.set_resources(
        &connection,
        ["main", "copy"].into_iter().map(|id| Resource { id: id.into(), name: "Main.dxf".into(), updated_at: String::new() }).collect(),
    )?;
    Ok((app, connection))
}
fn event(host: &Host, kind: &str) -> Result<Event, String> {
    host.take_events().into_iter().find(|e| e.metadata["type"] == kind).ok_or_else(|| format!("missing actual {kind} event"))
}
fn text(value: &Value, key: &str) -> Result<String, String> {
    value.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("missing {key}"))
}
fn open_input(app: &mut CadApp, connection: &str, input: Input) -> Result<u64, String> {
    let request = app.run("ui.provider.open", json!({"resource":"main"}))?;
    let request = text(&request, "requestId")?;
    event(&app.file_providers, "openRequested")?;
    let before = app.session.docs.len();
    app.file_providers.complete_resource_set(connection, request.clone(), input.clone())?;
    assert_eq!(app.session.docs.len(), before, "no mutation before actual app consumer");
    assert!(app.file_providers.complete_resource_set(connection, request.clone(), input).is_err());
    tick(app);
    let opened = event(&app.file_providers, "opened")?;
    assert_eq!(text(&opened.metadata, "requestId")?, request);
    assert!(!app.file_providers.busy());
    Ok(app.session.state().map_err(|e| e.to_string())?.uid)
}
fn opened() -> Result<(CadApp, String, u64), String> {
    let (mut app, c) = connected()?;
    app.ui.recent_files = vec!["local-history.dxf".into()];
    let uid = open_input(&mut app, &c, fixture::input(10)?)?;
    Ok((app, c, uid))
}
fn state(app: &CadApp, uid: u64) -> Result<&DocState, String> {
    app.session.docs.iter().find(|d| d.uid == uid).ok_or_else(|| "drawing disappeared".into())
}
fn resources(app: &CadApp, uid: u64) -> Result<Arc<ResourceSet>, String> {
    state(app, uid)?.resources.clone().ok_or_else(|| "missing immutable resources".into())
}
fn save(app: &mut CadApp, command: &str, destination: &str, name: &str) -> Result<(String, Arc<ResourceSet>), String> {
    let owner = app.session.state().map_err(|e| e.to_string())?.clone();
    let request = text(&app.run(command, Value::Null)?, "requestId")?;
    let dialog = event(&app.file_providers, "saveDialog")?;
    assert_eq!(text(&dialog.metadata, "documentId")?, owner.uid.to_string());
    assert_eq!(text(&dialog.metadata, "revision")?, owner.revision.to_string());
    app.file_providers.resolve_dialog(&request, destination, name)?;
    let transfer = event(&app.file_providers, "save")?;
    assert_eq!(text(&transfer.metadata, "requestId")?, request);
    assert_eq!(text(&transfer.metadata, "documentId")?, owner.uid.to_string());
    assert_eq!(text(&transfer.metadata, "revision")?, owner.revision.to_string());
    if command == "qsave" {
        assert_eq!(text(&transfer.metadata, "expectedVersion")?, owner.saved_resources.as_ref().ok_or("saved version missing")?.version);
    }
    let Some(LinkedPayload::Set(set)) = transfer.linked else { return Err("save omitted complete resource bytes".into()) };
    assert_eq!(text(&transfer.metadata, "setDigest")?, set.digest());
    Ok((request, set))
}
fn assert_bytes(set: &ResourceSet, original: &Input) -> Result<(), String> {
    assert_eq!(set.files.len(), 6);
    assert_eq!(set.edges.len(), 7);
    for file in &original.files {
        let actual = set.file(&file.key).ok_or("authorized resource lost")?;
        assert_eq!(actual.bytes.data(), file.bytes.data());
        assert_eq!(actual.kind, file.kind);
    }
    let png = set.file("images/main.png").ok_or("image lost")?;
    let decoded = image::load_from_memory(png.bytes.data()).map_err(|e| e.to_string())?.to_rgba8();
    assert_eq!(decoded.dimensions(), (1, 1));
    assert_eq!(decoded.get_pixel(0, 0).0, [10, 64, 128, 255]);
    Ok(())
}

#[test]
fn linked_open_preserves_complete_native_graph_and_authorized_original_bytes() -> Result<(), String> {
    let (app, _, uid) = opened()?;
    let doc = state(&app, uid)?;
    let set = resources(&app, uid)?;
    assert!(!doc.is_dirty() && doc.path.is_none());
    assert_bytes(&set, &fixture::input(10)?)?;
    assert_eq!(doc.doc.model.len(), 1);
    assert_eq!(doc.doc.layouts.iter().find(|layout| layout.name == "Sheet").ok_or("layout lost")?.entities.len(), 1);
    assert_eq!(doc.doc.blocks.get("Picture").ok_or("block lost")?.entities.len(), 1);
    let overlay = doc.doc.blocks.get("Overlay").ok_or("overlay lost")?;
    assert!(overlay.xref_overlay && overlay.xref_path.as_deref() == Some("../parts/overlay.dxf"));
    assert!(doc.doc.blocks.get("Child").is_some_and(|b| b.entities.is_empty()), "provided context skips native xref expansion");
    assert_eq!(doc.doc.text_styles.first().ok_or("style lost")?.font, "CADCraft Stroke");
    assert_eq!(app.ui.recent_files, vec!["local-history.dxf"]);
    assert_eq!(set.authority.as_ref(), Some(&("fixture".into(), "account-one".into())));
    Ok(())
}

#[test]
fn linked_ack_owns_frozen_native_revision_and_full_set_version_only() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    app.run("line", json!({"points":[[0,0],[10,10]]}))?;
    let captured = state(&app, uid)?.doc.clone();
    let (request, set) = save(&mut app, "qsave", "provider", "Main.dxf")?;
    assert_bytes(&set, &fixture::input(10)?)?;
    assert!(app.file_providers.result(request.clone(), "saved".into(), "main".into()).is_err());
    assert!(
        app.file_providers
            .save_resource_set_result("stale", request.clone(), "saved".into(), "main".into(), set.digest().into(), "version-2".into())
            .is_err()
    );
    assert!(
        app.file_providers
            .save_resource_set_result(&c, request.clone(), "saved".into(), "main".into(), "bad-digest".into(), "version-2".into())
            .is_err()
    );
    app.run("line", json!({"points":[[20,20],[30,30]]}))?;
    app.run("new", json!({}))?;
    let other = app.session.state().map_err(|e| e.to_string())?.uid;
    app.file_providers.save_resource_set_result(&c, request.clone(), "saved".into(), "main".into(), set.digest().into(), "version-2".into())?;
    assert!(app.file_providers.disconnect().is_err());
    tick(&mut app);
    assert_eq!(app.session.state().map_err(|e| e.to_string())?.uid, other);
    let doc = state(&app, uid)?;
    assert!(doc.is_dirty() && Arc::ptr_eq(&doc.saved, &captured));
    assert_eq!(resources(&app, uid)?.version, "version-2");
    let terminal = event(&app.file_providers, "saved")?;
    assert_eq!(text(&terminal.metadata, "setDigest")?, set.digest());
    assert_eq!(text(&terminal.metadata, "versionToken")?, "version-2");
    assert!(
        app.file_providers.save_resource_set_result(&c, request, "saved".into(), "main".into(), set.digest().into(), "version-2".into()).is_err()
    );
    assert!(!state(&app, other)?.is_dirty());
    Ok(())
}

fn refresh(app: &mut CadApp, c: &str, color: u8) -> Result<(), String> {
    let request = text(&app.run("ui.provider.refreshResources", json!({}))?, "requestId")?;
    let needed = event(&app.file_providers, "resourceSetNeeded")?;
    let Some(LinkedPayload::Main { key, bytes }) = needed.linked else { return Err("refresh omitted frozen native bytes".into()) };
    let mut input = fixture::input(color)?;
    input.main_key = key;
    input.name = text(&needed.metadata, "filename")?;
    input.main_hash = bytes.hash().into();
    input.main = None;
    app.file_providers.complete_resource_set(c, request.clone(), input)?;
    tick(app);
    assert_eq!(text(&event(&app.file_providers, "resourcesUpdated")?.metadata, "requestId")?, request);
    Ok(())
}
#[test]
fn linked_resource_generations_are_undoable_and_old_ack_cannot_clean_new_bytes() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    let before = resources(&app, uid)?;
    let (request, captured) = save(&mut app, "qsave", "provider", "Main.dxf")?;
    refresh(&mut app, &c, 99)?;
    let replacement = resources(&app, uid)?;
    assert_ne!(replacement.digest(), before.digest());
    assert!(state(&app, uid)?.is_dirty());
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "main".into(), captured.digest().into(), "version-2".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert_eq!(resources(&app, uid)?.digest(), replacement.digest());
    assert!(state(&app, uid)?.is_dirty());
    app.run("u", json!({}))?;
    assert_eq!(resources(&app, uid)?.digest(), before.digest());
    app.run("redo", json!({}))?;
    assert_eq!(resources(&app, uid)?.digest(), replacement.digest());
    Ok(())
}

#[test]
fn linked_device_export_exposes_every_attachment_without_durable_ack() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    app.run("line", json!({"points":[[0,0],[1,1]]}))?;
    let before = state(&app, uid)?.saved.clone();
    let (cancelled, _) = save(&mut app, "saveas", "device", "Cancelled.dxf")?;
    app.file_providers.cancel_resource_set(&c, cancelled.clone(), String::new())?;
    tick(&mut app);
    assert_eq!(text(&event(&app.file_providers, "saveCancelled")?.metadata, "requestId")?, cancelled);
    assert!(state(&app, uid)?.is_dirty() && Arc::ptr_eq(&state(&app, uid)?.saved, &before));
    let (request, set) = save(&mut app, "saveas", "device", "Portable.dxf")?;
    assert_bytes(&set, &fixture::input(10)?)?;
    assert_eq!(set.name, "Portable.dxf");
    app.file_providers.save_resource_set_result(&c, request, "download".into(), String::new(), set.digest().into(), String::new())?;
    tick(&mut app);
    event(&app.file_providers, "downloaded")?;
    assert!(state(&app, uid)?.is_dirty() && Arc::ptr_eq(&state(&app, uid)?.saved, &before));
    assert_eq!(app.file_providers.bound_document("main"), Some(uid));
    Ok(())
}

fn refused_session(preexisting: bool, case: &str) -> Result<(CadApp, String), String> {
    if preexisting {
        let (mut app, c, _) = opened()?;
        app.run("line", json!({"points":[[2,3],[17,19]]}))?;
        assert!(app.session.state().map_err(|e| e.to_string())?.is_dirty(), "{case}: edited fixture must be dirty");
        Ok((app, c))
    } else {
        let pair = connected()?;
        assert!(pair.0.session.docs.is_empty(), "{case}: empty fixture must start empty");
        Ok(pair)
    }
}

fn refused_request(host: &Host, resource: &str, case: &str) -> Result<String, String> {
    assert!(!host.busy(), "{case}: fixture must start idle");
    assert!(host.take_events().is_empty(), "{case}: unexpected setup events");
    let request = host.request_open(resource)?;
    let requested = host.take_events();
    assert_eq!(requested.len(), 1, "{case}: exactly one open request");
    let requested = requested.first().ok_or("open request missing")?;
    assert_eq!(requested.metadata["type"], "openRequested", "{case}");
    assert_eq!(text(&requested.metadata, "requestId")?, request, "{case}");
    assert_eq!(text(&requested.metadata, "resourceId")?, resource, "{case}");
    Ok(request)
}

fn assert_refused_document(doc: &DocState, previous: &DocState, native: &[u8], case: &str) -> Result<(), String> {
    assert_eq!(
        (doc.uid, doc.revision, &doc.title, &doc.path),
        (previous.uid, previous.revision, &previous.title, &previous.path),
        "{case}: document identity changed"
    );
    assert!(Arc::ptr_eq(&doc.doc, &previous.doc), "{case}: drawing changed");
    assert_eq!(cadcraft_io::write(&doc.doc, "Main.dxf").map_err(|e| e.to_string())?.as_slice(), native, "{case}: native content changed");
    assert!(Arc::ptr_eq(&doc.saved, &previous.saved), "{case}: saved checkpoint changed");
    assert_eq!(doc.is_dirty(), previous.is_dirty(), "{case}: dirty state changed");
    assert_eq!((doc.undo.len(), doc.redo.len()), (previous.undo.len(), previous.redo.len()), "{case}: history changed");
    for (actual, expected) in [(&doc.resources, &previous.resources), (&doc.saved_resources, &previous.saved_resources)] {
        match (actual, expected) {
            (Some(actual), Some(expected)) => assert!(Arc::ptr_eq(actual, expected), "{case}: resource/checkpoint changed"),
            (None, None) => {}
            _ => return Err(format!("{case}: resource/checkpoint presence changed")),
        }
    }
    Ok(())
}

fn assert_refused_terminal(host: &Host, request: &str, resource: &str, case: &str) -> Result<(), String> {
    let events = host.take_events();
    assert!(!events.iter().any(|event| event.metadata["type"] == "opened"), "{case}: invalid closure published opened");
    assert_eq!(events.len(), 1, "{case}: exactly one terminal failure expected");
    let failed = events.first().ok_or("terminal failure missing")?;
    assert_eq!(failed.metadata["type"], "openFailed", "{case}");
    assert_eq!(text(&failed.metadata, "requestId")?, request, "{case}");
    assert_eq!(text(&failed.metadata, "resourceId")?, resource, "{case}");
    let error = text(&failed.metadata, "error")?;
    assert!(!error.is_empty(), "{case}: failure must explain rejection");
    if case == "owned unsupported XRECORD" {
        assert_eq!(error, "unclassified foreign DXF object in linked set", "{case}: retained object must reach native foreign policy");
    }
    assert!(!host.busy(), "{case}: rejection left provider busy");
    Ok(())
}

fn refused(input: Input, case: &str) -> Result<(), String> {
    for preexisting in [false, true] {
        let (mut app, c) = refused_session(preexisting, case)?;
        let before = app.session.docs.clone();
        let native_before =
            before.iter().map(|doc| cadcraft_io::write(&doc.doc, "Main.dxf").map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
        let active = app.session.active;
        let active_uid = app.session.docs.get(active).map(|doc| doc.uid);
        let epoch = app.session.doc_epoch;
        let recent = app.ui.recent_files.clone();
        let main_binding = app.file_providers.bound_document("main");
        let copy_binding = app.file_providers.bound_document("copy");
        let resource = if preexisting { "copy" } else { "main" };
        let request = refused_request(&app.file_providers, resource, case)?;
        app.file_providers.complete_resource_set(&c, request.clone(), input.clone())?;
        assert_eq!(app.session.docs.len(), before.len(), "{case}: queueing must not mutate session");
        tick(&mut app);
        if !preexisting {
            assert!(app.session.docs.is_empty(), "{case}: invalid closure must not mutate empty session");
        }
        assert_eq!(app.session.docs.len(), before.len(), "{case}: document count changed");
        assert_eq!(app.session.active, active, "{case}: active index changed");
        assert_eq!(app.session.docs.get(active).map(|doc| doc.uid), active_uid, "{case}: active UID changed");
        assert_eq!(app.session.doc_epoch, epoch, "{case}: document epoch changed");
        assert_eq!(app.ui.recent_files, recent, "{case}: recents changed");
        assert_eq!(app.file_providers.bound_document("main"), main_binding, "{case}: main owner changed");
        assert_eq!(app.file_providers.bound_document("copy"), copy_binding, "{case}: copy owner changed");
        for ((doc, previous), native) in app.session.docs.iter().zip(&before).zip(&native_before) {
            assert_refused_document(doc, previous, native, case)?;
        }
        assert_refused_terminal(&app.file_providers, &request, resource, case)?;
    }
    Ok(())
}
#[test]
fn linked_native_closure_rejects_missing_unreachable_and_unclassified_resources() -> Result<(), String> {
    let mut missing = fixture::input(10)?;
    missing.files.retain(|f| f.key != "parts/nested.dxf");
    refused(missing, "missing nested resource")?;
    let mut escaping = fixture::input(10)?;
    escaping.edges.first_mut().ok_or("edge missing")?.target_key = "../unauthorized.png".into();
    refused(escaping, "escaping edge")?;
    let mut wrong_kind = fixture::input(10)?;
    wrong_kind.files.iter_mut().find(|f| f.key == "images/main.png").ok_or("image missing")?.kind = "font".into();
    refused(wrong_kind, "wrong resource kind")?;
    let mut extra = fixture::input(10)?;
    let mut file = extra.files.first().ok_or("file missing")?.clone();
    file.key = "unreachable.dxf".into();
    extra.files.push(file);
    refused(extra, "unreachable extra resource")?;
    let mut unknown = fixture::input(10)?;
    let main = unknown.main.as_ref().ok_or("main missing")?;
    let mut drawing = cadcraft_io::read(main.data(), "Main.dxf").map_err(|e| e.to_string())?;
    let handle = drawing.new_handle();
    let owner = drawing.new_handle();
    let sentinel = "unknown extension semantics";
    let foreign = Arc::make_mut(&mut drawing.foreign);
    assert!(foreign.targets.insert(owner, ("NOD".into(), String::new())).is_none());
    foreign.dictionary.push(("UnsupportedFixture".into(), handle));
    foreign.objects.push(cadcraft_engine::doc::foreign::ForeignObject {
        handle,
        kind: "XRECORD".into(),
        tags: vec![
            cadcraft_engine::doc::RawTag { code: 5, value: handle.hex() },
            cadcraft_engine::doc::RawTag { code: 330, value: owner.hex() },
            cadcraft_engine::doc::RawTag { code: 100, value: "AcDbXrecord".into() },
            cadcraft_engine::doc::RawTag { code: 280, value: "1".into() },
            cadcraft_engine::doc::RawTag { code: 1, value: sentinel.into() },
        ],
    });
    // Validate the exact bytes delivered below: the native writer must not sanitize the negative fixture.
    let bytes = Bytes::new(cadcraft_io::write(&drawing, "Main.dxf").map_err(|e| e.to_string())?)?;
    let roundtrip = cadcraft_io::read(bytes.data(), "Main.dxf").map_err(|e| e.to_string())?;
    let retained = &roundtrip.foreign;
    let entries = retained.dictionary.iter().filter(|(name, _)| name == "UnsupportedFixture").collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "unsupported NOD entry must survive exact native roundtrip");
    let retained_handle = entries.first().ok_or("unsupported NOD entry dropped")?.1;
    let object = retained.objects.iter().find(|object| object.handle == retained_handle).ok_or("owned XRECORD dropped by native roundtrip")?;
    assert_eq!(object.kind, "XRECORD");
    for (code, value) in [(5, object.handle.hex()), (100, "AcDbXrecord".into()), (280, "1".into()), (1, sentinel.into())] {
        assert!(object.tags.iter().any(|tag| tag.code == code && tag.value == value), "retained XRECORD tag {code} missing");
    }
    let owner_tag = object.tags.iter().find(|tag| tag.code == 330).ok_or("retained XRECORD owner missing")?;
    assert!(
        retained.targets.iter().any(|(handle, role)| handle.hex() == owner_tag.value && role.0 == "NOD" && role.1.is_empty()),
        "retained XRECORD must remain owned by NOD"
    );
    unknown.main_hash = bytes.hash().into();
    unknown.main = Some(bytes);
    refused(unknown, "owned unsupported XRECORD")?;
    Ok(())
}

#[test]
fn linked_delivery_failure_reaches_actual_consumer_and_reentrant_ack_wins() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    app.run("line", json!({"points":[[0,0],[2,2]]}))?;
    let request = text(&app.run("qsave", Value::Null)?, "requestId")?;
    event(&app.file_providers, "saveDialog")?;
    app.file_providers.resolve_dialog(&request, "provider", "Main.dxf")?;
    let host = app.file_providers.clone();
    assert!(host.publish_linked_events_with(|_, _, _| Err("real dispatcher error"), |_| {}));
    assert!(host.pending_for(uid) && host.disconnect().is_err());
    tick(&mut app);
    assert_eq!(text(&event(&host, "saveFailed")?.metadata, "requestId")?, request);
    assert!(state(&app, uid)?.is_dirty());
    let retry = text(&app.run("qsave", Value::Null)?, "requestId")?;
    event(&host, "saveDialog")?;
    host.resolve_dialog(&retry, "provider", "Main.dxf")?;
    let mut accepted = false;
    host.publish_linked_events_with(
        |metadata, _, payload| {
            if metadata["type"] == "save" {
                let Some(LinkedPayload::Set(set)) = payload else { return Err("missing set".to_string()) };
                host.save_resource_set_result(&c, retry.clone(), "saved".into(), "main".into(), set.digest().into(), "version-2".into())?;
                accepted = true;
                return Err("dispatch error after accepted synchronous receipt".into());
            }
            Ok(())
        },
        |_| {},
    );
    assert!(accepted && host.pending_for(uid));
    tick(&mut app);
    event(&host, "saved")?;
    assert!(!state(&app, uid)?.is_dirty() && !host.busy());
    Ok(())
}

#[test]
fn linked_connection_cancel_and_declared_limits_preserve_owners() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    let request = text(&app.run("qsave", Value::Null)?, "requestId")?;
    event(&app.file_providers, "saveDialog")?;
    app.file_providers.resolve_dialog(&request, "cancel", "")?;
    assert!(app.file_providers.disconnect().is_err());
    tick(&mut app);
    event(&app.file_providers, "saveCancelled")?;
    app.file_providers.disconnect()?;
    let new_c = app.file_providers.connect("fixture".into(), "Linked fixture".into(), "account-two".into())?;
    app.file_providers.configure_linked_files(&new_c, 1)?;
    assert_ne!(c, new_c);
    assert!(app.run("qsave", Value::Null).is_err(), "cross-account export requires explicit Save As");
    assert!(app.file_providers.complete_resource_set(&c, request, fixture::input(10)?).is_err());
    let save_as = text(&app.run("saveas", Value::Null)?, "requestId")?;
    event(&app.file_providers, "saveDialog")?;
    app.file_providers.resolve_dialog(&save_as, "cancel", "")?;
    tick(&mut app);
    event(&app.file_providers, "saveCancelled")?;
    assert_eq!(resources(&app, uid)?.authority.as_ref(), Some(&("fixture".into(), "account-one".into())));
    assert!(!cadcraft_engine::linked_files::logical_key("../escape"));
    assert!(!cadcraft_engine::linked_files::logical_key(&"x/".repeat(33)));
    let input = fixture::input(10)?;
    let same = Bytes::new(input.files.first().ok_or("file missing")?.bytes.data().to_vec())?;
    assert!(Arc::ptr_eq(&same, &input.files.first().ok_or("file missing")?.bytes), "undo/capture bytes are interned once");
    Ok(())
}
#[test]
fn linked_absolute_mappings_and_xopen_never_follow_native_paths() -> Result<(), String> {
    let (mut app, c) = connected()?;
    let mut input = fixture::input(10)?;
    let mut drawing = cadcraft_io::read(input.main.as_ref().ok_or("main missing")?.data(), "Main.dxf").map_err(|e| e.to_string())?;
    let image_path = "C:\\not-authority\\image.png";
    for handle in drawing.model.handles() {
        assert!(drawing.model.modify(handle, |entity| {
            if let cadcraft_engine::doc::EntityKind::Image(image) = &mut entity.kind {
                image.path = image_path.into();
            }
        }));
    }
    let child_path = "C:\\not-authority\\child.dxf";
    let child = drawing.blocks.get_mut("Child").ok_or("child missing")?;
    Arc::make_mut(child).xref_path = Some(child_path.into());
    // The layout/block still reference their relative image path; add a distinct explicit edge.
    input.edges.push(cadcraft_engine::linked_files::Edge {
        from_key: input.main_key.clone(),
        raw_reference: image_path.into(),
        role: "image".into(),
        target_key: "images/main.png".into(),
    });
    input.edges.iter_mut().find(|edge| edge.role == "xref" && edge.target_key == "parts/child.dxf").ok_or("xref edge missing")?.raw_reference =
        child_path.into();
    let main = Bytes::new(cadcraft_io::write(&drawing, "Main.dxf").map_err(|e| e.to_string())?)?;
    input.main_hash = main.hash().into();
    input.main = Some(main);
    let expected = input.files.iter().find(|file| file.key == "parts/child.dxf").ok_or("child bytes missing")?.bytes.clone();
    let uid = open_input(&mut app, &c, input)?;
    assert!(state(&app, uid)?.path.is_none());
    let list = app.run("-xref", json!({"option":"?"}))?;
    assert!(list["xrefs"].as_array().ok_or("list missing")?.iter().all(|row| row["foundAt"].is_null()));
    assert!(app.run("-xref", json!({"option":"reload"})).is_err());
    app.run("xopen", json!({"name":"Child"}))?;
    let sub = app.session.state().map_err(|e| e.to_string())?;
    assert_ne!(sub.uid, uid);
    assert!(sub.path.is_none());
    assert_eq!(sub.resources.as_ref().ok_or("xref context missing")?.main.bytes.data(), expected.data());
    Ok(())
}

fn depth_chain(count: usize) -> Result<Input, String> {
    let mut main = None;
    let mut files = Vec::new();
    let mut edges = Vec::new();
    for index in 0..count {
        let key = format!("drawings/{index}.dxf");
        let mut drawing = cadcraft_engine::doc::Drawing::new_imperial();
        if index + 1 < count {
            let raw = format!("{}.dxf", index + 1);
            let mut block = cadcraft_engine::doc::Block::new("Next");
            block.xref_path = Some(raw.clone());
            drawing.blocks.insert("Next".into(), Arc::new(block));
            edges.push(cadcraft_engine::linked_files::Edge {
                from_key: key.clone(),
                raw_reference: raw,
                role: "xref".into(),
                target_key: format!("drawings/{}.dxf", index + 1),
            });
        }
        let bytes = Bytes::new(cadcraft_io::write(&drawing, "Chain.dxf").map_err(|e| e.to_string())?)?;
        if index == 0 {
            main = Some(bytes);
        } else {
            files.push(cadcraft_engine::linked_files::File { key, kind: "dxf".into(), bytes });
        }
    }
    let main = main.ok_or("chain must contain a drawing")?;
    Ok(Input {
        main_key: "drawings/0.dxf".into(),
        name: "Chain.dxf".into(),
        main_hash: main.hash().into(),
        main: Some(main),
        files,
        edges,
        version: "v1".into(),
    })
}
#[test]
fn linked_declared_count_and_real_native_depth_fail_without_partial_open() -> Result<(), String> {
    refused(depth_chain(cadcraft_engine::doc::MAX_BLOCK_DEPTH + 2)?, "native depth overflow")?;
    let (mut app, c) = connected()?;
    let request = app.file_providers.request_open("main")?;
    event(&app.file_providers, "openRequested")?;
    let mut input = fixture::input(10)?;
    let file = input.files.first().ok_or("fixture file missing")?.clone();
    input.files.resize(cadcraft_engine::linked_files::MAX_FILES, file);
    assert!(app.file_providers.complete_resource_set(&c, request.clone(), input).is_err());
    assert!(app.session.docs.is_empty() && app.file_providers.busy());
    app.file_providers.cancel_resource_set(&c, request.clone(), String::new())?;
    assert_eq!(text(&event(&app.file_providers, "openCancelled")?.metadata, "requestId")?, request);
    tick(&mut app);
    assert!(!app.file_providers.busy());
    Ok(())
}

#[test]
fn linked_failure_inbox_survives_full_actions_and_accepted_set_reply_wins() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    let request = text(&app.run("qsave", Value::Null)?, "requestId")?;
    event(&app.file_providers, "saveDialog")?;
    app.file_providers.resolve_dialog(&request, "provider", "Main.dxf")?;
    let host = app.file_providers.clone();
    for _ in 0..16 {
        let read = host.request_open("main")?;
        host.complete_resource_set(&c, read, fixture::input(10)?)?;
    }
    assert!(host.publish_linked_events_with(|_, _, _| Err("delivery failure with full ordinary inbox"), |_| {}));
    assert!(host.pending_for(uid));
    tick(&mut app);
    let terminal = event(&host, "saveFailed")?;
    assert_eq!(text(&terminal.metadata, "requestId")?, request);
    assert!(!host.busy());

    let request = text(&app.run("ui.provider.refreshResources", json!({}))?, "requestId")?;
    let mut replied = false;
    host.publish_linked_events_with(
        |metadata, _, payload| {
            if metadata["type"] == "resourceSetNeeded" {
                let Some(LinkedPayload::Main { key, bytes }) = payload else { return Err("missing frozen native bytes".to_string()) };
                let mut input = fixture::input(77)?;
                input.main_key = key;
                input.name = text(metadata, "filename")?;
                input.main_hash = bytes.hash().into();
                input.main = None;
                host.complete_resource_set(&c, request.clone(), input)?;
                replied = true;
                return Err("dispatch failed after accepted complete set callback".into());
            }
            Ok(())
        },
        |_| {},
    );
    assert!(replied);
    tick(&mut app);
    assert_eq!(text(&event(&host, "resourcesUpdated")?.metadata, "requestId")?, request);
    let image = resources(&app, uid)?;
    let image = image.file("images/main.png").ok_or("replacement image lost")?;
    let decoded = image::load_from_memory(image.bytes.data()).map_err(|e| e.to_string())?.to_rgba8();
    assert_eq!(decoded.get_pixel(0, 0).0, [77, 64, 128, 255]);
    Ok(())
}
#[test]
fn linked_first_save_ingress_supplies_real_missing_bytes_and_keeps_main_native() -> Result<(), String> {
    let (mut app, c) = connected()?;
    let mut input = fixture::input(10)?;
    let native = input.main.as_ref().ok_or("main missing")?;
    let drawing = cadcraft_io::read(native.data(), "Main.dxf").map_err(|e| e.to_string())?;
    app.session.open_drawing(drawing, "Main.dxf", None);
    let uid = app.session.state().map_err(|e| e.to_string())?.uid;
    let request = text(&app.run("qsave", Value::Null)?, "requestId")?;
    event(&app.file_providers, "saveDialog")?;
    app.file_providers.resolve_dialog(&request, "provider", "Main.dxf")?;
    let needed = event(&app.file_providers, "resourceSetNeeded")?;
    assert_eq!(text(&needed.metadata, "operation")?, "save");
    assert!(text(&needed.metadata, "mainLogicalKey")?.is_empty(), "first logical root requires explicit host authorization");
    let Some(LinkedPayload::Main { bytes, .. }) = needed.linked else { return Err("missing actual captured native bytes".into()) };
    assert!(cadcraft_io::read(bytes.data(), "Main.dxf").is_ok());
    input.main = None;
    input.main_hash = bytes.hash().into();
    app.file_providers.complete_resource_set(&c, request.clone(), input.clone())?;
    tick(&mut app);
    let transfer = event(&app.file_providers, "save")?;
    let Some(LinkedPayload::Set(set)) = transfer.linked else { return Err("complete set was not published".into()) };
    assert_bytes(&set, &input)?;
    assert_eq!(set.main.bytes.data(), bytes.data());
    assert_eq!(set.main.key, "plans/main.dxf");
    assert!(app.file_providers.complete_resource_set(&c, request.clone(), input).is_err(), "one capture cannot be supplied twice");
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "created".into(), set.digest().into(), "created-v1".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert_eq!(app.file_providers.bound_document("created"), Some(uid));
    assert_eq!(resources(&app, uid)?.main.key, "plans/main.dxf");
    assert!(!state(&app, uid)?.is_dirty());
    Ok(())
}

#[test]
fn linked_full_set_receipt_cannot_steal_another_open_resource_alias() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    let read = app.file_providers.request_open("copy")?;
    event(&app.file_providers, "openRequested")?;
    app.file_providers.complete_resource_set(&c, read, fixture::input(10)?)?;
    tick(&mut app);
    event(&app.file_providers, "opened")?;
    let second = app.session.state().map_err(|e| e.to_string())?.uid;
    assert_ne!(uid, second);
    let first_index = app.session.docs.iter().position(|doc| doc.uid == uid).ok_or("original owner lost")?;
    app.run("document.switch", json!({"index":first_index}))?;
    let (request, set) = save(&mut app, "saveas", "provider", "Copy.dxf")?;
    assert!(
        app.file_providers
            .save_resource_set_result(&c, request.clone(), "saved".into(), "copy".into(), set.digest().into(), "new-version".into())
            .is_err()
    );
    assert!(app.file_providers.pending_for(uid));
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "third".into(), set.digest().into(), "new-version".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert_eq!(app.file_providers.bound_document("copy"), Some(second));
    assert_eq!(app.file_providers.bound_document("third"), Some(uid));
    Ok(())
}
#[test]
fn linked_saved_set_reopens_with_captured_native_dxf_and_complete_originals() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    app.run("line", json!({"points":[[3,4],[5,6]]}))?;
    let count = state(&app, uid)?.doc.model.len();
    let (request, set) = save(&mut app, "qsave", "provider", "Main.dxf")?;
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "main".into(), set.digest().into(), "version-2".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    let committed = resources(&app, uid)?;
    let input = Input {
        main_key: committed.main.key.clone(),
        name: committed.name.clone(),
        main: Some(committed.main.bytes.clone()),
        main_hash: committed.main.bytes.hash().into(),
        files: committed.files.clone(),
        edges: committed.edges.clone(),
        version: committed.version.clone(),
    };
    let (mut reopened, connection) = connected()?;
    let restored = open_input(&mut reopened, &connection, input)?;
    let restored = state(&reopened, restored)?;
    assert_eq!(restored.doc.model.len(), count);
    assert!(!restored.is_dirty());
    assert_eq!(restored.doc.blocks.get("Child").and_then(|block| block.xref_path.as_deref()), Some("../parts/child.dxf"));
    let restored = restored.resources.as_ref().ok_or("restored bytes missing")?;
    assert_eq!(restored.digest(), committed.digest());
    assert_bytes(restored, &fixture::input(10)?)?;
    Ok(())
}
#[test]
fn linked_refresh_refuses_real_drafts_and_block_editor_without_losing_checkpoints() -> Result<(), String> {
    let (mut app, c, uid) = opened()?;
    let before = resources(&app, uid)?;
    app.start("line");
    assert!(app.session.prompting());
    assert!(app.run("ui.provider.refreshResources", json!({})).is_err());
    assert!(Arc::ptr_eq(&resources(&app, uid)?, &before));
    assert!(app.file_providers.take_events().is_empty());
    app.session.cancel();
    app.run("bedit", json!({"name":"Picture"}))?;
    assert!(state(&app, uid)?.block_edit.is_some() && !state(&app, uid)?.block_unsaved());
    assert!(app.run("ui.provider.refreshResources", json!({})).is_err());
    assert!(Arc::ptr_eq(&resources(&app, uid)?, &before));
    let (request, set) = save(&mut app, "qsave", "provider", "Main.dxf")?;
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "main".into(), set.digest().into(), "version-2".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    let editor = state(&app, uid)?.block_edit.as_ref().ok_or("editor disappeared")?;
    assert_eq!(editor.host.resources.as_ref().ok_or("host context lost")?.version, "version-2");
    app.run("bclose", json!({"save":false}))?;
    assert_eq!(resources(&app, uid)?.version, "version-2");
    assert_eq!(state(&app, uid)?.saved_resources.as_ref().ok_or("checkpoint lost")?.version, "version-2");
    assert!(!state(&app, uid)?.is_dirty());
    Ok(())
}
#[test]
fn linked_raw_font_files_and_unnamed_shape_styles_require_authorized_bytes() -> Result<(), String> {
    let mut input = fixture::input(10)?;
    let mut drawing = cadcraft_io::read(input.main.as_ref().ok_or("main missing")?.data(), "Main.dxf").map_err(|e| e.to_string())?;
    drawing.text_styles.first_mut().ok_or("style missing")?.font = "DejaVu Sans".into();
    drawing.portable_fonts = true;
    let mut shape = cadcraft_engine::doc::DashElement::dash(1.0);
    shape.shape = Some(1);
    shape.style = Some("../fonts/original.shx".into());
    drawing.linetypes.push(cadcraft_engine::doc::Linetype {
        name: "OriginalShape".into(),
        description: "synthetic shape font reference".into(),
        pattern: vec![shape],
    });
    let main = Bytes::new(cadcraft_io::write(&drawing, "Main.dxf").map_err(|e| e.to_string())?)?;
    let decoded = cadcraft_io::read(main.data(), "Main.dxf").map_err(|e| e.to_string())?;
    assert_eq!(decoded.text_styles.first().ok_or("style missing")?.font, "DejaVu Sans");
    input.main_hash = main.hash().into();
    input.main = Some(main);
    let shape_edge = cadcraft_engine::linked_files::Edge {
        from_key: input.main_key.clone(),
        raw_reference: "../fonts/original.shx".into(),
        role: "font".into(),
        target_key: "fonts/original.shx".into(),
    };
    input.edges.push(shape_edge.clone());
    refused(input.clone(), "missing raw font file")?;
    let bytes = Bytes::new(b"original synthetic authorized font-file bytes, no font rendering claim".to_vec())?;
    input.files.push(cadcraft_engine::linked_files::File { key: "plans/verdana.ttf".into(), kind: "font".into(), bytes: bytes.clone() });
    input.edges.push(cadcraft_engine::linked_files::Edge {
        from_key: input.main_key.clone(),
        raw_reference: "verdana.ttf".into(),
        role: "font".into(),
        target_key: "plans/verdana.ttf".into(),
    });
    let mut missing_shape = input.clone();
    missing_shape.edges.retain(|edge| edge != &shape_edge);
    refused(missing_shape, "missing shape-style edge")?;
    let (mut app, c) = connected()?;
    let uid = open_input(&mut app, &c, input)?;
    let set = resources(&app, uid)?;
    assert_eq!(set.file("plans/verdana.ttf").ok_or("raw font bytes lost")?.bytes.data(), bytes.data());
    assert_eq!(state(&app, uid)?.doc.text_styles.first().ok_or("style missing")?.font, "DejaVu Sans");
    Ok(())
}

fn normalized_font_input() -> Result<Input, String> {
    let mut input = fixture::input(10)?;
    let mut drawing = cadcraft_io::read(input.main.as_ref().ok_or("main missing")?.data(), "Main.dxf").map_err(|e| e.to_string())?;
    drawing.text_styles.first_mut().ok_or("style missing")?.font = "DejaVu Sans".into();
    drawing.portable_fonts = true;
    let emitted = String::from_utf8(cadcraft_io::write(&drawing, "Main.dxf").map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let marker = "  3\r\nverdana.ttf\r\n";
    assert_eq!(emitted.matches(marker).count(), 1, "the real writer must supply the portable STYLE filename and family metadata");
    let native = emitted.replacen(marker, "  3\r\n../fuentes/versión/verdana.ttf\r\n", 1);
    let main = Bytes::new(native.into_bytes())?;
    input.main_hash = main.hash().into();
    input.main = Some(main);
    input.files.push(cadcraft_engine::linked_files::File {
        key: "fuentes/versión/verdana.ttf".into(),
        kind: "font".into(),
        bytes: Bytes::new(b"contributor-original authorized normalized font bytes".to_vec())?,
    });
    input.edges.push(cadcraft_engine::linked_files::Edge {
        from_key: input.main_key.clone(),
        raw_reference: "../fuentes/versión/verdana.ttf".into(),
        role: "font".into(),
        target_key: "fuentes/versión/verdana.ttf".into(),
    });
    Ok(input)
}

#[test]
fn linked_normalized_font_paths_and_bytes_survive_save_native_decode_and_reopen() -> Result<(), String> {
    let input = normalized_font_input()?;
    let font = input.files.iter().find(|file| file.key == "fuentes/versión/verdana.ttf").ok_or("font missing")?.bytes.clone();
    let (mut app, c) = connected()?;
    let uid = open_input(&mut app, &c, input)?;
    assert_eq!(state(&app, uid)?.doc.text_styles.first().ok_or("style missing")?.font, "DejaVu Sans");
    app.run("line", json!({"points":[[0,0],[4,4]]}))?;
    let (request, set) = save(&mut app, "qsave", "provider", "Main.dxf")?;
    assert_eq!(set.file("fuentes/versión/verdana.ttf").ok_or("normalized font bytes were dropped")?.bytes.data(), font.data());
    assert!(set.edges.iter().any(|edge| edge.role == "font" && edge.raw_reference == "../fuentes/versión/verdana.ttf"));
    let decoded = cadcraft_io::read(set.main.bytes.data(), "Main.dxf").map_err(|e| e.to_string())?;
    let style = decoded.text_styles.first().ok_or("saved style missing")?;
    assert_eq!(style.font, "DejaVu Sans");
    assert_eq!(style.source_font_file.as_ref(), Some(&("DejaVu Sans".into(), "../fuentes/versión/verdana.ttf".into())));
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "main".into(), set.digest().into(), "font-v2".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert!(!state(&app, uid)?.is_dirty());
    let set = resources(&app, uid)?;
    let reopen = Input {
        main_key: set.main.key.clone(),
        name: set.name.clone(),
        main: Some(set.main.bytes.clone()),
        main_hash: set.main.bytes.hash().into(),
        files: set.files.clone(),
        edges: set.edges.clone(),
        version: set.version.clone(),
    };
    let (mut other, connection) = connected()?;
    let restored = open_input(&mut other, &connection, reopen)?;
    assert_eq!(resources(&other, restored)?.file("fuentes/versión/verdana.ttf").ok_or("reopened font missing")?.bytes.data(), font.data());
    assert_eq!(state(&other, restored)?.doc.text_styles.first().ok_or("reopened style missing")?.font, "DejaVu Sans");
    Ok(())
}

#[test]
fn linked_first_provider_capture_retains_codec_font_source_before_a_resource_set_exists() -> Result<(), String> {
    let mut input = normalized_font_input()?;
    let drawing = cadcraft_io::read(input.main.as_ref().ok_or("native missing")?.data(), "Main.dxf").map_err(|e| e.to_string())?;
    let (mut app, c) = connected()?;
    app.session.open_drawing(drawing, "Main.dxf", None);
    let request = text(&app.run("qsave", Value::Null)?, "requestId")?;
    event(&app.file_providers, "saveDialog")?;
    app.file_providers.resolve_dialog(&request, "provider", "Main.dxf")?;
    let needed = event(&app.file_providers, "resourceSetNeeded")?;
    let Some(LinkedPayload::Main { bytes, .. }) = needed.linked else { return Err("captured native missing".into()) };
    assert!(cadcraft_engine::linked_files::native_font_references(bytes.data())?.contains(&("../fuentes/versión/verdana.ttf".into(), "font".into())));
    input.main = None;
    input.main_hash = bytes.hash().into();
    app.file_providers.complete_resource_set(&c, request.clone(), input)?;
    tick(&mut app);
    let transfer = event(&app.file_providers, "save")?;
    let Some(LinkedPayload::Set(set)) = transfer.linked else { return Err("complete set missing".into()) };
    assert!(set.file("fuentes/versión/verdana.ttf").is_some());
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "first-font".into(), set.digest().into(), "v1".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert!(!app.session.state().map_err(|e| e.to_string())?.is_dirty());
    Ok(())
}

#[test]
fn linked_legitimate_font_edit_replaces_the_old_dependency_without_forcing_portable_fonts() -> Result<(), String> {
    let (mut app, c) = connected()?;
    let uid = open_input(&mut app, &c, normalized_font_input()?)?;
    app.run("style", json!({"name":"Standard","font":"../fonts/replacement.shx"}))?;
    let request = text(&app.run("qsave", Value::Null)?, "requestId")?;
    event(&app.file_providers, "saveDialog")?;
    app.file_providers.resolve_dialog(&request, "provider", "Main.dxf")?;
    let needed = event(&app.file_providers, "resourceSetNeeded")?;
    let Some(LinkedPayload::Main { key, bytes }) = needed.linked else { return Err("new font capture missing".into()) };
    let mut input = normalized_font_input()?;
    input.main = None;
    input.main_hash = bytes.hash().into();
    input.main_key = key;
    input.files.retain(|file| file.key != "fuentes/versión/verdana.ttf");
    input.edges.retain(|edge| edge.raw_reference != "../fuentes/versión/verdana.ttf");
    input.files.push(cadcraft_engine::linked_files::File {
        key: "fonts/replacement.shx".into(),
        kind: "font".into(),
        bytes: Bytes::new(b"original replacement font bytes".to_vec())?,
    });
    input.edges.push(cadcraft_engine::linked_files::Edge {
        from_key: input.main_key.clone(),
        raw_reference: "../fonts/replacement.shx".into(),
        role: "font".into(),
        target_key: "fonts/replacement.shx".into(),
    });
    app.file_providers.complete_resource_set(&c, request.clone(), input)?;
    tick(&mut app);
    let transfer = event(&app.file_providers, "save")?;
    let Some(LinkedPayload::Set(set)) = transfer.linked else { return Err("edited complete set missing".into()) };
    assert!(set.file("fuentes/versión/verdana.ttf").is_none());
    assert!(set.file("fonts/replacement.shx").is_some());
    let decoded = cadcraft_io::read(set.main.bytes.data(), "Main.dxf").map_err(|e| e.to_string())?;
    assert_eq!(decoded.text_styles.first().ok_or("edited style missing")?.font, "../fonts/replacement.shx");
    assert!(!decoded.portable_fonts);
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "main".into(), set.digest().into(), "edit-v2".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert!(!state(&app, uid)?.is_dirty());
    Ok(())
}

#[test]
fn native_font_source_metadata_is_inert_for_local_bytes_json_and_semantic_equality() -> Result<(), String> {
    let input = normalized_font_input()?;
    let drawing = cadcraft_io::read(input.main.as_ref().ok_or("native missing")?.data(), "Main.dxf").map_err(|e| e.to_string())?;
    let mut ordinary = drawing.clone();
    for style in &mut ordinary.text_styles {
        style.source_font_file = None;
    }
    assert_eq!(drawing, ordinary, "source provenance is not a change to ordinary model equality");
    assert_eq!(
        cadcraft_io::write(&drawing, "Local.dxf").map_err(|e| e.to_string())?,
        cadcraft_io::write(&ordinary, "Local.dxf").map_err(|e| e.to_string())?
    );
    let style = drawing.text_styles.first().ok_or("style missing")?;
    assert!(serde_json::to_value(style).map_err(|e| e.to_string())?.get("sourceFontFile").is_none());
    Ok(())
}

#[test]
fn linked_explicit_and_disabled_local_drawing_copies_disclose_omissions_and_keep_checkpoint() -> Result<(), String> {
    let (mut app, _c, uid) = opened()?;
    app.run("line", json!({"points":[[2,2],[8,8]]}))?;
    let saved = state(&app, uid)?.saved.clone();
    let saved_set = state(&app, uid)?.saved_resources.clone();
    let title = state(&app, uid)?.title.clone();
    let binary = state(&app, uid)?.binary_dxf;
    let version = state(&app, uid)?.doc.header.get("ACADVER").cloned();
    let root = std::env::temp_dir().join(format!("cad-linked-copy-{}-{uid}", std::process::id()));
    std::fs::create_dir(&root).map_err(|e| e.to_string())?;
    for enabled in [true, false] {
        app.file_providers.set_enabled(enabled)?;
        let output = root.join(if enabled { "Explicit.dxf" } else { "Disabled.dxf" });
        let filename = output.to_string_lossy().into_owned();
        let log_before = app.session.log.len();
        let reply = if enabled {
            app.run("saveas", json!({"path":filename,"binary":true,"version":"2000"}))?
        } else {
            let chosen = filename.clone();
            app.services.pick_save = Some(Box::new(move |_| Some(chosen.clone())));
            app.run("ui.saveas", Value::Null)?
        };
        assert_eq!(reply["linkedCopy"]["completeProject"], false);
        assert_eq!(reply["linkedCopy"]["checkpointAdvanced"], false);
        assert_eq!(reply["linkedCopy"]["filesOmitted"].as_array().ok_or("omission list missing")?.len(), 6);
        assert!(state(&app, uid)?.is_dirty());
        assert!(Arc::ptr_eq(&state(&app, uid)?.saved, &saved));
        assert_eq!(state(&app, uid)?.saved_resources.as_ref().map(|set| set.digest()), saved_set.as_ref().map(|set| set.digest()));
        assert!(state(&app, uid)?.path.is_none());
        assert_eq!(state(&app, uid)?.title, title);
        assert_eq!(state(&app, uid)?.binary_dxf, binary);
        assert_eq!(state(&app, uid)?.doc.header.get("ACADVER"), version.as_ref());
        assert!(app.session.log.iter().skip(log_before).any(|line| line.starts_with("Drawing-only copy: 6 cached linked files")));
        assert_eq!(app.file_providers.bound_document("main"), Some(uid));
        assert!(app.file_providers.take_events().is_empty());
        let decoded = cadcraft_io::read(&std::fs::read(&output).map_err(|e| e.to_string())?, "Copy.dxf").map_err(|e| e.to_string())?;
        assert_eq!(decoded.model.len(), state(&app, uid)?.doc.model.len());
        std::fs::remove_file(&output).map_err(|e| e.to_string())?;
    }
    let mut ordinary = fixture::app();
    ordinary.run("new", json!({}))?;
    ordinary.run("line", json!({"points":[[0,0],[1,1]]}))?;
    let local = root.join("Ordinary.dxf");
    let reply = ordinary.run("saveas", json!({"path":local.to_string_lossy()}))?;
    assert!(reply.get("linkedCopy").is_none());
    assert!(!ordinary.session.state().map_err(|e| e.to_string())?.is_dirty());
    std::fs::remove_file(local).map_err(|e| e.to_string())?;
    std::fs::remove_dir(root).map_err(|e| e.to_string())?;
    Ok(())
}

fn shared_depth_dag() -> Result<Input, String> {
    let mut rows = vec![("root.dxf".to_string(), vec!["a-shared.dxf".to_string(), "z01.dxf".to_string()])];
    for n in 1..=13 {
        rows.push((format!("z{n:02}.dxf"), vec![if n == 13 { "a-shared.dxf".into() } else { format!("z{:02}.dxf", n + 1) }]));
    }
    rows.push(("a-shared.dxf".into(), vec!["tail1.dxf".into()]));
    for n in 1..=4 {
        rows.push((format!("tail{n}.dxf"), if n == 4 { vec![] } else { vec![format!("tail{}.dxf", n + 1)] }));
    }
    depth_graph(rows, true)
}
fn depth_graph(rows: Vec<(String, Vec<String>)>, reverse_declarations: bool) -> Result<Input, String> {
    let mut main = None;
    let mut files = Vec::new();
    let mut edges = Vec::new();
    for (key, targets) in rows {
        let mut drawing = cadcraft_engine::doc::Drawing::new_imperial();
        for (n, to) in targets.iter().enumerate() {
            let mut block = cadcraft_engine::doc::Block::new(&format!("Ref{n}"));
            block.xref_path = Some(to.clone());
            drawing.blocks.insert(block.name.clone(), Arc::new(block));
        }
        // Put the root's short edge last for the old LIFO declaration traversal; the native
        // ordered reference traversal sees it first. Both used to hide the deeper tail.
        let mut references = targets.clone();
        if reverse_declarations {
            references.reverse();
        }
        for to in references {
            edges.push(cadcraft_engine::linked_files::Edge { from_key: key.clone(), raw_reference: to.clone(), role: "xref".into(), target_key: to });
        }
        let bytes = Bytes::new(cadcraft_io::write(&drawing, "Depth.dxf").map_err(|e| e.to_string())?)?;
        if key == "root.dxf" {
            main = Some(bytes);
        } else {
            files.push(cadcraft_engine::linked_files::File { key, kind: "dxf".into(), bytes });
        }
    }
    let main = main.ok_or("root missing")?;
    Ok(Input {
        main_key: "root.dxf".into(),
        name: "Depth.dxf".into(),
        main_hash: main.hash().into(),
        main: Some(main),
        files,
        edges,
        version: "depth-v1".into(),
    })
}
#[test]
fn linked_depth_checks_the_longest_shared_dag_path_including_cached_descendants() -> Result<(), String> {
    fixture::install_formats();
    let input = shared_depth_dag()?;
    let declarations = input.files.iter().map(|file| (file.key.clone(), file.kind.clone())).collect::<Vec<_>>();
    assert!(cadcraft_engine::linked_files::validate_declarations(&input.main_key, &declarations, &input.edges).is_err());
    refused(input, "shared DAG depth overflow")?;
    // The original cyclic, nested and overlay fixture remains accepted in the same actual codec path.
    let (_app, _connection, _uid) = opened()?;
    Ok(())
}

#[test]
fn linked_same_account_reconnect_requires_fresh_grant_but_keeps_full_local_export() -> Result<(), String> {
    let (mut app, old, uid) = opened()?;
    app.run("line", json!({"points":[[1,1],[7,7]]}))?;
    let before = resources(&app, uid)?;
    let revision = state(&app, uid)?.revision;
    app.file_providers.disconnect()?;
    let current = app.file_providers.connect("fixture".into(), "Linked fixture".into(), "account-one".into())?;
    app.file_providers.configure_linked_files(&current, 1)?;
    assert_ne!(old, current);
    assert_eq!(resources(&app, uid)?.grant_connection.as_deref(), Some(old.as_str()));
    let (device, export) = save(&mut app, "saveas", "device", "Portable.dxf")?;
    assert_bytes(&export, &fixture::input(10)?)?;
    app.file_providers.save_resource_set_result(&current, device, "download".into(), String::new(), export.digest().into(), String::new())?;
    tick(&mut app);
    event(&app.file_providers, "downloaded")?;
    assert!(state(&app, uid)?.is_dirty());
    assert_eq!(resources(&app, uid)?.grant_connection.as_deref(), Some(old.as_str()));
    let request = text(&app.run("qsave", Value::Null)?, "requestId")?;
    event(&app.file_providers, "saveDialog")?;
    app.file_providers.resolve_dialog(&request, "provider", "Main.dxf")?;
    let events = app.file_providers.take_events();
    assert!(!events.iter().any(|event| event.metadata["type"] == "save"));
    let needed = events.into_iter().find(|event| event.metadata["type"] == "resourceSetNeeded").ok_or("fresh grant request missing")?;
    assert_eq!(needed.metadata["requiresCurrentGrant"], true);
    let Some(LinkedPayload::Main { key, bytes }) = needed.linked else { return Err("fresh main missing".into()) };
    let mut input = fixture::input(10)?;
    input.main = None;
    input.main_key = key;
    input.main_hash = bytes.hash().into();
    assert!(app.file_providers.complete_resource_set(&old, request.clone(), input.clone()).is_err());
    assert_eq!(state(&app, uid)?.revision, revision);
    assert_eq!(resources(&app, uid)?.digest(), before.digest());
    app.file_providers.complete_resource_set(&current, request.clone(), input)?;
    tick(&mut app);
    let transfer = event(&app.file_providers, "save")?;
    let Some(LinkedPayload::Set(set)) = transfer.linked else { return Err("fresh complete transfer missing".into()) };
    assert_eq!(set.grant_connection.as_deref(), Some(current.as_str()));
    assert!(state(&app, uid)?.is_dirty());
    app.file_providers.save_resource_set_result(&current, request, "saved".into(), "fresh-main".into(), set.digest().into(), "fresh-v2".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert_eq!(resources(&app, uid)?.grant_connection.as_deref(), Some(current.as_str()));
    assert!(!state(&app, uid)?.is_dirty());
    Ok(())
}

// The first long arrival at X is blocked by active Y. The later root -> X -> Y
// shortcut has a different active context and must still check every tail descendant.
fn cycle_shortcut_depth(tail: usize) -> Result<Input, String> {
    let mut rows = vec![
        ("root.dxf".into(), vec!["a-y.dxf".into(), "z-x.dxf".into()]),
        ("a-y.dxf".into(), vec!["c01.dxf".into(), "t01.dxf".into()]),
        ("z-x.dxf".into(), vec!["a-y.dxf".into()]),
    ];
    for n in 1..=14 {
        let to = if n == 14 { "z-x.dxf".into() } else { format!("c{:02}.dxf", n + 1) };
        rows.push((format!("c{n:02}.dxf"), vec![to]));
    }
    for n in 1..=tail {
        let targets = if n == tail { vec![] } else { vec![format!("t{:02}.dxf", n + 1)] };
        rows.push((format!("t{n:02}.dxf"), targets));
    }
    depth_graph(rows, false)
}
fn assert_depth_refused(input: Input) -> Result<(), String> {
    let declarations = input.files.iter().map(|file| (file.key.clone(), file.kind.clone())).collect::<Vec<_>>();
    let error = cadcraft_engine::linked_files::validate_declarations(&input.main_key, &declarations, &input.edges)
        .err()
        .ok_or("depth17 declaration was accepted")?;
    assert!(error.contains("depth16"), "refusal must identify excessive nesting, not another input or analysis-budget failure: {error}");
    let (mut app, c) = connected()?;
    let request = app.file_providers.request_open("main")?;
    event(&app.file_providers, "openRequested")?;
    app.file_providers.complete_resource_set(&c, request.clone(), input)?;
    tick(&mut app);
    assert!(app.session.docs.is_empty(), "depth failure must not partially open a native drawing");
    let terminal = event(&app.file_providers, "openFailed")?;
    assert_eq!(text(&terminal.metadata, "requestId")?, request);
    assert!(text(&terminal.metadata, "error")?.contains("depth16"));
    assert!(!app.file_providers.busy());
    Ok(())
}
#[test]
fn linked_depth_cycle_context_shortcut_rejects_hidden_depth17() -> Result<(), String> {
    fixture::install_formats();
    let input = cycle_shortcut_depth(15)?;
    assert_eq!(input.files.len() + 1, 32);
    assert_eq!(input.edges.len(), 33);
    assert_depth_refused(input.clone())?;
    let mut reversed = input;
    reversed.edges.reverse();
    assert_depth_refused(reversed)?;
    Ok(())
}
fn assert_cycle_depth_boundary(input: Input) -> Result<(), String> {
    let declarations = input.files.iter().map(|file| (file.key.clone(), file.kind.clone())).collect::<Vec<_>>();
    cadcraft_engine::linked_files::validate_declarations(&input.main_key, &declarations, &input.edges)?;
    let (mut app, c) = connected()?;
    let uid = open_input(&mut app, &c, input.clone())?;
    assert!(!state(&app, uid)?.is_dirty());
    let (request, set) = save(&mut app, "qsave", "provider", "Depth.dxf")?;
    assert_eq!(set.files.len(), input.files.len());
    let mut edges = input.edges.clone();
    edges.sort();
    assert_eq!(set.edges, edges);
    for file in &input.files {
        assert_eq!(set.file(&file.key).ok_or("boundary resource lost")?.bytes.data(), file.bytes.data());
    }
    let native = cadcraft_io::read(set.main.bytes.data(), "Depth.dxf").map_err(|error| error.to_string())?;
    assert_eq!(native.blocks.values().filter(|block| block.xref_path.is_some()).count(), 2);
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "main".into(), set.digest().into(), "boundary-v2".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert!(!state(&app, uid)?.is_dirty());
    Ok(())
}
#[test]
fn linked_depth_cycle_context_shortcut_preserves_legal_depth16_and_cached_save() -> Result<(), String> {
    fixture::install_formats();
    let input = cycle_shortcut_depth(14)?;
    assert_eq!(input.files.len() + 1, 31);
    assert_eq!(input.edges.len(), 32);
    assert_cycle_depth_boundary(input.clone())?;
    let mut reversed = input;
    reversed.edges.reverse();
    assert_cycle_depth_boundary(reversed)?;
    Ok(())
}

fn native_small_cover_input(large: usize, small: usize, small_clique: bool) -> Result<Input, String> {
    let total = large.checked_add(small).ok_or("fixture vertex count overflow")?;
    let names: Vec<_> = (0..total).map(|node| if node == 0 { "root.dxf".into() } else { format!("n{node:03}.dxf") }).collect();
    let mut rows = Vec::new();
    for (from, name) in names.iter().enumerate() {
        let mut targets: Vec<_> = if from < large { (large..total).collect() } else { (0..large).collect() };
        if small_clique && from >= large {
            targets.extend((large..total).filter(|to| *to != from));
        }
        let targets = targets
            .into_iter()
            .map(|to| names.get(to).cloned().ok_or_else(|| "fixture target missing".to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        rows.push((name.clone(), targets));
    }
    depth_graph(rows, false)
}
#[test]
fn linked_dense_k8_64_preserves_native_open_cached_save_and_full_set_ack() -> Result<(), String> {
    fixture::install_formats();
    let input = native_small_cover_input(64, 8, false)?;
    assert_eq!(input.files.len() + 1, 72);
    assert_eq!(input.edges.len(), 1024);
    let declarations = input.files.iter().map(|file| (file.key.clone(), file.kind.clone())).collect::<Vec<_>>();
    cadcraft_engine::linked_files::validate_declarations(&input.main_key, &declarations, &input.edges)?;
    let (mut app, c) = connected()?;
    let uid = open_input(&mut app, &c, input.clone())?;
    assert!(!state(&app, uid)?.is_dirty());
    app.run("line", json!({"points":[[0,0],[3,3]]}))?;
    assert!(state(&app, uid)?.is_dirty());
    let (request, set) = save(&mut app, "qsave", "provider", "Depth.dxf")?;
    assert_eq!(set.files.len(), 71);
    let mut expected_edges = input.edges.clone();
    expected_edges.sort();
    assert_eq!(set.edges, expected_edges);
    for file in &input.files {
        assert_eq!(set.file(&file.key).ok_or("dense authorized bytes lost")?.bytes.data(), file.bytes.data());
    }
    let decoded = cadcraft_io::read(set.main.bytes.data(), "Depth.dxf").map_err(|error| error.to_string())?;
    assert_eq!(decoded.blocks.values().filter(|block| block.xref_path.is_some()).count(), 8);
    assert_eq!(decoded.model.len(), 1);
    app.file_providers.save_resource_set_result(&c, request, "saved".into(), "main".into(), set.digest().into(), "dense-v2".into())?;
    tick(&mut app);
    event(&app.file_providers, "saved")?;
    assert!(!state(&app, uid)?.is_dirty());
    assert_eq!(resources(&app, uid)?.version, "dense-v2");
    Ok(())
}
