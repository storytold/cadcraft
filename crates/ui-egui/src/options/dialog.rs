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
pub(super) const TABS: [(&str, &str); 6] = [
    ("display", "Display"),
    ("files", "Open and Save"),
    ("drafting", "Drafting"),
    ("selection", "Selection"),
    ("preferences", "User Preferences"),
    ("profiles", "Profiles"),
];

const WIDTH: f32 = 580.0;
const MIN_TAB_HEIGHT: f32 = 200.0;

/// Show the Options dialog (`name` is `"options"` or `"options:<tab>"`).
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, name: &str, open: &mut bool) {
    let tab = name.strip_prefix("options:").and_then(|t| TABS.iter().find(|(id, _)| *id == t)).map_or("display", |(id, _)| *id);
    let mut changes = Map::new();
    let mut next_tab = None;
    let mut start = None;
    let mut close = false;
    let center = ctx.content_rect().center();
    egui::Window::new(crate::tl!("Options"))
        .id(egui::Id::unique("cc_options"))
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
            let top = ui.cursor().top();
            match tab {
                "files" => files(app, ui, &mut changes),
                "drafting" => drafting(app, ui, &mut changes, &mut start),
                "selection" => selection(app, ui, &mut changes),
                "preferences" => preferences(app, ui, &mut changes),
                "profiles" => profiles(app, ui, &mut changes),
                _ => display(app, ui, &mut changes),
            }
            // The same height on every tab, so switching tabs doesn't move the buttons.
            ui.add_space((MIN_TAB_HEIGHT - (ui.cursor().top() - top)).max(8.0));
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
/// `label` is the visible row label, so a screen reader names the colour well by it.
fn color_row(ui: &mut egui::Ui, current: Option<Color32>, default: Color32, key: &str, changes: &mut Map<String, Value>, label: egui::Id) {
    ui.horizontal(|ui| {
        let shown = current.unwrap_or(default);
        let mut rgb = [shown.r(), shown.g(), shown.b()];
        if ui.color_edit_button_srgb(&mut rgb).labelled_by(label).changed() {
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

/// Display tab: interface theme, animation preference and scale, drawing-area colours, crosshair size, history length and the system-cursor option; edits accumulate in `changes`.
fn display(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    grid(ui, "options_display", |ui| {
        let theme_caption = ui.label(crate::tl!("Interface theme"));
        let mut theme = app.ui.theme;
        let theme_combo = egui::ComboBox::from_id_salt("options_theme").selected_text(theme_label(theme)).show_ui(ui, |ui| {
            for t in ThemePref::ALL {
                ui.selectable_value(&mut theme, t, theme_label(t));
            }
        });
        theme_combo.response.labelled_by(theme_caption.id);
        if theme != app.ui.theme {
            changes.insert("theme".into(), json!(theme.as_str()));
        }
        ui.end_row();

        let animations_caption = ui.label(crate::tl!("Animation effects"));
        let mut animations = app.ui.animations;
        let animations_combo = egui::ComboBox::from_id_salt("options_animations").selected_text(animations_label(animations)).show_ui(ui, |ui| {
            for a in AnimationPref::ALL {
                ui.selectable_value(&mut animations, a, animations_label(a));
            }
        });
        animations_combo.response.labelled_by(animations_caption.id);
        if animations != app.ui.animations {
            changes.insert("animations".into(), json!(animations.as_str()));
        }
        ui.end_row();

        let scale_caption = ui.label(crate::tl!("Interface scale"));
        let scale = app.options.interface_scale;
        let percent = |s: f32| format!("{} %", (s * 100.0).round());
        let scale_combo = egui::ComboBox::from_id_salt("options_interface_scale").selected_text(percent(scale)).show_ui(ui, |ui| {
            for n in [75u16, 100, 125, 150, 175, 200, 250, 300] {
                let s = f32::from(n) / 100.0;
                if ui.selectable_label((scale - s).abs() < 1e-4, format!("{n} %")).clicked() && (scale - s).abs() >= 1e-4 {
                    changes.insert("interfaceScale".into(), json!(s));
                }
            }
        });
        scale_combo.response.labelled_by(scale_caption.id);
        ui.end_row();

        let background_caption = ui.label(crate::tl!("Model background"));
        color_row(ui, app.options.model_background, Tokens::DARK.canvas, "modelBackground", changes, background_caption.id);
        ui.end_row();

        let paper_caption = ui.label(crate::tl!("Layout paper"));
        color_row(ui, app.options.paper_background, Tokens::DARK.paper, "paperBackground", changes, paper_caption.id);
        ui.end_row();

        let crosshair_caption = ui.label(crate::tl!("Crosshair size"));
        ui.horizontal(|ui| {
            let mut size = app.session.settings.cursorsize;
            // The slider's own value box is a separate spin button: name both halves with the row label.
            let slider = ui.add(egui::Slider::new(&mut size, 1.0..=100.0).integer().show_value(false)).labelled_by(crosshair_caption.id);
            let field = ui.add(egui::DragValue::new(&mut size).range(1.0..=100.0).max_decimals(0).suffix(" %")).labelled_by(crosshair_caption.id);
            if slider.changed() || field.changed() {
                changes.insert("CURSORSIZE".into(), json!(size));
            }
        });
        ui.end_row();

        let history_caption = ui.label(crate::tl!("Command line history"));
        let mut lines = app.ui.history_lines;
        if ui.add(egui::DragValue::new(&mut lines).range(0..=crate::cmdline::MAX_LINES)).labelled_by(history_caption.id).changed() {
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

/// Files tab: save format and default version, new-drawing units, autosave interval and autosave file location; edits accumulate in `changes`.
fn files(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    grid(ui, "options_files", |ui| {
        save_format_row(ui, app, changes);
        ui.end_row();
        save_version_row(ui, app, changes);
        ui.end_row();
        units_row(ui, app, changes);
        ui.end_row();
        autosave_row(ui, changes);
        ui.end_row();
        savefile_row(ui, changes);
        ui.end_row();
    });
    hint(ui, crate::tl!("New and the first drawing at startup use these units unless a default template is set (QNEWTEMPLATE)."));
    hint(ui, crate::tl!("A drawing opened from a file keeps that file's format."));
    hint(ui, crate::tl!("Drawings with no file yet are saved in the default version. 0 minutes turns automatic save off."));
}

fn save_format_row(ui: &mut egui::Ui, app: &CadApp, changes: &mut Map<String, Value>) {
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
}

fn save_version_row(ui: &mut egui::Ui, app: &CadApp, changes: &mut Map<String, Value>) {
    let version_caption = ui.label(crate::tl!("Default save version"));
    let current = crate::SaveVersion::parse(&app.ui.save_version).unwrap_or_default();
    let mut version = current;
    let version_combo = egui::ComboBox::from_id_salt("options_save_version").selected_text(version.year()).show_ui(ui, |ui| {
        for v in crate::SaveVersion::ALL {
            ui.selectable_value(&mut version, v, v.year());
        }
    });
    version_combo.response.labelled_by(version_caption.id);
    if version != current {
        changes.insert("saveVersion".into(), json!(version.year()));
    }
}

fn units_row(ui: &mut egui::Ui, app: &CadApp, changes: &mut Map<String, Value>) {
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
}

fn autosave_row(ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    let minutes_caption = ui.label(crate::tl!("Automatic save every"));
    let mut minutes = cadcraft_engine::autosave::savetime();
    let max = cadcraft_engine::autosave::MAX_SAVETIME;
    if ui.add(egui::DragValue::new(&mut minutes).range(0..=max).suffix(format!(" {}", crate::tl!("min")))).labelled_by(minutes_caption.id).changed() {
        changes.insert("SAVETIME".into(), json!(minutes));
    }
}

fn savefile_row(ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    let savefile_label = ui.label(crate::tl!("Automatic save file location"));
    // Edited as text; the folder changes when the field loses focus.
    let id = egui::Id::unique("cc_options_savefilepath");
    let current = cadcraft_engine::autosave::savefilepath();
    let mut path = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.clone());
    let r = ui.add(egui::TextEdit::singleline(&mut path).desired_width(330.0)).labelled_by(savefile_label.id);
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, path.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if path.trim() != current {
            changes.insert("SAVEFILEPATH".into(), json!(path.trim()));
        }
    }
}

/// Drafting tab: object-snap marker size and aperture previews plus the Drafting Settings entry; edits accumulate in `changes` and the entry may request a command through `start`.
fn drafting(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>, start: &mut Option<&'static str>) {
    let t = Tokens::get();
    grid(ui, "options_drafting", |ui| {
        let marker_caption = ui.label(crate::tl!("Object snap marker size"));
        ui.horizontal(|ui| {
            let mut size = app.options.snap_marker_size;
            let slider = ui.add(egui::Slider::new(&mut size, super::SNAP_MARKER_SIZES).integer().show_value(false)).labelled_by(marker_caption.id);
            let field = ui.add(egui::DragValue::new(&mut size).range(super::SNAP_MARKER_SIZES).max_decimals(0)).labelled_by(marker_caption.id);
            if slider.changed() || field.changed() {
                changes.insert("snapMarkerSize".into(), json!(size));
            }
            preview_square(ui, size, t.snap);
        });
        ui.end_row();

        let aperture_caption = ui.label(crate::tl!("Object snap aperture"));
        ui.horizontal(|ui| {
            let mut aperture = app.session.settings.aperture;
            let slider = ui.add(egui::Slider::new(&mut aperture, 1.0..=50.0).integer().show_value(false)).labelled_by(aperture_caption.id);
            let field = ui.add(egui::DragValue::new(&mut aperture).range(1.0..=50.0).max_decimals(0).suffix(" px")).labelled_by(aperture_caption.id);
            if slider.changed() || field.changed() {
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

/// Selection tab: pickbox and selection-mode controls, selection preview/area effects and the Visual Effect Settings window; edits accumulate in `changes`.
fn selection(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    let t = Tokens::get();
    let st = &app.session.settings;
    let mut effects = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique(VISUAL_EFFECTS)).unwrap_or(false));
    ui.columns(2, |cols| {
        let [left, right] = cols else { return };
        // Left: how objects are picked.
        let pickbox_caption = heading(left, crate::tl!("Pickbox size"));
        left.horizontal(|ui| {
            let mut pickbox = st.pickbox;
            let slider = ui.add(egui::Slider::new(&mut pickbox, 0.0..=50.0).integer().show_value(false)).labelled_by(pickbox_caption.id);
            let field = ui.add(egui::DragValue::new(&mut pickbox).range(0.0..=50.0).max_decimals(0).suffix(" px")).labelled_by(pickbox_caption.id);
            if slider.changed() || field.changed() {
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
        let selection_caption = left.label(crate::tl!("Selection effect color"));
        color_row(
            left,
            app.options.selection_color,
            crate::theme::Tokens::of(crate::theme::active()).selection,
            "selectionColor",
            changes,
            selection_caption.id,
        );

        // Right: grips and the preview under the cursor.
        let grip_caption = heading(right, crate::tl!("Grip size"));
        right.horizontal(|ui| {
            let mut grip = st.gripsize;
            let slider = ui.add(egui::Slider::new(&mut grip, 1.0..=20.0).integer().show_value(false)).labelled_by(grip_caption.id);
            let field = ui.add(egui::DragValue::new(&mut grip).range(1.0..=20.0).max_decimals(0).suffix(" px")).labelled_by(grip_caption.id);
            if slider.changed() || field.changed() {
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
            let field = ui.add_enabled(st.grips > 0, egui::DragValue::new(&mut limit).range(0..=32_767));
            let caption = ui.label(crate::tl!("Object selection limit for display of grips"));
            if field.labelled_by(caption.id).changed() {
                changes.insert("GRIPOBJLIMIT".into(), json!(limit));
            }
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
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(VISUAL_EFFECTS), effects));
}

/// Whether the Visual Effect Settings window is open (egui temp data).
const VISUAL_EFFECTS: &str = "cc_options_visual_effects";

/// Strong (bold) label for section headings in the Options Selection tab and the Visual Effect Settings window.
fn heading(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(RichText::new(text).strong())
}

/// Visual Effect Settings: the selection area (window and crossing fills) and the objects the
/// cursor never highlights (PREVIEWFILTER). Changes apply at once, as on the Options tabs.
fn visual_effects(ctx: &egui::Context, st: &cadcraft_engine::Settings, open: &mut bool, changes: &mut Map<String, Value>) {
    let mut close = false;
    egui::Window::new(crate::tl!("Visual Effect Settings"))
        .id(egui::Id::unique("cc_visual_effects"))
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
                    let window_caption = ui.label(crate::tl!("Window selection color:"));
                    aci_row(ui, st.windowareacolor, "WINDOWAREACOLOR", changes, window_caption.id);
                    let crossing_caption = ui.label(crate::tl!("Crossing selection color:"));
                    aci_row(ui, st.crossingareacolor, "CROSSINGAREACOLOR", changes, crossing_caption.id);
                    let opacity_caption = ui.label(crate::tl!("Selection area opacity:"));
                    ui.horizontal(|ui| {
                        let mut opacity = st.selectionareaopacity;
                        let slider = ui.add(egui::Slider::new(&mut opacity, 0..=100).show_value(false)).labelled_by(opacity_caption.id);
                        let field = ui.add(egui::DragValue::new(&mut opacity).range(0..=100)).labelled_by(opacity_caption.id);
                        if slider.changed() || field.changed() {
                            changes.insert("SELECTIONAREAOPACITY".into(), json!(opacity));
                        }
                    });
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
fn aci_row(ui: &mut egui::Ui, current: u8, key: &str, changes: &mut Map<String, Value>, label: egui::Id) {
    ui.horizontal(|ui| {
        let c = cadcraft_color::aci_rgb(current);
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c.0, c.1, c.2));
        let mut v = current;
        if ui.add(egui::DragValue::new(&mut v).range(1..=255).prefix(format!("{} ", crate::tl!("Color")))).labelled_by(label).changed() {
            changes.insert(key.into(), json!(v));
        }
    });
}

/// Preferences tab: interface language, largest-array limit and time-sensitive right-click; edits accumulate in `changes`.
fn preferences(app: &CadApp, ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    grid(ui, "options_preferences", |ui| {
        let language_caption = ui.label(crate::tl!("Interface language"));
        let mut lang = app.ui.interface_language;
        let language_combo = egui::ComboBox::from_id_salt("options_language").selected_text(lang.name()).show_ui(ui, |ui| {
            for p in crate::i18n::Preference::for_picker() {
                ui.selectable_value(&mut lang, p, p.name());
            }
        });
        language_combo.response.labelled_by(language_caption.id);
        if lang != app.ui.interface_language {
            changes.insert("interfaceLanguage".into(), json!(lang.code()));
        }
        ui.end_row();

        let array_caption = ui.label(crate::tl!("Largest array"));
        let mut max = app.session.settings.maxarray;
        if ui
            .add(egui::DragValue::new(&mut max).range(100..=10_000_000).speed(1000.0).suffix(format!(" {}", crate::tl!("objects"))))
            .labelled_by(array_caption.id)
            .changed()
        {
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
