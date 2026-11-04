//! DXF capture over the existing format hooks; no storage backend or filesystem authority.
use crate::{CadApp, file_providers::Action};
use cadcraft_engine::cmd::file::{self, SaveAs};
use serde_json::{Value, json};

pub(crate) fn save(app: &mut CadApp, save_as: bool) -> Result<Value, String> {
    let result = capture(app, save_as);
    if let Err(error) = &result {
        app.session.echo(error.clone());
        app.set_status(error.clone());
    }
    result
}
/// Queue a provider read through CadApp::run without creating a filesystem picker.
pub(crate) fn open_recent(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    let result = params
        .get("resource")
        .and_then(Value::as_str)
        .ok_or_else(|| "a file provider resource ID is required".to_string())
        .and_then(|resource| app.file_providers.request_open(resource))
        .map(|request| json!({"pending":"fileProvider","requestId":request}));
    if let Err(error) = &result {
        app.session.echo(error.clone());
        app.set_status(error.clone());
    }
    result
}

fn capture(app: &mut CadApp, save_as: bool) -> Result<Value, String> {
    let host = app.file_providers.clone();
    let state = app.session.state().map_err(|e| e.to_string())?;
    if host.pending_for(state.uid) {
        return Err("a provider save is already pending for this drawing".into());
    }
    if state.block_unsaved() {
        return Err("save or discard Block Editor changes before saving with a provider".into());
    }
    let drawing = state.drawing();
    if host.linked_enabled() && drawing.refedit.is_some() {
        return Err("finish in-place reference editing before capturing linked files".into());
    }
    // Native DXF stores these paths, not the referenced bytes. Never silently promise that a
    // provider upload has copied unavailable external files into a portable project.
    let external_image = drawing
        .model
        .iter()
        .chain(drawing.layouts.iter().flat_map(|l| l.entities.iter()))
        .chain(drawing.blocks.values().flat_map(|b| b.entities.iter()))
        .any(|e| matches!(&e.kind,cadcraft_doc::EntityKind::Image(image) if !image.path.is_empty()));
    if !host.linked_enabled() && (external_image || drawing.blocks.values().any(|b| b.xref_path.as_ref().is_some_and(|p| !p.is_empty()))) {
        return Err(
            "DXF references external images or drawings; make a self-contained drawing before provider save (external files are not uploaded)".into(),
        );
    }
    let mut name = host.bound_filename(state.uid).unwrap_or_else(|| {
        let stem = state.title.strip_suffix(".dxf").or_else(|| state.title.strip_suffix(".dwg")).unwrap_or(&state.title);
        format!("{stem}.dxf")
    });
    if name.trim() == ".dxf" {
        name = "Drawing.dxf".into();
    }
    let encode = || -> Result<Vec<u8>, String> {
        match file::write_as() {
            Some(writer) => writer(drawing, &name, &SaveAs::default()),
            None => (file::io().ok_or("file formats are unavailable")?.write)(drawing, &name),
        }
    };
    let bytes = if host.linked_enabled() {
        crate::file_provider_links::check_native(drawing)?;
        std::panic::catch_unwind(|| crate::file_provider_links::preserve_fonts(encode()?, drawing))
            .map_err(|_| "native linked drawing encoder failed".to_string())??
    } else {
        encode()?
    };
    let request = if host.linked_enabled() {
        host.begin_linked(state.clone(), name, cadcraft_engine::linked_files::Bytes::new(bytes)?, save_as)?
    } else {
        host.begin(vec![state.clone()], state.uid, state.revision, name, bytes, save_as)?
    };
    app.set_status("Choose where to save the DXF drawing");
    Ok(json!({"pending":"fileProvider","requestId":request}))
}
pub(crate) fn tick(app: &mut CadApp) {
    let host = app.file_providers.clone();
    if !host.connected() {
        return;
    }
    for action in host.take_actions() {
        match action {
            Action::Linked { request, input } => linked_reply(app, &request, input),
            Action::LinkedOpenFailed { request, resource, error } => {
                app.session.echo(error.clone());
                app.set_status(error.clone());
                host.finish_linked_open(&request, &resource, "", Err(error), None);
            }
            Action::ResourceFailed { request, capture, error } => {
                if !error.is_empty() {
                    app.session.echo(error.clone());
                    app.set_status(error.clone());
                }
                host.finish_failed_resources(&request, &capture, &error);
            }
            Action::Save { request, capture, status, resource } => {
                let alive = app.session.docs.iter().any(|d| d.uid == capture.doc);
                if status == "saved" {
                    acknowledge_capture(app, &capture);
                    app.set_status("Drawing saved with file provider");
                } else if status == "failed" {
                    app.set_status("File provider save failed; drawing remains unsaved");
                }
                host.finish_save(&request, &capture, &status, &resource, alive);
            }
            Action::Open { request, resource, name, bytes } => {
                let result = if let Some(uid) = host.bound_document(&resource).filter(|uid| app.session.docs.iter().any(|d| d.uid == *uid)) {
                    match app.session.docs.iter().position(|d| d.uid == uid) {
                        Some(index) => app.run("document.switch", json!({"index":index})).map(|_| uid),
                        None => Err("provider drawing closed".into()),
                    }
                } else {
                    app.run("open", json!({"data":file::base64_encode(&bytes),"name":name})).and_then(|value| {
                        app.opened(Ok(value));
                        app.session.state().map(|d| d.uid).map_err(|e| e.to_string())
                    })
                };
                if let Err(error) = &result {
                    app.open_failed(error);
                }
                host.finish_open(&request, &resource, &name, result);
            }
        }
    }
    host.refresh(app.session.docs.iter().map(|d| (d.uid, d.revision, d.title.clone(), d.is_dirty())).collect());
}

/// Advance captured native/resource checkpoints without replacing later live revisions.
fn acknowledge_capture(app: &mut CadApp, capture: &crate::file_providers::Capture) {
    for snapshot in &capture.snapshots {
        if let Some(doc) = app.session.docs.iter_mut().find(|d| d.uid == snapshot.uid) {
            doc.saved = snapshot.drawing().clone();
            doc.path = None;
            if let Some(set) = &capture.linked_set {
                let committed = set.committed(capture.ack_version.clone().unwrap_or_default());
                doc.checkpoint_resources(&snapshot.resources, committed);
            }
        }
    }
}

fn request(params: &Value) -> Result<&str, String> {
    params.get("requestId").and_then(Value::as_str).ok_or_else(|| "resource requestId required".into())
}
/// Start an explicit undoable attachment refresh through the same actual app command route.
pub(crate) fn refresh_resources(app: &mut CadApp) -> Result<Value, String> {
    let result = begin_refresh(app);
    if let Err(error) = &result {
        app.session.echo(error.clone());
        app.set_status(error.clone());
    }
    result
}
fn begin_refresh(app: &mut CadApp) -> Result<Value, String> {
    if app.session.prompting() || app.session.lisp_busy() || app.session.lisp_running() {
        return Err("finish the active command before refreshing linked files".into());
    }
    let state = app.session.state().map_err(|e| e.to_string())?;
    if state.block_edit.is_some() || state.drawing().refedit.is_some() {
        return Err("finish Block Editor/reference edits before refreshing linked files".into());
    }
    let name = app.file_providers.bound_filename(state.uid).unwrap_or_else(|| "Drawing.dxf".into());
    crate::file_provider_links::check_native(state.drawing())?;
    let bytes = std::panic::catch_unwind(|| {
        let bytes = (file::io().ok_or("native codec unavailable")?.write)(state.drawing(), &name)?;
        crate::file_provider_links::preserve_fonts(bytes, state.drawing())
    })
    .map_err(|_| "native linked drawing encoder failed".to_string())??;
    let id = app.file_providers.begin_resource_refresh(state.clone(), name, cadcraft_engine::linked_files::Bytes::new(bytes)?)?;
    Ok(json!({"pending":"linkedResources","requestId":id}))
}
fn linked_reply(app: &mut CadApp, request: &str, input: cadcraft_engine::linked_files::Input) {
    let host = app.file_providers.clone();
    let result = (|| {
        let context = host.linked_context(request)?;
        let set = crate::file_provider_links::validate(input, context.main)?;
        if context.kind == crate::file_providers::LinkedKind::Save {
            return host.finish_linked_resolution(request, set);
        }
        host.ready_linked_set(request, set)?;
        let command = if context.kind == crate::file_providers::LinkedKind::Open { "ui.provider.openSet" } else { "ui.provider.applyResources" };
        app.run(command, json!({"requestId":request})).map(|_| ())
    })();
    if let Err(error) = result {
        app.session.echo(error.clone());
        app.set_status(error.clone());
        host.fail_linked(request, error);
    }
}
/// Consume one validated OpenSet before emitting opened, with no local xref fallback.
pub(crate) fn open_set(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    let request = request(params)?;
    let host = app.file_providers.clone();
    let (context, set) = host.take_ready_linked_set(request)?;
    if context.kind != crate::file_providers::LinkedKind::Open {
        return Err("not a resource-set open owner".into());
    }
    let existing = host.bound_document(&context.resource).and_then(|uid| app.session.docs.iter().position(|doc| doc.uid == uid));
    let (result, version) = if let Some(index) = existing {
        (app.run("document.switch", json!({"index":index})).and_then(|_| app.session.state().map(|doc| doc.uid).map_err(|e| e.to_string())), None)
    } else {
        let decoded = file::read_resource_set(set.clone())?;
        let result = app.session.finish_open(decoded).map_err(|e| e.to_string()).and_then(|value| {
            app.opened(Ok(value));
            app.session.state().map(|doc| doc.uid).map_err(|e| e.to_string())
        });
        (result, Some(set.version.clone()))
    };
    let reply = result.as_ref().map(|doc| json!({"documentId":doc.to_string(),"setDigest":set.digest()})).map_err(Clone::clone);
    host.finish_linked_open(request, &context.resource, &set.name, result, version);
    reply
}
/// Replace only the exact requested document revision; newer drawing/resource edits are preserved.
pub(crate) fn apply_resources(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    let request = request(params)?;
    let host = app.file_providers.clone();
    let (context, set) = host.take_ready_linked_set(request)?;
    if context.kind != crate::file_providers::LinkedKind::Refresh {
        return Err("not a resource refresh owner".into());
    }
    let result = (|| {
        if app.session.prompting() || app.session.lisp_busy() || app.session.lisp_running() {
            return Err("finish the active command before applying linked files".into());
        }
        let doc = app.session.docs.iter_mut().find(|doc| doc.uid == context.doc).ok_or("resource owner was closed")?;
        if doc.revision != context.revision || doc.block_edit.is_some() || doc.drawing().refedit.is_some() {
            return Err("resource reply belongs to an older drawing revision".into());
        }
        doc.replace_resources(set)?;
        Ok(doc.revision)
    })();
    let reply = result.as_ref().map(|revision| json!({"revision":revision.to_string()})).map_err(Clone::clone);
    host.finish_resource_refresh(request, result);
    reply
}

#[cfg(test)]
mod tests;
