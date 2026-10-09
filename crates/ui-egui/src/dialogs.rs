//! Dialogs: Drafting Settings, About, command reference, blocks; dispatches the Layer Properties
//! Manager ([`crate::layers`]), Quick Select ([`crate::quick`]) and Parameters Manager
//! ([`crate::parametric`]).

use egui::{RichText, vec2};

use crate::CadApp;
use crate::theme::Tokens;

pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    mtext_editor(app, ctx);
    crate::quick::quick_properties(app, ctx);
    let Some(d) = app.ui.dialog.clone() else { return };
    let mut open = true;
    match d.as_str() {
        "layers" => crate::layers::dialog(app, ctx, &mut open),
        "qselect" => crate::quick::qselect_dialog(app, ctx, &mut open),
        "parameters" => crate::parametric::parameters_dialog(app, ctx, &mut open),
        "dsettings" => dsettings(app, ctx, &mut open),
        "about" => about(ctx, &mut open),
        "commands" => commands(app, ctx, &mut open),
        "blocks" => blocks(app, ctx, &mut open),
        "quit" => quit(app, ctx, &mut open),
        _ => open = false,
    }
    if !open {
        app.ui.dialog = None;
    }
}

/// Closing the window with unsaved changes: Save / Don't Save / Cancel.
fn quit(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let close = |app: &mut CadApp| {
        app.quit_confirmed = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    };
    let dirty: Vec<String> = app.session.docs.iter().filter(|d| d.is_dirty()).map(|d| d.title.clone()).collect();
    let [first, ..] = dirty.as_slice() else {
        *open = false;
        close(app);
        return;
    };
    let what = if dirty.len() == 1 { format!("“{first}”") } else { format!("{} drawings", dirty.len()) };
    let (mut save, mut discard, mut cancel) = (false, false, false);
    egui::Window::new("CADCraft")
        .id(egui::Id::new("quit_dialog"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0))
        .open(open)
        .show(ctx, |ui| {
            ui.set_min_width(340.0);
            ui.label(RichText::new(format!("Save changes to {what}?")).strong());
            ui.label("Your changes will be lost if you don't save them.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                discard = ui.button("Don't Save").clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save = ui.button("Save").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                    cancel = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
            });
        });
    if discard || (save && app.save_all()) {
        *open = false;
        close(app);
    } else if save || cancel {
        *open = false;
    }
}

fn dsettings(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    egui::Window::new("Drafting Settings").open(open).resizable(false).show(ctx, |ui| {
        let s = &mut app.session.settings;
        ui.heading("Snap and Grid");
        ui.checkbox(&mut s.snapmode, "Snap On (F9)");
        ui.horizontal(|ui| {
            ui.label("Snap X spacing");
            ui.add(egui::DragValue::new(&mut s.snapunit.x).speed(0.05).range(0.0001..=1e6));
            ui.label("Y");
            ui.add(egui::DragValue::new(&mut s.snapunit.y).speed(0.05).range(0.0001..=1e6));
        });
        ui.checkbox(&mut s.gridmode, "Grid On (F7)");
        ui.horizontal(|ui| {
            ui.label("Grid spacing");
            ui.add(egui::DragValue::new(&mut s.gridunit.x).speed(0.05).range(0.0001..=1e6));
            ui.label("Major line every");
            ui.add(egui::DragValue::new(&mut s.gridmajor).range(1..=100));
        });
        ui.separator();
        ui.heading("Polar Tracking");
        ui.checkbox(&mut s.polarmode, "Polar Tracking On (F10)");
        let mut deg = s.polarang.to_degrees();
        ui.horizontal(|ui| {
            ui.label("Increment angle");
            egui::ComboBox::from_id_salt("polarang").selected_text(format!("{deg}")).show_ui(ui, |ui| {
                for a in [90.0, 45.0, 30.0, 22.5, 18.0, 15.0, 10.0, 5.0] {
                    ui.selectable_value(&mut deg, a, format!("{a}"));
                }
            });
        });
        s.polarang = deg.to_radians();
        ui.separator();
        ui.heading("Object Snap");
        let mut on = s.osmode & cadcraft_engine::snap::mode::OFF == 0;
        if ui.checkbox(&mut on, "Object Snap On (F3)").changed() {
            if on {
                s.osmode &= !cadcraft_engine::snap::mode::OFF;
            } else {
                s.osmode |= cadcraft_engine::snap::mode::OFF;
            }
        }
        egui::Grid::new("osnap_grid").num_columns(2).show(ui, |ui| {
            for (i, (bit, name)) in cadcraft_engine::snap::mode::ALL.iter().enumerate() {
                let mut v = s.osmode & bit != 0;
                if ui.checkbox(&mut v, *name).changed() {
                    if v {
                        s.osmode |= bit;
                    } else {
                        s.osmode &= !bit;
                    }
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.separator();
        ui.checkbox(&mut s.dynmode, "Enable Dynamic Input (F12)");
        ui.checkbox(&mut s.orthomode, "Ortho (F8)");
    });
}

fn about(ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let tab_id = egui::Id::new("about_tab");
    egui::Window::new("About CADCraft").open(open).default_size(vec2(640.0, 420.0)).collapsible(false).show(ctx, |ui| {
        let mut tab = ui.data_mut(|d| d.get_temp::<u8>(tab_id)).unwrap_or(0);
        ui.horizontal(|ui| {
            for (i, l) in ["About", "Contributors", "Models"].iter().enumerate() {
                if ui.selectable_label(tab == i as u8, *l).clicked() {
                    tab = i as u8;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(tab_id, tab));
        ui.separator();
        match tab {
            1 => crate::credits::contributors_ui(ui),
            2 => crate::credits::models_ui(ui),
            _ => {
                ui.heading("CADCraft");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("Computer-aided design and drafting: an open-source, clean-room CAD application written in pure Rust.");
                ui.add_space(6.0);
                ui.label(RichText::new("One of the Crafting Apps by the ArtCraft team and community.").color(t.text_dim));
                ui.hyperlink_to("getartcraft.com/apps/cadcraft", "https://getartcraft.com/apps/cadcraft");
                ui.hyperlink_to("Join us on Discord", "https://discord.gg/artcraft");
                ui.add_space(6.0);
                ui.label(RichText::new("MIT OR Apache-2.0. Not affiliated with Autodesk, Inc.").small().color(t.text_faint));
            }
        }
    });
}

fn commands(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut start = None;
    egui::Window::new("Command Reference").open(open).default_size(vec2(640.0, 480.0)).show(ctx, |ui| {
        let id = ui.id().with("cmdfilter");
        let mut filter = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Filter");
            ui.text_edit_singleline(&mut filter);
        });
        ui.data_mut(|d| d.insert_temp(id, filter.clone()));
        let f = filter.to_ascii_lowercase();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("cmdref").striped(true).num_columns(3).show(ui, |ui| {
                for c in cadcraft_engine::command_specs() {
                    if !f.is_empty() && !c.id.contains(&f) && !c.label.to_ascii_lowercase().contains(&f) {
                        continue;
                    }
                    if ui.link(c.id.to_ascii_uppercase()).clicked() {
                        start = Some(c.id);
                    }
                    ui.label(c.label);
                    ui.label(RichText::new(if c.aliases.is_empty() { String::new() } else { c.aliases.join(", ").to_ascii_uppercase() }).small());
                    ui.end_row();
                }
            });
        });
    });
    if let Some(c) = start {
        app.ui.dialog = None;
        app.start(c);
    }
}

fn blocks(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    egui::Window::new("Blocks").open(open).default_size(vec2(320.0, 360.0)).show(ctx, |ui| {
        let Ok(d) = app.session.doc() else { return };
        let names: Vec<&String> = d.blocks.keys().filter(|k| !k.starts_with('*')).collect();
        if names.is_empty() {
            ui.label("No blocks defined in this drawing.");
        }
        for n in names {
            ui.label(n);
        }
    });
}

/// The multiline text editor shown while MTEXT asks for its contents.
fn mtext_editor(app: &mut CadApp, ctx: &egui::Context) {
    let active = app.session.running.as_ref().is_some_and(|r| r.id == "mtext")
        && app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point);
    let id = egui::Id::new("mtext_editor_buffer");
    if !active {
        ctx.data_mut(|d| d.remove::<String>(id));
        return;
    }
    let mut buf = ctx.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_default();
    let mut submit = None;
    let mut cancel = false;
    egui::Window::new("Text Editor").collapsible(false).resizable(true).default_size(vec2(460.0, 220.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Style: Standard");
            ui.separator();
            let h = app.session.doc().map(|d| d.header.f64("TEXTSIZE", 0.2)).unwrap_or(0.2);
            ui.label(format!("Height: {h:.4}"));
            ui.separator();
            if ui.button("B").on_hover_text("Bold").clicked() {
                buf.push_str("{\\fArial|b1;}");
            }
            if ui.button("⅟").on_hover_text("Stack (type 1/2 then select)").clicked() {
                buf.push_str("\\S1/2;");
            }
            if ui.button("°").on_hover_text("Degree").clicked() {
                buf.push_str("%%d");
            }
            if ui.button("±").on_hover_text("Plus/minus").clicked() {
                buf.push_str("%%p");
            }
            if ui.button("⌀").on_hover_text("Diameter").clicked() {
                buf.push_str("%%c");
            }
        });
        let r = ui.add(
            egui::TextEdit::multiline(&mut buf).desired_rows(6).desired_width(f32::INFINITY).hint_text("Type text; Enter starts a new paragraph"),
        );
        if !r.has_focus() && buf.is_empty() {
            r.request_focus();
        }
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() || (r.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))) {
                submit = Some(buf.replace('\n', "\\P"));
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
            ui.label(RichText::new("⌘↩ to finish").small());
        });
    });
    ctx.data_mut(|d| d.insert_temp(id, buf));
    if let Some(t) = submit {
        ctx.data_mut(|d| d.remove::<String>(id));
        let _ = app.session.input(cadcraft_engine::Input::Text(t));
    } else if cancel {
        ctx.data_mut(|d| d.remove::<String>(id));
        app.session.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use egui::{ViewportCommand, ViewportEvent, ViewportId, ViewportInfo};
    use serde_json::json;

    fn sample_app() -> CadApp {
        let mut session = cadcraft_engine::Session::new();
        session.open_drawing(cadcraft_engine::sample::default_sample(), "Bracket", None);
        CadApp::new(session, Services::default())
    }

    /// One frame in which the window manager asks to close the window; returns whether the app
    /// cancelled the close.
    fn close_frame(app: &mut CadApp, ctx: &egui::Context) -> bool {
        let mut input = egui::RawInput::default();
        input.viewports.insert(ViewportId::ROOT, ViewportInfo { events: vec![ViewportEvent::Close], ..Default::default() });
        let mut out = ctx.run_ui(input, |ui| app.logic(ui.ctx()));
        out.textures_delta.clear();
        out.viewport_output.get(&ViewportId::ROOT).is_some_and(|v| v.commands.contains(&ViewportCommand::CancelClose))
    }

    /// Issue #34: closing the window with unsaved changes asks first instead of losing them.
    #[test]
    fn closing_with_unsaved_changes_asks_first() {
        let ctx = egui::Context::default();
        let mut app = sample_app();
        assert!(!close_frame(&mut app, &ctx), "a saved drawing closes without asking");
        assert_eq!(app.ui.dialog, None);

        let r = app.run("line", json!({ "points": [[0.0, 0.0], [10.0, 0.0]] }));
        assert!(r.is_ok(), "{r:?}");
        assert!(close_frame(&mut app, &ctx), "unsaved changes must cancel the close");
        assert_eq!(app.ui.dialog.as_deref(), Some("quit"));

        // Once the user chose Don't Save, the close goes through.
        app.quit_confirmed = true;
        assert!(!close_frame(&mut app, &ctx));
    }

    /// Programmatic quit (control channel `app.quit`) never opens the dialog.
    #[test]
    fn programmatic_quit_skips_the_prompt() {
        let ctx = egui::Context::default();
        let mut app = sample_app();
        let r = app.run("line", json!({ "points": [[0.0, 0.0], [10.0, 0.0]] }));
        assert!(r.is_ok(), "{r:?}");
        let (req, _rx) = crate::control::ControlRequest::new("app.quit", json!({}));
        let _ = crate::control::handle(&mut app, &ctx, &req);
        assert!(!close_frame(&mut app, &ctx));
        assert_eq!(app.ui.dialog, None);
    }
}
