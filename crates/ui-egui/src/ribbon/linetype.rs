//! Selected-object and creation-default linetypes in the existing ribbon cell.
use crate::{
    CadApp,
    icons::{self, Icon},
    theme::Tokens,
};
use egui::{Rect, Sense, pos2, vec2};
use serde_json::json;

pub(super) enum Choice {
    Set(String),
    Manager,
}

fn value(app: &CadApp) -> (Option<String>, bool) {
    let Ok(doc) = app.session.doc() else { return (None, false) };
    let selection = app.session.selection();
    if selection.is_empty() {
        return (Some(doc.header.str("CELTYPE", "ByLayer")), true);
    }
    let Some(first) = selection.first().and_then(|h| doc.entity(*h)) else { return (None, false) };
    let name = &first.common.linetype;
    let common = selection.iter().all(|h| doc.entity(*h).is_some_and(|e| e.common.linetype.eq_ignore_ascii_case(name)));
    let editable = selection.iter().all(|h| doc.entity(*h).is_some_and(|e| doc.layer(&e.common.layer).is_none_or(|l| !l.locked)));
    (common.then(|| name.clone()), editable)
}

pub(super) fn show(ui: &mut egui::Ui, app: &CadApp, rect: Rect) -> Option<Choice> {
    let (current, _) = value(app);
    let shown = current.as_deref().map(super::by_name).unwrap_or_default();
    let t = Tokens::get();
    let response = ui.interact(rect, ui.scope_id().with("ribbon_linetype"), Sense::click());
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, crate::tl!("Ribbon Linetype"));
        info.current_text_value = Some(shown.clone());
        info
    });
    ui.painter().rect_filled(rect, 2.0, if response.hovered() { t.control_hover } else { t.control });
    if let (Some(name), Ok(doc)) = (current.as_deref(), app.session.doc()) {
        crate::layers::lt_sample(ui.painter(), Rect::from_min_size(rect.min + vec2(5.0, 4.0), vec2(30.0, 12.0)), doc.linetype(name), t.text);
    }
    let clip = Rect::from_min_max(pos2(rect.left() + 40.0, rect.top()), pos2(rect.right() - 16.0, rect.bottom()));
    let text = ui.painter().layout_no_wrap(shown, super::font(), t.text);
    ui.painter().with_clip_rect(clip).galley(pos2(clip.left(), rect.center().y - text.size().y / 2.0), text, t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.right() - 8.0, rect.center().y), vec2(9.0, 9.0)), Icon::ChevronDown, false);
    let mut choice = None;
    egui::Popup::from_toggle_button_response(&response).width(230.0).show(|ui| {
        choice = menu(ui, app);
    });
    choice
}

/// Shared choices for the expanded picker and collapsed Properties submenu.
pub(super) fn menu(ui: &mut egui::Ui, app: &CadApp) -> Option<Choice> {
    let (current, editable) = value(app);
    let t = Tokens::get();
    let mut choice = None;
    let Ok(doc) = app.session.doc() else { return None };
    // Build and sort the loaded names only while the menu is open.
    let mut names: Vec<_> = doc
        .linetypes
        .iter()
        .filter(|l| !l.name.eq_ignore_ascii_case("ByLayer") && !l.name.eq_ignore_ascii_case("ByBlock"))
        .map(|l| l.name.clone())
        .collect();
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.splice(0..0, ["ByLayer".to_string(), "ByBlock".to_string()]);
    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
        for name in names {
            ui.horizontal(|ui| {
                let (sample, _) = ui.allocate_exact_size(vec2(30.0, 16.0), Sense::hover());
                crate::layers::lt_sample(ui.painter(), sample, doc.linetype(&name), t.text);
                let selected = current.as_ref().is_some_and(|value| value.eq_ignore_ascii_case(&name));
                if ui.add_enabled(editable, egui::Button::selectable(selected, super::by_name(&name))).clicked() {
                    choice = Some(Choice::Set(name));
                    ui.close();
                }
            });
        }
    });
    ui.separator();
    if ui.button(crate::tl!("Linetype Manager")).clicked() {
        choice = Some(Choice::Manager);
        ui.close();
    }
    choice
}

pub(super) fn apply(app: &mut CadApp, choice: Choice) {
    let Choice::Set(name) = choice else {
        app.start("linetype");
        return;
    };
    let selection = app.session.selection();
    let result = if selection.is_empty() {
        app.run("linetype", json!({"current":name}))
    } else {
        app.run("properties.set", json!({"handles":selection.iter().map(|h| h.hex()).collect::<Vec<_>>(),"linetype":name}))
    };
    if let Err(error) = result {
        app.set_status(error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::{Session, doc::Handle};

    #[test]
    fn inherited_linetypes_remain_logical_across_layers() {
        let mut app = CadApp::new(Session::default(), crate::Services::default());
        app.run("new", json!({})).unwrap();
        let mut handles = Vec::new();
        for (name, y) in [("CC_A", 0), ("CC_B", 40)] {
            app.run("layer.new", json!({"name":name})).unwrap();
            app.run("layer.current", json!({"name":name})).unwrap();
            let result = app.run("line", json!({"points":[[0,y],[100,y]]})).unwrap();
            handles.push(Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap());
        }
        app.session.set_selection(handles.clone());
        assert_eq!(value(&app), (Some("ByLayer".into()), true));
        apply(&mut app, Choice::Set("ByBlock".into()));
        let doc = app.session.doc().unwrap();
        assert_eq!(doc.header.str("CELTYPE", ""), "ByLayer");
        for handle in handles {
            assert_eq!(doc.entity(handle).unwrap().common.linetype, "ByBlock");
        }
    }
}
