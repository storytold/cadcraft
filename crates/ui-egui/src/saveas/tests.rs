use std::cell::RefCell;
use std::rc::Rc;

use cadcraft_engine::Session;
use serde_json::json;

use super::*;
use crate::Services;

/// An app whose file picker answers `answer` and records every name it was offered.
fn app_with_picker(answer: Option<String>) -> (CadApp, Rc<RefCell<Vec<String>>>) {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&asked);
    let services = Services {
        pick_open: None,
        pick_save: Some(Box::new(move |name: &str| {
            log.borrow_mut().push(name.to_string());
            answer.clone()
        })),
        ..Services::default()
    };
    (CadApp::new(Session::new(), services), asked)
}

/// Save As opens the dialog, not the picker, on the default save format (for an opened drawing
/// too, #2199) and the drawing's own version, and its choices become the `saveas` parameters.
#[test]
fn save_as_opens_the_dialog_with_the_default_format_and_the_drawings_own_version() {
    let (mut app, asked) = app_with_picker(Some("x.dxf".into()));
    app.start("ui.saveas");
    assert_eq!(app.ui.dialog.as_deref(), Some("saveas"));
    assert!(asked.borrow().is_empty(), "the picker waits for Save...");
    let st = app.saveas.clone().unwrap();
    assert_eq!((st.file_type, st.version, st.binary), (FileType::Dxf, SaveVersion::R2018, false));
    // A drawing opened as a 2000 DWG, with DWG as the default save format.
    app.ui.save_format = crate::SaveFormat::Dwg;
    let mut d = cadcraft_doc::Drawing::new_metric();
    d.header.set_str("ACADVER", "AC1015");
    app.session.open_drawing(d, "plan.dwg", Some("/tmp/plan.dwg".into()));
    open(&mut app);
    let st = app.saveas.clone().unwrap();
    assert_eq!((st.file_type, st.version), (FileType::Dwg, SaveVersion::R2000));
    assert_eq!(st.suggested_name("plan.dwg"), "plan.dwg");
    assert_eq!(st.params("/tmp/plan.dwg"), json!({ "path": "/tmp/plan.dwg", "version": "2000" }));
    // With DXF as the default, the same DWG is offered as DXF in its own version.
    app.ui.save_format = crate::SaveFormat::Dxf;
    open(&mut app);
    let st = app.saveas.clone().unwrap();
    assert_eq!((st.file_type, st.version), (FileType::Dxf, SaveVersion::R2000));
    // A template from a file is offered in the default format too, as upstream's Save As does.
    app.session.open_drawing(cadcraft_doc::Drawing::new_metric(), "base.dwt", Some("/tmp/base.dwt".into()));
    open(&mut app);
    assert_eq!(app.saveas.as_ref().map(|s| s.file_type), Some(FileType::Dxf));
}

/// Choices that don't go together settle (binary is for DXF), images and templates carry no
/// version, and the chosen extension is always the one written.
#[test]
fn choices_settle_and_become_saveas_parameters() {
    let mut st = SaveAsState { file_type: FileType::Dwg, version: SaveVersion::R2004, binary: true };
    st.settle();
    assert!(!st.binary, "binary is for DXF");
    assert_eq!(st.params("a.dwg"), json!({ "path": "a.dwg", "version": "2004" }));
    // Images and templates carry no version.
    let st = SaveAsState { file_type: FileType::Pdf, version: SaveVersion::R2018, binary: true };
    assert_eq!(st.params("a.pdf"), json!({ "path": "a.pdf" }));
    let st = SaveAsState { file_type: FileType::Dwt, ..st };
    assert_eq!(st.params("a.dwt"), json!({ "path": "a.dwt" }));
    let st = SaveAsState { file_type: FileType::Dxf, version: SaveVersion::R2004, binary: true };
    assert_eq!(st.params("a.dxf"), json!({ "path": "a.dxf", "version": "2004", "binary": true }));
    assert_eq!(st.suggested_name("Plan.dwg"), "Plan.dxf");
    assert_eq!(st.suggested_name(""), "Drawing.dxf");
    assert_eq!(FileType::from_ext(".DWG"), Some(FileType::Dwg));
    assert_eq!(with_extension("/tmp/plan", "dxf"), "/tmp/plan.dxf");
    assert_eq!(with_extension("/tmp/plan.DXF", "dxf"), "/tmp/plan.DXF");
    assert_eq!(with_extension("/tmp/plan.dwg", "dxf"), "/tmp/plan.dwg.dxf", "the chosen type is the type written");
    assert!(SaveVersion::ALL.iter().all(|v| version_label(*v).starts_with("AutoCAD ")));
}

/// Save… offers the picker a name with the chosen extension and keeps the dialog when it is
/// cancelled; without a picker the command line says how to save.
#[test]
fn save_asks_the_picker_with_the_chosen_type_and_keeps_the_dialog_when_cancelled() {
    let (mut app, asked) = app_with_picker(None);
    open(&mut app);
    if let Some(st) = app.saveas.as_mut() {
        st.file_type = FileType::Svg;
    }
    assert_eq!(save(&mut app), Some(false));
    assert_eq!(*asked.borrow(), vec!["Drawing1.svg".to_string()]);
    // Without a picker the command line says how to save.
    let mut app = CadApp::new(Session::new(), Services::default());
    assert_eq!(save(&mut app), None);
    assert!(app.session.log.iter().any(|l| l.contains("no file dialog")));
}

/// QSAVE of a drawing with no file skips the dialog: the close prompt's Save must finish at once.
#[test]
fn qsave_of_an_untitled_drawing_goes_straight_to_the_picker() {
    let (mut app, asked) = app_with_picker(None);
    app.start("qsave");
    assert_eq!(*asked.borrow(), vec!["Drawing1.dxf".to_string()]);
    assert!(app.ui.dialog.is_none());
}

/// The dialog draws for every file type and keeps the choice across frames.
#[test]
fn the_dialog_renders_its_choices() {
    let (mut app, _) = app_with_picker(None);
    for ft in FileType::ALL {
        open(&mut app);
        if let Some(st) = app.saveas.as_mut() {
            st.file_type = ft;
        }
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), ..Default::default() };
        for _ in 0..2 {
            let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
            output.textures_delta.clear();
        }
        assert_eq!(app.ui.dialog.as_deref(), Some("saveas"), "{ft:?}");
        assert_eq!(app.saveas.as_ref().map(|s| s.file_type), Some(ft));
    }
}
