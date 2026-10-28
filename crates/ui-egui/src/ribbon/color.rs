//! Ribbon color choice: preserve logical ByLayer/ByBlock values and reuse the shared palette.
use cadcraft_color::{Color, Rgb};
use egui::{Rect, Sense, pos2, vec2};
use serde_json::json;

use crate::{
    CadApp,
    icons::{self, Icon},
    theme::Tokens,
};

/// The common logical value, its first object's resolved swatch, and editability.
fn value(app: &CadApp) -> (Option<Color>, Rgb, bool) {
    let Ok(doc) = app.session.doc() else { return (None, Rgb(255, 255, 255), false) };
    let selection = app.session.selection();
    let (color, layer, editable) = if selection.is_empty() {
        let color = doc
            .header
            .get("CECOLOR_RGB")
            .and_then(|v| v.as_i64())
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v <= 0x00ff_ffff)
            .map(|v| Color::True(Rgb::from_u32(v)))
            .unwrap_or_else(|| Color::from_aci(i16::try_from(doc.header.i64("CECOLOR", 256)).unwrap_or(256)));
        (Some(color), doc.header.str("CLAYER", "0"), true)
    } else {
        let Some(first) = selection.first().and_then(|handle| doc.entity(*handle)) else { return (None, Rgb(255, 255, 255), false) };
        let mut common = Some(first.common.color);
        let mut editable = doc.layer(&first.common.layer).is_none_or(|layer| !layer.locked);
        for handle in selection.iter().skip(1) {
            let Some(entity) = doc.entity(*handle) else {
                common = None;
                editable = false;
                break;
            };
            if common.is_some_and(|color| entity.common.color != color) {
                common = None;
            }
            if editable && doc.layer(&entity.common.layer).is_some_and(|layer| layer.locked) {
                editable = false;
            }
        }
        (common, first.common.layer.clone(), editable)
    };
    let layer_color = doc.layer(&layer).map_or(Color::Index(7), |layer| layer.color);
    (color, color.unwrap_or(Color::ByLayer).resolve(layer_color, Color::Index(7)), editable)
}

/// Render inside the existing ribbon cell; edits use the same commands as Properties.
pub(super) fn show(ui: &mut egui::Ui, app: &CadApp, rect: Rect) -> Option<Color> {
    let (current, resolved, editable) = value(app);
    let label = current.map_or_else(String::new, |color| crate::i18n::t(&color.name()).to_string());
    let id = ui.scope_id().with("ribbon_color");
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, editable, crate::tl!("Ribbon Color"));
        info.current_text_value = Some(label.clone());
        info
    });
    let t = Tokens::get();
    ui.painter().rect_filled(rect, 2.0, if response.hovered() { t.control_hover } else { t.control });
    if current.is_some() {
        crate::layers::swatch(
            ui.painter(),
            Rect::from_min_size(pos2(rect.left() + 5.0, rect.center().y - 5.0), vec2(10.0, 10.0)),
            Color::True(resolved),
        );
    }
    let galley = ui.painter().layout_no_wrap(label.clone(), super::font(), t.text);
    let clip = Rect::from_min_max(pos2(rect.left() + 20.0, rect.top()), pos2(rect.right() - 16.0, rect.bottom()));
    ui.painter().with_clip_rect(clip).galley(pos2(clip.left(), rect.center().y - galley.size().y / 2.0), galley, t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.right() - 8.0, rect.center().y), vec2(9.0, 9.0)), Icon::ChevronDown, !editable);
    let mut picked = None;
    egui::Popup::from_toggle_button_response(&response).width(300.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        // A true-color draft belongs to the current drawing, selection and logical color.
        let palette_id = id.with((app.session.docs.get(app.session.active).map(|doc| doc.uid), app.session.selection(), current));
        ui.add_enabled_ui(editable, |ui| {
            picked = crate::layers::color_palette(ui, palette_id, current.unwrap_or(Color::ByLayer), true);
        });
        if picked.is_some() {
            ui.close();
        }
    });
    picked
}

/// Apply to the selected objects without changing the drawing's creation defaults.
pub(super) fn apply(app: &mut CadApp, color: Color) {
    let selection = app.session.selection();
    let result = if selection.is_empty() {
        app.run("color", json!({"color": color.name()}))
    } else {
        app.run("properties.set", json!({"handles": selection.iter().map(|h| h.hex()).collect::<Vec<_>>(), "color": color.name()}))
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
    fn logical_bylayer_survives_different_resolved_swatches_and_true_color_is_exact() {
        let mut app = CadApp::new(Session::default(), crate::Services::default());
        app.run("new", json!({})).unwrap();
        let mut handles = Vec::new();
        for (name, color, y) in [("CC_A", 1, 0), ("CC_B", 3, 40)] {
            app.run("layer.new", json!({"name":name,"color":color})).unwrap();
            app.run("layer.current", json!({"name":name})).unwrap();
            let result = app.run("line", json!({"points":[[0,y],[100,y]]})).unwrap();
            handles.push(Handle::parse_hex(result["handles"][0].as_str().unwrap()).unwrap());
        }
        app.session.set_selection(handles.clone());
        assert_eq!(value(&app), (Some(Color::ByLayer), Rgb(255, 0, 0), true));
        app.session.set_selection(Vec::new());
        apply(&mut app, Color::True(Rgb(12, 34, 56)));
        assert_eq!(value(&app), (Some(Color::True(Rgb(12, 34, 56))), Rgb(12, 34, 56), true));
        app.session.set_selection(vec![handles[0]]);
        apply(&mut app, Color::True(Rgb(98, 76, 54)));
        assert_eq!(value(&app), (Some(Color::True(Rgb(98, 76, 54))), Rgb(98, 76, 54), true));
        assert_eq!(app.session.doc().unwrap().entity(handles[1]).unwrap().common.color, Color::ByLayer);
        app.session.set_selection(Vec::new());
        assert_eq!(value(&app).0, Some(Color::True(Rgb(12, 34, 56))));
        apply(&mut app, Color::ByBlock);
        assert!(app.session.doc().unwrap().header.get("CECOLOR_RGB").is_none());
        assert_eq!(value(&app).0, Some(Color::ByBlock));
    }
}
