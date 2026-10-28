//! Dialogs: Drafting Settings, Drawing Units, About, command reference, blocks; dispatches the
//! Layer Properties Manager ([`crate::layers`]), Quick Select ([`crate::quick`]), Parameters
//! Manager ([`crate::parametric`]) and the style and settings managers ([`crate::managers`]).

use egui::{RichText, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    crate::mtext_editor::show(app, ctx);
    crate::quick::quick_properties(app, ctx);
    let Some(d) = app.ui.dialog.clone() else { return };
    let mut open = true;
    match d.as_str() {
        "spacemouse" => crate::spacemouse::dialog(app, ctx, &mut open),
        "language" => language(app, ctx, &mut open),
        "layers" => crate::layers::dialog(app, ctx, &mut open),
        "qselect" => crate::quick::qselect_dialog(app, ctx, &mut open),
        "parameters" => crate::parametric::parameters_dialog(app, ctx, &mut open),
        "dsettings" => dsettings(app, ctx, &mut open),
        "units" => units(app, ctx, &mut open),
        "hatch" => crate::hatchdlg::dialog(app, ctx, &mut open),
        "aliasedit" => crate::aliasdlg::dialog(app, ctx, &mut open),
        "about" => about(ctx, &mut open),
        "preview" => crate::exchange::dialog(app, ctx, &mut open),
        "commands" => commands(app, ctx, &mut open),
        "blocks" => blocks(app, ctx, &mut open),
        "bedit" => crate::bedit::dialog(app, ctx, &mut open),
        "authoring" => crate::bedit::authoring_dialog(app, ctx, &mut open),
        "stylesmanager" => crate::plotstyles::dialog(app, ctx, &mut open),
        o if o == "options" || o.starts_with("options:") => crate::options::dialog(app, ctx, o, &mut open),
        other => crate::managers::dialog(app, ctx, other, &mut open),
    }
    // A dialog that closed by opening another (Plot Preview back to Plot) keeps the new one.
    if !open && app.ui.dialog.as_deref() == Some(d.as_str()) {
        app.ui.dialog = None;
    }
}

/// Drafting Settings window: snap/grid modes and spacing, isometric snap, polar tracking and object snaps, editing the session settings directly.
fn dsettings(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let settings = app.session.drafting_settings();
    let (mut snapunit, mut gridunit) = (settings.snapunit, settings.gridunit);
    egui::Window::new(crate::tl!("Drafting Settings")).id(egui::Id::unique("Drafting Settings")).open(open).resizable(false).show(ctx, |ui| {
        let s = &mut app.session.settings;
        ui.heading(crate::tl!("Snap and Grid"));
        ui.checkbox(&mut s.snapmode, crate::tl!("Snap On (F9)"));
        // SNAPSTYL: the isometric snap and grid (ISODRAFT).
        ui.horizontal(|ui| {
            ui.radio_value(&mut s.isodraft, false, crate::tl!("Rectangular snap"));
            ui.radio_value(&mut s.isodraft, true, crate::tl!("Isometric snap"));
        });
        ui.horizontal(|ui| {
            let x_label = ui.label(crate::tl!("Snap X spacing"));
            ui.add(egui::DragValue::new(&mut snapunit.x).speed(0.05).range(0.0001..=1e6).clamp_existing_to_range(false)).labelled_by(x_label.id);
            let y_label = ui.label(crate::tl!("Y"));
            ui.add(egui::DragValue::new(&mut snapunit.y).speed(0.05).range(0.0001..=1e6).clamp_existing_to_range(false)).labelled_by(y_label.id);
        });
        // SNAPTYPE and POLARDIST.
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Snap type"));
            ui.radio_value(&mut s.polarsnap, false, crate::tl!("Grid snap"));
            ui.radio_value(&mut s.polarsnap, true, crate::tl!("PolarSnap"));
        });
        ui.add_enabled_ui(s.polarsnap, |ui| {
            ui.horizontal(|ui| {
                let label = ui.label(crate::tl!("Polar distance"));
                ui.add(egui::DragValue::new(&mut s.polardist).speed(0.05).range(0.0..=1e6)).labelled_by(label.id);
            });
        });
        ui.checkbox(&mut s.gridmode, crate::tl!("Grid On (F7)"));
        ui.horizontal(|ui| {
            let grid_label = ui.label(crate::tl!("Grid spacing"));
            ui.add(egui::DragValue::new(&mut gridunit.x).speed(0.05).range(0.0..=1e6).clamp_existing_to_range(false)).labelled_by(grid_label.id);
            let major_label = ui.label(crate::tl!("Major line every"));
            ui.add(egui::DragValue::new(&mut s.gridmajor).range(1..=100)).labelled_by(major_label.id);
        });
        ui.separator();
        ui.heading(crate::tl!("Polar Tracking"));
        ui.checkbox(&mut s.polarmode, crate::tl!("Polar Tracking On (F10)"));
        // POLARANG is held in radians and 15 or 30 come back as 14.999999999999998: round like
        // the additional angles, and write back only a newly picked angle.
        let current = (s.polarang.to_degrees() * 1e6).round() / 1e6;
        let mut deg = current;
        ui.horizontal(|ui| {
            let label = ui.label(crate::tl!("Increment angle"));
            let combo = egui::ComboBox::from_id_salt("polarang").selected_text(format!("{deg}")).show_ui(ui, |ui| {
                for a in [90.0, 45.0, 30.0, 22.5, 18.0, 15.0, 10.0, 5.0] {
                    ui.selectable_value(&mut deg, a, format!("{a}"));
                }
            });
            combo.response.labelled_by(label.id);
        });
        if deg != current {
            s.polarang = deg.to_radians();
        }
        tracking_options(ui, s);
        ui.separator();
        ui.heading(crate::tl!("Object Snap"));
        let mut on = s.osmode & cadcraft_engine::snap::mode::OFF == 0;
        if ui.checkbox(&mut on, crate::tl!("Object Snap On (F3)")).changed() {
            if on {
                s.osmode &= !cadcraft_engine::snap::mode::OFF;
            } else {
                s.osmode |= cadcraft_engine::snap::mode::OFF;
            }
        }
        egui::Grid::new("osnap_grid").num_columns(2).show(ui, |ui| {
            for (i, (bit, name)) in cadcraft_engine::snap::mode::ALL.iter().enumerate() {
                let mut v = s.osmode & bit != 0;
                if ui.checkbox(&mut v, crate::i18n::t(name)).changed() {
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
        ui.checkbox(&mut s.dynmode, crate::tl!("Enable Dynamic Input (F12)"));
        // Pointer input format for second and next points (DYNPIFORMAT, DYNPICOORDS).
        ui.indent("dynpi", |ui| {
            ui.add_enabled_ui(s.dynmode, |ui| {
                ui.label(crate::tl!("Second and next points:"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.dynpi_cartesian, false, crate::tl!("Polar (distance < angle)"));
                    ui.radio_value(&mut s.dynpi_cartesian, true, crate::tl!("Cartesian (x, y)"));
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.dynpi_absolute, false, crate::tl!("Relative"));
                    ui.radio_value(&mut s.dynpi_absolute, true, crate::tl!("Absolute"));
                });
                // DYNPROMPT: the prompt and its options (Down Arrow) beside the cursor.
                ui.checkbox(&mut s.dynprompt, crate::tl!("Show command prompting near the crosshairs"));
            });
        });
        ui.checkbox(&mut s.orthomode, crate::tl!("Ortho (F8)"));
    });
    // Drawing edits go through the same command as the CLI, including dirty state and undo.
    for (name, before, after) in [("SNAPUNIT", settings.snapunit, snapunit), ("GRIDUNIT", settings.gridunit, gridunit)] {
        if before != after {
            let _ = app.run("setvar", json!({ "name": name, "value": [after.x, after.y] }));
        }
    }
}

/// The polar and object snap tracking options of Drafting Settings: additional angles
/// (POLARMODE bit 4, POLARADDANG), tracking on every polar angle (bit 2) and angles relative to
/// the last segment (bit 1).
fn tracking_options(ui: &mut egui::Ui, s: &mut cadcraft_engine::Settings) {
    use cadcraft_engine::snap::tracking::polarmode;
    /// A radio button for POLARMODE `bit` being `on`.
    fn flag(ui: &mut egui::Ui, flags: &mut u32, bit: u32, on: bool, label: &str) {
        if ui.radio((*flags & bit != 0) == on, label).clicked() {
            *flags = if on { *flags | bit } else { *flags & !bit };
        }
    }
    let mut additional = s.polar_flags & polarmode::ADDITIONAL != 0;
    ui.horizontal(|ui| {
        let additional_label = ui.checkbox(&mut additional, crate::tl!("Additional angles"));
        if additional_label.changed() {
            s.polar_flags ^= polarmode::ADDITIONAL;
        }
        // The list as typed (degrees separated by `;`), kept while it doesn't parse yet.
        let id = ui.scope_id().with("polaraddang");
        let shown = cadcraft_engine::sysvars::format_angle_list(&s.polaraddang);
        let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| shown.clone());
        let resp = ui.add_enabled(additional, egui::TextEdit::singleline(&mut text).desired_width(140.0)).labelled_by(additional_label.id);
        if resp.changed()
            && let Some(angles) = cadcraft_engine::sysvars::parse_angle_list(&text)
        {
            s.polaraddang = angles;
        }
        if resp.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(id));
        }
    });
    ui.label(crate::tl!("Object Snap Tracking Settings"));
    ui.horizontal(|ui| {
        flag(ui, &mut s.polar_flags, polarmode::TRACK_POLAR, false, crate::tl!("Track orthogonally only"));
        flag(ui, &mut s.polar_flags, polarmode::TRACK_POLAR, true, crate::tl!("Track using all polar angle settings"));
    });
    ui.label(crate::tl!("Polar angle measurement"));
    ui.horizontal(|ui| {
        flag(ui, &mut s.polar_flags, polarmode::RELATIVE, false, crate::tl!("Absolute"));
        flag(ui, &mut s.polar_flags, polarmode::RELATIVE, true, crate::tl!("Relative to last segment"));
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

/// Length precision choices for a LUNITS format. Architectural (4) and Fractional (5) step through
/// binary fractions of an inch (LUPREC n is 1/2^n, 0 is whole inches); the other formats show a decimal sample.
fn length_precisions(lunits: i64) -> Vec<(i64, String)> {
    use cadcraft_engine::units::format_distance;
    if matches!(lunits, 4 | 5) {
        (0..=8).map(|p| (p, if p == 0 { "1\"".to_string() } else { format!("1/{}\"", 1i64 << p) })).collect()
    } else {
        (0..=8).map(|p| (p, format_distance(1.5, lunits, p))).collect()
    }
}

/// Drawing Units (UNITS): edits a copy of LUNITS/LUPREC/AUNITS/AUPREC/INSUNITS and applies it
/// through the `units` command on OK.
fn units(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    use cadcraft_engine::units::{format_angle, format_distance};
    let id = egui::Id::unique("units_dialog_values");
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
            pick(ui, "units_luprec", lp, &length_precisions(*lu));
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

/// About window with About, Contributors and Models tabs, version, licence and project links.
fn about(ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let tab_id = egui::Id::unique("about_tab");
    egui::Window::new(crate::tl!("About CADCraft"))
        .id(egui::Id::unique("About CADCraft"))
        .open(open)
        .default_size(vec2(640.0, 420.0))
        .collapsible(false)
        .show(ctx, |ui| {
            let mut tab = ui.data_mut(|d| d.get_temp::<u8>(tab_id)).unwrap_or(0);
            ui.horizontal(|ui| {
                for (i, l) in ["About", "Contributors", "Models"].iter().enumerate() {
                    if ui.selectable_label(tab == i as u8, crate::i18n::t(l)).clicked() {
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
                    ui.heading(crate::tl!("CADCraft"));
                    ui.label(crate::tf!("Version {version}", version = env!("CARGO_PKG_VERSION")));
                    ui.label(crate::tl!("Computer-aided design and drafting: an open-source, clean-room CAD application written in pure Rust."));
                    ui.add_space(6.0);
                    ui.label(RichText::new(crate::tl!("One of the Crafting Apps by the ArtCraft team and community.")).color(t.text_dim));
                    ui.hyperlink_to("getartcraft.com/apps/cadcraft", "https://getartcraft.com/apps/cadcraft");
                    ui.hyperlink_to(crate::tl!("Join us on Discord"), "https://discord.gg/artcraft");
                    ui.add_space(6.0);
                    ui.label(RichText::new(crate::tl!("MIT OR Apache-2.0. Not affiliated with Autodesk, Inc.")).small().color(t.text_faint));
                }
            }
        });
}

/// Command Reference window; filters commands by id or label (English or translated) and starts the clicked command after closing the dialog.
fn commands(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut start = None;
    egui::Window::new(crate::tl!("Command Reference")).id(egui::Id::unique("Command Reference")).open(open).default_size(vec2(640.0, 480.0)).show(
        ctx,
        |ui| {
            let id = ui.scope_id().with("cmdfilter");
            let mut filter = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_default();
            ui.horizontal(|ui| {
                let label = ui.label(crate::tl!("Filter"));
                ui.text_edit_singleline(&mut filter).labelled_by(label.id);
            });
            ui.data_mut(|d| d.insert_temp(id, filter.clone()));
            let f = filter.to_lowercase();
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("cmdref").striped(true).num_columns(3).show(ui, |ui| {
                    for c in cadcraft_engine::command_specs() {
                        if !f.is_empty()
                            && !c.id.contains(&f)
                            && !c.label.to_ascii_lowercase().contains(&f)
                            && !crate::i18n::t(c.label).to_lowercase().contains(&f)
                        {
                            continue;
                        }
                        if ui.link(c.id.to_ascii_uppercase()).clicked() {
                            start = Some(c.id);
                        }
                        ui.label(crate::i18n::t(c.label));
                        ui.label(RichText::new(if c.aliases.is_empty() { String::new() } else { c.aliases.join(", ").to_ascii_uppercase() }).small());
                        ui.end_row();
                    }
                });
            });
        },
    );
    if let Some(c) = start {
        app.ui.dialog = None;
        app.start(c);
    }
}

/// Blocks window listing named (non-anonymous) blocks; clicking one starts INSERT with that block name already supplied.
fn blocks(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let mut pick = None;
    egui::Window::new(crate::tl!("Blocks")).id(egui::Id::unique("Blocks")).open(open).default_size(vec2(320.0, 360.0)).show(ctx, |ui| {
        let Ok(d) = app.session.doc() else { return };
        let names: Vec<&String> = d.blocks.keys().filter(|k| !k.starts_with('*')).collect();
        if names.is_empty() {
            ui.label(crate::tl!("No blocks defined in this drawing."));
        }
        for n in names {
            if ui.button(n).on_hover_text(crate::tl!("Insert this block")).clicked() {
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
    if app.session.running.as_ref().is_none_or(|r| r.id != "insert") {
        return;
    }
    if let Err(e) = app.session.input(cadcraft_engine::Input::Text(name.to_string())) {
        app.session.echo(e.to_string());
    }
}

/// Interface Language combo; selecting a language runs `ui.language` with its code.
fn language(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    egui::Window::new(crate::tl!("Interface Language")).id(egui::Id::unique("interface_language")).open(open).resizable(false).show(ctx, |ui| {
        egui::ComboBox::from_id_salt("interface_language_choice").selected_text(app.ui.interface_language.name()).show_ui(ui, |ui| {
            for preference in crate::i18n::Preference::for_picker() {
                if ui.selectable_label(app.ui.interface_language == preference, preference.name()).clicked() {
                    let _ = app.run("ui.language", serde_json::json!({"lang": preference.code()}));
                    ui.ctx().request_repaint();
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use cadcraft_engine::doc::EntityKind;
    use cadcraft_engine::geom::Vec2;
    use cadcraft_engine::{Input, Session};
    use serde_json::json;

    use crate::{CadApp, Services};

    #[test]
    fn fractional_length_precisions_list_binary_fractions() {
        let labels = ["1\"", "1/2\"", "1/4\"", "1/8\"", "1/16\"", "1/32\"", "1/64\"", "1/128\"", "1/256\""];
        let want: Vec<(i64, String)> = labels.iter().enumerate().map(|(k, l)| (k as i64, l.to_string())).collect();
        assert_eq!(super::length_precisions(4), want);
        assert_eq!(super::length_precisions(5), want);
        assert_eq!(super::length_precisions(2).len(), 9);
    }

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
        // X scale, Y scale and rotation take their defaults.
        for _ in 0..3 {
            app.session.input(Input::Enter).unwrap();
        }
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
