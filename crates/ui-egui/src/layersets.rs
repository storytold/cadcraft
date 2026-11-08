//! Layer sets: the saved layer states as a quick switcher in the Layers palette.
//!
//! Choosing a state switches the layers to it at once, but only what a set is for: on/off, frozen
//! and locked. Colours, linetypes and the rest stay as they are now (the Layer Properties manager
//! restores everything). The save button stores the layers' current state under a name: a new
//! name makes a new set, an existing name updates that set. A right-click updates, renames or
//! deletes the set shown. The field shows the set the layers match, or "Unsaved Layer State".
//! "At Open" is always there: the layers as the drawing was opened, kept for the session only.

use std::collections::HashMap;

use egui::{Key, Rect, RichText, Sense, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// The sets to choose from: "At Open" first (unless a saved state has that name), then the saved ones.
fn sets<'a>(d: &'a cadcraft_doc::Drawing, at_open: &'a [cadcraft_doc::Layer]) -> Vec<(&'a str, &'a [cadcraft_doc::Layer])> {
    let saved = d.layer_states.iter().map(|st| (st.name.as_str(), st.layers.as_slice()));
    let shadowed = d.layer_states.iter().any(|st| st.name == cadcraft_engine::AT_OPEN);
    (!shadowed).then_some((cadcraft_engine::AT_OPEN, at_open)).into_iter().chain(saved).collect()
}

/// The set the layers match: the one chosen last if it still matches, else the first that does.
/// Layers a set doesn't know (made later) don't count, but at least one of its layers must still be
/// there; the current layer's frozen state doesn't count either (a switch never freezes it).
fn matching(d: &cadcraft_doc::Drawing, sets: &[(&str, &[cadcraft_doc::Layer])], last: Option<&str>) -> Option<String> {
    // Names compared as the engine compares them (ASCII case only).
    let now: HashMap<String, (bool, bool, bool)> = d.layers.iter().map(|l| (l.name.to_ascii_lowercase(), (l.on, l.frozen, l.locked))).collect();
    let current = d.header.str("CLAYER", "0").to_ascii_lowercase();
    let fits = |layers: &[cadcraft_doc::Layer]| {
        let mut shared = 0usize;
        let all = layers.iter().all(|l| {
            let key = l.name.to_ascii_lowercase();
            now.get(&key).is_none_or(|&(on, frozen, locked)| {
                shared += 1;
                on == l.on && (frozen == l.frozen || key == current) && locked == l.locked
            })
        });
        all && shared > 0
    };
    if let Some((n, _)) = sets.iter().find(|(n, layers)| Some(*n) == last && fits(layers)) {
        return Some((*n).to_string());
    }
    sets.iter().find(|(_, layers)| fits(layers)).map(|(n, _)| (*n).to_string())
}

/// Naming a set in place: (renaming this set, or saving when `None`; the text; focus given).
type Naming = (Option<String>, String, bool);

pub(crate) fn switcher(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Ok(st) = app.session.state() else { return };
    let d = &*st.doc;
    let last_id = ui.scope_id().with("layerset_last");
    let edit_id = ui.scope_id().with("layerset_name");
    let last: Option<String> = ui.data(|m| m.get_temp(last_id));
    let mut editing: Option<Naming> = ui.data(|m| m.get_temp(edit_id));
    let names: Vec<String> = d.layer_states.iter().map(|st| st.name.clone()).collect();
    let sets = sets(d, &st.at_open);
    let shown = matching(d, &sets, last.as_deref());
    let listed: Vec<String> = sets.iter().map(|(n, _)| (*n).to_string()).collect();
    let mut act: Option<(&str, serde_json::Value)> = None;
    // A name entered: the field stays until the command took it.
    let mut commit: Option<(&str, serde_json::Value, String)> = None;
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let w = ui.available_width() - 40.0;
        if let Some((renaming, text, ready)) = editing.as_mut() {
            let resp = ui.add_sized(
                vec2(w, 24.0),
                egui::TextEdit::singleline(text).id(egui::Id::unique("layerset_name_field")).hint_text(crate::tl!("Layer state name")),
            );
            if !*ready {
                // Focus until the field has it (the command line takes typing from a field without focus),
                // then the offered name selected once, so typing replaces it.
                if !resp.has_focus() {
                    resp.request_focus();
                } else {
                    if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), resp.id) {
                        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
                        state.cursor.set_char_range(Some(all));
                        state.store(ui.ctx(), resp.id);
                    }
                    *ready = true;
                }
            } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                editing = None;
            } else if resp.lost_focus() {
                // Enter saves; a click elsewhere is no answer, so nothing is overwritten by it.
                let name = text.trim().to_string();
                let enter = ui.input(|i| i.key_pressed(Key::Enter));
                match renaming {
                    _ if !enter || name.is_empty() => editing = None,
                    Some(from) if *from == name => editing = None,
                    Some(from) => commit = Some(("layerstate.rename", json!({ "from": from.clone(), "to": name }), name)),
                    None => commit = Some(("layerstate.save", json!({ "name": name }), name)),
                }
            }
            return;
        }
        let label = shown.clone().unwrap_or_else(|| crate::tl!("Unsaved Layer State").to_string());
        // Drawn like the current-layer field above. The list has an id of its own: the right-click
        // menu shares the field's default popup id and closes that popup on every left click.
        let (rect, combo) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, 3.0, t.control);
        p.text(pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, &label, crate::theme::body(), t.text);
        icons::paint(p, Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(12.0, 12.0)), Icon::ChevronDown, false);
        egui::Popup::from_toggle_button_response(&combo)
            .id(ui.scope_id().with("layerset_list"))
            .width(w)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for n in &listed {
                        if ui.selectable_label(shown.as_deref() == Some(n.as_str()), n).clicked() {
                            act = Some(("layerstate.restore", json!({ "name": n, "only": "visibility" })));
                            chosen = Some(n.clone());
                        }
                    }
                });
                if names.is_empty() {
                    ui.label(RichText::new(crate::tl!("No saved layer states")).color(t.text_dim));
                }
            });
        let combo = combo.on_hover_text(crate::tl!("Switches layers on and off, frozen and locked"));
        combo.context_menu(|ui| {
            // "At Open" is no saved state: nothing to rename or delete.
            let Some(n) = shown.as_ref().filter(|n| names.contains(n)) else {
                ui.label(RichText::new(crate::tl!("Unsaved Layer State")).color(t.text_dim));
                return;
            };
            if ui.button(crate::tl!("Rename")).clicked() {
                editing = Some((Some(n.clone()), n.clone(), false));
                ui.close();
            }
            if ui.button(crate::tl!("Delete")).clicked() {
                act = Some(("layerstate.delete", json!({ "name": n })));
            }
        });
        if icons::button(ui, Icon::Save, 24.0, "Save Layer State", false).clicked() {
            // The set shown is offered: Enter updates it, another name makes a new one.
            let offer = shown.clone().or_else(|| last.clone()).filter(|l| names.contains(l)).unwrap_or_default();
            editing = Some((None, offer, false));
        }
    });
    if let Some((id, p, name)) = commit {
        match app.run(id, p) {
            Ok(_) => {
                editing = None;
                chosen = Some(name);
            }
            // Taken or not allowed: the message is on the command line; the text stays, selected.
            Err(_) => {
                if let Some(e) = editing.as_mut() {
                    e.2 = false;
                }
            }
        }
    }
    if let Some((id, p)) = act {
        if app.run(id, p).is_err() {
            chosen = None;
        }
        if id == "layerstate.delete" {
            ui.data_mut(|m| m.remove::<String>(last_id));
        }
    }
    ui.data_mut(|m| {
        if let Some(n) = chosen {
            m.insert_temp(last_id, n);
        }
        match editing {
            Some(e) => {
                m.insert_temp(edit_id, e);
            }
            None => {
                m.remove::<Naming>(edit_id);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use cadcraft_doc::{Drawing, Layer, LayerState};

    use super::{matching, sets};

    fn layer(name: &str, on: bool) -> Layer {
        Layer { name: name.into(), on, ..Layer::default() }
    }

    #[test]
    fn the_field_shows_the_set_the_layers_match() {
        let mut d = Drawing::new_metric();
        d.layers.push(layer("Walls", true));
        d.layers.push(layer("Plumbing", false));
        d.layer_states.push(LayerState { name: "all on".into(), layers: vec![layer("Walls", true), layer("Plumbing", true)] });
        d.layer_states.push(LayerState { name: "no plumbing".into(), layers: vec![layer("walls", true), layer("Plumbing", false)] });
        d.layer_states.push(LayerState { name: "same".into(), layers: vec![layer("Plumbing", false)] });
        let at_open = vec![layer("Walls", true), layer("Plumbing", true)];
        let m = |d: &Drawing, last| matching(d, &sets(d, &at_open), last);
        assert_eq!(m(&d, None).as_deref(), Some("no plumbing"), "names match regardless of case");
        assert_eq!(m(&d, Some("same")).as_deref(), Some("same"), "the one chosen last wins while it matches");
        assert_eq!(m(&d, Some("all on")).as_deref(), Some("no plumbing"));
        d.layers.push(layer("Later", false));
        assert_eq!(m(&d, None).as_deref(), Some("no plumbing"), "a layer made later doesn't count");
        d.layers.iter_mut().for_each(|l| l.locked = true);
        assert_eq!(m(&d, None), None);
        // A set none of whose layers is left fits nothing.
        d.layer_states.push(LayerState { name: "gone".into(), layers: vec![layer("Renamed", true)] });
        assert_eq!(m(&d, Some("gone")), None);
        // As opened: "At Open" comes first.
        d.layers.iter_mut().for_each(|l| {
            l.locked = false;
            l.on = true;
        });
        assert_eq!(m(&d, None).as_deref(), Some(cadcraft_engine::AT_OPEN));
        assert_eq!(sets(&d, &at_open).len(), 5, "At Open and the four saved");
    }
}
