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
    // This single-file transfer does not upload external image or drawing bytes.
    if has_external_files(drawing) {
        return Err(
            "DXF references external images or drawings; make a self-contained drawing before provider save (external files are not uploaded)".into(),
        );
    }
    let mut name = host.bound_filename(state.uid).unwrap_or_else(|| match state.title.rsplit_once('.') {
        Some((_, extension)) if extension.eq_ignore_ascii_case("dxf") => state.title.clone(),
        Some((stem, extension)) if extension.eq_ignore_ascii_case("dwg") => format!("{stem}.dxf"),
        _ => format!("{}.dxf", state.title),
    });
    if name.trim() == ".dxf" {
        name = "Drawing.dxf".into();
    }
    let bytes = match file::write_as() {
        Some(writer) => writer(drawing, &name, &SaveAs::default())?,
        None => (file::io().ok_or("file formats are unavailable")?.write)(drawing, &name)?,
    };
    let request = host.begin(vec![state.clone()], state.uid, state.revision, name, bytes, save_as)?;
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
            Action::Save { request, capture, status, resource } => {
                let alive = app.session.docs.iter().any(|d| d.uid == capture.doc);
                if status == "saved" {
                    for snapshot in &capture.snapshots {
                        if let Some(doc) = app.session.docs.iter_mut().find(|d| d.uid == snapshot.uid) {
                            doc.saved = snapshot.drawing().clone();
                            doc.path = None;
                        }
                    }
                    app.set_status("Drawing saved with file provider");
                } else if status == "failed" {
                    app.set_status("File provider save failed; drawing remains unsaved");
                }
                host.finish_save(&request, &capture, &status, &resource, alive);
            }
            Action::Open { request, resource, name, bytes } => {
                let result = open_resource(app, &resource, &name, &bytes);
                if let Err(error) = &result {
                    app.open_failed(error);
                }
                host.finish_open(&request, &resource, &name, result);
            }
        }
    }
    host.refresh(app.session.docs.iter().map(|d| (d.uid, d.revision, d.title.clone(), d.is_dirty())).collect());
}

/// The same modeled dependency boundary applies before provider Save and Open. In particular,
/// provider bytes must not reach native OPEN's automatic IMAGE/XREF filesystem loaders.
fn has_external_files(drawing: &cadcraft_doc::Drawing) -> bool {
    drawing
        .model
        .iter()
        .chain(drawing.layouts.iter().flat_map(|layout| layout.entities.iter()))
        .chain(drawing.blocks.values().flat_map(|block| block.entities.iter()))
        .any(|entity| matches!(&entity.kind, cadcraft_doc::EntityKind::Image(image) if !image.path.is_empty()))
        || drawing.blocks.values().any(|block| block.xref_path.as_ref().is_some_and(|path| !path.is_empty()))
}

fn open_resource(app: &mut CadApp, resource: &str, name: &str, bytes: &[u8]) -> Result<u64, String> {
    if let Some((index, uid)) =
        app.file_providers.bound_document(resource).and_then(|uid| app.session.docs.iter().position(|doc| doc.uid == uid).map(|index| (index, uid)))
    {
        return app.run("document.switch", json!({"index":index})).map(|_| uid);
    }
    // Host admission already bounds these bytes. Preflight with the installed native decoder,
    // before any document is opened or the unchanged native command follows external paths.
    // The actual Open still runs through CadApp's existing command route and format hooks.
    let hooks = file::io().ok_or("file formats are unavailable")?;
    let drawing = std::panic::catch_unwind(|| (hooks.read)(bytes, name)).map_err(|_| "file provider drawing decoder failed".to_string())??;
    if has_external_files(&drawing) {
        return Err(
            "DXF references external images or drawings; open a self-contained drawing with the file provider (external files are not loaded)".into(),
        );
    }
    drop(drawing);
    app.run("open", json!({"data":file::base64_encode(bytes),"name":name})).and_then(|value| {
        app.opened(Ok(value));
        app.session.state().map(|doc| doc.uid).map_err(|error| error.to_string())
    })
}

#[cfg(test)]
mod tests;
