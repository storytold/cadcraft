//! DXF capture over the existing format hooks; no storage backend or filesystem authority.
use crate::{CadApp, file_providers::Action};
use cadcraft_engine::cmd::file::{self, SaveAs};
use serde_json::{Value, json};

pub(crate) fn save(app: &mut CadApp, save_as: bool) -> Result<Value, String> {
    let host = app.file_providers.clone();
    let state = app.session.state().map_err(|e| e.to_string())?;
    if host.pending_for(state.uid) {
        return Err("a provider save is already pending for this drawing".into());
    }
    if state.block_unsaved() {
        return Err("save or discard Block Editor changes before saving with a provider".into());
    }
    let drawing = state.drawing();
    // Native DXF stores these paths, not the referenced bytes. Never silently promise that a
    // provider upload has copied unavailable external files into a portable project.
    let external_image = drawing
        .model
        .iter()
        .chain(drawing.layouts.iter().flat_map(|l| l.entities.iter()))
        .chain(drawing.blocks.values().flat_map(|b| b.entities.iter()))
        .any(|e| matches!(&e.kind,cadcraft_doc::EntityKind::Image(image) if !image.path.is_empty()));
    if external_image || drawing.blocks.values().any(|b| b.xref_path.as_ref().is_some_and(|p| !p.is_empty())) {
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

#[cfg(test)]
mod tests;
