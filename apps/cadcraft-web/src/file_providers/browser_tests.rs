//! Original, self-contained DXF fixture. Rust closures create genuine JS exceptions; no JS source.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cadcraft_engine::{Session, cmd::file};
use cadcraft_ui_egui::{CadApp, Services};
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn error(message: impl ToString) -> JsValue {
    JsValue::from_str(&message.to_string())
}
fn field(object: &JsValue, name: &str) -> Result<JsValue, JsValue> {
    Reflect::get(object, &name.into())
}
fn text(object: &JsValue, name: &str) -> Result<String, JsValue> {
    field(object, name)?.as_string().ok_or_else(|| error(format!("missing string field {name}")))
}
fn call(object: &JsValue, name: &str, arguments: &[JsValue]) -> Result<JsValue, JsValue> {
    let function = field(object, name)?.dyn_into::<Function>()?;
    let array = Array::new();
    for argument in arguments {
        array.push(argument);
    }
    function.apply(object, &array)
}

struct Page {
    window: web_sys::Window,
    api: JsValue,
    host: cadcraft_ui_egui::file_providers::Host,
    ctx: egui::Context,
    events: Rc<RefCell<Vec<JsValue>>>,
    listener: Closure<dyn FnMut(web_sys::Event)>,
}
impl Page {
    fn install(app: &CadApp) -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| error("no browser window"))?;
        if Reflect::has(&window, &super::API.into())? {
            return Err(error("fixture requires a fresh owned page"));
        }
        let host = app.file_providers.clone();
        let ctx = egui::Context::default();
        let events = Rc::new(RefCell::new(Vec::new()));
        let seen = events.clone();
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Ok(event) = event.dyn_into::<web_sys::CustomEvent>() {
                seen.borrow_mut().push(event.detail());
            }
        });
        window.add_event_listener_with_callback(super::EVENT, listener.as_ref().unchecked_ref())?;
        let mut page = Self { window, api: JsValue::UNDEFINED, host, ctx, events, listener };
        super::install(page.host.clone(), page.ctx.clone())?;
        page.api = field(&page.window, super::API)?;
        Ok(page)
    }
    fn api(&self, method: &str, arguments: &[JsValue]) -> Result<JsValue, JsValue> {
        call(&self.api, method, arguments)
    }
    fn latest(&self, kind: &str) -> Result<JsValue, JsValue> {
        self.events
            .borrow()
            .iter()
            .rev()
            .find(|event| text(event, "type").is_ok_and(|name| name == kind))
            .cloned()
            .ok_or_else(|| error(format!("missing real window event {kind}")))
    }
    fn tick(&self, app: &mut CadApp) {
        self.ctx.run_ui(Default::default(), |ui| app.logic(ui.ctx())).drop_without_applying_deltas();
    }
    fn choose(&self, app: &mut CadApp) -> Result<String, JsValue> {
        let value = app.run("qsave", json!({})).map_err(error)?;
        value
            .get("requestId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| error("qsave did not create a real provider choice"))
    }
    fn captured_save(&self, app: &mut CadApp) -> Result<String, JsValue> {
        let request = self.choose(app)?;
        super::publish(&self.host);
        assert_eq!(text(&self.latest("saveDialog")?, "requestId")?, request);
        self.api("resolveSaveDialog", &[request.clone().into(), "provider".into(), "Fixture.dxf".into()])?;
        Ok(request)
    }
    // Both the injected callback and production publisher are entered through catch-enabled
    // Function::call0. Restore the exact own descriptor (or remove our inherited-method shadow)
    // BEFORE assertions, even when an uncaught production exception returns Err from call0.
    fn with_replacement(&self, target: &JsValue, key: &str, replacement: &JsValue) -> Result<Result<JsValue, JsValue>, JsValue> {
        let object = field(&js_sys::global(), "Object")?;
        let descriptor = call(&object, "getOwnPropertyDescriptor", &[target.clone(), key.into()])?;
        let original = field(target, key)?;
        let host = self.host.clone();
        let operation = Closure::<dyn FnMut()>::new(move || super::publish(&host));
        let outcome = match Reflect::set(target, &key.into(), replacement) {
            Ok(true) => operation.as_ref().unchecked_ref::<Function>().call0(&JsValue::UNDEFINED),
            Ok(false) => Err(error("injected method is not writable")),
            Err(error) => Err(error),
        };
        if descriptor.is_undefined() {
            if !Reflect::delete_property(target.unchecked_ref::<Object>(), &key.into())? {
                return Err(error("could not remove injected method"));
            }
        } else {
            call(&object, "defineProperty", &[target.clone(), key.into(), descriptor.clone()])?;
        }
        let restored = call(&object, "getOwnPropertyDescriptor", &[target.clone(), key.into()])?;
        if field(target, key)? != original || restored.is_undefined() != descriptor.is_undefined() {
            return Err(error("injected property was not restored exactly"));
        }
        if !descriptor.is_undefined() {
            for attribute in ["value", "get", "set", "writable", "enumerable", "configurable"] {
                if field(&restored, attribute)? != field(&descriptor, attribute)? {
                    return Err(error("injected property descriptor changed"));
                }
            }
        }
        Ok(outcome)
    }
    fn fail_copy(&self) -> Result<u32, JsValue> {
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let fault = Closure::<dyn FnMut(JsValue) -> Result<JsValue, JsValue>>::new(move |_| {
            seen.set(seen.get() + 1);
            Err(js_sys::RangeError::new("synthetic CAD byte-copy failure").into())
        });
        let array = field(&js_sys::global(), "Uint8Array")?;
        self.with_replacement(&array, "from", fault.as_ref())??;
        Ok(calls.get())
    }
    fn fail_dispatch(&self, kind: &str) -> Result<u32, JsValue> {
        let original = field(&self.window, "dispatchEvent")?.dyn_into::<Function>()?;
        let window: JsValue = self.window.clone().into();
        let wanted = kind.to_string();
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let fault = Closure::<dyn FnMut(JsValue) -> Result<JsValue, JsValue>>::new(move |event: JsValue| {
            if text(&field(&event, "detail")?, "type")? == wanted {
                seen.set(seen.get() + 1);
                Err(js_sys::RangeError::new("synthetic CAD metadata dispatch failure").into())
            } else {
                original.call1(&window, &event)
            }
        });
        self.with_replacement(&self.window, "dispatchEvent", fault.as_ref())??;
        Ok(calls.get())
    }
}
impl Drop for Page {
    fn drop(&mut self) {
        let _ = self.window.remove_event_listener_with_callback(super::EVENT, self.listener.as_ref().unchecked_ref());
        let _ = Reflect::delete_property(self.window.unchecked_ref::<Object>(), &super::API.into());
    }
}

#[wasm_bindgen_test]
fn real_window_delivery_recovery_preserves_dxf_and_consumes_actual_cad_actions() -> Result<(), JsValue> {
    // Production codec hooks, on this fixture's fresh browser page only. Native app fixtures
    // use cadcraft_io directly and never replace their process-global test hooks.
    file::set_io(file::IoHooks {
        read: |bytes, name| cadcraft_io::read(bytes, name).map_err(|error| error.to_string()),
        write: |drawing, name| cadcraft_io::write(drawing, name).map_err(|error| error.to_string()),
        plot: None,
    });
    file::set_write_as(|drawing, name, options| {
        cadcraft_io::write_with(drawing, name, &cadcraft_io::SaveOptions { version: options.version, binary: options.binary })
            .map_err(|error| error.to_string())
    });
    let mut app = CadApp::new(Session::new(), Services { no_file_system: true, ..Default::default() });
    app.run("line", json!({"points":[[0,0],[10,10]]})).map_err(error)?;
    let page = Page::install(&app)?;
    let connection = page.api("registerProvider", &["fixture".into(), "Fixture storage".into(), "account-one".into()])?;
    let resources = Array::new();
    for id in ["file-1", "file-2"] {
        let resource = Object::new();
        Reflect::set(&resource, &"id".into(), &id.into())?;
        Reflect::set(&resource, &"name".into(), &"Fixture.dxf".into())?;
        Reflect::set(&resource, &"updatedAt".into(), &"".into())?;
        resources.push(&resource);
    }
    page.api("setResources", &[connection.clone(), resources.into()])?;
    let uid = app.session.state().map_err(error)?.uid;
    let checkpoint = app.session.state().map_err(error)?.saved.clone();
    let chooser = page.choose(&mut app)?;
    assert_eq!(page.fail_dispatch("saveDialog")?, 1);
    page.tick(&mut app);
    super::publish(&page.host);
    assert_eq!(text(&page.latest("saveFailed")?, "requestId")?, chooser);
    assert!(page.api("resolveSaveDialog", &[chooser.into(), "provider".into(), "Late.dxf".into()]).is_err());
    assert!(!page.host.busy());
    assert!(Arc::ptr_eq(&app.session.state().map_err(error)?.saved, &checkpoint));

    let read = page.api("requestOpen", &["file-2".into()])?;
    assert_eq!(page.fail_dispatch("openRequested")?, 1);
    let failed_read = page.latest("openFailed")?;
    assert_eq!(field(&failed_read, "requestId")?, read);
    assert_eq!(text(&failed_read, "resourceId")?, "file-2");
    assert_eq!(field(&failed_read, "connectionId")?, connection);
    assert!(!page.host.busy());

    let failed = page.captured_save(&mut app)?;
    let later_read = page.api("requestOpen", &["file-2".into()])?;
    app.set_status("Awaiting the synthetic byte-copy failure");
    assert_eq!(page.fail_copy()?, 1, "actual catch-enabled production copy must throw exactly once");
    assert!(!page.events.borrow().iter().any(|event| text(event, "type").is_ok_and(|kind| kind == "save")));
    assert_eq!(field(&page.latest("openRequested")?, "requestId")?, later_read);
    assert!(page.api("saveResult", &[failed.clone().into(), "saved".into(), "file-1".into()]).is_err());
    page.api("cancelOpen", &[later_read, "".into()])?;
    page.tick(&mut app);
    super::publish(&page.host);
    assert_eq!(text(&page.latest("saveFailed")?, "requestId")?, failed);
    let state = app.session.state().map_err(error)?;
    assert_eq!(state.uid, uid);
    assert!(state.is_dirty() && Arc::ptr_eq(&state.saved, &checkpoint));
    assert!(!page.host.busy() && page.host.bound_document("file-1").is_none());
    assert!(app.status.as_ref().is_some_and(|(message, _)| message.contains("File provider save failed")));

    let retry = page.captured_save(&mut app)?;
    assert_ne!(retry, failed);
    super::publish(&page.host);
    let saved = page.latest("save")?;
    assert_eq!(text(&saved, "requestId")?, retry);
    assert_eq!(text(&saved, "filename")?, "Fixture.dxf");
    assert_eq!(text(&saved, "documentId")?, uid.to_string());
    assert_eq!(field(&saved, "connectionId")?, connection);
    let bytes = field(&saved, "bytes")?.dyn_into::<Uint8Array>()?;
    assert!(bytes.length() > 0);
    let drawing = cadcraft_io::read(&bytes.to_vec(), "Fixture.dxf").map_err(error)?;
    assert_eq!(drawing.model.len(), app.session.state().map_err(error)?.doc.model.len());
    assert!(!drawing.model.is_empty());
    assert!(page.host.pending_for(uid) && app.session.state().map_err(error)?.is_dirty());
    let captured = app.session.state().map_err(error)?.doc.clone();
    let captured_revision = app.session.state().map_err(error)?.revision;
    app.run("line", json!({"points":[[20,20],[30,30]]})).map_err(error)?;
    page.api("saveResult", &[retry.clone().into(), "saved".into(), "file-1".into()])?;
    page.tick(&mut app);
    super::publish(&page.host);
    let terminal = page.latest("saved")?;
    assert_eq!(text(&terminal, "requestId")?, retry);
    assert_eq!(text(&terminal, "resourceId")?, "file-1");
    assert_eq!(text(&terminal, "revision")?, captured_revision.to_string());
    let state = app.session.state().map_err(error)?;
    assert!(state.is_dirty() && Arc::ptr_eq(&state.saved, &captured), "ACK owns its snapshot, never later edits");
    assert_eq!(page.host.bound_document("file-1"), Some(uid));
    assert!(!page.host.busy());

    let open = page.api("requestOpen", &["file-2".into()])?;
    super::publish(&page.host);
    page.api("openResource", &[open.clone(), "Fixture.dxf".into(), bytes.into()])?;
    page.tick(&mut app);
    super::publish(&page.host);
    let opened = page.latest("opened")?;
    assert_eq!(field(&opened, "requestId")?, open);
    assert_eq!(text(&opened, "resourceId")?, "file-2");
    assert_eq!(field(&opened, "connectionId")?, connection);
    assert_eq!(text(&opened, "namespace")?, "account-one");
    let imported = app.session.state().map_err(error)?;
    assert_ne!(imported.uid, uid);
    assert_eq!(imported.doc.model.len(), drawing.model.len());
    let original = app.session.docs.iter().find(|doc| doc.uid == uid).ok_or_else(|| error("original drawing was lost on provider open"))?;
    assert!(original.is_dirty() && Arc::ptr_eq(&original.saved, &captured));
    assert!(!page.host.busy());
    page.api("unregisterProvider", &[])?;
    Ok(())
}
