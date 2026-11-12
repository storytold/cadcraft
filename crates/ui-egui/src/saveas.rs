//! The Save As dialog: the file type, the DXF/DWG version and binary DXF, chosen before the
//! host's file picker, which cannot report the type the user picked. "Save..." opens the picker
//! for the chosen type and runs `saveas` (images: an export, as SAVEAS to an image does).
//!
//! QSAVE of a drawing that has no file yet skips this dialog and goes straight to the picker
//! (`ui.saveas.pick`): the close prompt's "Save" relies on that save finishing in the same call.

use egui::{RichText, vec2};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{CadApp, SaveFormat, SaveVersion};

/// A file type the dialog saves as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Dxf,
    Dwg,
    Dwt,
    Svg,
    Png,
    Pdf,
}

impl FileType {
    pub const ALL: [FileType; 6] = [FileType::Dxf, FileType::Dwg, FileType::Dwt, FileType::Svg, FileType::Png, FileType::Pdf];

    /// The file extension, without the dot.
    pub fn ext(self) -> &'static str {
        match self {
            FileType::Dxf => "dxf",
            FileType::Dwg => "dwg",
            FileType::Dwt => "dwt",
            FileType::Svg => "svg",
            FileType::Png => "png",
            FileType::Pdf => "pdf",
        }
    }

    /// The name the dialog shows (translated; the format names stay as they are).
    pub fn label(self) -> &'static str {
        match self {
            FileType::Dxf => crate::tl!("Drawing (DXF)"),
            FileType::Dwg => crate::tl!("Drawing (DWG)"),
            FileType::Dwt => crate::tl!("Drawing Template (DWT)"),
            FileType::Svg => "SVG",
            FileType::Png => "PNG",
            FileType::Pdf => "PDF",
        }
    }

    /// The type of an extension.
    pub fn from_ext(ext: &str) -> Option<FileType> {
        let ext = ext.trim().trim_start_matches('.');
        FileType::ALL.into_iter().find(|t| t.ext().eq_ignore_ascii_case(ext))
    }

    /// A drawing file whose version the dialog asks for (DXF, DWG).
    fn versioned(self) -> bool {
        matches!(self, FileType::Dxf | FileType::Dwg)
    }
}

/// The dialog's choices until Save.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveAsState {
    pub file_type: FileType,
    pub version: SaveVersion,
    pub binary: bool,
}

impl SaveAsState {
    /// The choices a drawing starts from: the default save format, the drawing's own version (an
    /// untitled drawing: the default save version) and its binary flag.
    pub fn for_app(app: &CadApp) -> SaveAsState {
        let st = app.session.state().ok();
        let untitled = st.as_ref().is_none_or(|s| s.path.is_none());
        // The default save format, for an opened drawing too (#2199: AutoCAD's Options > Open and
        // Save > "Save as"); the dialog can change it.
        let file_type = match app.ui.save_format {
            SaveFormat::Dxf => FileType::Dxf,
            SaveFormat::Dwg => FileType::Dwg,
        };
        let own =
            app.session.doc().ok().and_then(|d| d.header.get("ACADVER").and_then(cadcraft_doc::HVal::as_str).and_then(SaveVersion::for_acadver));
        let version = match (untitled, own) {
            (false, Some(v)) => v,
            _ => SaveVersion::parse(&app.ui.save_version).unwrap_or_default(),
        };
        SaveAsState { file_type, version, binary: st.as_ref().is_some_and(|s| s.binary_dxf) && file_type == FileType::Dxf }
    }

    /// The `saveas` parameters for `path`.
    pub fn params(&self, path: &str) -> Value {
        let mut p = json!({ "path": path });
        if self.file_type.versioned()
            && let Some(o) = p.as_object_mut()
        {
            o.insert("version".into(), json!(self.version.year()));
            if self.binary && self.file_type == FileType::Dxf {
                o.insert("binary".into(), json!(true));
            }
        }
        p
    }

    /// The file name the picker suggests: the drawing's name with the chosen extension.
    pub fn suggested_name(&self, title: &str) -> String {
        let stem = std::path::Path::new(title).file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.trim().is_empty());
        format!("{}.{}", stem.unwrap_or_else(|| "Drawing".into()), self.file_type.ext())
    }

    /// Binary is for DXF only.
    fn settle(&mut self) {
        if self.file_type != FileType::Dxf {
            self.binary = false;
        }
    }
}

/// Open the dialog for the active drawing.
pub fn open(app: &mut CadApp) {
    app.saveas = Some(SaveAsState::for_app(app));
    app.ui.dialog = Some("saveas".into());
}

/// `path` with the chosen type's extension: kept when the picker gave it (any case), added
/// otherwise (`plan.dwg` saved as DXF is `plan.dwg.dxf`), so the type chosen is the type written.
pub(crate) fn with_extension(path: &str, ext: &str) -> String {
    let has = std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));
    if has { path.to_string() } else { format!("{path}.{ext}") }
}

/// Ask the host where to save and run `saveas` with the dialog's choices, on the drawing that is
/// active now. `Some(true)` once saved, or once an asynchronous file dialog is shown (the save
/// happens when it answers, [`CadApp::pick_then`]); `Some(false)` when the picker was cancelled
/// or the save failed (the dialog stays); `None` without a picker (said on the command line).
pub fn save(app: &mut CadApp) -> Option<bool> {
    let st = app.saveas.clone().unwrap_or_else(|| SaveAsState::for_app(app));
    let title = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    if app.services.pick_save.is_none() {
        app.session
            .echo(r#"SAVEAS: there is no file dialog here; run it with parameters, e.g. saveas {"path": "plan.dxf", "version": 2018}"#.to_string());
        return None;
    }
    let asynchronous = app.services.start_pick.is_some();
    let saved = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = saved.clone();
    let name = st.suggested_name(&title);
    let r = app.pick_then(crate::Pick::Save(name), move |app, path| {
        let r = app.run("saveas", st.params(&with_extension(&path, st.file_type.ext())));
        flag.set(r.is_ok());
        r
    })?;
    Some(r.is_ok() && (asynchronous || saved.get()))
}

/// A dimmed, wrapping note.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(Tokens::get().text_dim)).wrap());
}

/// The Save As window: file type, version and binary DXF, then Save… (the picker) or Cancel. The
/// choices live in `app.saveas` until the window closes.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    if app.saveas.is_none() {
        app.saveas = Some(SaveAsState::for_app(app));
    }
    let Some(mut st) = app.saveas.clone() else { return };
    let (mut save_clicked, mut cancel) = (false, false);
    egui::Window::new(crate::tl!("Save As"))
        .id(egui::Id::unique("saveas_dialog"))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(440.0);
            egui::Grid::new("saveas_grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label(crate::tl!("File type"));
                craft_ui::ComboBox::from_id_salt("saveas_type").selected_text(st.file_type.label()).width(260.0).show_ui(ui, |ui| {
                    for ft in FileType::ALL {
                        ui.selectable_value(&mut st.file_type, ft, ft.label());
                    }
                });
                ui.end_row();
                if st.file_type.versioned() {
                    ui.label(crate::tl!("Version"));
                    craft_ui::ComboBox::from_id_salt("saveas_version").selected_text(version_label(st.version)).width(260.0).show_ui(ui, |ui| {
                        for v in SaveVersion::ALL {
                            ui.selectable_value(&mut st.version, v, version_label(v));
                        }
                    });
                    ui.end_row();
                }
                if st.file_type == FileType::Dxf {
                    ui.label("");
                    ui.checkbox(&mut st.binary, crate::tl!("Binary DXF"));
                    ui.end_row();
                }
            });
            st.settle();
            if !st.file_type.versioned() && st.file_type != FileType::Dwt {
                note(ui, crate::tl!("An export of the drawing as shown: the drawing keeps its own file."));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save_clicked = ui.button(crate::tl!("Save...")).clicked();
                    cancel = ui.button(crate::tl!("Cancel")).clicked();
                });
            });
        });
    app.saveas = Some(st);
    // Closed once saved, or when there is no picker to ask (the command line says how to save).
    if cancel || (save_clicked && save(app) != Some(false)) {
        *open = false;
    }
    if !*open {
        app.saveas = None;
    }
}

/// A version's label: "AutoCAD 2018".
pub fn version_label(v: SaveVersion) -> String {
    format!("AutoCAD {}", v.year())
}

#[cfg(test)]
mod tests;
