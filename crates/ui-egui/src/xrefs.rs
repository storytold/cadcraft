//! The External References palette: the drawing's xrefs with their status, type and path, and
//! Attach, Reload, Unload, Detach, Bind, Path and Open. A thin view over the engine's XREF
//! commands (`xref`, `-xref`, `xattach`, `xopen`): it reads the drawing and acts through
//! [`CadApp::run`] / [`CadApp::start`].

use egui::RichText;
use serde_json::{Value, json};

use crate::CadApp;
use crate::docking::Panel;

/// XREF typed or chosen from a menu opens this palette; XATTACH there asks for the file first
/// (with the host's file picker, where there is one). JSON calls (with parameters) run the
/// engine's commands as they are.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !params.is_null() {
        return None;
    }
    match cadcraft_engine::cmd::resolve_alias(id).as_str() {
        "xref" => Some(open_palette(app)),
        "xattach" => {
            let picked = app.services.pick_open.as_ref().and_then(|f| f())?;
            let path = relative_to_host(app, &picked);
            if let Err(e) = app.session.start("xattach") {
                return Some(Err(e.to_string()));
            }
            // Submit the picked filename as literal text, not through the command line: a path is
            // attacker-controlled (the file picker returns whatever the user selected), and
            // `cmdline` interprets command syntax, so a name like `'closeall` would run an engine
            // command instead of naming the file (CWE-74).
            if let Err(e) = app.session.input(cadcraft_engine::Input::Text(path)) {
                return Some(Err(e.to_string()));
            }
            Some(Ok(Value::Null))
        }
        _ => None,
    }
}

/// Show the palette. The first time it docks below Properties in the right dock; after that it
/// comes back where it was.
fn open_palette(app: &mut CadApp) -> Result<Value, String> {
    crate::docking::command(app, false, &json!({ "operation": "open", "panel": Panel::ExternalRefs.id() }))
}

/// A picked file inside the host drawing's folder is attached by its relative path, so the
/// project folder can move.
fn relative_to_host(app: &CadApp, picked: &str) -> String {
    let host = app.session.state().ok().and_then(|st| st.path.clone());
    let dir = host.as_deref().and_then(|h| std::path::Path::new(h).parent());
    match dir.and_then(|d| std::path::Path::new(picked).strip_prefix(d).ok()) {
        Some(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
        _ => picked.to_string(),
    }
}

/// One xref as the palette lists it.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    name: String,
    status: String,
    overlay: bool,
    path: String,
    references: usize,
}

/// The xrefs of the active drawing (cached per drawing revision: counting references walks it).
fn rows(app: &CadApp, ui: &egui::Ui) -> Vec<Row> {
    let Ok(st) = app.session.state() else { return Vec::new() };
    let key = (st.uid, st.revision);
    let id = egui::Id::unique("xref-rows");
    if let Some((k, rows)) = ui.data(|d| d.get_temp::<((u64, u64), Vec<Row>)>(id))
        && k == key
    {
        return rows;
    }
    let d = &st.doc;
    let xrefs: Vec<_> = d.blocks.values().filter(|b| b.xref_path.is_some() && !b.name.contains('|')).collect();
    let mut counts = vec![0usize; xrefs.len()];
    if !xrefs.is_empty() {
        let mut count = |store: &cadcraft_engine::doc::EntityStore| {
            for e in store.iter() {
                if let cadcraft_engine::doc::EntityKind::Insert(i) = &e.kind
                    && let Some(k) = xrefs.iter().position(|b| b.name.eq_ignore_ascii_case(&i.block))
                    && let Some(c) = counts.get_mut(k)
                {
                    *c += 1;
                }
            }
        };
        count(&d.model);
        d.layouts.iter().for_each(|l| count(&l.entities));
        d.blocks.values().filter(|b| !b.name.contains('|')).for_each(|b| count(&b.entities));
    }
    let rows: Vec<Row> = xrefs
        .iter()
        .zip(counts)
        .map(|(b, references)| {
            let loaded = b.xref_status == cadcraft_engine::doc::XrefStatus::Loaded;
            Row {
                name: b.name.clone(),
                status: if loaded && references == 0 { "Unreferenced".into() } else { b.xref_status.label().into() },
                overlay: b.xref_overlay,
                path: b.xref_path.clone().unwrap_or_default(),
                references,
            }
        })
        .collect();
    ui.data_mut(|d| d.insert_temp(id, (key, rows.clone())));
    rows
}

/// What a click in the palette asks for, run once the palette is drawn.
enum Act {
    Start(&'static str),
    Run(&'static str, Value),
}

/// The palette body.
pub fn section(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let rows = rows(app, ui);
    let sel_id = egui::Id::unique("xref-selected");
    let path_id = egui::Id::unique("xref-path-edit");
    let mut selected: Option<String> = ui.data(|d| d.get_temp(sel_id));
    if !selected.as_ref().is_some_and(|s| rows.iter().any(|r| &r.name == s)) {
        selected = rows.first().map(|r| r.name.clone());
    }
    let mut act = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if ui.button(crate::tl!("Attach DWG...")).clicked() {
            act = Some(Act::Start("xattach"));
        }
        if ui.add_enabled(!rows.is_empty(), egui::Button::new(crate::tl!("Reload All"))).clicked() {
            act = Some(Act::Run("-xref", json!({ "option": "reload", "names": "*" })));
        }
    });
    ui.add_space(6.0);
    if rows.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(crate::tl!("No external references are attached.")).color(t.text_dim));
        });
    } else {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            egui::Grid::new("xref-grid").num_columns(4).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
                for h in [crate::tl!("Reference Name"), crate::tl!("Status"), crate::tl!("Type"), crate::tl!("Saved Path")] {
                    ui.label(RichText::new(h).color(t.text_dim));
                }
                ui.end_row();
                for r in &rows {
                    let on = selected.as_deref() == Some(r.name.as_str());
                    if ui.selectable_label(on, &r.name).clicked() {
                        selected = Some(r.name.clone());
                        ui.data_mut(|d| d.remove::<String>(path_id));
                    }
                    let bad = matches!(r.status.as_str(), "Not Found" | "Unresolved");
                    ui.label(RichText::new(crate::i18n::t(&r.status)).color(if bad { t.warn } else { t.text }));
                    ui.label(crate::i18n::t(if r.overlay { "Overlay" } else { "Attach" }));
                    ui.label(&r.path).on_hover_text(crate::tf!("References: {n}", n = r.references));
                    ui.end_row();
                }
            });
        });
    }
    ui.data_mut(|d| match &selected {
        Some(s) => {
            d.insert_temp(sel_id, s.clone());
        }
        None => d.remove::<String>(sel_id),
    });
    if let Some(r) = selected.as_ref().and_then(|s| rows.iter().find(|r| &r.name == s)) {
        ui.add_space(8.0);
        ui.separator();
        details(ui, r, path_id, &mut act);
    }
    match act {
        Some(Act::Start(id)) => app.start(id),
        Some(Act::Run(id, p)) => {
            if let Err(e) = app.run(id, p) {
                app.set_status(e);
            }
        }
        None => {}
    }
}

/// The selected xref: its actions, type and path.
fn details(ui: &mut egui::Ui, r: &Row, path_id: egui::Id, act: &mut Option<Act>) {
    let name = r.name.clone();
    let loaded = r.status == "Loaded" || r.status == "Unreferenced";
    let op = |option: &str| json!({ "option": option, "names": name });
    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        if ui.button(crate::tl!("Reload")).clicked() {
            *act = Some(Act::Run("-xref", op("reload")));
        }
        if ui.add_enabled(r.status != "Unloaded", egui::Button::new(crate::tl!("Unload"))).clicked() {
            *act = Some(Act::Run("-xref", op("unload")));
        }
        if ui.button(crate::tl!("Detach")).clicked() {
            *act = Some(Act::Run("-xref", op("detach")));
        }
        if ui
            .add_enabled(loaded, egui::Button::new(crate::tl!("Bind")))
            .on_hover_text(crate::tl!("Make it a block of this drawing; its layers become NAME$0$LAYER"))
            .clicked()
        {
            *act = Some(Act::Run("-xref", op("bind")));
        }
        if ui
            .add_enabled(loaded, egui::Button::new(crate::tl!("Insert")))
            .on_hover_text(crate::tl!("Make it a block of this drawing, keeping its layer names"))
            .clicked()
        {
            *act = Some(Act::Run("-xref", json!({ "option": "bind", "names": name, "insert": true })));
        }
        if ui.add_enabled(r.status != "Not Found", egui::Button::new(crate::tl!("Open"))).clicked() {
            *act = Some(Act::Run("xopen", json!({ "name": name })));
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(crate::tl!("Type"));
        let mut overlay = r.overlay;
        let a = ui.radio_value(&mut overlay, false, crate::tl!("Attach")).clicked();
        let o = ui.radio_value(&mut overlay, true, crate::tl!("Overlay")).clicked();
        if (a || o) && overlay != r.overlay {
            *act = Some(Act::Run("-xref", json!({ "option": "type", "names": name, "overlay": overlay })));
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let label = ui.label(crate::tl!("Saved Path"));
        let mut text: String = ui.data(|d| d.get_temp(path_id)).unwrap_or_else(|| r.path.clone());
        let w = (ui.available_width() - 70.0).max(60.0);
        let edit = ui.add(egui::TextEdit::singleline(&mut text).desired_width(w)).labelled_by(label.id);
        let apply = ui.add_enabled(!text.trim().is_empty() && text != r.path, egui::Button::new(crate::tl!("Apply"))).clicked()
            || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && text != r.path);
        if apply {
            *act = Some(Act::Run("-xref", json!({ "option": "path", "names": name, "path": text.trim() })));
            ui.data_mut(|d| d.remove::<String>(path_id));
        } else if text != r.path {
            ui.data_mut(|d| d.insert_temp(path_id, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(path_id));
        }
    });
}
