//! Dialogs: Drafting Settings, Drawing Units, About, command reference, blocks; dispatches the
//! Layer Properties Manager ([`crate::layers`]), Quick Select ([`crate::quick`]), Parameters
//! Manager ([`crate::parametric`]) and the style and settings managers ([`crate::managers`]).

use egui::{RichText, vec2};
use serde_json::json;

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
        "units" => units(app, ctx, &mut open),
        "about" => about(ctx, &mut open),
        "commands" => commands(app, ctx, &mut open),
        "blocks" => blocks(app, ctx, &mut open),
        other => crate::managers::dialog(app, ctx, other, &mut open),
    }
    if !open {
        app.ui.dialog = None;
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

/// LUNITS values in the order the Type list shows them.
const LENGTH_TYPES: [(i64, &str); 5] = [(4, "Architectural"), (2, "Decimal"), (3, "Engineering"), (5, "Fractional"), (1, "Scientific")];
/// AUNITS values.
const ANGLE_TYPES: [(i64, &str); 5] = [(0, "Decimal Degrees"), (1, "Deg/Min/Sec"), (2, "Grads"), (3, "Radians"), (4, "Surveyor's Units")];
/// INSUNITS names, indexed by value (DXF Reference, header group code 70).
const INSERTION_UNITS: [&str; 25] = [
    "Unitless",
    "Inches",
    "Feet",
    "Miles",
    "Millimeters",
    "Centimeters",
    "Meters",
    "Kilometers",
    "Microinches",
    "Mils",
    "Yards",
    "Angstroms",
    "Nanometers",
    "Microns",
    "Decimeters",
    "Decameters",
    "Hectometers",
    "Gigameters",
    "Astronomical Units",
    "Light Years",
    "Parsecs",
    "US Survey Feet",
    "US Survey Inch",
    "US Survey Yard",
    "US Survey Mile",
];

/// A combo box over `(value, label)` options.
fn pick(ui: &mut egui::Ui, salt: &str, value: &mut i64, options: &[(i64, String)]) {
    let current = options.iter().find(|o| o.0 == *value).map(|o| o.1.clone()).unwrap_or_default();
    egui::ComboBox::from_id_salt(salt).selected_text(current).width(180.0).show_ui(ui, |ui| {
        for (k, label) in options {
            ui.selectable_value(value, *k, label);
        }
    });
}

/// Drawing Units (UNITS): edits a copy of LUNITS/LUPREC/AUNITS/AUPREC/INSUNITS and applies it
/// through the `units` command on OK.
fn units(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    use cadcraft_engine::units::{format_angle, format_distance};
    let id = egui::Id::new("units_dialog_values");
    let mut v = ctx.data_mut(|d| d.get_temp::<[i64; 5]>(id)).unwrap_or_else(|| {
        app.session
            .doc()
            .map(|d| {
                let h = &d.header;
                [h.i64("LUNITS", 2), h.i64("LUPREC", 4), h.i64("AUNITS", 0), h.i64("AUPREC", 0), h.i64("INSUNITS", 1)]
            })
            .unwrap_or([2, 4, 0, 0, 1])
    });
    let (mut apply, mut cancel) = (false, false);
    egui::Window::new("Drawing Units").open(open).resizable(false).collapsible(false).show(ctx, |ui| {
        let [lu, lp, au, ap, ins] = &mut v;
        let types = |t: &[(i64, &str)]| t.iter().map(|(k, l)| (*k, l.to_string())).collect::<Vec<_>>();
        egui::Grid::new("units_grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
            ui.strong("Length");
            ui.end_row();
            ui.label("Type");
            pick(ui, "units_lunits", lu, &types(&LENGTH_TYPES));
            ui.end_row();
            ui.label("Precision");
            pick(ui, "units_luprec", lp, &(0..=8).map(|p| (p, format_distance(1.5, *lu, p))).collect::<Vec<_>>());
            ui.end_row();
            ui.strong("Angle");
            ui.end_row();
            ui.label("Type");
            pick(ui, "units_aunits", au, &types(&ANGLE_TYPES));
            ui.end_row();
            ui.label("Precision");
            pick(ui, "units_auprec", ap, &(0..=8).map(|p| (p, format_angle(45f64.to_radians(), *au, p))).collect::<Vec<_>>());
            ui.end_row();
            ui.strong("Insertion scale");
            ui.end_row();
            ui.label("Units to scale inserted content");
            let names = INSERTION_UNITS.iter().enumerate().map(|(k, l)| (k as i64, l.to_string())).collect::<Vec<_>>();
            pick(ui, "units_insunits", ins, &names);
            ui.end_row();
            ui.strong("Sample output");
            ui.end_row();
            ui.label("");
            ui.label(format!("{},{},{}", format_distance(1.5, *lu, *lp), format_distance(2.0039, *lu, *lp), format_distance(0.0, *lu, *lp)));
            ui.end_row();
            ui.label("");
            ui.label(format_angle(45f64.to_radians(), *au, *ap));
            ui.end_row();
        });
        ui.separator();
        ui.horizontal(|ui| {
            apply = ui.button("OK").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if apply {
        let [lu, lp, au, ap, ins] = v;
        let _ = app.run("units", json!({ "lunits": lu, "luprec": lp, "aunits": au, "auprec": ap, "insunits": ins }));
    }
    if apply || cancel || !*open {
        ctx.data_mut(|d| d.remove::<[i64; 5]>(id));
        *open = false;
    } else {
        ctx.data_mut(|d| d.insert_temp(id, v));
    }
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
    let mut pick = None;
    egui::Window::new("Blocks").open(open).default_size(vec2(320.0, 360.0)).show(ctx, |ui| {
        let Ok(d) = app.session.doc() else { return };
        let names: Vec<&String> = d.blocks.keys().filter(|k| !k.starts_with('*')).collect();
        if names.is_empty() {
            ui.label("No blocks defined in this drawing.");
        }
        for n in names {
            if ui.button(n).on_hover_text("Insert this block").clicked() {
                pick = Some(n.clone());
            }
        }
    });
    if let Some(n) = pick {
        insert_block(app, &n);
    }
}

/// Start INSERT with `name` already given at its block name prompt, so the next prompt asks for
/// the insertion point.
fn insert_block(app: &mut CadApp, name: &str) {
    app.start("insert");
    if !app.session.running.as_ref().is_some_and(|r| r.id == "insert") {
        return;
    }
    if let Err(e) = app.session.input(cadcraft_engine::Input::Text(name.to_string())) {
        app.session.echo(e.to_string());
    }
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
            let finish = if ui.ctx().os().is_mac() { "⌘↩ to finish" } else { "Ctrl+Enter to finish" };
            ui.label(RichText::new(finish).small());
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
    use cadcraft_engine::doc::EntityKind;
    use cadcraft_engine::geom::Vec2;
    use cadcraft_engine::{Input, Session};
    use serde_json::json;

    use crate::{CadApp, Services};

    #[test]
    fn picking_a_block_starts_insert_with_its_name() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let c = app.session.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
        let h = c["handle"].as_str().unwrap().to_string();
        app.session.execute("block", &json!({"name": "Door 2", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
        // A command already running is replaced, as when a command is typed.
        app.start("line");

        super::insert_block(&mut app, "Door 2");
        assert!(app.session.running.as_ref().is_some_and(|r| r.id == "insert"));
        assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Specify insertion point".to_string()));

        app.session.input(Input::Point(Vec2::new(5.0, 6.0))).unwrap();
        app.session.input(Input::Enter).unwrap();
        app.session.input(Input::Enter).unwrap();
        assert!(app.session.running.is_none());
        let d = app.session.doc().unwrap();
        let mut ins = Vec::new();
        for e in d.model.iter() {
            if let EntityKind::Insert(i) = &e.kind {
                ins.push(i.clone());
            }
        }
        assert_eq!(ins.len(), 1);
        assert_eq!(ins[0].block, "Door 2");
        assert!((ins[0].insert.x - 5.0).abs() < 1e-9 && (ins[0].insert.y - 6.0).abs() < 1e-9);
    }

    #[test]
    fn picking_a_missing_block_keeps_asking_for_a_name() {
        let mut app = CadApp::new(Session::new(), Services::default());
        super::insert_block(&mut app, "Nope");
        assert!(app.session.running.as_ref().is_some_and(|r| r.id == "insert"));
        assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Enter block name".to_string()));
    }
}
