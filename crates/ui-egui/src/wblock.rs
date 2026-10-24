//! Write Block (WBLOCK) dialog: choose source (block / objects / entire drawing), set the
//! insertion base point and pick the destination file.

use egui::vec2;
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// Which pick session is pending (dialog is hidden while the user picks on canvas).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pending {
    Objects,
    Base,
}

/// Which objects will be written to the new file.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Source {
    /// All entities in model space.
    #[default]
    EntireDrawing,
    /// A named block definition.
    Block,
    /// The current selection or objects picked interactively.
    Objects,
}

/// Dialog state while WBLOCK is open.
#[derive(Default)]
pub struct State {
    pub source: Source,
    pub block_name: String,
    /// Base point X (typed or picked on canvas).
    pub base_x: String,
    /// Base point Y (typed or picked on canvas).
    pub base_y: String,
    pub path: String,
    pub error: Option<String>,
    /// Non-None while the dialog is hidden for a canvas pick.
    pending: Option<Pending>,
    /// Handles collected by a canvas pick.
    pub picked_handles: Vec<String>,
    /// UID of the drawing that was active when the pick started; guards against cross-drawing picks.
    pick_uid: Option<u64>,
}

impl State {
    pub fn reset(&mut self) {
        *self = State::default();
    }
}

/// WBLOCK typed at the command line without params → open the dialog (same pattern as BEDIT).
pub fn route(app: &mut CadApp, id: &str, params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    if !matches!(id, "wblock" | "w") || !params.is_null() {
        return None;
    }
    let has_selection = app.session.state().is_ok_and(|st| !st.selection.is_empty());
    app.wblock.reset();
    if has_selection {
        app.wblock.source = Source::Objects;
        // Pre-fill picked handles from current selection.
        if let Ok(st) = app.session.state() {
            app.wblock.picked_handles = st.selection.iter().map(|h| h.hex()).collect();
        }
    }
    app.ui.dialog = Some("wblock".into());
    Some(Ok(serde_json::Value::Null))
}

/// Called every frame: if a WBLOCKPICK session just ended, take its results and reopen the dialog.
pub fn resume(app: &mut CadApp) {
    let Some(pending) = app.wblock.pending else { return };
    if app.session.running.is_some() {
        return;
    }
    let picks = app.session.wblock_picks.take();
    // Drop picks that came from a different drawing than the one the pick started in.
    let current_uid = app.session.state().ok().map(|s| s.uid);
    if current_uid != app.wblock.pick_uid {
        app.wblock.pending = None;
        app.wblock.pick_uid = None;
        return;
    }
    app.wblock.pending = None;
    app.wblock.pick_uid = None;
    match (pending, picks) {
        (Pending::Objects, Some(p)) => {
            app.wblock.picked_handles = p.handles.iter().map(|h| h.hex()).collect();
        }
        (Pending::Base, Some(p)) => {
            if let Some(base) = p.base {
                app.wblock.base_x = format!("{}", base.x);
                app.wblock.base_y = format!("{}", base.y);
            }
        }
        // ESC / cancel: don't reopen the dialog, close the whole command.
        _ => return,
    }
    app.ui.dialog = Some("wblock".into());
}

/// Start a canvas pick session, hiding the dialog.
fn start_pick(app: &mut CadApp, mode: Pending) {
    app.wblock.pending = Some(mode);
    app.wblock.error = None;
    app.wblock.pick_uid = app.session.state().ok().map(|s| s.uid);
    app.session.wblock_picks = None;
    app.ui.dialog = None;
    app.start("wblockpick");
    if app.session.running.is_none() {
        // wblockpick failed to start: restore the dialog instead of closing silently.
        app.wblock.pending = None;
        app.wblock.pick_uid = None;
        app.ui.dialog = Some("wblock".into());
        return;
    }
    let kw = match mode {
        Pending::Objects => "Objects",
        Pending::Base => "Base",
    };
    if let Err(e) = app.session.input(cadcraft_engine::Input::Keyword(kw.into())) {
        app.session.echo(e.to_string());
    }
}

/// The Write Block window.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let block_names: Vec<String> = app
        .session
        .doc()
        .map(|d| {
            let mut v: Vec<String> = d.blocks.values().filter(|b| !b.anonymous && !b.name.starts_with('*')).map(|b| b.name.clone()).collect();
            v.sort_by_key(|n| n.to_ascii_lowercase());
            v
        })
        .unwrap_or_default();

    let (mut ok, mut cancel) = (false, false);
    let mut pick_objects = false;
    let mut pick_base = false;

    egui::Window::new(crate::tl!("Write Block"))
        .id(egui::Id::new("Write Block"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .min_width(420.0)
        .show(ctx, |ui| {
            ui.set_max_width(480.0);

            // ── Source ──────────────────────────────────────────────────────
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong(crate::tl!("Source"));
                ui.add_space(4.0);

                // Block radio + dropdown
                ui.horizontal(|ui| {
                    ui.radio_value(&mut app.wblock.source, Source::Block, crate::tl!("Block"));
                    ui.add_space(8.0);
                    ui.add_enabled_ui(app.wblock.source == Source::Block, |ui| {
                        egui::ComboBox::from_id_salt("wblock_block_name")
                            .selected_text(if app.wblock.block_name.is_empty() { crate::tl!("(select)") } else { app.wblock.block_name.as_str() })
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                for n in &block_names {
                                    ui.selectable_value(&mut app.wblock.block_name, n.clone(), n);
                                }
                            });
                    });
                });

                ui.radio_value(&mut app.wblock.source, Source::EntireDrawing, crate::tl!("Entire drawing"));
                ui.radio_value(&mut app.wblock.source, Source::Objects, crate::tl!("Objects"));

                ui.add_space(4.0);

                // Objects + base point sub-section (only when Objects selected)
                ui.add_enabled_ui(app.wblock.source == Source::Objects, |ui| {
                    // Base point
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.strong(crate::tl!("Base point"));
                            ui.add_space(8.0);
                            if ui.small_button(crate::tl!("Pick point")).clicked() {
                                pick_base = true;
                            }
                        });
                        egui::Grid::new("wblock_base").num_columns(2).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                            ui.label("X");
                            ui.add(egui::TextEdit::singleline(&mut app.wblock.base_x).desired_width(120.0).hint_text("0"));
                            ui.end_row();
                            ui.label("Y");
                            ui.add(egui::TextEdit::singleline(&mut app.wblock.base_y).desired_width(120.0).hint_text("0"));
                            ui.end_row();
                        });
                    });

                    // Object count + Select button
                    ui.horizontal(|ui| {
                        let n = app.wblock.picked_handles.len();
                        if n == 0 {
                            ui.colored_label(t.warn, crate::tl!("No objects selected"));
                        } else {
                            ui.label(format!("{n} {}", crate::tl!("object(s) selected")));
                        }
                        ui.add_space(8.0);
                        if ui.button(crate::tl!("Select objects")).clicked() {
                            pick_objects = true;
                        }
                    });
                });
            });

            ui.add_space(6.0);

            // ── Destination ─────────────────────────────────────────────────
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong(crate::tl!("Destination"));
                ui.add_space(4.0);
                ui.label(crate::tl!("File name and path"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut app.wblock.path).desired_width(330.0).hint_text("/path/to/output.dxf"));
                    if ui.button("…").clicked() {
                        let name = if app.wblock.path.trim().is_empty() { "untitled.dxf" } else { app.wblock.path.trim() };
                        if let Some(p) = app.services.pick_save.as_ref().and_then(|f| f(name)) {
                            app.wblock.path = p;
                        }
                    }
                });
            });

            if let Some(e) = &app.wblock.error {
                ui.add_space(4.0);
                ui.colored_label(t.warn, e);
            }

            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        cancel = true;
                    }
                    let can_ok = !app.wblock.path.is_empty()
                        && match app.wblock.source {
                            Source::Block => !app.wblock.block_name.is_empty(),
                            Source::Objects => !app.wblock.picked_handles.is_empty(),
                            Source::EntireDrawing => true,
                        };
                    if ui.add_enabled(can_ok, egui::Button::new(crate::tl!("OK"))).clicked() {
                        ok = true;
                    }
                });
            });
        });

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }

    if pick_objects {
        start_pick(app, Pending::Objects);
    } else if pick_base {
        start_pick(app, Pending::Base);
    } else if ok {
        if confirm(app) {
            *open = false;
        }
    } else if cancel {
        *open = false;
    }
}

pub fn confirm(app: &mut CadApp) -> bool {
    let path = app.wblock.path.trim().to_string();
    if path.is_empty() {
        app.wblock.error = Some(crate::tl!("A file path is required.").to_string());
        return false;
    }

    let params = match app.wblock.source {
        Source::Block => {
            let name = app.wblock.block_name.trim().to_string();
            if name.is_empty() {
                app.wblock.error = Some(crate::tl!("Select a block.").to_string());
                return false;
            }
            json!({ "path": path, "name": name })
        }
        Source::EntireDrawing => {
            json!({ "path": path, "name": "*" })
        }
        Source::Objects => {
            let parse_coord = |s: &str| -> Option<f64> {
                let s = s.trim();
                if s.is_empty() {
                    return Some(0.0);
                }
                s.parse::<f64>().ok().filter(|v| v.is_finite())
            };
            let bx = match parse_coord(&app.wblock.base_x) {
                Some(v) => v,
                None => {
                    app.wblock.error = Some(crate::tl!("Invalid base X coordinate.").to_string());
                    return false;
                }
            };
            let by = match parse_coord(&app.wblock.base_y) {
                Some(v) => v,
                None => {
                    app.wblock.error = Some(crate::tl!("Invalid base Y coordinate.").to_string());
                    return false;
                }
            };
            if app.wblock.picked_handles.is_empty() {
                app.wblock.error = Some(crate::tl!("No objects selected").to_string());
                return false;
            }
            let handles: Vec<serde_json::Value> = app.wblock.picked_handles.iter().map(|h| json!(h)).collect();
            json!({ "path": path, "handles": handles, "base": [bx, by] })
        }
    };

    match app.run("wblock", params) {
        Ok(v) => {
            let n = v["entities"].as_u64().unwrap_or(0);
            app.session.echo(format!("{n} object(s) written to \"{path}\"."));
            app.wblock.error = None;
            true
        }
        Err(e) => {
            app.wblock.error = Some(e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use cadcraft_engine::geom::Vec2;
    use cadcraft_engine::{Input, Session};
    use serde_json::{Value, json};

    use super::*;
    use crate::Services;

    fn app_with_line() -> (CadApp, cadcraft_doc::Handle) {
        let mut s = Session::new();
        let r = s.execute("line", &json!({ "points": [[10, 20], [15, 20]] })).unwrap();
        let h = cadcraft_doc::Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
        s.set_selection(Vec::new());
        (CadApp::new(s, Services::default()), h)
    }

    #[test]
    fn typed_wblock_opens_the_dialog_but_json_calls_do_not() {
        let (mut app, _) = app_with_line();
        assert!(route(&mut app, "wblock", &json!({ "path": "x.dxf" })).is_none());
        assert_eq!(app.ui.dialog, None);
        assert!(route(&mut app, "wblock", &Value::Null).is_some());
        assert_eq!(app.ui.dialog.as_deref(), Some("wblock"));
        assert_eq!(app.wblock.source, Source::EntireDrawing);
    }

    #[test]
    fn a_selection_preselects_objects() {
        let (mut app, h) = app_with_line();
        app.session.set_selection(vec![h]);
        route(&mut app, "wblock", &Value::Null);
        assert_eq!(app.wblock.source, Source::Objects);
        assert_eq!(app.wblock.picked_handles, vec![h.hex()]);
    }

    #[test]
    fn select_objects_hides_the_dialog_and_brings_it_back_with_the_objects() {
        let (mut app, h) = app_with_line();
        route(&mut app, "wblock", &Value::Null);
        start_pick(&mut app, Pending::Objects);
        assert_eq!(app.ui.dialog, None, "hidden while picking");
        resume(&mut app);
        assert_eq!(app.ui.dialog, None, "still picking");
        app.session.input(Input::Pick(vec![h])).unwrap();
        app.session.input(Input::Enter).unwrap();
        resume(&mut app);
        assert_eq!(app.ui.dialog.as_deref(), Some("wblock"));
        assert_eq!(app.wblock.picked_handles, vec![h.hex()]);
    }

    #[test]
    fn pick_point_fills_the_base_point() {
        let (mut app, _) = app_with_line();
        route(&mut app, "wblock", &Value::Null);
        start_pick(&mut app, Pending::Base);
        app.session.input(Input::Point(Vec2::new(10.0, 5.0))).unwrap();
        resume(&mut app);
        assert_eq!(app.ui.dialog.as_deref(), Some("wblock"));
        assert_eq!((app.wblock.base_x.as_str(), app.wblock.base_y.as_str()), ("10", "5"));
    }

    #[test]
    fn escape_while_picking_ends_the_command() {
        let (mut app, _) = app_with_line();
        route(&mut app, "wblock", &Value::Null);
        start_pick(&mut app, Pending::Objects);
        app.session.cancel();
        resume(&mut app);
        assert_eq!(app.ui.dialog, None);
        assert!(app.session.running.is_none());
    }
}
