//! The Buttons section of the 3D Mouse dialog: one row per device button with a searchable command
//! picker and a reset, and a reset for the whole table. It edits a copy of the settings; the
//! dialog sends the result through `ui.spacemouse`, which validates it.

use egui::{RichText, TextEdit, Ui};
use serde_json::Value;

use super::bindings::{self, Binding, Buttons, MAX_BUTTONS};
use super::{Settings, State};
use crate::theme::Tokens;

/// What the picker returned this frame.
enum Pick {
    Keep,
    Clear,
    Command(&'static str),
}

/// Draws the section; true when `settings.buttons` was edited.
pub(super) fn section(ui: &mut Ui, state: &State, settings: &mut Settings) -> bool {
    ui.label(RichText::new(crate::tl!("Buttons")).strong());
    // The two default buttons, any button the device has reported and any that is bound.
    let last = settings.buttons.keys().copied().chain([2, state.highest_button]).max().unwrap_or(2).min(MAX_BUTTONS);
    let defaults = bindings::default_buttons();
    let mut changed = false;
    egui::ScrollArea::vertical().max_height(280.0).id_salt("spacemouse_buttons").show(ui, |ui| {
        for index in 1..=last {
            changed |= row(ui, state, &mut settings.buttons, &defaults, index);
        }
    });
    if ui.add_enabled(settings.buttons != defaults, egui::Button::new(crate::tl!("Reset all bindings"))).clicked() {
        settings.buttons = defaults;
        changed = true;
    }
    changed
}

fn row(ui: &mut Ui, state: &State, buttons: &mut Buttons, defaults: &Buttons, index: u8) -> bool {
    let held = state.held & (1u32 << (index - 1)) != 0;
    let mut name = RichText::new(crate::tf!("Button {n}", n = index));
    if held {
        name = name.strong().background_color(Tokens::get().select_bg);
    }
    let current = buttons.get(&index).cloned();
    let mut changed = false;
    ui.horizontal(|ui| {
        let label = ui.add_sized([72.0, ui.spacing().interact_size.y], egui::Label::new(name));
        match picker(ui, index, label.id, current.as_ref()) {
            Pick::Keep => {}
            Pick::Clear => {
                buttons.remove(&index);
                changed = true;
            }
            Pick::Command(id) => {
                // Keep the parameters when the same command is picked again.
                let params = current.as_ref().filter(|b| b.command == id).map_or(Value::Null, |b| b.params.clone());
                buttons.insert(index, Binding::with_params(id, params));
                changed = true;
            }
        }
        if ui
            .add_enabled(buttons.get(&index) != defaults.get(&index), egui::Button::new(crate::tl!("Reset")))
            .on_hover_text(crate::tl!("Restore this button's default"))
            .clicked()
        {
            match defaults.get(&index) {
                Some(binding) => buttons.insert(index, binding.clone()),
                None => buttons.remove(&index),
            };
            changed = true;
        }
    });
    if let Some(binding) = buttons.get(&index).filter(|b| !b.params.is_null()) {
        let text: String = binding.params.to_string().chars().take(64).collect();
        ui.horizontal(|ui| {
            ui.add_space(78.0);
            ui.weak(text).on_hover_text(crate::tl!("Parameters, set through the ui.spacemouse control method"));
        });
    }
    changed
}

/// `Label (id)`, the label translated.
fn describe(id: &str) -> String {
    let label = bindings::bindable_commands().iter().find(|c| c.0 == id).map_or(id, |c| c.1);
    format!("{} ({id})", crate::i18n::t(label))
}

/// A combo box over every bindable command with a search field (matching ids and English or
/// translated labels) and a None choice.
fn picker(ui: &mut Ui, index: u8, label: egui::Id, current: Option<&Binding>) -> Pick {
    let selected = current.map_or_else(|| crate::tl!("None").to_string(), |b| describe(&b.command));
    let filter_id = egui::Id::unique(("spacemouse_button_filter", index));
    let mut pick = Pick::Keep;
    let combo = craft_ui::ComboBox::from_id_salt(("spacemouse_button", index)).selected_text(selected).width(260.0).show_ui(ui, |ui| {
        let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
        let edit = ui.add(TextEdit::singleline(&mut filter).hint_text(crate::tl!("Search commands")).desired_width(f32::INFINITY));
        edit.request_focus();
        if ui.selectable_label(current.is_none(), crate::tl!("None")).clicked() {
            pick = Pick::Clear;
        }
        let needle = filter.trim().to_lowercase();
        let matches: Vec<_> = bindings::bindable_commands()
            .iter()
            .filter(|(id, label)| {
                needle.is_empty()
                    || id.contains(needle.as_str())
                    || label.to_lowercase().contains(&needle)
                    || crate::i18n::t(label).to_lowercase().contains(&needle)
            })
            .collect();
        let row_height = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical().max_height(240.0).show_rows(ui, row_height, matches.len(), |ui, range| {
            for &&(id, _) in matches.get(range).unwrap_or_default() {
                if ui.selectable_label(current.is_some_and(|b| b.command == id), describe(id)).clicked() {
                    pick = Pick::Command(id);
                }
            }
        });
        if matches!(pick, Pick::Keep) {
            ui.data_mut(|d| d.insert_temp(filter_id, filter));
        } else {
            ui.data_mut(|d| d.remove::<String>(filter_id));
        }
    });
    combo.response.labelled_by(label);
    pick
}
