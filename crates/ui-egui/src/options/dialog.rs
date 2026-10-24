//! The Options dialog. It reads the engine's settings and the interface state, and changes them
//! only through the OPTIONS JSON form (`app.run("options", …)`), so the dialog, scripts and the
//! control channel share one path. Changes apply at once.
//!
//! The open tab is part of the dialog name (`"options:selection"`), so `ui.set` can show any tab.

use egui::{Color32, RichText, Sense, Stroke, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::animation::AnimationPref;
use crate::theme::{ThemePref, Tokens};

/// The tabs: (id in the dialog name, label).
pub(super) const TABS: [(&str, &str); 7] = [
    ("display", "Display"),
    ("files", "Open and Save"),
    ("fonts", "Fonts"),
    ("drafting", "Drafting"),
    ("selection", "Selection"),
    ("preferences", "User Preferences"),
    ("profiles", "Profiles"),
];

const WIDTH: f32 = 610.0;
/// Height of every tab's page.
const TAB_HEIGHT: f32 = 340.0;

/// Show the Options dialog (`name` is `"options"` or `"options:<tab>"`).
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) {
    let tab = name.strip_prefix("options:").and_then(|t| TABS.iter().find(|(id, _)| *id == t)).map_or("display", |(id, _)| *id);
    let mut changes = Map::new();
    let mut next_tab = None;
    let mut start = None;
    let mut close = false;
    let center = ctx.content_rect().center();
    egui::Window::new(crate::tl!("Options"))
        .id(egui::Id::new("cc_options"))
        .open(open)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(center)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.horizontal_wrapped(|ui| {
                for (id, label) in TABS {
                    if ui.selectable_label(tab == id, crate::i18n::t(label)).clicked() {
                        next_tab = Some(id);
                    }
                }
            });
            ui.separator();
            ui.add_space(4.0);
            // Every tab is the same height (a taller one scrolls), so switching tabs neither
            // resizes the dialog nor moves it.
            egui::ScrollArea::vertical().id_salt(("options_tab", tab)).max_height(TAB_HEIGHT).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_min_height(TAB_HEIGHT);
                match tab {
                    "files" => files(app, ui, &mut changes),
                    "fonts" => fonts(app, ui, &mut changes),
                    "drafting" => drafting(app, ui, &mut changes, &mut start),
                    "selection" => selection(app, ui, &mut changes),
                    "preferences" => preferences(app, ui, &mut changes),
                    "profiles" => profiles(app, ui, &mut changes),
                    _ => display(app, ui, &mut changes),
                }
            });
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Close")).clicked() {
                        close = true;
                    }
                    ui.label(RichText::new(crate::tl!("Changes apply at once and are kept for next time.")).small().color(Tokens::get().text_faint));
                });
            });
        });
    if close {
        *open = false;
    }
    if let Some(t) = next_tab {
        app.ui.dialog = Some(format!("options:{t}"));
    }
    if !changes.is_empty() {
        let _ = app.run("options", Value::Object(changes));
    }
    if let Some(id) = start {
        app.start(id);
    }
}

fn grid(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing(vec2(14.0, 8.0)).show(ui, add);
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(Tokens::get().text_dim));
}

fn theme_label(t: ThemePref) -> &'static str {
    match t {
        ThemePref::System => crate::tl!("Use System Setting"),
        ThemePref::Light => crate::tl!("Light"),
        ThemePref::Dark => crate::tl!("Dark"),
        ThemePref::Black => crate::tl!("Black"),
        ThemePref::LightBlue => crate::tl!("Light Blue"),
    }
}

fn animations_label(a: AnimationPref) -> &'static str {
    match a {
        AnimationPref::System => crate::tl!("Use System Setting"),
        AnimationPref::On => crate::tl!("On"),
        AnimationPref::Off => crate::tl!("Off"),
    }
}

/// A colour button, its hex value and a Default button; a change goes into `changes` under `key`.
fn color_row(ui: &mut egui::Ui, current: Option<Color32>, default: Color32, key: &str, changes: &mut Map<String, Value>) {
    ui.horizontal(|ui| {
        let shown = current.unwrap_or(default);
        let mut rgb = [shown.r(), shown.g(), shown.b()];
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            changes.insert(key.into(), json!(super::hex(Color32::from_rgb(rgb[0], rgb[1], rgb[2]))));
        }
        ui.monospace(super::hex(shown));
        if ui.add_enabled(current.is_some(), egui::Button::new(crate::tl!("Default"))).clicked() {
            changes.insert(key.into(), Value::Null);
        }
    });
}

/// A square of `side` points in `color` on the drawing-area background (marker and pickbox
/// previews).
fn preview_square(ui: &mut egui::Ui, side: f32, color: Color32) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, t.canvas);
    let side = side.clamp(1.0, 32.0);
    p.rect_stroke(egui::Rect::from_center_size(rect.center(), vec2(side, side)), 0.0, Stroke::new(2.0, color), egui::StrokeKind::Middle);
}

fn display(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    grid(ui, "options_display", |ui| {
        ui.label(crate::tl!("Interface theme"));
        let mut theme = app.ui.theme;
        egui::ComboBox::from_id_salt("options_theme").selected_text(theme_label(theme)).show_ui(ui, |ui| {
            for t in ThemePref::ALL {
                ui.selectable_value(&mut theme, t, theme_label(t));
            }
        });
        if theme != app.ui.theme {
            changes.insert("theme".into(), json!(theme.as_str()));
        }
        ui.end_row();

        ui.label(crate::tl!("Animation effects"));
        let mut animations = app.ui.animations;
        egui::ComboBox::from_id_salt("options_animations").selected_text(animations_label(animations)).show_ui(ui, |ui| {
            for a in AnimationPref::ALL {
                ui.selectable_value(&mut animations, a, animations_label(a));
            }
        });
        if animations != app.ui.animations {
            changes.insert("animations".into(), json!(animations.as_str()));
        }
        ui.end_row();

        ui.label(crate::tl!("Interface scale"));
        let scale = app.options.interface_scale;
        let percent = |s: f32| format!("{} %", (s * 100.0).round());
        egui::ComboBox::from_id_salt("options_interface_scale").selected_text(percent(scale)).show_ui(ui, |ui| {
            for n in [75u16, 100, 125, 150, 175, 200, 250, 300] {
                let s = f32::from(n) / 100.0;
                if ui.selectable_label((scale - s).abs() < 1e-4, format!("{n} %")).clicked() && (scale - s).abs() >= 1e-4 {
                    changes.insert("interfaceScale".into(), json!(s));
                }
            }
        });
        ui.end_row();

        ui.label(crate::tl!("Model background"));
        color_row(ui, app.options.model_background, Tokens::DARK.canvas, "modelBackground", changes);
        ui.end_row();

        ui.label(crate::tl!("Layout paper"));
        color_row(ui, app.options.paper_background, Tokens::DARK.paper, "paperBackground", changes);
        ui.end_row();

        ui.label(crate::tl!("Crosshair size"));
        let mut size = app.session.settings.cursorsize;
        if ui.add(egui::Slider::new(&mut size, 1.0..=100.0).integer().suffix(" %")).changed() {
            changes.insert("CURSORSIZE".into(), json!(size));
        }
        ui.end_row();

        ui.label(crate::tl!("Command line history"));
        let mut lines = app.ui.history_lines;
        if ui.add(egui::DragValue::new(&mut lines).range(0..=crate::cmdline::MAX_LINES)).changed() {
            changes.insert("historyLines".into(), json!(lines));
        }
        ui.end_row();
    });
    ui.add_space(6.0);
    let mut system_cursor = app.ui.system_cursor;
    if ui.checkbox(&mut system_cursor, crate::tl!("Keep the system cursor visible over the drawing area")).changed() {
        changes.insert("systemCursor".into(), json!(system_cursor));
    }
    hint(ui, crate::tl!("Screen magnifiers, screen readers and recorders follow the system cursor."));
}

fn files(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    grid(ui, "options_files", |ui| {
        ui.label(crate::tl!("Save new drawings as"));
        ui.horizontal(|ui| {
            for f in crate::SaveFormat::ALL {
                let label = match f {
                    crate::SaveFormat::Dxf => crate::tl!("DXF"),
                    crate::SaveFormat::Dwg => crate::tl!("DWG"),
                };
                if ui.radio(app.ui.save_format == f, label).clicked() && app.ui.save_format != f {
                    changes.insert("saveFormat".into(), json!(f.as_str()));
                }
            }
        });
        ui.end_row();

        ui.label(crate::tl!("Default save version"));
        let current = crate::SaveVersion::parse(&app.ui.save_version).unwrap_or_default();
        let mut version = current;
        egui::ComboBox::from_id_salt("options_save_version").selected_text(version.year()).show_ui(ui, |ui| {
            for v in crate::SaveVersion::ALL {
                ui.selectable_value(&mut version, v, v.year());
            }
        });
        if version != current {
            changes.insert("saveVersion".into(), json!(version.year()));
        }
        ui.end_row();

        ui.label(crate::tl!("Units of new drawings"));
        ui.horizontal(|ui| {
            let metric = app.session.settings.measureinit;
            if ui.radio(!metric, crate::tl!("Imperial (inches)")).clicked() && metric {
                changes.insert("MEASUREINIT".into(), json!(0));
            }
            if ui.radio(metric, crate::tl!("Metric (millimetres)")).clicked() && !metric {
                changes.insert("MEASUREINIT".into(), json!(1));
            }
        });
        ui.end_row();

        ui.label(crate::tl!("Automatic save every"));
        let mut minutes = cadcraft_engine::autosave::savetime();
        let max = cadcraft_engine::autosave::MAX_SAVETIME;
        if ui.add(egui::DragValue::new(&mut minutes).range(0..=max).suffix(format!(" {}", crate::tl!("min")))).changed() {
            changes.insert("SAVETIME".into(), json!(minutes));
        }
        ui.end_row();

        ui.label(crate::tl!("Automatic save file location"));
        // Edited as text; the folder changes when the field loses focus.
        let id = egui::Id::new("cc_options_savefilepath");
        let current = cadcraft_engine::autosave::savefilepath();
        let mut path = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.clone());
        let r = ui.add(egui::TextEdit::singleline(&mut path).desired_width(330.0));
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(id, path.clone()));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(id));
            if path.trim() != current {
                changes.insert("SAVEFILEPATH".into(), json!(path.trim()));
            }
        }
        ui.end_row();
    });
    hint(ui, crate::tl!("New and the first drawing at startup use these units unless a default template is set (QNEWTEMPLATE)."));
    hint(ui, crate::tl!("A drawing opened from a file keeps that file's format."));
    hint(ui, crate::tl!("Drawings with no file yet are saved in the default version. 0 minutes turns automatic save off."));
}

/// The fonts found in the user's font folders, kept while the font set is unchanged.
#[derive(Clone, Default)]
struct FontsSeen {
    generation: Option<u64>,
    fonts: Vec<(String, Option<String>)>,
}

/// Options ▸ Fonts: the folders searched for fonts before the system's, the fonts found there and
/// the current drawing's fonts that can't be found.
fn fonts(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    let t = Tokens::get();
    let folders = cadcraft_fonts::ttf::font_dirs();
    let sel_id = egui::Id::new("cc_options_font_sel");
    let typed_id = egui::Id::new("cc_options_font_typed");
    let mut sel: Option<usize> = ui.data(|d| d.get_temp(sel_id)).filter(|i: &usize| *i < folders.len());
    let mut typed: String = ui.data(|d| d.get_temp(typed_id)).unwrap_or_default();
    let mut next = folders.clone();
    let mut changed = false;

    ui.label(RichText::new(crate::tl!("Font folders")).strong());
    hint(
        ui,
        crate::tl!(
            "Put TrueType or OpenType fonts (.ttf, .otf, .ttc) in a folder and add it here. These folders are searched before the system fonts; a folder higher in the list wins."
        ),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).inner_margin(4.0).show(ui, |ui| {
            ui.set_width(400.0);
            ui.set_min_height(92.0);
            egui::ScrollArea::vertical().id_salt("cc_options_font_dirs").max_height(92.0).auto_shrink([false, true]).show(ui, |ui| {
                if folders.is_empty() {
                    ui.label(RichText::new(crate::tl!("No font folders yet")).color(t.text_faint).italics());
                }
                for (i, f) in folders.iter().enumerate() {
                    let exists = cfg!(target_arch = "wasm32") || std::path::Path::new(f).is_dir();
                    let mut text = RichText::new(f.as_str());
                    if !exists {
                        text = text.color(t.text_faint).strikethrough();
                    }
                    let r = ui.selectable_label(sel == Some(i), text);
                    let r = if exists { r } else { r.on_hover_text(crate::tl!("This folder does not exist")) };
                    if r.clicked() {
                        sel = Some(i);
                    }
                }
            });
        });
        ui.vertical(|ui| {
            let full = folders.len() >= cadcraft_fonts::ttf::MAX_FONT_DIRS;
            if let Some(pick) = app.services.pick_folder.as_ref()
                && ui.add_enabled(!full, egui::Button::new(crate::tl!("Add Folder..."))).clicked()
                && let Some(dir) = pick()
            {
                next.push(dir);
                sel = Some(next.len() - 1);
                changed = true;
            }
            if ui.add_enabled(sel.is_some(), egui::Button::new(crate::tl!("Remove"))).clicked()
                && let Some(i) = sel
            {
                next.remove(i);
                sel = None;
                changed = true;
            }
            if ui.add_enabled(sel.is_some_and(|i| i > 0), egui::Button::new(crate::tl!("Move Up"))).clicked()
                && let Some(i) = sel
            {
                next.swap(i, i - 1);
                sel = Some(i - 1);
                changed = true;
            }
            if ui.add_enabled(sel.is_some_and(|i| i + 1 < folders.len()), egui::Button::new(crate::tl!("Move Down"))).clicked()
                && let Some(i) = sel
            {
                next.swap(i, i + 1);
                sel = Some(i + 1);
                changed = true;
            }
        });
    });
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut typed).hint_text(crate::tl!("Folder path")).desired_width(330.0));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let ok = !typed.trim().is_empty() && folders.len() < cadcraft_fonts::ttf::MAX_FONT_DIRS;
        if (ui.add_enabled(ok, egui::Button::new(crate::tl!("Add"))).clicked() || (enter && ok)) && !next.iter().any(|f| f == typed.trim()) {
            next.push(typed.trim().to_string());
            sel = Some(next.len() - 1);
            typed.clear();
            changed = true;
        }
        if ui.button(crate::tl!("Look for Fonts Again")).on_hover_text(crate::tl!("After adding or removing font files in a folder")).clicked() {
            cadcraft_fonts::ttf::rescan();
        }
    });
    ui.data_mut(|d| {
        d.insert_temp(typed_id, typed);
        if let Some(i) = sel {
            d.insert_temp(sel_id, i);
        } else {
            d.remove::<usize>(sel_id);
        }
    });
    if changed {
        changes.insert("fontFolders".into(), json!(next));
    }

    // The fonts found there.
    ui.add_space(8.0);
    let seen_id = egui::Id::new("cc_options_fonts_seen");
    let generation = cadcraft_fonts::ttf::generation();
    let mut seen: FontsSeen = ui.data(|d| d.get_temp(seen_id)).unwrap_or_default();
    if seen.generation != Some(generation) {
        let fonts = cadcraft_fonts::ttf::installed().into_iter().filter(|f| f.user).map(|f| (f.name, f.family)).collect();
        seen = FontsSeen { generation: Some(generation), fonts };
        ui.data_mut(|d| d.insert_temp(seen_id, seen.clone()));
    }
    ui.label(RichText::new(format!("{} {}", crate::tl!("Fonts found in these folders:"), seen.fonts.len())).strong());
    egui::ScrollArea::vertical().id_salt("cc_options_fonts_found").max_height(90.0).auto_shrink([false, true]).show(ui, |ui| {
        for (name, family) in &seen.fonts {
            ui.horizontal(|ui| {
                ui.label(name.as_str());
                if let Some(family) = family {
                    ui.label(RichText::new(format!("— {family}")).color(t.text_dim));
                }
            });
        }
    });

    // The current drawing's fonts that can't be found.
    if let Ok(d) = app.session.doc() {
        let missing = cadcraft_engine::cmd::missing_fonts(d);
        ui.add_space(8.0);
        if missing.is_empty() {
            hint(ui, crate::tl!("Every font this drawing uses was found."));
        } else {
            ui.label(RichText::new(crate::tl!("Fonts this drawing uses that were not found:")).strong());
            let sub = cadcraft_engine::cmd::substitute_font();
            for (style, font) in missing.iter().take(12) {
                ui.label(format!("{font}  ({} {style})  → {sub}", crate::tl!("style")));
            }
        }
    }
}

fn drafting(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>, start: &mut Option<&'static str>) {
    let t = Tokens::get();
    grid(ui, "options_drafting", |ui| {
        ui.label(crate::tl!("Object snap marker size"));
        ui.horizontal(|ui| {
            let mut size = app.options.snap_marker_size;
            if ui.add(egui::Slider::new(&mut size, super::SNAP_MARKER_SIZES).integer()).changed() {
                changes.insert("snapMarkerSize".into(), json!(size));
            }
            preview_square(ui, size, t.snap);
        });
        ui.end_row();

        ui.label(crate::tl!("Object snap aperture"));
        ui.horizontal(|ui| {
            let mut aperture = app.session.settings.aperture;
            if ui.add(egui::Slider::new(&mut aperture, 1.0..=50.0).integer().suffix(" px")).changed() {
                changes.insert("APERTURE".into(), json!(aperture));
            }
            preview_square(ui, aperture as f32, t.crosshair);
        });
        ui.end_row();
    });
    hint(ui, crate::tl!("The aperture is how near the cursor must come to a point for an object snap to find it."));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(crate::tl!("Snap, grid, polar tracking and Dynamic Input:"));
        if ui.button(crate::tl!("Drafting Settings...")).clicked() {
            *start = Some("dsettings");
        }
    });
}

fn selection(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    let t = Tokens::get();
    let st = &app.session.settings;
    let mut effects = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new(VISUAL_EFFECTS)).unwrap_or(false));
    ui.columns(2, |cols| {
        let [left, right] = cols else { return };
        // Left: how objects are picked.
        heading(left, crate::tl!("Pickbox size"));
        left.horizontal(|ui| {
            let mut pickbox = st.pickbox;
            if ui.add(egui::Slider::new(&mut pickbox, 0.0..=50.0).integer().suffix(" px")).changed() {
                changes.insert("PICKBOX".into(), json!(pickbox));
            }
            preview_square(ui, (pickbox * 3.0) as f32, t.crosshair);
        });
        left.add_space(8.0);
        heading(left, crate::tl!("Selection modes"));
        let mut pickfirst = st.pickfirst;
        if left.checkbox(&mut pickfirst, crate::tl!("Select objects first, then choose a command")).changed() {
            changes.insert("PICKFIRST".into(), json!(i32::from(pickfirst)));
        }
        let mut shift_adds = !st.pickadd;
        if left.checkbox(&mut shift_adds, crate::tl!("Hold Shift to add to the selection")).changed() {
            changes.insert("PICKADD".into(), json!(i32::from(!shift_adds)));
        }
        let mut grouping = st.pickstyle & 1 != 0;
        let mut two_clicks = !st.pickdrag;
        if left
            .checkbox(&mut two_clicks, crate::tl!("Select using two clicks only"))
            .on_hover_text(crate::tl!("Define each corner of the selection window with a separate click."))
            .changed()
        {
            changes.insert("PICKDRAG".into(), json!(if two_clicks { 0 } else { 2 }));
        }
        if left.checkbox(&mut grouping, crate::tl!("Object grouping")).changed() {
            changes.insert("PICKSTYLE".into(), json!((st.pickstyle & !1) | u8::from(grouping)));
        }
        let mut qp = st.qpmode;
        if left.checkbox(&mut qp, crate::tl!("Show Quick Properties for selected objects")).changed() {
            changes.insert("QPMODE".into(), json!(i32::from(qp)));
        }
        left.add_space(4.0);
        left.label(crate::tl!("Selection effect color"));
        color_row(left, app.options.selection_color, crate::theme::Tokens::of(crate::theme::active()).selection, "selectionColor", changes);

        // Right: grips and the preview under the cursor.
        heading(right, crate::tl!("Grip size"));
        right.horizontal(|ui| {
            let mut grip = st.gripsize;
            if ui.add(egui::Slider::new(&mut grip, 1.0..=20.0).integer().suffix(" px")).changed() {
                changes.insert("GRIPSIZE".into(), json!(grip));
            }
            preview_square(ui, (grip * 2.0) as f32, t.grip);
        });
        right.add_space(8.0);
        heading(right, crate::tl!("Grips"));
        let mut grips = st.grips > 0;
        if right.checkbox(&mut grips, crate::tl!("Show grips")).changed() {
            changes.insert("GRIPS".into(), json!(u8::from(grips)));
        }
        right.horizontal(|ui| {
            let mut limit = st.gripobjlimit;
            if ui.add_enabled(st.grips > 0, egui::DragValue::new(&mut limit).range(0..=32_767)).changed() {
                changes.insert("GRIPOBJLIMIT".into(), json!(limit));
            }
            ui.label(crate::tl!("Object selection limit for display of grips"));
        });
        right.add_space(8.0);
        heading(right, crate::tl!("Preview"));
        right.label(crate::tl!("Selection preview"));
        let (mut asking, mut idle) = (st.selectionpreview & 2 != 0, st.selectionpreview & 1 != 0);
        let a = right.checkbox(&mut asking, crate::tl!("When a command is active")).changed();
        let b = right.checkbox(&mut idle, crate::tl!("When no command is active")).changed();
        if a || b {
            changes.insert("SELECTIONPREVIEW".into(), json!(u8::from(idle) | (u8::from(asking) << 1)));
        }
        if right.button(crate::tl!("Visual Effect Settings...")).clicked() {
            effects = true;
        }
    });
    if effects {
        visual_effects(ui.ctx(), st, &mut effects, changes);
    }
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(VISUAL_EFFECTS), effects));
}

/// Whether the Visual Effect Settings window is open (egui temp data).
const VISUAL_EFFECTS: &str = "cc_options_visual_effects";

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong());
}

/// Visual Effect Settings: the selection area (window and crossing fills) and the objects the
/// cursor never highlights (PREVIEWFILTER). Changes apply at once, as on the Options tabs.
fn visual_effects(ctx: &egui::Context, st: &cadcraft_engine::Settings, open: &mut bool, changes: &mut Map<String, Value>) {
    let mut close = false;
    egui::Window::new(crate::tl!("Visual Effect Settings"))
        .id(egui::Id::new("cc_visual_effects"))
        .open(open)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.columns(2, |cols| {
                let [left, right] = cols else { return };
                heading(left, crate::tl!("Selection Area Effect"));
                let mut area = st.selectionarea;
                if left.checkbox(&mut area, crate::tl!("Indicate selection area")).changed() {
                    changes.insert("SELECTIONAREA".into(), json!(i32::from(area)));
                }
                left.add_enabled_ui(st.selectionarea, |ui| {
                    ui.label(crate::tl!("Window selection color:"));
                    aci_row(ui, st.windowareacolor, "WINDOWAREACOLOR", changes);
                    ui.label(crate::tl!("Crossing selection color:"));
                    aci_row(ui, st.crossingareacolor, "CROSSINGAREACOLOR", changes);
                    ui.label(crate::tl!("Selection area opacity:"));
                    let mut opacity = st.selectionareaopacity;
                    if ui.add(egui::Slider::new(&mut opacity, 0..=100)).changed() {
                        changes.insert("SELECTIONAREAOPACITY".into(), json!(opacity));
                    }
                });

                heading(right, crate::tl!("Selection Preview Filter"));
                right.label(crate::tl!("Exclude:"));
                for (bit, label) in [
                    (1u8, crate::tl!("Objects on locked layers")),
                    (2, crate::tl!("Xrefs")),
                    (4, crate::tl!("Tables")),
                    (32, crate::tl!("Groups")),
                    (8, crate::tl!("Multiline text")),
                    (16, crate::tl!("Hatches")),
                ] {
                    let mut on = st.previewfilter & bit != 0;
                    if right.checkbox(&mut on, label).changed() {
                        changes.insert("PREVIEWFILTER".into(), json!(if on { st.previewfilter | bit } else { st.previewfilter & !bit }));
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::tl!("Close")).clicked() {
                        close = true;
                    }
                });
            });
        });
    if close {
        *open = false;
    }
}

/// A colour by its index (ACI 1-255): a swatch and the number; a change goes into `changes`.
fn aci_row(ui: &mut egui::Ui, current: u8, key: &str, changes: &mut Map<String, Value>) {
    ui.horizontal(|ui| {
        let c = cadcraft_color::aci_rgb(current);
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c.0, c.1, c.2));
        let mut v = current;
        if ui.add(egui::DragValue::new(&mut v).range(1..=255).prefix(format!("{} ", crate::tl!("Color")))).changed() {
            changes.insert(key.into(), json!(v));
        }
    });
}

fn preferences(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    grid(ui, "options_preferences", |ui| {
        ui.label(crate::tl!("Interface language"));
        let mut lang = app.ui.interface_language;
        egui::ComboBox::from_id_salt("options_language").selected_text(lang.name()).show_ui(ui, |ui| {
            for p in crate::i18n::Preference::for_picker() {
                ui.selectable_value(&mut lang, p, p.name());
            }
        });
        if lang != app.ui.interface_language {
            changes.insert("interfaceLanguage".into(), json!(lang.code()));
        }
        ui.end_row();

        ui.label(crate::tl!("Largest array"));
        let mut max = app.session.settings.maxarray;
        if ui.add(egui::DragValue::new(&mut max).range(100..=10_000_000).speed(1000.0).suffix(format!(" {}", crate::tl!("objects")))).changed() {
            changes.insert("MAXARRAY".into(), json!(max));
        }
        ui.end_row();
    });
    hint(ui, crate::tl!("An array that would create more objects than this is refused."));
    ui.add_space(6.0);
    let mut timed = app.options.time_sensitive_right_click;
    if ui.checkbox(&mut timed, crate::tl!("Time-sensitive right-click: a quick click is Enter, holding opens the menu")).changed() {
        changes.insert("timeSensitiveRightClick".into(), json!(timed));
    }
    hint(ui, crate::tl!("Turned off, a right-click while a command runs always opens the shortcut menu."));
}

fn profiles(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    ui.label(crate::tl!(
        "CADCraft keeps these preferences, object snap modes and drafting toggles for your user account and restores them when it starts."
    ));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button(crate::tl!("Reset to Defaults")).clicked() {
            changes.insert("reset".into(), json!(true));
        }
        if ui.button(crate::tl!("Copy as JSON")).clicked() {
            ui.ctx().copy_text(serde_json::to_string_pretty(&super::report(app)).unwrap_or_default());
        }
    });
    hint(ui, crate::tl!("Reset keeps the interface language."));
}
