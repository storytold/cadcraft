//! The optional linkedFilesV1 owners share the ordinary provider queues and terminal guards.
use super::{Action, Capture, Host, MAX_BYTES, MAX_EVENTS, MAX_PENDING, State, Terminal, TerminalOwner, bounded_error, valid_id, valid_native_name};
use cadcraft_engine::{
    DocState,
    linked_files::{self, Bytes, Input, ResourceSet},
};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub enum LinkedPayload {
    Main { key: String, bytes: Arc<Bytes> },
    Set(Arc<ResourceSet>),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Open,
    Save,
    Refresh,
}
pub(crate) struct Context {
    pub kind: Kind,
    pub main: Option<Arc<Bytes>>,
    pub name: String,
    pub key: String,
    pub doc: u64,
    pub revision: u64,
    pub resource: String,
}
impl State {
    fn linked_event(&mut self, metadata: Value, payload: LinkedPayload) {
        self.event(metadata, None);
        if let Some(event) = self.events.last_mut() {
            event.linked = Some(payload);
        }
    }
    pub(super) fn fail_update(&mut self, request: &str, error: String) {
        if let Some(mut capture) = self.updates.remove(request) {
            capture.linked_main = None;
            capture.linked_set = None;
            self.queued_sets.remove(request);
            self.ready_sets.remove(request);
            self.finishing_saves.insert(request.into(), capture.snapshots.iter().map(|s| s.uid).collect());
            self.delivery_failures.push(Action::ResourceFailed { request: request.into(), capture, error });
        }
    }
    fn owner_count(&self) -> usize {
        self.dialogs.len() + self.pending.len() + self.finishing_saves.len() + self.updates.len() + self.opens.len()
    }
    fn context(&self, request: &str) -> Result<Context, String> {
        let capture = self.pending.get(request).map(|c| (Kind::Save, c)).or_else(|| self.updates.get(request).map(|c| (Kind::Refresh, c)));
        if let Some((kind, capture)) = capture {
            if capture.linked_main.is_none() {
                return Err("resource set is not awaiting supplied bytes".into());
            }
            return Ok(Context {
                kind,
                main: capture.linked_main.clone(),
                name: capture.name.clone(),
                key: capture.linked_key.clone(),
                doc: capture.doc,
                revision: capture.revision,
                resource: String::new(),
            });
        }
        if let Some(resource) = self.opens.get(request) {
            return Ok(Context {
                kind: Kind::Open,
                main: None,
                name: String::new(),
                key: String::new(),
                doc: 0,
                revision: 0,
                resource: resource.clone(),
            });
        }
        Err("unknown or completed resource-set request".into())
    }
}
impl Host {
    pub fn linked_enabled(&self) -> bool {
        let s = self.state();
        s.provider.is_some() && s.linked_enabled
    }
    pub fn configure_linked_files(&self, connection: &str, version: u32) -> Result<(), String> {
        let mut s = self.state();
        if s.provider.is_none() || s.connection != connection || version != 1 || s.busy() {
            return Err("configure linkedFilesV1 only for the current idle connection".into());
        }
        s.linked_enabled = true;
        Ok(())
    }
    pub(crate) fn begin_linked(&self, snapshot: DocState, name: String, main: Arc<Bytes>, save_as: bool) -> Result<String, String> {
        let mut s = self.state();
        if !s.enabled
            || !s.linked_enabled
            || s.provider.is_none()
            || !valid_native_name(&name)
            || s.pending_for(snapshot.uid)
            || s.owner_count() >= MAX_PENDING
            || s.events.len() >= MAX_EVENTS
            || s.retained_bytes() > MAX_BYTES
        {
            return Err("linked capture unavailable, pending or over budget".into());
        }
        if snapshot
            .resources
            .as_ref()
            .and_then(|r| r.authority.as_ref())
            .is_some_and(|owner| s.provider.as_ref().is_some_and(|p| owner != &(p.provider_id.clone(), p.namespace.clone())))
            && !save_as
        {
            return Err("exporting another account's linked drawing requires explicit Save As".into());
        }
        if !save_as && s.bindings.contains_key(&snapshot.uid) && !s.versions.contains_key(&snapshot.uid) {
            return Err("legacy binding has no full-set version; use Save As or explicitly reopen a clean complete set".into());
        }
        let request = s.next("dialog")?;
        let intent = if save_as { "saveAs" } else { "save" }.to_string();
        let key = snapshot.resources.as_ref().map(|set| set.main.key.clone()).unwrap_or_default();
        s.event(
            json!({"type":"saveDialog","requestId":request,"documentId":snapshot.uid.to_string(),"revision":snapshot.revision.to_string(),
                      "intent":intent,"filename":name,"linkedFilesVersion":1}),
            None,
        );
        let capture = Capture {
            doc: snapshot.uid,
            revision: snapshot.revision,
            snapshots: vec![snapshot],
            name,
            intent,
            destination: String::new(),
            bytes: None,
            linked_main: Some(main),
            linked_key: key,
            linked_set: None,
            ack_version: None,
        };
        s.dialogs.insert(request.clone(), capture);
        Ok(request)
    }
    pub(crate) fn resolve_linked_dialog(&self, request: &str, destination: &str, name: &str) -> Result<(), String> {
        if !matches!(destination, "provider" | "device" | "cancel") || (destination != "cancel" && !valid_native_name(name)) {
            return Err("choose provider/device/cancel and a native DXF filename".into());
        }
        let mut s = self.state();
        if s.events.len() >= MAX_EVENTS {
            return Err("provider event queue is full".into());
        }
        if destination == "cancel" && s.actions.len() >= MAX_PENDING {
            return Err("provider command queue is full; retry cancellation".into());
        }
        let mut capture = s.dialogs.remove(request).ok_or("unknown save dialog")?;
        if destination == "cancel" {
            capture.linked_main = None;
            s.finishing_saves.insert(request.into(), capture.snapshots.iter().map(|v| v.uid).collect());
            s.actions.push(Action::Save { request: request.into(), capture, status: "cancel".into(), resource: String::new() });
            return Ok(());
        }
        capture.destination = destination.into();
        capture.name = name.into();
        let source = capture.snapshots.iter().find(|v| v.uid == capture.doc).and_then(|v| v.resources.as_deref());
        let main = capture.linked_main.clone().ok_or("linked main capture is unavailable")?;
        let p = s.provider.as_ref().ok_or("provider disconnected")?;
        let fresh_grant = destination == "provider" && source.is_some_and(|set| !set.granted_to(&p.provider_id, &p.namespace, &s.connection));
        let cached = if fresh_grant {
            Err("a fresh current-connection resource grant is required".into())
        } else {
            crate::file_provider_links::cached(main.clone(), name, source)
        };
        s.pending.insert(request.into(), capture);
        match cached {
            Ok(set) => {
                drop(s);
                self.finish_linked_resolution(request, set)
            }
            Err(_) => {
                let c = s.pending.get(request).ok_or("missing linked owner")?;
                let metadata = json!({"type":"resourceSetNeeded","operation":"save","requestId":request,"documentId":c.doc.to_string(),
                    "revision":c.revision.to_string(),"filename":c.name,"destination":c.destination,"mainLogicalKey":c.linked_key,"mainSha256":main.hash(),"requiresCurrentGrant":fresh_grant});
                let key = c.linked_key.clone();
                s.linked_event(metadata, LinkedPayload::Main { key, bytes: main });
                Ok(())
            }
        }
    }
    /// Admission uses declared hashes/lengths before the WASM adapter copies any payload buffers.
    pub fn preflight_linked_bytes(&self, additional: usize) -> Result<(), String> {
        if self.state().retained_bytes().saturating_add(additional) > MAX_BYTES {
            return Err("retained linked and ordinary transfers exceed 80 MiB".into());
        }
        Ok(())
    }
    /// A catch-enabled asset copy failure retires only the current, still-owned request.
    /// Its terminal is produced by the actual app consumer even when normal actions are full.
    pub fn fail_linked_copy(&self, connection: &str, request: &str) {
        let mut s = self.state();
        if s.connection != connection || s.queued_sets.contains(request) {
            return;
        }
        let Ok(context) = s.context(request) else { return };
        let error = "linked resource byte copy failed".to_string();
        match context.kind {
            Kind::Save => s.fail_delivery(&json!({"type":"resourceSetNeeded","requestId":request})),
            Kind::Refresh => s.fail_update(request, error),
            Kind::Open => {
                if s.queued_opens.contains(request) {
                    return;
                }
                s.queued_sets.insert(request.into());
                s.queued_opens.insert(request.into());
                s.delivery_failures.push(Action::LinkedOpenFailed { request: request.into(), resource: context.resource, error });
            }
        }
    }
    pub fn preflight_linked_reply(&self, connection: &str, request: &str) -> Result<(), String> {
        let s = self.state();
        if !s.linked_enabled
            || s.connection != connection
            || s.queued_sets.contains(request)
            || s.actions.len() >= MAX_PENDING
            || s.pending.get(request).is_some_and(|capture| capture.linked_set.is_some())
        {
            return Err("resource reply is stale, already queued or unavailable".into());
        }
        s.context(request)?;
        Ok(())
    }
    pub fn complete_resource_set(&self, connection: &str, request: String, input: Input) -> Result<(), String> {
        let mut s = self.state();
        if !s.linked_enabled
            || s.provider.is_none()
            || s.connection != connection
            || s.queued_sets.contains(&request)
            || s.pending.get(&request).is_some_and(|capture| capture.linked_set.is_some())
            || s.actions.len() >= MAX_PENDING
            || s.retained_bytes() > MAX_BYTES
            || input.files.len() >= linked_files::MAX_FILES
            || input.edges.len() > linked_files::MAX_EDGES
        {
            return Err("resource reply unavailable, already queued or over capacity".into());
        }
        let context = s.context(&request)?;
        if context.kind == Kind::Open {
            if s.queued_opens.contains(&request) || input.main.is_none() || !valid_id(&input.version) {
                return Err("open requires complete native bytes and an opaque committed version".into());
            }
            s.queued_opens.insert(request.clone());
        } else if context.main.as_ref().is_none_or(|main| main.hash() != input.main_hash)
            || context.name != input.name
            || (!context.key.is_empty() && context.key != input.main_key)
        {
            return Err("resource response differs from its captured drawing/name/logical root".into());
        }
        s.queued_sets.insert(request.clone());
        s.actions.push(Action::Linked { request, input });
        Ok(())
    }
    pub fn cancel_resource_set(&self, connection: &str, request: String, error: String) -> Result<(), String> {
        if error.len() > 1024 {
            return Err("resource error is too large".into());
        }
        let mut s = self.state();
        if s.connection != connection || !s.linked_enabled || s.queued_sets.contains(&request) {
            return Err("resource response is stale or already queued".into());
        }
        let kind = s.context(&request)?.kind;
        if kind == Kind::Refresh {
            s.fail_update(&request, error);
            return Ok(());
        }
        drop(s);
        if kind == Kind::Open {
            self.cancel_open(request, error)
        } else {
            self.result(request, if error.is_empty() { "cancel" } else { "failed" }.into(), String::new())
        }
    }
    pub fn save_resource_set_result(
        &self,
        connection: &str,
        request: String,
        status: String,
        resource: String,
        digest: String,
        version: String,
    ) -> Result<(), String> {
        {
            let s = self.state();
            if s.connection != connection || !s.linked_enabled {
                return Err("stale resource-set result connection".into());
            }
        }
        self.result_inner(request, status, resource, Some((digest, version)))
    }
    pub(crate) fn linked_context(&self, request: &str) -> Result<Context, String> {
        self.state().context(request)
    }
    pub(crate) fn finish_linked_resolution(&self, request: &str, set: Arc<ResourceSet>) -> Result<(), String> {
        let mut s = self.state();
        if s.events.len() >= MAX_EVENTS {
            return Err("provider event queue is full".into());
        }
        let (provider, namespace) = s.provider.as_ref().map(|p| (p.provider_id.clone(), p.namespace.clone())).ok_or("provider disconnected")?;
        let capture = s.pending.get(request).ok_or("unknown linked capture")?;
        if capture.linked_main.as_ref().is_none_or(|main| main.hash() != set.main.bytes.hash())
            || (!capture.linked_key.is_empty() && capture.linked_key != set.main.key)
            || capture.name != set.name
        {
            return Err("validated resource set differs from captured owner".into());
        }
        let resource = if capture.intent == "save" { s.bindings.get(&capture.doc).cloned() } else { None };
        let version = if capture.intent == "save" { s.versions.get(&capture.doc).cloned() } else { None };
        let original = capture.snapshots.iter().find(|doc| doc.uid == capture.doc).and_then(|doc| doc.resources.as_ref());
        if capture.destination == "provider"
            && original.is_some_and(|prior| !prior.granted_to(&provider, &namespace, &s.connection))
            && !s.queued_sets.contains(request)
        {
            return Err("complete an authorized current-connection resource reply before provider reuse".into());
        }
        let set = if capture.destination == "provider" { set.owned_by(provider, namespace, s.connection.clone()) } else { set };
        let metadata = json!({"type":"save","requestId":request,"documentId":capture.doc.to_string(),"revision":capture.revision.to_string(),
            "intent":capture.intent,"filename":capture.name,"resourceId":resource,"expectedVersion":version,"destination":capture.destination,
            "linkedFilesVersion":1,"setDigest":set.digest(),"manifest":set.manifest()});
        if let Some(capture) = s.pending.get_mut(request) {
            capture.linked_key = set.main.key.clone();
            capture.linked_set = Some(set.clone());
        }
        s.queued_sets.remove(request);
        s.linked_event(metadata, LinkedPayload::Set(set));
        Ok(())
    }
    pub(crate) fn ready_linked_set(&self, request: &str, set: Arc<ResourceSet>) -> Result<(), String> {
        let mut s = self.state();
        let context = s.context(request)?;
        if !s.queued_sets.contains(request) || !matches!(context.kind, Kind::Open | Kind::Refresh) || s.ready_sets.contains_key(request) {
            return Err("resource reply has no ready owner".into());
        }
        let p = s.provider.as_ref().ok_or("provider disconnected")?;
        let set = set.owned_by(p.provider_id.clone(), p.namespace.clone(), s.connection.clone());
        s.ready_sets.insert(request.into(), set);
        Ok(())
    }
    pub(crate) fn take_ready_linked_set(&self, request: &str) -> Result<(Context, Arc<ResourceSet>), String> {
        let mut s = self.state();
        let context = s.context(request)?;
        let set = s.ready_sets.remove(request).ok_or("resource set is not ready")?;
        Ok((context, set))
    }
    pub(crate) fn finish_linked_open(&self, request: &str, resource: &str, name: &str, result: Result<u64, String>, version: Option<String>) {
        let mut s = self.state();
        if !s.opens.get(request).is_some_and(|r| r == resource) || !s.queued_sets.remove(request) {
            return;
        }
        if let (Ok(doc), Some(version)) = (&result, version) {
            s.versions.insert(*doc, version);
        }
        drop(s);
        self.finish_open(request, resource, name, result);
    }
    pub(crate) fn fail_linked(&self, request: &str, error: String) {
        let mut s = self.state();
        let Ok(context) = s.context(request) else {
            s.queued_sets.remove(request);
            s.ready_sets.remove(request);
            return;
        };
        s.ready_sets.remove(request);
        s.queued_sets.remove(request);
        match context.kind {
            Kind::Save => s.fail_delivery(&json!({"type":"save","requestId":request})),
            Kind::Refresh => s.fail_update(request, error),
            Kind::Open => {
                drop(s);
                self.finish_open(request, &context.resource, "", Err(error));
            }
        }
    }
    pub(crate) fn begin_resource_refresh(&self, snapshot: DocState, name: String, main: Arc<Bytes>) -> Result<String, String> {
        let mut s = self.state();
        if !s.linked_enabled
            || !s.enabled
            || s.events.len() >= MAX_EVENTS
            || s.owner_count() >= MAX_PENDING
            || s.retained_bytes() > MAX_BYTES
            || s.updates.values().any(|c| c.doc == snapshot.uid)
        {
            return Err("resource refresh unavailable or already pending".into());
        }
        let p = s.provider.as_ref().ok_or("provider disconnected")?;
        if snapshot
            .resources
            .as_ref()
            .and_then(|set| set.authority.as_ref())
            .is_some_and(|owner| owner != &(p.provider_id.clone(), p.namespace.clone()))
        {
            return Err("another account's resources need an explicit Save As export".into());
        }
        let key = snapshot.resources.as_ref().map(|set| set.main.key.clone()).unwrap_or_default();
        let request = s.next("resources")?;
        s.linked_event(
            json!({"type":"resourceSetNeeded","operation":"refresh","requestId":request,"documentId":snapshot.uid.to_string(),
            "revision":snapshot.revision.to_string(),"filename":name,"mainLogicalKey":key,"mainSha256":main.hash()}),
            LinkedPayload::Main { key: key.clone(), bytes: main.clone() },
        );
        s.updates.insert(
            request.clone(),
            Capture {
                doc: snapshot.uid,
                revision: snapshot.revision,
                snapshots: vec![snapshot],
                name,
                intent: "refresh".into(),
                destination: String::new(),
                bytes: None,
                linked_main: Some(main),
                linked_key: key,
                linked_set: None,
                ack_version: None,
            },
        );
        Ok(request)
    }
    pub(crate) fn finish_resource_refresh(&self, request: &str, result: Result<u64, String>) {
        let mut s = self.state();
        let Some(capture) = s.updates.remove(request) else { return };
        s.queued_sets.remove(request);
        s.ready_sets.remove(request);
        s.finishing_saves.insert(request.into(), vec![capture.doc]);
        Self::refresh_terminal(&mut s, request, &capture, result);
    }
    pub(crate) fn finish_failed_resources(&self, request: &str, capture: &Capture, error: &str) {
        let mut s = self.state();
        if !s.finishing_saves.get(request).is_some_and(|docs| docs.contains(&capture.doc)) || s.finished.iter().any(|v| v.request == request) {
            return;
        }
        Self::refresh_terminal(&mut s, request, capture, Err(error.into()));
    }
    fn refresh_terminal(s: &mut State, request: &str, capture: &Capture, result: Result<u64, String>) {
        let metadata = match result {
            Ok(revision) => {
                json!({"type":"resourcesUpdated","requestId":request,"documentId":capture.doc.to_string(),"revision":revision.to_string()})
            }
            Err(error) => {
                json!({"type":if error.is_empty(){"resourcesCancelled"}else{"resourcesFailed"},"requestId":request,"documentId":capture.doc.to_string(),"error":bounded_error(&error)})
            }
        };
        s.finished.push_back(Terminal { request: request.into(), metadata, owner: TerminalOwner::Save });
        s.flush_finished();
    }
}
