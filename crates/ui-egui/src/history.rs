//! The Drawing History palette: the versions every save leaves behind and the named milestones
//! OMNI-SNAP takes, newest first, with Open and Compare. A thin view over the engine's
//! `omni-versions`, `omni-log` and `omni-diff`: it lists through [`CadApp::run`] and acts through
//! the same commands, so the palette, the command line and the control channel behave alike.

use egui::RichText;
use serde_json::{Value, json};

use crate::CadApp;
use crate::docking::Panel;

/// How often the folder is read while the palette is open (seconds): versions appear when a save
/// or a snapshot adds one, and reading a folder every frame would be wasteful.
const REFRESH: f64 = 1.0;
/// The most versions the palette lists in one go.
const LIMIT: u64 = 500;

/// OMNI-LOG typed or chosen from a menu opens this palette; with parameters it stays a command.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !params.is_null() {
        return None;
    }
    match cadcraft_engine::cmd::resolve_alias(id).as_str() {
        "omni-log" | "omni-versions" => Some(open_palette(app)),
        _ => None,
    }
}

/// Show the palette. The first time it docks below the Layers palette (where the palettes column
/// is); after that it comes back where it was.
pub(crate) fn open_palette(app: &mut CadApp) -> Result<Value, String> {
    let panel = Panel::DrawingHistory;
    let first = !app.ui.docking.layout.contains(&panel) && !app.ui.docking.hidden.iter().any(|(p, _)| *p == panel);
    let r = crate::docking::command(app, false, &json!({ "operation": "open", "panel": panel.id() }))?;
    if first && app.ui.docking.layout.contains(&Panel::Layers) {
        let moved = json!({ "operation": "move", "panel": panel.id(), "target": Panel::Layers.id(), "zone": "bottom" });
        if let Ok(r) = crate::docking::command(app, false, &moved) {
            return Ok(r);
        }
    }
    Ok(r)
}

/// One version as the palette lists it.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    file: String,
    time: String,
    description: String,
    kind: String,
    user: String,
    bytes: u64,
    compressed: bool,
}

/// A version listing read once and kept until the next refresh.
#[derive(Clone, Debug, Default, PartialEq)]
struct Listing {
    /// The drawing state it was read for: a different drawing or revision reads again at once.
    key: String,
    at: f64,
    folder: Option<String>,
    count: usize,
    capped: bool,
    rows: Vec<Row>,
}

fn row_of(v: &Value) -> Option<Row> {
    Some(Row {
        file: v.get("file")?.as_str()?.to_string(),
        time: v.get("time").and_then(Value::as_str).unwrap_or_default().to_string(),
        description: v.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
        kind: v.get("kind").and_then(Value::as_str).unwrap_or("save").to_string(),
        user: v.get("user").and_then(Value::as_str).unwrap_or_default().to_string(),
        bytes: v.get("bytes").and_then(Value::as_u64).unwrap_or(0),
        compressed: v.get("compressed").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// The versions of the active drawing: read again when the drawing changes, when `force` is set
/// (a button, an action) and otherwise at most once a second.
fn listing(app: &mut CadApp, ui: &mut egui::Ui, force: bool) -> Listing {
    let key = match app.session.state() {
        Ok(st) => format!("{}:{}:{}", st.uid, st.revision, st.path.clone().unwrap_or_default()),
        Err(_) => "none".to_string(),
    };
    let id = egui::Id::unique("history-listing");
    let now = ui.input(|i| i.time);
    if let Some(cached) = ui.data(|d| d.get_temp::<Listing>(id))
        && !force
        && cached.key == key
        && now - cached.at < REFRESH
    {
        return cached;
    }
    let mut out = Listing { key, at: now, ..Listing::default() };
    if let Ok(v) = app.run("omni-versions", json!({ "limit": LIMIT })) {
        out.folder = v.get("folder").and_then(Value::as_str).map(str::to_string);
        out.count = v.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
        out.capped = v.get("capped").and_then(Value::as_bool).unwrap_or(false);
        out.rows = v.get("versions").and_then(Value::as_array).map(|a| a.iter().filter_map(row_of).collect()).unwrap_or_default();
    }
    ui.data_mut(|d| d.insert_temp(id, out.clone()));
    out
}

/// A size for the palette ("1.2 kB"), matching the command line's listing.
fn size(n: u64) -> String {
    match n {
        n if n < 10_000 => format!("{n} B"),
        n if n < 10_000_000 => format!("{:.1} kB", n as f64 / 1000.0),
        n => format!("{:.1} MB", n as f64 / 1_000_000.0),
    }
}

/// The ISO stamp the engine reports as the shorter "2026-10-21 10:15" the palette shows.
fn when(iso: &str) -> String {
    let s: String = iso.chars().take(16).collect();
    s.replace('T', " ")
}

/// What a click in the palette asks for, run once the palette is drawn.
enum Act {
    Start(&'static str),
    Run(&'static str, Value),
}

/// The palette body.
pub fn section(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let mut act = None;
    let mut force = false;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        if ui
            .button(crate::tl!("Take Snapshot..."))
            .on_hover_text(crate::tl!("Name the drawing as it is now, unsaved changes included, and keep it as a milestone"))
            .clicked()
        {
            act = Some(Act::Start("omni-snap"));
        }
        if ui.button(crate::tl!("Refresh")).clicked() {
            force = true;
        }
    });
    ui.add_space(6.0);
    let list = listing(app, ui, force);
    let sel_id = egui::Id::unique("history-selected");
    let mut selected: Option<String> = ui.data(|d| d.get_temp(sel_id));
    if !selected.as_ref().is_some_and(|s| list.rows.iter().any(|r| &r.file == s)) {
        selected = list.rows.first().map(|r| r.file.clone());
    }
    match &list.folder {
        None => {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(crate::tl!("Save the drawing first: every save with changes leaves a version behind.")).color(t.text_dim));
            });
        }
        Some(dir) => {
            if list.rows.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(crate::tl!("No versions yet: every save with changes leaves one behind.")).color(t.text_dim));
                });
            } else {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(crate::tf!("{n} versions (newest first)", n = list.count)).color(t.text_dim));
                    if app.session.state().is_ok_and(|st| st.is_dirty()) {
                        ui.label(RichText::new(crate::tl!("Unsaved changes since the last save")).color(t.warn));
                    }
                });
                if list.capped {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new(crate::tl!("The folder holds more versions than the palette lists.")).color(t.warn));
                    });
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    egui::Grid::new("history-grid").num_columns(4).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
                        for h in [crate::tl!("Time"), crate::tl!("Kind"), crate::tl!("Description"), crate::tl!("Size")] {
                            ui.label(RichText::new(h).color(t.text_dim));
                        }
                        ui.end_row();
                        for r in &list.rows {
                            let on = selected.as_deref() == Some(r.file.as_str());
                            let label = ui.selectable_label(on, when(&r.time));
                            if label.clicked() {
                                selected = Some(r.file.clone());
                            }
                            if label.double_clicked() {
                                act = Some(Act::Run("omni-diff", json!({ "snapshot": r.file })));
                            }
                            label.on_hover_text(&r.file);
                            let kind = if r.kind == "milestone" { crate::tl!("Milestone") } else { crate::tl!("Saved") };
                            ui.label(kind);
                            ui.label(&r.description);
                            let packed = ui.label(size(r.bytes));
                            if r.compressed {
                                packed.on_hover_text(crate::tl!("Stored gzipped on disk"));
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(dir).color(t.text_dim).small());
                });
            }
            ui.data_mut(|d| match &selected {
                Some(s) => {
                    d.insert_temp(sel_id, s.clone());
                }
                None => d.remove::<String>(sel_id),
            });
            if let Some(r) = selected.as_ref().and_then(|s| list.rows.iter().find(|r| &r.file == s)) {
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(8.0);
                    if ui.button(crate::tl!("Open")).on_hover_text(crate::tl!("Open this version as a drawing of its own")).clicked() {
                        act = Some(Act::Run("omni-log", json!({ "open": r.file })));
                    }
                    if ui
                        .button(crate::tl!("Compare"))
                        .on_hover_text(crate::tl!("Lay this version under the drawing in red and report what changed"))
                        .clicked()
                    {
                        act = Some(Act::Run("omni-diff", json!({ "snapshot": r.file })));
                    }
                    if ui
                        .button(crate::tl!("Restore"))
                        .on_hover_text(crate::tl!("Put this version back into the drawing as an unsaved change; UNDO goes back"))
                        .clicked()
                    {
                        act = Some(Act::Run("omni-restore", json!({ "snapshot": r.file })));
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(crate::tf!("Saved by {user}", user = r.user)).color(t.text_dim).small());
                });
            }
        }
    }
    match act {
        Some(Act::Start(id)) => {
            app.start(id);
            ui.data_mut(|d| d.remove::<Listing>(egui::Id::unique("history-listing")));
        }
        Some(Act::Run(id, p)) => {
            // Opening a version is an open like any other: the revision becomes the active drawing
            // as a tab of its own, and its geometry may sit anywhere, so the canvas fits it.
            let opens = cadcraft_engine::cmd::resolve_alias(id) == "omni-log";
            match app.run(id, p) {
                Ok(_) if opens => app.canvas.zoom_pending = true,
                Ok(_) => {}
                Err(e) => app.set_status(e),
            }
            ui.data_mut(|d| d.remove::<Listing>(egui::Id::unique("history-listing")));
        }
        None => {}
    }
}
