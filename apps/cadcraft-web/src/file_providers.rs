//! Same-origin file-provider bridge. Persistence belongs to the registered provider, not WASM.
//!
//! Callbacks enqueue commands for a later egui frame. Exporting bytes never acknowledges them.

use cadcraft_engine::linked_files::{self, Bytes, Edge, File, Input};
use cadcraft_ui_egui::file_providers::{Host, LinkedPayload, Resource};
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};

thread_local! {
    static CURRENT_CONTEXT: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}
const API: &str = "cadcraftFileProviders";
const EVENT: &str = "cadcraft-file-provider";
const MAX_BYTES: u32 = 80 * 1024 * 1024;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = Uint8Array, js_name = from)]
    fn copy_event_bytes(bytes: &[u8]) -> Result<Uint8Array, JsValue>;
    #[wasm_bindgen(catch, js_namespace = Uint8Array, js_name = from)]
    fn copy_resource_view(bytes: &Uint8Array) -> Result<Uint8Array, JsValue>;
}

fn error(message: impl ToString) -> JsValue {
    JsValue::from_str(&message.to_string())
}

fn object(value: &serde_json::Value) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(&value.to_string())
}

fn set(target: &JsValue, key: &str, value: &JsValue) -> Result<(), JsValue> {
    if Reflect::set(target, &key.into(), value)? { Ok(()) } else { Err(error("file-provider API is not writable")) }
}

// Inspect UTF-16 length before copying a host string into WASM memory.
fn bounded_string(value: JsValue, max_units: u32) -> Result<String, JsValue> {
    let text = value.dyn_into::<js_sys::JsString>().map_err(|_| error("resource fields must be strings"))?;
    if text.length() > max_units {
        return Err(error("resource field is too large"));
    }
    text.as_string().ok_or_else(|| error("invalid resource field"))
}

fn resource(value: JsValue) -> Result<Resource, JsValue> {
    if value.is_null() || !value.is_object() || Array::is_array(&value) {
        return Err(error("resource entries must be objects"));
    }
    let entry = value.dyn_into::<Object>()?;
    let id = bounded_string(Reflect::get(&entry, &"id".into())?, 160)?;
    let name = bounded_string(Reflect::get(&entry, &"name".into())?, 255)?;
    let updated = Reflect::get(&entry, &"updatedAt".into())?;
    let updated_at = if updated.is_undefined() { String::new() } else { bounded_string(updated, 100)? };
    Ok(Resource { id, name, updated_at })
}

fn wake_later(ctx: &egui::Context) {
    let fallback = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        CURRENT_CONTEXT.with(|current| current.borrow().as_ref().unwrap_or(&fallback).request_repaint());
    });
}

/// Makes the capability discoverable without enabling it or changing the browser's writer.
pub fn install(host: Host, ctx: egui::Context) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| error("no window"))?;
    CURRENT_CONTEXT.with(|current| *current.borrow_mut() = Some(ctx.clone()));
    if Reflect::has(&window, &API.into())? {
        // A graphics restart keeps the page capability and pending requests, and renews its wake target.
        return Ok(());
    }
    // A delayed result from an earlier page must never address a new document's request.
    let crypto = Reflect::get(&window, &"crypto".into())?;
    let random = Reflect::get(&crypto, &"getRandomValues".into())?.dyn_into::<Function>()?;
    let nonce = Uint8Array::new_with_length(16);
    random.call1(&crypto, &nonce)?;
    host.set_session(nonce.to_vec().iter().map(|byte| format!("{byte:02x}")).collect()).map_err(error)?;

    let api = Object::new();
    set(&api, "protocolVersion", &1.into())?;
    set(&api, "app", &"cadcraft".into())?;
    set(&api, "capabilities", &object(&serde_json::json!(["revisionAck", "externalRecents", "saveDestination", "openBytes", "linkedFilesV1"]))?)?;

    let h = host.clone();
    let wake = ctx.clone();
    let register = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<String, JsValue>>::new(move |id, label, namespace| {
        let id = bounded_string(id, 160)?;
        let label = bounded_string(label, 100)?;
        let namespace = bounded_string(namespace, 160)?;
        let connection = h.connect(id, label, namespace).map_err(error)?;
        wake_later(&wake);
        Ok(connection)
    });
    set(&api, "registerProvider", register.as_ref())?;
    register.forget();

    let h = host.clone();
    let wake = ctx.clone();
    let unregister = Closure::<dyn FnMut() -> Result<bool, JsValue>>::new(move || {
        h.disconnect().map_err(error)?;
        wake_later(&wake);
        Ok(true)
    });
    set(&api, "unregisterProvider", unregister.as_ref())?;
    unregister.forget();

    let h = host.clone();
    let wake = ctx.clone();
    let enabled = Closure::<dyn FnMut(JsValue) -> Result<bool, JsValue>>::new(move |value: JsValue| {
        let value = value.as_bool().ok_or_else(|| error("enabled must be a boolean"))?;
        h.set_enabled(value).map_err(error)?;
        wake_later(&wake);
        Ok(true)
    });
    set(&api, "setEnabled", enabled.as_ref())?;
    enabled.forget();

    let h = host.clone();
    let wake = ctx.clone();
    let resources = Closure::<dyn FnMut(JsValue, JsValue) -> Result<(), JsValue>>::new(move |connection, value: JsValue| {
        let connection = bounded_string(connection, 160)?;
        if !Array::is_array(&value) {
            return Err(error("resources must be an array of at most 100 entries"));
        }
        let entries = value.dyn_into::<Array>()?;
        if entries.length() > 100 {
            return Err(error("resources must be an array of at most 100 entries"));
        }
        // Read only bounded metadata; do not copy or serialize arbitrary host properties.
        let resources = entries.iter().map(resource).collect::<Result<Vec<_>, _>>()?;
        h.set_resources(&connection, resources).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "setResources", resources.as_ref())?;
    resources.forget();

    let h = host.clone();
    let wake = ctx.clone();
    let request_open = Closure::<dyn FnMut(JsValue) -> Result<String, JsValue>>::new(move |resource| {
        let resource = bounded_string(resource, 160)?;
        let request = h.request_open(&resource).map_err(error)?;
        wake_later(&wake);
        Ok(request)
    });
    set(&api, "requestOpen", request_open.as_ref())?;
    request_open.forget();

    let h = host.clone();
    let wake = ctx.clone();
    let open = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(move |request, name, value: JsValue| {
        let request = bounded_string(request, 160)?;
        let name = bounded_string(name, 255)?;
        let bytes = value.dyn_into::<Uint8Array>().map_err(|_| error("bytes must be a Uint8Array"))?;
        if bytes.length() > MAX_BYTES {
            return Err(error("file exceeds the provider limit"));
        }
        h.open(request, name, bytes.to_vec()).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "openResource", open.as_ref())?;
    open.forget();

    let h = host.clone();
    let wake = ctx.clone();
    let cancel_open = Closure::<dyn FnMut(JsValue, JsValue) -> Result<(), JsValue>>::new(move |request, message| {
        let request = bounded_string(request, 160)?;
        let message = bounded_string(message, 1024)?;
        h.cancel_open(request, message).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "cancelOpen", cancel_open.as_ref())?;
    cancel_open.forget();

    let h = host.clone();
    let wake = ctx.clone();
    let result = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(move |request, status, resource| {
        let request = bounded_string(request, 160)?;
        let status = bounded_string(status, 8)?;
        let resource = bounded_string(resource, 160)?;
        h.result(request, status, resource).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "saveResult", result.as_ref())?;
    result.forget();

    let h = host.clone();
    let wake = ctx.clone();
    let dialog = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(move |request, destination, name| {
        let request = bounded_string(request, 160)?;
        let destination = bounded_string(destination, 8)?;
        let name = bounded_string(name, 255)?;
        h.resolve_dialog(&request, &destination, &name).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "resolveSaveDialog", dialog.as_ref())?;
    dialog.forget();

    let h = host.clone();
    let snapshot = Closure::<dyn FnMut() -> Result<JsValue, JsValue>>::new(move || object(&h.snapshot()));
    set(&api, "snapshot", snapshot.as_ref())?;
    snapshot.forget();
    install_linked_api(&api, host.clone(), ctx.clone())?;
    set(&window, API, &Object::freeze(&api))?;
    dispatch(&serde_json::json!({"type": "ready", "protocolVersion": 1}), None)
}

fn string_field(value: &JsValue, key: &str, limit: u32) -> Result<String, JsValue> {
    bounded_string(Reflect::get(value, &key.into())?, limit)
}
struct IncomingFile {
    key: String,
    kind: String,
    expected: String,
    bytes: Uint8Array,
}
fn incoming(value: JsValue, kind: Option<&str>) -> Result<IncomingFile, JsValue> {
    let key = string_field(&value, "logicalKey", 1024)?;
    let kind = match kind {
        Some(kind) => kind.into(),
        None => string_field(&value, "kind", 8)?,
    };
    let expected = string_field(&value, "sha256", 64)?;
    if !linked_files::logical_key(&key) || expected.len() != 64 || !expected.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err(error("invalid logical resource key/hash"));
    }
    let bytes = Reflect::get(&value, &"bytes".into())?.dyn_into::<Uint8Array>().map_err(|_| error("resource bytes must be Uint8Array"))?;
    Ok(IncomingFile { key, kind, expected, bytes })
}
fn copy_incoming(host: &Host, connection: &str, request: &str, file: IncomingFile) -> Result<File, JsValue> {
    let copied = copy_resource_view(&file.bytes).inspect_err(|error| {
        log::error!("linked resource byte copy failed: {error:?}");
        host.fail_linked_copy(connection, request);
    })?;
    let bytes = Bytes::new(copied.to_vec()).map_err(error)?;
    if bytes.hash() != file.expected.to_ascii_lowercase() {
        return Err(error("resource bytes do not match SHA256"));
    }
    Ok(File { key: file.key, kind: file.kind, bytes })
}
/// Preflight every declared array/string/aggregate length before copying any payload buffer.
fn input_set(host: &Host, connection: &str, request: &str, value: JsValue) -> Result<Input, JsValue> {
    let main = Reflect::get(&value, &"main".into())?;
    let key = string_field(&main, "logicalKey", 1024)?;
    let name = string_field(&main, "name", 255)?;
    let main_hash = string_field(&main, "sha256", 64)?;
    let version = Reflect::get(&value, &"versionToken".into())?;
    let version = if version.is_undefined() { String::new() } else { bounded_string(version, 160)? };
    if !linked_files::logical_key(&key) || name.len() > 255 || main_hash.len() != 64 || !main_hash.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err(error("invalid resource main identity"));
    }
    let array = Reflect::get(&value, &"files".into())?.dyn_into::<Array>().map_err(|_| error("files must be an array"))?;
    let edges = Reflect::get(&value, &"edges".into())?.dyn_into::<Array>().map_err(|_| error("edges must be an array"))?;
    let file_count = array.length();
    let edge_count = edges.length();
    if file_count >= linked_files::MAX_FILES as u32 || edge_count > linked_files::MAX_EDGES as u32 {
        return Err(error("resource count exceeds limits"));
    }
    let main_value = Reflect::get(&main, &"bytes".into())?;
    let main_file = if main_value.is_undefined() { None } else { Some(incoming(main.clone(), Some("dxf"))?) };
    let mut total = main_file.as_ref().map_or(0u64, |f| u64::from(f.bytes.length()));
    let mut files = Vec::new();
    for index in 0..file_count {
        let file = incoming(array.get(index), None)?;
        if !matches!(file.kind.as_str(), "dxf" | "raster" | "font") {
            return Err(error("unsupported resource kind"));
        }
        total = total.checked_add(u64::from(file.bytes.length())).ok_or_else(|| error("resource byte count overflow"))?;
        files.push(file);
    }
    if total > u64::from(MAX_BYTES) {
        return Err(error("resource buffers exceed 80 MiB"));
    }
    let mut references = Vec::new();
    for index in 0..edge_count {
        let item = edges.get(index);
        let edge = Edge {
            from_key: string_field(&item, "fromKey", 1024)?,
            raw_reference: string_field(&item, "rawReference", 1024)?,
            role: string_field(&item, "role", 8)?,
            target_key: string_field(&item, "targetKey", 1024)?,
        };
        if !linked_files::logical_key(&edge.from_key)
            || !linked_files::logical_key(&edge.target_key)
            || edge.raw_reference.len() > 1024
            || edge.raw_reference.chars().any(char::is_control)
            || edge.raw_reference.split(['/', '\\']).count() > 32
            || !matches!(edge.role.as_str(), "image" | "xref" | "font" | "bigFont")
        {
            return Err(error("invalid resource reference"));
        }
        references.push(edge);
    }
    let declarations = files.iter().map(|file| (file.key.clone(), file.kind.clone())).collect::<Vec<_>>();
    linked_files::validate_declarations(&key, &declarations, &references).map_err(error)?;
    let additional = main_file
        .iter()
        .chain(files.iter())
        .map(|file| Bytes::additional_charge(&file.expected.to_ascii_lowercase(), file.bytes.length() as usize))
        .fold(0usize, usize::saturating_add);
    host.preflight_linked_bytes(additional).map_err(error)?;
    let main = main_file.map(|file| copy_incoming(host, connection, request, file)).transpose()?.map(|file| file.bytes);
    let files = files.into_iter().map(|file| copy_incoming(host, connection, request, file)).collect::<Result<Vec<_>, _>>()?;
    Ok(Input { main_key: key, name, main, main_hash: main_hash.to_ascii_lowercase(), files, edges: references, version })
}
fn install_linked_api(api: &Object, host: Host, ctx: egui::Context) -> Result<(), JsValue> {
    let h = host.clone();
    let wake = ctx.clone();
    let configure = Closure::<dyn FnMut(JsValue, JsValue) -> Result<(), JsValue>>::new(move |connection, version: JsValue| {
        if version.as_f64() != Some(1.0) {
            return Err(error("only linkedFilesV1 is supported"));
        }
        h.configure_linked_files(&bounded_string(connection, 160)?, 1).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(api, "configureLinkedFiles", configure.as_ref())?;
    configure.forget();
    let h = host.clone();
    let wake = ctx.clone();
    let complete = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(move |connection, request, set| {
        let connection = bounded_string(connection, 160)?;
        let request = bounded_string(request, 160)?;
        // Reject wrong connections/unknown owners before allocating copied file data.
        h.preflight_linked_reply(&connection, &request).map_err(error)?;
        let input = input_set(&h, &connection, &request, set);
        wake_later(&wake); // Includes a recoverable copy failure awaiting the actual app consumer.
        h.complete_resource_set(&connection, request, input?).map_err(error)?;
        Ok(())
    });
    set(api, "completeResourceSet", complete.as_ref())?;
    complete.forget();
    let h = host.clone();
    let wake = ctx.clone();
    let cancel = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(move |connection, request, message| {
        h.cancel_resource_set(&bounded_string(connection, 160)?, bounded_string(request, 160)?, bounded_string(message, 1024)?).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(api, "cancelResourceSet", cancel.as_ref())?;
    cancel.forget();
    let h = host;
    let wake = ctx;
    let result = Closure::<dyn FnMut(JsValue, JsValue, JsValue, JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(
        move |connection, request, status, resource, digest, version| {
            h.save_resource_set_result(
                &bounded_string(connection, 160)?,
                bounded_string(request, 160)?,
                bounded_string(status, 8)?,
                bounded_string(resource, 160)?,
                bounded_string(digest, 64)?,
                bounded_string(version, 160)?,
            )
            .map_err(error)?;
            wake_later(&wake);
            Ok(())
        },
    );
    set(api, "saveResourceSetResult", result.as_ref())?;
    result.forget();
    Ok(())
}

fn dispatch(metadata: &serde_json::Value, bytes: Option<Vec<u8>>) -> Result<(), JsValue> {
    dispatch_linked(metadata, bytes, None)
}
fn dispatch_linked(metadata: &serde_json::Value, bytes: Option<Vec<u8>>, linked: Option<LinkedPayload>) -> Result<(), JsValue> {
    let detail = object(metadata)?;
    if let Some(bytes) = bytes {
        set(&detail, "bytes", &copy_event_bytes(&bytes)?.into())?;
    }
    if let Some(payload) = &linked {
        match payload {
            LinkedPayload::Main { key, bytes } => {
                set(&detail, "mainLogicalKey", &key.clone().into())?;
                set(&detail, "bytes", &copy_event_bytes(bytes.data())?.into())?;
            }
            LinkedPayload::Set(resources) => {
                set(&detail, "bytes", &copy_event_bytes(resources.main.bytes.data())?.into())?;
                set(&detail, "mainLogicalKey", &resources.main.key.clone().into())?;
                let attachments = Array::new();
                for file in &resources.files {
                    let row = Object::new();
                    set(&row, "logicalKey", &file.key.clone().into())?;
                    set(&row, "kind", &file.kind.clone().into())?;
                    set(&row, "sha256", &file.bytes.hash().into())?;
                    set(&row, "bytes", &copy_event_bytes(file.bytes.data())?.into())?;
                    attachments.push(&row);
                }
                set(&detail, "attachments", &attachments.into())?;
            }
        }
    }
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    let event = web_sys::CustomEvent::new_with_event_init_dict(EVENT, &init)?;
    web_sys::window().ok_or_else(|| error("no window"))?.dispatch_event(&event)?;
    drop(linked); // Keep all retained resource allocations charged across synchronous callbacks.
    Ok(())
}

pub fn publish(host: &Host) {
    if host.publish_linked_events_with(dispatch_linked, |error| log::error!("file-provider event delivery failed: {error:?}")) {
        CURRENT_CONTEXT.with(|current| {
            if let Some(ctx) = current.borrow().as_ref() {
                ctx.request_repaint();
            }
        });
    }
}

#[cfg(test)]
mod browser_tests;
