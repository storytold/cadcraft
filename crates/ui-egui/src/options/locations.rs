//! The Options dialog's Files tab, as AutoCAD's: the files CADCraft uses rather than how it saves
//! (Open and Save). The default template for new drawings (QNEWTEMPLATE), the font that stands in
//! for a missing one (FONTALT) and the fonts searched for characters a text's font lacks
//! (FONTFALLBACK). They are process-wide settings, kept in the user profile.

use serde_json::{Map, Value, json};

/// A text field that commits when it loses focus; `Some(text)` (trimmed) on a change.
fn commit_text(ui: &mut egui::Ui, salt: &str, current: &str, label: egui::Id) -> Option<String> {
    let id = egui::Id::unique(("cc_options_locations", salt));
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(330.0)).labelled_by(label);
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if text.trim() != current {
            return Some(text.trim().to_string());
        }
    }
    None
}

/// Files tab: the default template and the font substitutes, edited as text and sent when a field
/// loses focus.
pub(super) fn locations(ui: &mut egui::Ui, changes: &mut Map<String, Value>) {
    super::dialog::grid(ui, "options_locations", |ui| {
        let rows = [
            ("QNEWTEMPLATE", crate::tl!("Default template for new drawings"), cadcraft_engine::cmd::file::qnew_template()),
            ("FONTALT", crate::tl!("Alternate font"), cadcraft_fonts::ttf::font_alt()),
            ("FONTFALLBACK", crate::tl!("Fallback fonts"), cadcraft_fonts::ttf::fallback_fonts()),
        ];
        for (var, label, current) in rows {
            let caption = ui.label(label);
            if let Some(text) = commit_text(ui, var, &current, caption.id) {
                changes.insert(var.into(), json!(text));
            }
            ui.end_row();
        }
    });
    super::dialog::hint(ui, crate::tl!("The template is a .dwt or .dxf file that New and the first drawing at startup are made from; empty: none."));
    super::dialog::hint(ui, crate::tl!("The alternate font replaces a text style font that can't be found; empty: the built-in drafting font."));
    super::dialog::hint(ui, crate::tl!("Fallback fonts (names separated by commas) are searched for characters a text's own font lacks."));
    super::dialog::hint(ui, crate::tl!("The automatic save file location is on Open and Save."));
}
