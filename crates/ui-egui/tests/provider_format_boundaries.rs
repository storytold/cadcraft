//! Real CadApp command/format boundaries. This integration binary owns its process-global
//! format hooks: every hook delegates to cadcraft-io, and no writer reports synthetic success.
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_engine::cmd::file::{IoHooks, SaveAs, set_io, set_write_as};
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, Image, Layout, Line};
use cadcraft_engine::{DocState, Session};
use cadcraft_ui_egui::file_providers::{Event, Resource};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

const EXTERNAL_REFERENCE_ERROR: &str =
    "DXF references external images or drawings; make a self-contained drawing before provider save (external files are not uploaded)";

thread_local! {
    // Hooks run synchronously on the command's thread. Parallel cases cannot consume one
    // another's observations; all cases install exactly the same immutable OnceLock hooks.
    static WRITES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn real_read(bytes: &[u8], name: &str) -> Result<Drawing, String> {
    cadcraft_io::read(bytes, name).map_err(|error| error.to_string())
}

fn real_write(drawing: &Drawing, name: &str) -> Result<Vec<u8>, String> {
    WRITES.with(|writes| writes.borrow_mut().push(name.into()));
    cadcraft_io::write(drawing, name).map_err(|error| error.to_string())
}

fn real_write_as(drawing: &Drawing, name: &str, options: &SaveAs) -> Result<Vec<u8>, String> {
    WRITES.with(|writes| writes.borrow_mut().push(name.into()));
    let options = cadcraft_io::SaveOptions { version: options.version, binary: options.binary };
    cadcraft_io::write_with(drawing, name, &options).map_err(|error| error.to_string())
}

fn writes() -> Vec<String> {
    WRITES.with(|writes| writes.borrow().clone())
}

/// Observe the actual picker invocation without replacing either the encoder or disk writer.
fn picker(path: Option<String>) -> (Services, Rc<RefCell<Vec<String>>>) {
    let names = Rc::new(RefCell::new(Vec::new()));
    let seen = names.clone();
    let services = Services {
        pick_save: Some(Box::new(move |name| {
            seen.borrow_mut().push(name.into());
            path.clone()
        })),
        ..Services::default()
    };
    (services, names)
}

fn app_with_real_formats(services: Services) -> Result<CadApp, String> {
    set_io(IoHooks { read: real_read, write: real_write, plot: None });
    set_write_as(real_write_as);
    WRITES.with(|writes| writes.borrow_mut().clear());
    let mut app = CadApp::new(Session::new(), services);
    app.run("line", json!({"points":[[0,0],[10,20]]}))?;
    assert!(app.session.state().map_err(|error| error.to_string())?.is_dirty());
    app.ui.recent_files = vec!["existing-local-history.dxf".into()];
    Ok(app)
}

fn logic(app: &mut CadApp) {
    let ctx = egui::Context::default();
    let mut output = ctx.run_pass(egui::RawInput::default(), |ctx| app.logic(ctx));
    output.textures_delta.clear();
}

fn connect(app: &mut CadApp) -> Result<(), String> {
    let host = &app.file_providers;
    host.set_session("0123456789abcdef0123456789abcdef".into())?;
    let connection = host.connect("boundary-provider".into(), "Boundary storage".into(), "boundary-account".into())?;
    host.set_resources(
        &connection,
        vec![Resource { id: "existing-provider-resource".into(), name: "Existing.dxf".into(), updated_at: "before".into() }],
    )?;
    logic(app);
    assert!(app.file_providers.take_events().is_empty());
    Ok(())
}

struct Before {
    document: DocState,
    active: usize,
    epoch: u64,
    document_count: usize,
    recents: Vec<String>,
    provider: Value,
    log_len: usize,
}

fn before(app: &CadApp) -> Result<Before, String> {
    Ok(Before {
        document: app.session.state().map_err(|error| error.to_string())?.clone(),
        active: app.session.active,
        epoch: app.session.doc_epoch,
        document_count: app.session.docs.len(),
        recents: app.ui.recent_files.clone(),
        provider: app.file_providers.snapshot(),
        log_len: app.session.log.len(),
    })
}

/// A refusal or an unacknowledged capture must not change the drawing or its saved checkpoint.
fn assert_document_unchanged(app: &CadApp, before: &Before) -> Result<(), String> {
    let after = app.session.state().map_err(|error| error.to_string())?;
    let original = &before.document;
    assert_eq!((app.session.active, app.session.doc_epoch, app.session.docs.len()), (before.active, before.epoch, before.document_count));
    assert_eq!((after.uid, after.revision), (original.uid, original.revision));
    assert!(Arc::ptr_eq(&after.doc, &original.doc));
    assert!(Arc::ptr_eq(&after.saved, &original.saved));
    assert_eq!((&after.path, &after.title, &after.space), (&original.path, &original.title, &original.space));
    assert_eq!((&after.selection, &after.previous_selection), (&original.selection, &original.previous_selection));
    assert_eq!((after.undo.len(), after.redo.len(), after.is_dirty()), (original.undo.len(), original.redo.len(), original.is_dirty()));
    assert_eq!(app.ui.recent_files, before.recents);
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum ImageSpace {
    Model,
    Layout,
    Block,
}

fn insert_external_image(app: &mut CadApp, space: ImageSpace) -> Result<(), String> {
    let state = app.session.state_mut().map_err(|error| error.to_string())?;
    let drawing = Arc::make_mut(&mut state.doc);
    let handle = drawing.new_handle();
    let image = Entity::new(handle, EntityKind::Image(Image { path: "synthetic-external-image.png".into(), ..Image::default() }));
    match space {
        ImageSpace::Model => drawing.model.push(image),
        ImageSpace::Layout => {
            let mut layout = Layout::new("External image layout", 3);
            layout.entities.push(image);
            drawing.layouts.push(layout);
        }
        ImageSpace::Block => {
            let mut block = Block::new("ExternalImageBlock");
            block.entities.push(image);
            drawing.blocks.insert(block.name.clone(), Arc::new(block));
        }
    }
    Ok(())
}

fn insert_external_xref(app: &mut CadApp) -> Result<(), String> {
    let state = app.session.state_mut().map_err(|error| error.to_string())?;
    let drawing = Arc::make_mut(&mut state.doc);
    let mut block = Block::new("ExternalDrawing");
    block.xref_path = Some("synthetic-external-drawing.dxf".into());
    drawing.blocks.insert(block.name.clone(), Arc::new(block));
    Ok(())
}

fn assert_refused_before_encoding(app: &mut CadApp, command: &str) -> Result<(), String> {
    // Refresh the public document snapshot after the synthetic fixture was installed.
    logic(app);
    let before = before(app)?;
    let error = app.run(command, Value::Null).err().ok_or("external references unexpectedly admitted a provider capture")?;
    assert_eq!(error, EXTERNAL_REFERENCE_ERROR);
    assert!(writes().is_empty(), "the app must refuse external references before calling its real encoder");
    assert_document_unchanged(app, &before)?;
    assert!(!app.file_providers.busy());
    assert!(!app.file_providers.pending_for(before.document.uid));
    assert!(app.file_providers.take_events().is_empty(), "no provider chooser, write or ACK may be published");
    assert_eq!(app.file_providers.snapshot(), before.provider);
    assert_eq!(app.status.as_ref().map(|(text, _)| text.as_str()), Some(EXTERNAL_REFERENCE_ERROR));
    assert_eq!(app.session.log.len(), before.log_len + 1);
    assert_eq!(app.session.log.last(), Some(&error));
    Ok(())
}

#[test]
fn provider_save_and_save_as_refuse_external_images_in_model_layout_and_blocks() -> Result<(), String> {
    for space in [ImageSpace::Model, ImageSpace::Layout, ImageSpace::Block] {
        for command in ["qsave", "ui.saveas"] {
            let (services, picked) = picker(None);
            let mut app = app_with_real_formats(services)?;
            connect(&mut app)?;
            insert_external_image(&mut app, space)?;
            assert_refused_before_encoding(&mut app, command)?;
            assert!(picked.borrow().is_empty(), "{space:?}/{command} must not fall back to a local picker");
        }
    }
    Ok(())
}

#[test]
fn provider_save_and_save_as_refuse_external_xrefs_before_encoding_or_admission() -> Result<(), String> {
    for command in ["qsave", "ui.saveas"] {
        let (services, picked) = picker(None);
        let mut app = app_with_real_formats(services)?;
        connect(&mut app)?;
        insert_external_xref(&mut app)?;
        assert_refused_before_encoding(&mut app, command)?;
        assert!(picked.borrow().is_empty());
    }
    Ok(())
}

fn only_event(app: &CadApp, event_type: &str, request: &str) -> Result<Event, String> {
    let events = app.file_providers.take_events();
    assert_eq!(events.len(), 1, "expected exactly one {event_type}");
    let event = events.into_iter().next().ok_or("missing provider event")?;
    assert_eq!(event.metadata["type"], event_type);
    assert_eq!(event.metadata["requestId"], request);
    Ok(event)
}

/// Check actual native decoder output rather than accepting arbitrary nonempty writer bytes.
fn assert_native_line(bytes: &[u8], name: &str) -> Result<(), String> {
    let drawing = real_read(bytes, name)?;
    assert_eq!(drawing.model.len(), 1);
    let entity = drawing.model.iter().next().ok_or("native format lost the synthetic line")?;
    assert_eq!(entity.kind, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 20.0, 0.0) }));
    Ok(())
}

fn provider_positive_control(command: &str, intent: &str) -> Result<(), String> {
    let (services, picked) = picker(None);
    let mut app = app_with_real_formats(services)?;
    connect(&mut app)?;
    let before = before(&app)?;
    let value = app.run(command, Value::Null)?;
    assert_eq!(value["pending"], "fileProvider");
    let request = value["requestId"].as_str().ok_or("save command did not return its capture ID")?;
    let dialog = only_event(&app, "saveDialog", request)?;
    assert_eq!(dialog.metadata["documentId"], before.document.uid.to_string());
    assert_eq!(dialog.metadata["revision"], before.document.revision.to_string());
    assert_eq!(dialog.metadata["intent"], intent);
    let encoded_name = dialog.metadata["filename"].as_str().ok_or("save dialog lost its native filename")?;
    assert_eq!(encoded_name, format!("{}.dxf", before.document.title));
    assert_eq!(writes(), vec![encoded_name.to_string()]);
    assert!(dialog.bytes.is_none());
    assert_document_unchanged(&app, &before)?;
    app.file_providers.resolve_dialog(request, "provider", "SelfContained.dxf")?;
    let save = only_event(&app, "save", request)?;
    assert_eq!(save.metadata["destination"], "provider");
    assert_eq!(save.metadata["documentId"], before.document.uid.to_string());
    assert_eq!(save.metadata["revision"], before.document.revision.to_string());
    assert_eq!(save.metadata["filename"], "SelfContained.dxf");
    assert_native_line(save.bytes.as_deref().ok_or("provider write omitted its real DXF bytes")?, "SelfContained.dxf")?;
    assert_document_unchanged(&app, &before)?;
    assert_eq!(app.file_providers.snapshot()["resources"], before.provider["resources"]);
    app.file_providers.result(request.into(), "saved".into(), "self-contained-resource".into())?;
    assert_document_unchanged(&app, &before)?;
    logic(&mut app);
    assert_provider_ack(&app, &before, request)?;
    assert!(picked.borrow().is_empty());
    assert_eq!(writes().len(), 1);
    Ok(())
}

fn assert_provider_ack(app: &CadApp, before: &Before, request: &str) -> Result<(), String> {
    let state = app.session.state().map_err(|error| error.to_string())?;
    assert_eq!((state.uid, state.revision), (before.document.uid, before.document.revision));
    assert!(Arc::ptr_eq(&state.doc, &before.document.doc));
    assert!(Arc::ptr_eq(&state.saved, &before.document.doc));
    assert!(!state.is_dirty());
    assert_eq!(state.path, before.document.path);
    assert_eq!(app.ui.recent_files, before.recents);
    assert_eq!(app.file_providers.bound_document("self-contained-resource"), Some(state.uid));
    let terminal = only_event(app, "saved", request)?;
    assert_eq!(terminal.metadata["documentId"], state.uid.to_string());
    assert_eq!(terminal.metadata["revision"], before.document.revision.to_string());
    assert_eq!(terminal.metadata["resourceId"], "self-contained-resource");
    assert!(terminal.bytes.is_none());
    assert!(!app.file_providers.busy());
    assert_eq!(app.file_providers.snapshot()["resources"][0]["name"], "SelfContained.dxf");
    Ok(())
}

#[test]
fn self_contained_provider_save_and_save_as_encode_real_dxf_and_wait_for_app_ack() -> Result<(), String> {
    for (command, intent) in [("qsave", "save"), ("ui.saveas", "saveAs")] {
        provider_positive_control(command, intent)?;
    }
    Ok(())
}

fn assert_local_save(app: &CadApp, path: &str, value: &Value) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    assert_eq!(value["path"], path);
    assert_eq!(value["bytes"].as_u64(), Some(bytes.len() as u64));
    assert_native_line(&bytes, path)?;
    let state = app.session.state().map_err(|error| error.to_string())?;
    assert_eq!(state.path.as_deref(), Some(path));
    assert!(Arc::ptr_eq(&state.doc, &state.saved));
    assert!(!state.is_dirty());
    assert_eq!(writes(), vec![path.to_string()]);
    assert!(!app.file_providers.busy());
    assert!(!app.file_providers.pending_for(state.uid));
    assert!(app.file_providers.take_events().is_empty(), "a local file must not create a provider chooser, write or ACK");
    Ok(())
}

#[test]
#[cfg(not(all(target_os = "freebsd", target_arch = "aarch64")))]
fn provider_connected_explicit_dwg_save_as_uses_the_real_local_format_and_writer() -> Result<(), String> {
    let directory = TempDir::new("provider-explicit-local-dwg");
    let path = directory.path_str("Explicit local drawing.dwg");
    let (services, picked) = picker(None);
    let mut app = app_with_real_formats(services)?;
    connect(&mut app)?;
    let before = before(&app)?;
    let value = app.run("saveas", json!({"path":path,"version":2018}))?;
    assert_local_save(&app, &path, &value)?;
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    assert!(bytes.starts_with(b"AC1032"), "the actual local file must be a 2018 DWG, not renamed DXF");
    assert!(picked.borrow().is_empty(), "a programmatic explicit path must not open either chooser");
    assert_eq!(app.file_providers.snapshot(), before.provider);
    assert_eq!(app.ui.recent_files, before.recents);
    assert_eq!(app.session.state().map_err(|error| error.to_string())?.uid, before.document.uid);
    Ok(())
}

#[test]
fn disabled_provider_save_as_keeps_the_ordinary_picker_and_real_local_dxf_writer() -> Result<(), String> {
    let directory = TempDir::new("provider-disabled-local-picker");
    let path = directory.path_str("Picked local drawing.dxf");
    let (services, picked) = picker(Some(path.clone()));
    let mut app = app_with_real_formats(services)?;
    assert!(!app.file_providers.connected() && !app.file_providers.enabled());
    let before = before(&app)?;
    let value = app.run("ui.saveas", Value::Null)?;
    assert_local_save(&app, &path, &value)?;
    assert_eq!(*picked.borrow(), vec![format!("{}.dxf", before.document.title)]);
    assert_eq!(app.file_providers.snapshot(), before.provider);
    assert_eq!(app.ui.recent_files, before.recents);
    assert!(!app.file_providers.connected() && !app.file_providers.enabled());
    Ok(())
}
