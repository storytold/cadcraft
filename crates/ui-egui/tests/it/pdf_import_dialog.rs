//! PDF import exposes named placement controls and gives the page field initial keyboard focus.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::exchange::set_pdf_import;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_engine::geom::Vec3;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::{Action, ActionData, ActionRequest, Live, Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

fn read_pdf(_: &[u8], options: &serde_json::Value) -> Result<(Drawing, usize), String> {
    if options["page"].as_u64() != Some(2) {
        return Err("expected page 2".into());
    }
    let mut drawing = Drawing::new_metric();
    drawing
        .add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }))
        .map_err(|e| e.to_string())?;
    Ok((drawing, 4))
}

fn harness(path: String) -> Harness<'static, CadApp> {
    let services = Services { pick_open: Some(Box::new(move || Some(path.clone()))), ..Services::default() };
    let mut app = CadApp::new(Session::new(), services);
    app.start("pdfimport");
    Harness::builder().with_size(egui::vec2(900.0, 700.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    )
}

fn set_number(h: &mut Harness<'_, CadApp>, name: &str, value: f64) {
    let (target_node, target_tree) = h.get_by_role_and_label(Role::SpinButton, name).accesskit_node().locate();
    h.event(egui::Event::AccessKitActionRequest(ActionRequest {
        action: Action::SetValue,
        target_node,
        target_tree,
        data: Some(ActionData::NumericValue(value)),
    }));
    h.run_steps(2);
}

fn pdf_path() -> (std::path::PathBuf, String) {
    let name = format!("cadcraft-import-{}-{}.pdf", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed));
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, b"%PDF-1.7\nsynthetic\n").unwrap();
    (path.clone(), path.to_string_lossy().into_owned())
}

#[test]
fn page_focus_placement_acceptance_and_escape_cancellation_work() {
    set_pdf_import(read_pdf);
    let (path, name) = pdf_path();
    let mut h = harness(name.clone());
    h.run_steps(3);
    assert!(h.get_by_role_and_label(Role::SpinButton, "Page number").is_focused());
    for label in ["Page number", "Scale factor", "Insertion X", "Insertion Y"] {
        h.get_by_role_and_label(Role::SpinButton, label);
    }
    h.get_by_role_and_label(Role::PasswordInput, "Password");
    for label in ["Import text as TEXT", "Import filled shapes as solid hatches", "Use the PDF's line widths"] {
        h.get_by_role_and_label(Role::CheckBox, label);
    }
    h.get_by_role_and_label(Role::Button, "Import Page");
    h.get_by_role_and_label(Role::Button, "Cancel");

    // The reader refuses page 1: the window stays open and announces the failure.
    h.get_by_role_and_label(Role::Button, "Import Page").click();
    h.run_steps(3);
    assert!(h.state().ui.dialog.is_some(), "a failed import keeps the options window open");
    assert_eq!(h.get_by_label_contains("Import failed").accesskit_node().live(), Live::Assertive);

    set_number(&mut h, "Page number", 2.0);
    set_number(&mut h, "Scale factor", 2.0);
    set_number(&mut h, "Insertion X", 10.0);
    set_number(&mut h, "Insertion Y", 20.0);
    h.get_by_role_and_label(Role::Button, "Import Page").click();
    h.run_steps(3);
    let drawing = h.state().session.doc().unwrap();
    let Some(entity) = drawing.model.iter().next() else { panic!("the page was imported") };
    let EntityKind::Line(line) = &entity.kind else { panic!("the line remains editable geometry") };
    assert_eq!(line.a, Vec3::new(10.0, 20.0, 0.0));
    assert_eq!(line.b, Vec3::new(30.0, 20.0, 0.0));
    assert!(h.state().ui.dialog.is_none(), "successful import closes the options window");

    let mut cancelled = harness(name);
    cancelled.run_steps(3);
    cancelled.key_press(egui::Key::Escape);
    cancelled.run_steps(3);
    assert!(cancelled.state().ui.dialog.is_none(), "Escape closes the PDF options window");
    assert!(cancelled.state().session.doc().unwrap().model.is_empty(), "cancelling leaves the drawing unchanged");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_page_goes_to_the_drawing_the_window_was_opened_for() {
    set_pdf_import(read_pdf);
    let (path, name) = pdf_path();
    let mut h = harness(name);
    h.run_steps(3);
    // Another drawing is made active while the window is still open.
    h.state_mut().run("new", serde_json::json!({ "template": "." })).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().session.active, 1);
    set_number(&mut h, "Page number", 2.0);
    h.get_by_role_and_label(Role::Button, "Import Page").click();
    h.run_steps(3);
    let docs = &h.state().session.docs;
    assert!(!docs[0].doc.model.is_empty(), "the page went into the drawing that asked for it");
    assert!(docs[1].doc.model.is_empty(), "the drawing shown meanwhile is unchanged");
    std::fs::remove_file(path).unwrap();
}
