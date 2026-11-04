//! Optional same-origin storage capability. No provider is enabled by default.
//!
//! Requests own immutable native project captures. A host result never selects a live tab,
//! revision, filesystem path or account. This module contains no storage or authentication.
use cadcraft_engine::DocState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

mod linked;
use cadcraft_engine::linked_files::{Bytes, Input, ResourceSet};
pub(crate) use linked::Kind as LinkedKind;
pub use linked::LinkedPayload;
const MAX_BYTES: usize = 80 * 1024 * 1024;
const MAX_PENDING: usize = 16;
const MAX_EVENTS: usize = 64;
const MAX_RESOURCES: usize = 100;
const EXTENSION: &str = ".dxf";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resource {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub updated_at: String,
}
pub struct Event {
    pub metadata: Value,
    pub bytes: Option<Vec<u8>>,
    pub linked: Option<LinkedPayload>,
}
impl Event {
    fn terminal(&self) -> bool {
        matches!(
            self.metadata.get("type").and_then(Value::as_str),
            Some(
                "saved"
                    | "downloaded"
                    | "saveCancelled"
                    | "saveFailed"
                    | "opened"
                    | "openFailed"
                    | "openCancelled"
                    | "resourcesUpdated"
                    | "resourcesFailed"
                    | "resourcesCancelled"
            )
        )
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Identity {
    provider_id: String,
    label: String,
    namespace: String,
}
/// The exact states encoded at Save, including a main document's branch tabs where applicable.
pub(crate) struct Capture {
    pub doc: u64,
    pub revision: u64,
    pub snapshots: Vec<DocState>,
    pub name: String,
    intent: String,
    destination: String,
    bytes: Option<Vec<u8>>,
    linked_main: Option<Arc<Bytes>>,
    linked_key: String,
    pub(crate) linked_set: Option<Arc<ResourceSet>>,
    pub(crate) ack_version: Option<String>,
}
pub(crate) enum Action {
    Linked { request: String, input: Input },
    ResourceFailed { request: String, capture: Capture, error: String },
    LinkedOpenFailed { request: String, resource: String, error: String },
    Save { request: String, capture: Capture, status: String, resource: String },
    Open { request: String, resource: String, name: String, bytes: Vec<u8> },
}
#[derive(Clone, Copy)]
enum TerminalOwner {
    Open,
    Save,
}
#[derive(Clone)]
struct Terminal {
    request: String,
    metadata: Value,
    owner: TerminalOwner,
}
#[derive(Default)]
struct State {
    provider: Option<Identity>,
    linked_enabled: bool,
    updates: HashMap<String, Capture>,
    queued_sets: HashSet<String>,
    ready_sets: HashMap<String, Arc<ResourceSet>>,
    versions: HashMap<u64, String>,
    enabled: bool,
    nonce: String,
    serial: u64,
    connection: String,
    resources: Vec<Resource>,
    bindings: HashMap<u64, String>,
    names: HashMap<u64, String>,
    dialogs: HashMap<String, Capture>,
    pending: HashMap<String, Capture>,
    // Keep every captured document alias owned until its terminal event can be queued.
    finishing_saves: HashMap<String, Vec<u64>>,
    opens: HashMap<String, String>,
    queued_opens: HashSet<String>,
    actions: Vec<Action>,
    // Only already admitted owners enter this inbox: at most16 saves/refreshes plus16 reads, no transfer bytes.
    delivery_failures: Vec<Action>,
    events: Vec<Event>,
    // At most MAX_PENDING saves plus MAX_PENDING reads; never stores transfer bytes.
    finished: VecDeque<Terminal>,
    publishing_terminals: usize,
    publishing_bytes: usize,
    documents: Value,
}
impl State {
    fn busy(&self) -> bool {
        !self.updates.is_empty()
            || !self.queued_sets.is_empty()
            || !self.ready_sets.is_empty()
            || !self.dialogs.is_empty()
            || !self.pending.is_empty()
            || !self.finishing_saves.is_empty()
            || !self.opens.is_empty()
            || !self.actions.is_empty()
            || !self.delivery_failures.is_empty()
            || !self.finished.is_empty()
            || self.events.iter().any(Event::terminal)
            || self.publishing_terminals != 0
    }
    fn next(&mut self, kind: &str) -> Result<String, String> {
        if self.nonce.is_empty() {
            return Err("file provider session is not initialized".into());
        }
        self.serial = self.serial.checked_add(1).ok_or("file provider request counter exhausted")?;
        Ok(format!("{}-{kind}-{}", self.nonce, self.serial))
    }
    fn event(&mut self, mut metadata: Value, bytes: Option<Vec<u8>>) {
        if let (Some(provider), Some(fields)) = (&self.provider, metadata.as_object_mut()) {
            fields.insert("providerId".into(), provider.provider_id.clone().into());
            fields.insert("namespace".into(), provider.namespace.clone().into());
            fields.insert("connectionId".into(), self.connection.clone().into());
        }
        self.events.push(Event { metadata, bytes, linked: None });
    }
    fn retained_bytes(&self) -> usize {
        let captures = self.dialogs.values().chain(self.pending.values()).map(|p| p.bytes.as_ref().map_or(0, Vec::len));
        let events = self.events.iter().map(|e| e.bytes.as_ref().map_or(0, Vec::len));
        let actions = self.actions.iter().chain(&self.delivery_failures).map(|a| match a {
            Action::Open { bytes, .. } => bytes.len(),
            _ => 0,
        });
        captures.chain(events).chain(actions).fold(0, usize::saturating_add).saturating_add(Bytes::retained()).saturating_add(self.publishing_bytes)
    }
    fn pending_for(&self, doc: u64) -> bool {
        self.dialogs.values().chain(self.pending.values()).chain(self.updates.values()).any(|c| c.snapshots.iter().any(|d| d.uid == doc))
            || self.finishing_saves.values().any(|aliases| aliases.contains(&doc))
            || self.actions.iter().any(|a| matches!(a, Action::Save { capture, .. } if capture.snapshots.iter().any(|d| d.uid == doc)))
    }
    fn flush_finished(&mut self) {
        while self.events.len() < MAX_EVENTS {
            let Some(terminal) = self.finished.pop_front() else { break };
            self.event(terminal.metadata, None);
            match terminal.owner {
                TerminalOwner::Open => {
                    self.opens.remove(&terminal.request);
                    self.queued_opens.remove(&terminal.request);
                }
                TerminalOwner::Save => {
                    self.finishing_saves.remove(&terminal.request);
                }
            }
        }
    }
    fn take_events(&mut self) -> Vec<Event> {
        let events = std::mem::take(&mut self.events);
        self.flush_finished();
        if events.is_empty() { std::mem::take(&mut self.events) } else { events }
    }
    fn fail_delivery(&mut self, metadata: &Value) {
        let Some(request) = metadata.get("requestId").and_then(Value::as_str) else { return };
        // A synchronous completeResourceSet already owns the reply, even if dispatch later throws.
        if metadata.get("type").and_then(Value::as_str) == Some("resourceSetNeeded") && self.queued_sets.contains(request) {
            return;
        }
        if metadata.get("type").and_then(Value::as_str) == Some("resourceSetNeeded") && self.updates.contains_key(request) {
            self.fail_update(request, "linked resource event delivery failed".into());
            return;
        }
        match metadata.get("type").and_then(Value::as_str) {
            Some(kind @ ("save" | "saveDialog" | "resourceSetNeeded")) => {
                let capture = if kind == "saveDialog" { self.dialogs.remove(request) } else { self.pending.remove(request) };
                if let Some(mut capture) = capture {
                    capture.bytes = None;
                    capture.linked_main = None;
                    capture.linked_set = None;
                    self.finishing_saves.insert(request.into(), capture.snapshots.iter().map(|doc| doc.uid).collect());
                    self.delivery_failures.push(Action::Save { request: request.into(), capture, status: "failed".into(), resource: String::new() });
                }
            }
            Some("openRequested") if !self.queued_opens.contains(request) => {
                if let Some(resource) = self.opens.get(request).cloned() {
                    // The terminal can wait behind a full event queue. Reject late payloads
                    // now, while retaining the read's account owner until it can be queued.
                    self.queued_opens.insert(request.into());
                    self.finished.push_back(Terminal {
                        request: request.into(),
                        metadata: json!({"type":"openFailed","requestId":request,"resourceId":resource,"error":"file-provider event delivery failed"}),
                        owner: TerminalOwner::Open,
                    });
                    self.flush_finished();
                }
            }
            _ => {}
        }
    }
}
#[derive(Clone, Default)]
pub struct Host(Arc<Mutex<State>>);
struct PublishingBatch {
    host: Host,
    terminals: usize,
    bytes: usize,
}
impl Drop for PublishingBatch {
    fn drop(&mut self) {
        let mut state = self.host.state();
        state.publishing_terminals = state.publishing_terminals.saturating_sub(self.terminals);
        state.publishing_bytes = state.publishing_bytes.saturating_sub(self.bytes);
    }
}
impl Host {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn set_session(&self, nonce: String) -> Result<(), String> {
        let mut s = self.state();
        if !s.nonce.is_empty() || s.provider.is_some() || !valid_id(&nonce) || nonce.len() < 16 {
            return Err("invalid or already initialized file provider session".into());
        }
        s.nonce = nonce;
        Ok(())
    }
    pub fn connect(&self, provider_id: String, label: String, namespace: String) -> Result<String, String> {
        if !valid_id(&provider_id) || !valid_label(&label) || !valid_id(&namespace) {
            return Err("invalid file provider identity".into());
        }
        let mut s = self.state();
        let identity = Identity { provider_id, label, namespace };
        if let Some(current) = &s.provider {
            return if *current == identity { Ok(s.connection.clone()) } else { Err("unregister before changing provider or account".into()) };
        }
        s.connection = s.next("connection")?;
        s.provider = Some(identity);
        s.enabled = true;
        Ok(s.connection.clone())
    }
    pub fn disconnect(&self) -> Result<(), String> {
        let mut s = self.state();
        if s.busy() {
            return Err("finish or cancel pending operations and consume completion events before unregistering".into());
        }
        s.provider = None;
        s.enabled = false;
        s.connection.clear();
        s.resources.clear();
        s.bindings.clear();
        s.names.clear();
        s.versions.clear();
        s.linked_enabled = false;
        s.events.clear();
        s.documents = Value::Null;
        Ok(())
    }
    pub fn connected(&self) -> bool {
        self.state().provider.is_some()
    }
    pub fn enabled(&self) -> bool {
        let s = self.state();
        s.provider.is_some() && s.enabled
    }
    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let mut s = self.state();
        if s.provider.is_none() {
            return Err("file provider is not connected".into());
        }
        s.enabled = enabled;
        Ok(())
    }
    pub fn busy(&self) -> bool {
        self.state().busy()
    }
    pub fn pending_for(&self, doc: u64) -> bool {
        self.state().pending_for(doc)
    }
    pub fn bound_document(&self, resource: &str) -> Option<u64> {
        self.state().bindings.iter().find(|(_, r)| r.as_str() == resource).map(|(d, _)| *d)
    }
    pub fn bound_filename(&self, doc: u64) -> Option<String> {
        self.state().names.get(&doc).cloned()
    }
    /// Captures are produced synchronously by the editor before this event is issued.
    pub(crate) fn begin(
        &self,
        snapshots: Vec<DocState>,
        doc: u64,
        revision: u64,
        name: String,
        bytes: Vec<u8>,
        save_as: bool,
    ) -> Result<String, String> {
        let mut s = self.state();
        if s.provider.is_none()
            || !s.enabled
            || !valid_native_name(&name)
            || bytes.is_empty()
            || bytes.len() > MAX_BYTES
            || s.retained_bytes().saturating_add(bytes.len()) > MAX_BYTES
            || s.pending_for(doc)
            || s.dialogs.len() + s.pending.len() + s.finishing_saves.len() + s.updates.len() >= MAX_PENDING
            || s.events.len() >= MAX_EVENTS
            || !snapshots.iter().any(|snapshot| snapshot.uid == doc)
        {
            return Err("provider unavailable, a save is pending, or capture exceeds 80 MiB".into());
        }
        let request = s.next("dialog")?;
        let intent = if save_as { "saveAs" } else { "save" }.to_string();
        s.event(json!({"type":"saveDialog","requestId":request,"documentId":doc.to_string(),"revision":revision.to_string(),"intent":intent,"filename":name}), None);
        s.dialogs.insert(
            request.clone(),
            Capture {
                doc,
                revision,
                snapshots,
                name,
                intent,
                destination: String::new(),
                bytes: Some(bytes),
                linked_main: None,
                linked_key: String::new(),
                linked_set: None,
                ack_version: None,
            },
        );
        Ok(request)
    }
    pub fn resolve_dialog(&self, request: &str, destination: &str, name: &str) -> Result<(), String> {
        if self.state().dialogs.get(request).is_some_and(|c| c.linked_main.is_some()) {
            return self.resolve_linked_dialog(request, destination, name);
        }
        if !matches!(destination, "provider" | "device" | "cancel") || (destination != "cancel" && !valid_native_name(name)) {
            return Err("choose provider, device or cancel and a native project filename".into());
        }
        let mut s = self.state();
        if s.events.len() >= MAX_EVENTS {
            return Err("provider event queue is full".into());
        }
        let mut capture = s.dialogs.remove(request).ok_or("unknown or completed save dialog")?;
        if destination == "cancel" {
            capture.bytes = None;
            s.finishing_saves.insert(request.into(), capture.snapshots.iter().map(|doc| doc.uid).collect());
            s.actions.push(Action::Save { request: request.into(), capture, status: "cancel".into(), resource: String::new() });
            return Ok(());
        }
        capture.name = name.into();
        capture.destination = destination.into();
        let resource = if capture.intent == "save" { s.bindings.get(&capture.doc).cloned() } else { None };
        let bytes = capture.bytes.take();
        s.event(json!({"type":"save","requestId":request,"documentId":capture.doc.to_string(),"revision":capture.revision.to_string(),"intent":capture.intent,"filename":name,"resourceId":resource,"destination":destination}), bytes);
        s.pending.insert(request.into(), capture);
        Ok(())
    }
    pub fn result(&self, request: String, status: String, resource: String) -> Result<(), String> {
        self.result_inner(request, status, resource, None)
    }
    fn result_inner(&self, request: String, status: String, resource: String, receipt: Option<(String, String)>) -> Result<(), String> {
        if !matches!(status.as_str(), "saved" | "download" | "failed" | "cancel") || (!resource.is_empty() && !valid_id(&resource)) {
            return Err("invalid provider result".into());
        }
        let mut s = self.state();
        if s.queued_sets.contains(&request) {
            return Err("resource reply already queued".into());
        }
        let capture = s.pending.get(&request).ok_or("unknown or completed save request")?;
        if capture.linked_main.is_some() && matches!(status.as_str(), "saved" | "download") {
            let set = capture.linked_set.as_ref().ok_or("complete linked resources before reporting success")?;
            let (digest, version) = receipt.as_ref().ok_or("linked save requires saveResourceSetResult")?;
            if digest != set.digest() || (status == "saved" && (!valid_id(version))) {
                return Err("linked set digest/version receipt differs".into());
            }
        }
        if (status == "saved" && (capture.destination != "provider" || !valid_id(&resource)))
            || (status == "download" && capture.destination != "device")
        {
            return Err("result does not match the chosen destination".into());
        }
        if status == "saved" && s.bindings.iter().any(|(doc, id)| *doc != capture.doc && *id == resource) {
            return Err("resource is bound to another open document".into());
        }
        if status == "saved" && capture.intent == "saveAs" && s.bindings.get(&capture.doc) == Some(&resource) {
            return Err("Save As must produce a distinct resource".into());
        }
        if s.actions.len() >= MAX_PENDING {
            return Err("provider command queue is full".into());
        }
        let mut capture = s.pending.remove(&request).ok_or("unknown save request")?;
        capture.ack_version = receipt.map(|(_, version)| version);
        s.finishing_saves.insert(request.clone(), capture.snapshots.iter().map(|doc| doc.uid).collect());
        s.actions.push(Action::Save { request, capture, status, resource });
        Ok(())
    }
    pub fn set_resources(&self, connection: &str, resources: Vec<Resource>) -> Result<(), String> {
        let mut seen = HashSet::new();
        if resources.len() > MAX_RESOURCES
            || resources.iter().any(|r| !valid_id(&r.id) || !valid_native_name(&r.name) || r.updated_at.len() > 100 || !seen.insert(r.id.clone()))
        {
            return Err("invalid native project resource list".into());
        }
        let mut s = self.state();
        if s.provider.is_none() || s.connection != connection {
            return Err("resource list belongs to another connection".into());
        }
        s.resources = resources;
        Ok(())
    }
    pub fn request_open(&self, resource: &str) -> Result<String, String> {
        let mut s = self.state();
        if !s.enabled
            || s.provider.is_none()
            || s.opens.len() >= MAX_PENDING
            || s.events.len() >= MAX_EVENTS
            || !s.resources.iter().any(|r| r.id == resource)
        {
            return Err("resource is unavailable in the current provider account".into());
        }
        let request = s.next("open")?;
        s.opens.insert(request.clone(), resource.into());
        s.event(json!({"type":"openRequested","requestId":request,"resourceId":resource}), None);
        Ok(request)
    }
    pub fn open(&self, request: String, name: String, bytes: Vec<u8>) -> Result<(), String> {
        if self.linked_enabled() {
            return Err("linkedFilesV1 reads require completeResourceSet".into());
        }
        if !valid_native_name(&name) || bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err("invalid native project resource".into());
        }
        let mut s = self.state();
        if s.actions.len() >= MAX_PENDING || s.retained_bytes().saturating_add(bytes.len()) > MAX_BYTES {
            return Err("provider command queue is full".into());
        }
        if s.queued_opens.contains(&request) {
            return Err("open response already queued".into());
        }
        let resource = s.opens.get(&request).cloned().ok_or("unknown or completed open request")?;
        s.queued_opens.insert(request.clone());
        s.actions.push(Action::Open { request, resource, name, bytes });
        Ok(())
    }
    pub fn cancel_open(&self, request: String, error: String) -> Result<(), String> {
        if error.len() > 1024 {
            return Err("invalid open result".into());
        }
        let mut s = self.state();
        if s.queued_opens.contains(&request) {
            return Err("open response already queued".into());
        }
        if s.events.len() >= MAX_EVENTS {
            return Err("provider event queue is full".into());
        }
        let resource = s.opens.remove(&request).ok_or("unknown or completed open request")?;
        s.event(json!({"type":if error.is_empty(){"openCancelled"}else{"openFailed"},"requestId":request,"resourceId":resource,"error":error}), None);
        Ok(())
    }
    pub(crate) fn take_actions(&self) -> Vec<Action> {
        let mut state = self.state();
        let mut actions = std::mem::take(&mut state.actions);
        actions.append(&mut state.delivery_failures);
        actions
    }
    pub fn take_events(&self) -> Vec<Event> {
        self.state().take_events()
    }
    /// Failed publication consumes only a still-owned request, never an accepted callback.
    /// Recovery actions retain captured aliases until the real application produces a terminal.
    /// Returns whether the platform must request a repaint for failure consumption.
    pub fn publish_events_with<E>(&self, mut dispatch: impl FnMut(&Value, Option<Vec<u8>>) -> Result<(), E>, on_error: impl FnMut(E)) -> bool {
        self.publish_owned_events_with(
            |event| {
                if event.linked.is_some() { Err(None) } else { dispatch(&event.metadata, event.bytes).map_err(Some) }
            },
            on_error,
        )
    }
    /// The complete-set publisher keeps the same atomic publication/terminal ownership guards.
    pub fn publish_linked_events_with<E>(
        &self,
        mut dispatch: impl FnMut(&Value, Option<Vec<u8>>, Option<LinkedPayload>) -> Result<(), E>,
        on_error: impl FnMut(E),
    ) -> bool {
        self.publish_owned_events_with(|event| dispatch(&event.metadata, event.bytes, event.linked).map_err(Some), on_error)
    }
    fn publish_owned_events_with<E>(&self, mut dispatch: impl FnMut(Event) -> Result<(), Option<E>>, mut on_error: impl FnMut(E)) -> bool {
        let mut failed = false;
        for _ in 0..2 {
            let (events, _publishing) = {
                let mut state = self.state();
                let events = state.take_events();
                let terminals = events.iter().filter(|event| event.terminal()).count();
                let bytes = events.iter().map(|event| event.bytes.as_ref().map_or(0, Vec::len)).fold(0usize, usize::saturating_add);
                state.publishing_terminals = state.publishing_terminals.saturating_add(terminals);
                state.publishing_bytes = state.publishing_bytes.saturating_add(bytes);
                (events, PublishingBatch { host: self.clone(), terminals, bytes })
            };
            if events.is_empty() {
                break;
            }
            for event in events {
                let metadata = event.metadata.clone();
                if let Err(error) = dispatch(event) {
                    failed = true;
                    self.state().fail_delivery(&metadata);
                    if let Some(error) = error {
                        on_error(error);
                    }
                }
            }
        }
        failed
    }
    pub fn snapshot(&self) -> Value {
        let s = self.state();
        json!({"protocolVersion":1,"enabled":s.enabled && s.provider.is_some(),"provider":s.provider.as_ref().map(|p|json!({"id":p.provider_id,"label":p.label,"namespace":p.namespace})),"connectionId":s.connection,"resources":s.resources,"documents":s.documents,"pending":s.busy()})
    }
    pub(crate) fn refresh(&self, docs: Vec<(u64, u64, String, bool)>) {
        let mut s = self.state();
        let live: HashSet<_> = docs.iter().map(|d| d.0).collect();
        s.bindings.retain(|id, _| live.contains(id));
        s.names.retain(|id, _| live.contains(id));
        s.versions.retain(|id, _| live.contains(id));
        s.documents = json!(docs.into_iter().map(|(id,rev,name,dirty)|json!({"id":id.to_string(),"revision":rev.to_string(),"name":name,"dirty":dirty,"resourceId":s.bindings.get(&id)})).collect::<Vec<_>>());
    }
    pub(crate) fn finish_save(&self, request: &str, capture: &Capture, status: &str, resource: &str, alive: bool) {
        let mut s = self.state();
        if !s.finishing_saves.get(request).is_some_and(|aliases| aliases.contains(&capture.doc))
            || s.finished.iter().any(|terminal| terminal.request == request)
        {
            return;
        }
        if status == "saved" {
            if alive {
                s.bindings.insert(capture.doc, resource.into());
                s.names.insert(capture.doc, capture.name.clone());
                if let Some(version) = &capture.ack_version {
                    s.versions.insert(capture.doc, version.clone());
                }
            }
            s.resources.retain(|r| r.id != resource);
            s.resources.insert(0, Resource { id: resource.into(), name: capture.name.clone(), updated_at: String::new() });
            s.resources.truncate(MAX_RESOURCES);
        }
        let mut metadata = json!({"type":match status { "saved"=>"saved", "download"=>"downloaded", "cancel"=>"saveCancelled", _=>"saveFailed" },"requestId":request,"documentId":capture.doc.to_string(),"revision":capture.revision.to_string(),"resourceId":resource,"filename":capture.name,"documentOpen":alive});
        if let Some(set) = &capture.linked_set {
            metadata["setDigest"] = set.digest().into();
            metadata["versionToken"] = capture.ack_version.clone().into();
        }
        s.finished.push_back(Terminal { request: request.into(), metadata, owner: TerminalOwner::Save });
        s.flush_finished();
    }
    pub(crate) fn finish_open(&self, request: &str, resource: &str, name: &str, result: Result<u64, String>) {
        let mut s = self.state();
        if !s.opens.get(request).is_some_and(|owned| owned == resource)
            || !s.queued_opens.contains(request)
            || s.finished.iter().any(|terminal| terminal.request == request)
        {
            return;
        }
        let metadata = match result {
            Ok(doc) => {
                s.bindings.insert(doc, resource.into());
                s.names.insert(doc, name.into());
                json!({"type":"opened","requestId":request,"resourceId":resource,"documentId":doc.to_string()})
            }
            Err(error) => json!({"type":"openFailed","requestId":request,"resourceId":resource,"error":bounded_error(&error)}),
        };
        s.finished.push_back(Terminal { request: request.into(), metadata, owner: TerminalOwner::Open });
        s.flush_finished();
    }
    /// Draw the recent lists and return a selected resource; the caller dispatches its command.
    pub fn recent_ui(&self, ui: &mut egui::Ui) -> Option<String> {
        let selected = self.provider_recent_ui(ui);
        self.device_recent_ui(ui);
        selected
    }
    pub(crate) fn provider_recent_ui(&self, ui: &mut egui::Ui) -> Option<String> {
        let (label, resources, enabled) = {
            let s = self.state();
            (s.provider.as_ref().map(|p| p.label.clone()), s.resources.clone(), s.enabled)
        };
        let label = label.filter(|_| enabled)?;
        ui.add_space(12.0);
        ui.strong(format!("Recent files — {label}"));
        if resources.is_empty() {
            ui.weak("No recent files from this provider");
        }
        let mut selected = None;
        for resource in resources {
            if ui.button(&resource.name).on_hover_text(&resource.updated_at).clicked() {
                selected = Some(resource.id);
                ui.ctx().request_repaint();
            }
        }
        selected
    }
    pub(crate) fn device_recent_ui(&self, ui: &mut egui::Ui) {
        if self.enabled() {
            ui.separator();
            ui.weak("Recent files on this device");
        }
    }
}
fn bounded_error(message: &str) -> String {
    let mut error = String::new();
    for ch in message.chars().filter(|ch| !ch.is_control()) {
        if error.len().saturating_add(ch.len_utf8()) > 1024 {
            break;
        }
        error.push(ch);
    }
    error
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 160 && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}
fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 100 && !value.chars().any(char::is_control)
}
fn valid_native_name(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 255
        && !value.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
        && value.to_ascii_lowercase().ends_with(EXTENSION)
}
#[cfg(test)]
mod tests;
