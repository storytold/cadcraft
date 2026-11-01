//! A PDF opens as a new, unsaved drawing; the source is never its save path.
use std::path::PathBuf;

use crate::CadApp;

pub struct Pending {
    pub path: PathBuf,
    pub page: usize,
}

/// Every result is presented in full; the normal status bar only has room for one note.
#[derive(Clone)]
pub struct Report {
    pub name: String,
    pub error: Option<String>,
    pub notes: Vec<String>,
}

impl Report {
    pub fn from_result(path: &std::path::Path, result: &Result<serde_json::Value, String>) -> Self {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        match result {
            Ok(value) => Self {
                name,
                error: None,
                notes: value
                    .get("warnings")
                    .and_then(serde_json::Value::as_array)
                    .map(|warnings| warnings.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_default(),
            },
            Err(error) => Self { name, error: Some(error.clone()), notes: Vec::new() },
        }
    }
}

/// Present conversion limitations and failures prominently without preventing the user from saving.
pub fn show_report(app: &mut CadApp, ctx: &egui::Context) {
    let Some(report) = app.pdf_reports.front().cloned() else { return };
    let mut close = false;
    let result = crate::modal::instant(ctx, egui::Id::unique("pdf_import_result")).show(ctx, |ui| {
        ui.set_width(520.0);
        if let Some(error) = &report.error {
            ui.heading("PDF import failed");
            ui.label(format!("No drawing was created from {}.", report.name));
            ui.separator();
            ui.label(error);
        } else {
            ui.heading("PDF import completed");
            ui.label(format!("Imported {} as a new, unsaved drawing.", report.name));
            ui.label("Check the source PDF and a known dimension before using the drawing.");
            ui.separator();
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for note in &report.notes {
                    ui.label(note);
                }
            });
        }
        ui.add_space(8.0);
        if ui.button("OK").clicked() {
            close = true;
        }
    });
    if close || result.should_close() {
        app.pdf_reports.pop_front();
    }
}

pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let Some(pending) = &mut app.pdf_import else { return };
    let mut open = true;
    let mut start = false;
    let mut cancel = false;
    egui::Window::new("Import vector PDF").id(egui::Id::unique("pdf_import_page")).open(&mut open).collapsible(false).resizable(false).show(
        ctx,
        |ui| {
            ui.label(format!("File: {}", pending.path.file_name().unwrap_or_default().to_string_lossy()));
            ui.horizontal(|ui| {
                ui.label("Page number (starting at 1):");
                ui.add(egui::DragValue::new(&mut pending.page).range(1..=10000));
            });
            ui.separator();
            ui.label("Geometry comes in at PDF paper size. Check and scale it to a known dimension.");
            ui.label("PDF import is experimental. Some text, images, fills or clipping may not convert.");
            ui.label("Review the imported drawing against the PDF before using it.");
            ui.horizontal(|ui| {
                if ui.button("Import page").clicked() {
                    start = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        },
    );
    if start {
        if let (Some(pending), Some(import)) = (app.pdf_import.take(), app.services.import_pdf) {
            app.opening.start_pdf(&pending.path, pending.page, import);
        }
    } else if !open || cancel {
        app.pdf_import = None;
    }
}

#[cfg(test)]
mod tests {
    use crate::{CadApp, Services};
    use cadcraft_engine::Session;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    fn fake_pdf(_: &std::path::Path, _: usize) -> Result<cadcraft_doc::Drawing, String> {
        let mut drawing = cadcraft_doc::Drawing::new_metric();
        drawing.read_notes.push("Scale note for test".into());
        drawing.read_notes.push("Text and fills omitted for test".into());
        Ok(drawing)
    }

    #[test]
    fn pdf_import_is_unsaved_and_suggests_dwg_without_touching_the_source() {
        let names = Rc::new(RefCell::new(Vec::new()));
        let asked = names.clone();
        let services = Services {
            import_pdf: Some(fake_pdf),
            pick_save: Some(Box::new(move |name| {
                asked.borrow_mut().push(name.to_owned());
                None
            })),
            ..Services::default()
        };
        let mut app = CadApp::new(Session::new(), services);
        let path = std::path::Path::new("/tmp/source plan.pdf");
        app.open_path(path);
        let pending = app.pdf_import.take().unwrap();
        assert_eq!(pending.page, 1);
        app.opening.start_pdf(path, 2, fake_pdf);
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.opening.busy() && Instant::now() < deadline {
            app.finish_opening();
            std::thread::sleep(Duration::from_millis(5));
        }
        app.finish_opening();
        let doc = app.session.state().unwrap();
        assert!(doc.path.is_none());
        assert!(doc.is_dirty());
        assert!(doc.imported_pdf);
        assert_eq!(app.pdf_reports.front().unwrap().notes, ["Scale note for test", "Text and fills omitted for test"]);
        crate::menus::run_ui_command(&mut app, "ui.saveas", &serde_json::Value::Null).unwrap().unwrap();
        assert_eq!(*names.borrow(), ["source plan - page 2.dwg"]);
    }

    #[test]
    fn result_dialog_shows_all_notes_and_failure_reason() {
        use egui_kittest::kittest::Queryable;
        let mut app = CadApp::new(Session::new(), Services::default());
        let path = std::path::Path::new("/tmp/plan.pdf");
        let success = Ok(serde_json::json!({"warnings":["scale warning", "text omitted"]}));
        app.pdf_reports.push_back(super::Report::from_result(path, &success));
        let failure = Err("clipping cannot be converted safely".to_string());
        app.pdf_reports.push_back(super::Report::from_result(path, &failure));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(800.0, 600.0))
            .build_ui_state(|ui, app: &mut CadApp| super::show_report(app, ui.ctx()), app);
        h.run_steps(3);
        assert!(h.query_by_label("scale warning").is_some());
        assert!(h.query_by_label("text omitted").is_some());
        h.get_by_label("OK").click();
        h.run_steps(3);
        assert!(h.query_by_label("PDF import failed").is_some());
        assert!(h.query_by_label("clipping cannot be converted safely").is_some());
        h.get_by_label("OK").click();
        h.run_steps(3);
        assert!(h.state().pdf_reports.is_empty());
    }
}
