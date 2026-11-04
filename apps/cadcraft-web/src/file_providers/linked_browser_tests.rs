//! Original Rust-only Window test: real native DXF, authorized bytes and the actual app consumer.
use super::{Page, error, field, text};
use cadcraft_engine::linked_files::{Bytes, File, Input};
use cadcraft_ui_egui::CadApp;
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use serde_json::json;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_test::wasm_bindgen_test;
#[path = "../../../../crates/ui-egui/tests/support/linked_files.rs"]
mod fixture;

fn property(object: &JsValue, key: &str, value: &JsValue) -> Result<(), JsValue> {
    if Reflect::set(object, &key.into(), value)? { Ok(()) } else { Err(error("fixture field is not writable")) }
}
fn encoded(value: &serde_json::Value) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(&value.to_string())
}
fn input_object(input: &Input) -> Result<JsValue, JsValue> {
    let object = Object::new();
    let main = Object::new();
    property(&main, "logicalKey", &input.main_key.clone().into())?;
    property(&main, "name", &input.name.clone().into())?;
    property(&main, "sha256", &input.main_hash.clone().into())?;
    if let Some(bytes) = &input.main {
        property(&main, "bytes", &Uint8Array::from(bytes.data()).into())?;
    }
    property(&object, "main", &main)?;
    property(&object, "versionToken", &input.version.clone().into())?;
    let files = Array::new();
    for file in &input.files {
        let row = Object::new();
        property(&row, "logicalKey", &file.key.clone().into())?;
        property(&row, "kind", &file.kind.clone().into())?;
        property(&row, "sha256", &file.bytes.hash().into())?;
        property(&row, "bytes", &Uint8Array::from(file.bytes.data()).into())?;
        files.push(&row);
    }
    property(&object, "files", &files)?;
    property(&object, "edges", &encoded(&json!(input.edges))?)?;
    Ok(object.into())
}
fn choose(page: &Page, app: &mut CadApp, command: &str, destination: &str, name: &str) -> Result<String, JsValue> {
    let request = app.run(command, serde_json::Value::Null).map_err(error)?;
    let request = request.get("requestId").and_then(serde_json::Value::as_str).ok_or_else(|| error("missing real chooser"))?.to_string();
    super::super::publish(&page.host);
    assert_eq!(text(&page.latest("saveDialog")?, "requestId")?, request);
    page.api("resolveSaveDialog", &[request.clone().into(), destination.into(), name.into()])?;
    Ok(request)
}
fn assert_export(event: &JsValue, color: u8) -> Result<Input, JsValue> {
    let main = Bytes::new(field(event, "bytes")?.dyn_into::<Uint8Array>()?.to_vec()).map_err(error)?;
    let rows = field(event, "attachments")?.dyn_into::<Array>()?;
    assert_eq!(rows.length(), 6, "device and provider exports need every attachment");
    let original = fixture::input(color).map_err(error)?;
    let mut files = Vec::new();
    for row in rows.iter() {
        let key = text(&row, "logicalKey")?;
        let expected = original.files.iter().find(|file| file.key == key).ok_or_else(|| error("unexpected attachment"))?;
        let bytes = Bytes::new(field(&row, "bytes")?.dyn_into::<Uint8Array>()?.to_vec()).map_err(error)?;
        assert_eq!(bytes.data(), expected.bytes.data());
        assert_eq!(bytes.hash(), text(&row, "sha256")?);
        files.push(File { key, kind: text(&row, "kind")?, bytes });
    }
    let image = files.iter().find(|f| f.key == "images/main.png").ok_or_else(|| error("image missing"))?;
    let decoded = image::load_from_memory(image.bytes.data()).map_err(error)?.to_rgba8();
    assert_eq!(decoded.dimensions(), (1, 1));
    assert_eq!(decoded.get_pixel(0, 0).0, [color, 64, 128, 255]);
    let edges = js_sys::JSON::stringify(&field(&field(event, "manifest")?, "edges")?)?.as_string().ok_or_else(|| error("manifest edges missing"))?;
    Ok(Input {
        main_key: text(event, "mainLogicalKey")?,
        name: text(event, "filename")?,
        main_hash: main.hash().into(),
        main: Some(main),
        files,
        edges: serde_json::from_str(&edges).map_err(error)?,
        version: "version-3".into(),
    })
}
fn fail_second_copy(page: &Page) -> Result<u32, JsValue> {
    let constructor = field(&js_sys::global(), "Uint8Array")?;
    let original = field(&constructor, "from")?.dyn_into::<Function>()?;
    let receiver = constructor.clone();
    let count = Rc::new(Cell::new(0u32));
    let seen = count.clone();
    let fault = Closure::<dyn FnMut(JsValue) -> Result<JsValue, JsValue>>::new(move |value| {
        seen.set(seen.get() + 1);
        if seen.get() == 2 {
            Err(js_sys::RangeError::new("synthetic attachment-copy failure after native main copy").into())
        } else {
            original.call1(&receiver, &value)
        }
    });
    page.with_replacement(&constructor, "from", fault.as_ref())??;
    Ok(count.get())
}
fn ack(page: &Page, connection: &JsValue, request: &str, event: &JsValue, resource: &str, version: &str) -> Result<(), JsValue> {
    page.api(
        "saveResourceSetResult",
        &[connection.clone(), request.into(), "saved".into(), resource.into(), field(event, "setDigest")?, version.into()],
    )?;
    Ok(())
}
fn refresh(page: &Page, app: &mut CadApp, connection: &JsValue, color: u8) -> Result<String, JsValue> {
    let request = app.run("ui.provider.refreshResources", json!({})).map_err(error)?;
    let request = request.get("requestId").and_then(serde_json::Value::as_str).ok_or_else(|| error("refresh owner missing"))?.to_string();
    super::super::publish(&page.host);
    let needed = page.latest("resourceSetNeeded")?;
    let main = Bytes::new(field(&needed, "bytes")?.dyn_into::<Uint8Array>()?.to_vec()).map_err(error)?;
    let mut input = fixture::input(color).map_err(error)?;
    input.main = None;
    input.main_hash = main.hash().into();
    input.main_key = text(&needed, "mainLogicalKey")?;
    input.name = text(&needed, "filename")?;
    page.api("completeResourceSet", &[connection.clone(), request.clone().into(), input_object(&input)?])?;
    page.tick(app);
    super::super::publish(&page.host);
    assert_eq!(text(&page.latest("resourcesUpdated")?, "requestId")?, request);
    Ok(app.session.state().map_err(error)?.resources.as_ref().ok_or_else(|| error("resources missing"))?.digest().into())
}

fn incoming_failure(page: &Page, app: &mut CadApp, connection: &JsValue, input: &Input) -> Result<(), JsValue> {
    let reply = input_object(input)?;
    let read = page.api("requestOpen", &["main".into()])?;
    super::super::publish(&page.host);
    let constructor = field(&js_sys::global(), "Uint8Array")?;
    let calls = Rc::new(Cell::new(0u32));
    let seen = calls.clone();
    let fault = Closure::<dyn FnMut(JsValue) -> Result<JsValue, JsValue>>::new(move |_| {
        seen.set(seen.get() + 1);
        Err(js_sys::RangeError::new("synthetic authorized-input copy failure").into())
    });
    let outcome =
        page.with_operation(&constructor, "from", fault.as_ref(), || page.api("completeResourceSet", &[connection.clone(), read.clone(), reply]))?;
    assert!(outcome.is_err());
    assert_eq!(calls.get(), 1);
    assert!(app.session.docs.is_empty() && page.host.busy());
    page.tick(app);
    super::super::publish(&page.host);
    assert_eq!(field(&page.latest("openFailed")?, "requestId")?, read);
    assert!(!page.host.busy() && app.session.docs.is_empty());
    assert!(page.api("completeResourceSet", &[connection.clone(), read, input_object(input)?]).is_err());
    Ok(())
}
fn delivery_recovery(page: &Page, app: &mut CadApp, connection: &JsValue, uid: u64) -> Result<(String, JsValue), JsValue> {
    app.run("line", json!({"points":[[0,0],[10,10]]})).map_err(error)?;
    let failed = choose(page, app, "qsave", "provider", "Main.dxf")?;
    assert_eq!(fail_second_copy(page)?, 2, "fault must reach an attachment after copying actual native DXF");
    assert!(page.host.pending_for(uid));
    assert!(page.api("unregisterProvider", &[]).is_err());
    page.tick(app);
    super::super::publish(&page.host);
    assert_eq!(text(&page.latest("saveFailed")?, "requestId")?, failed);
    assert!(app.session.state().map_err(error)?.is_dirty());

    let retry = choose(page, app, "qsave", "provider", "Main.dxf")?;
    super::super::publish(&page.host);
    let transfer = page.latest("save")?;
    assert_eq!(text(&transfer, "requestId")?, retry);
    assert_eq!(text(&transfer, "documentId")?, uid.to_string());
    assert_eq!(text(&transfer, "namespace")?, "account-one");
    assert_eq!(&field(&transfer, "connectionId")?, connection);
    assert_eq!(text(&transfer, "expectedVersion")?, "version-1");
    assert_export(&transfer, 10)?;
    assert!(page.api("saveResult", &[retry.clone().into(), "saved".into(), "main".into()]).is_err());
    assert!(
        page.api(
            "saveResourceSetResult",
            &[connection.clone(), retry.clone().into(), "saved".into(), "main".into(), "wrong-digest".into(), "version-2".into()]
        )
        .is_err()
    );
    Ok((retry, transfer))
}
fn preserve_later_resources(page: &Page, app: &mut CadApp, connection: &JsValue, retry: &str, transfer: &JsValue) -> Result<(), JsValue> {
    app.run("line", json!({"points":[[20,20],[30,30]]})).map_err(error)?;
    let replacement = refresh(page, app, connection, 88)?;
    ack(page, connection, retry, transfer, "main", "version-2")?;
    page.tick(app);
    super::super::publish(&page.host);
    assert_eq!(text(&page.latest("saved")?, "requestId")?, retry);
    let doc = app.session.state().map_err(error)?;
    assert!(doc.is_dirty(), "older full-set ACK must preserve newer geometry and attachment generation");
    assert_eq!(doc.resources.as_ref().ok_or_else(|| error("replacement lost"))?.digest(), replacement);

    Ok(())
}
fn device_and_reopen(page: &Page, app: &mut CadApp, connection: &JsValue, uid: u64, input: &Input, read: &JsValue) -> Result<(), JsValue> {
    let current = choose(page, app, "qsave", "provider", "Main.dxf")?;
    super::super::publish(&page.host);
    let full = page.latest("save")?;
    assert_eq!(text(&full, "expectedVersion")?, "version-2");
    let reopen = assert_export(&full, 88)?;
    ack(page, connection, &current, &full, "main", "version-3")?;
    page.tick(app);
    super::super::publish(&page.host);
    assert!(!app.session.state().map_err(error)?.is_dirty());
    app.run("line", json!({"points":[[40,40],[50,50]]})).map_err(error)?;
    let device = choose(page, app, "saveas", "device", "Portable.dxf")?;
    super::super::publish(&page.host);
    let export = page.latest("save")?;
    assert_export(&export, 88)?;
    page.api(
        "saveResourceSetResult",
        &[connection.clone(), device.clone().into(), "download".into(), "".into(), field(&export, "setDigest")?, "".into()],
    )?;
    page.tick(app);
    super::super::publish(&page.host);
    assert_eq!(text(&page.latest("downloaded")?, "requestId")?, device);
    assert!(app.session.state().map_err(error)?.is_dirty(), "device transfer has no durability credit");

    let second = page.api("requestOpen", &["copy".into()])?;
    super::super::publish(&page.host);
    page.api("completeResourceSet", &[connection.clone(), second.clone(), input_object(&reopen)?])?;
    page.tick(app);
    super::super::publish(&page.host);
    assert_eq!(field(&page.latest("opened")?, "requestId")?, second);
    assert_ne!(app.session.state().map_err(error)?.uid, uid);
    assert_eq!(app.session.state().map_err(error)?.resources.as_ref().ok_or_else(|| error("reopened resources missing"))?.files.len(), 6);
    assert!(app.session.docs.iter().find(|doc| doc.uid == uid).ok_or_else(|| error("original drawing lost"))?.is_dirty());
    page.api("unregisterProvider", &[])?;
    let next = page.api("registerProvider", &["fixture".into(), "Linked fixture".into(), "account-two".into()])?;
    assert_ne!(&next, connection);
    assert!(page.api("completeResourceSet", &[connection.clone(), read.clone(), input_object(input)?]).is_err());
    Ok(())
}

#[wasm_bindgen_test]
fn real_window_linked_files_preserve_native_closure_and_recover_attachment_delivery() -> Result<(), JsValue> {
    let mut app = fixture::app();
    let page = Page::install(&app)?;
    assert!(!page.host.enabled());
    let connection = page.api("registerProvider", &["fixture".into(), "Linked fixture".into(), "account-one".into()])?;
    page.api("configureLinkedFiles", &[connection.clone(), 1.into()])?;
    page.api("setResources", &[connection.clone(), encoded(&json!([{"id":"main","name":"Main.dxf"},{"id":"copy","name":"Copy.dxf"}]))?])?;
    let input = fixture::input(10).map_err(error)?;
    incoming_failure(&page, &mut app, &connection, &input)?;
    let read = page.api("requestOpen", &["main".into()])?;
    super::super::publish(&page.host);
    page.api("completeResourceSet", &[connection.clone(), read.clone(), input_object(&input)?])?;
    assert!(app.session.docs.is_empty(), "Window callback must wait for actual CadApp consumer");
    page.tick(&mut app);
    super::super::publish(&page.host);
    assert_eq!(field(&page.latest("opened")?, "requestId")?, read);
    let uid = app.session.state().map_err(error)?.uid;
    let original = app.session.state().map_err(error)?.resources.clone().ok_or_else(|| error("linked context not installed"))?;
    assert_eq!(original.files.len(), 6);
    assert!(app.session.state().map_err(error)?.doc.blocks.get("Child").is_some_and(|b| b.entities.is_empty()));

    let (retry, transfer) = delivery_recovery(&page, &mut app, &connection, uid)?;
    preserve_later_resources(&page, &mut app, &connection, &retry, &transfer)?;
    device_and_reopen(&page, &mut app, &connection, uid, &input, &read)?;
    Ok(())
}
