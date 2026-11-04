//! A panic in the interface keeps the user's work: unsaved drawings are autosaved at once, the
//! app says so and writes a report, and a panic that keeps coming back leaves only a way to close
//! with the recovery copies kept. The panic hook is process-wide, so everything runs in one test.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services};

fn panic_in_a_nested_ui(app: &mut CadApp, ctx: &egui::Context, text: &'static str) -> String {
    let mut painted = String::new();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let ctx = ui.ctx().clone();
        app.guarded(&ctx, |_| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label("before");
                    std::panic::panic_any(text);
                });
            });
        });
        app.show_fault(ui.ctx());
    });
    out.textures_delta.clear(); // no renderer here
    out.shapes.iter().for_each(|s| collect(&s.shape, &mut painted));
    painted
}

fn painted_text(app: &mut CadApp, ctx: &egui::Context) -> String {
    let mut painted = String::new();
    for _ in 0..2 {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.show_fault(ui.ctx()));
        out.textures_delta.clear(); // no renderer here
        painted.clear();
        out.shapes.iter().for_each(|s| collect(&s.shape, &mut painted));
    }
    painted
}

fn collect(shape: &egui::Shape, text: &mut String) {
    match shape {
        egui::Shape::Text(t) => {
            text.push_str(t.galley.text());
            text.push('\n');
        }
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, text)),
        _ => {}
    }
}

fn recovery_copies(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "sv$")).count()).unwrap_or(0)
}

#[test]
fn a_panic_saves_the_work_and_reports_and_repeated_ones_close_safely() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let root = std::env::temp_dir().join(format!("cadcraft-ui-faults-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (auto, reports) = (root.join("autosave"), root.join("crash-reports"));
    cadcraft_ui_egui::fault::install_report_hook(reports.clone());

    let mut app = CadApp::new(Session::new(), Services::default());
    app.enable_autosave(&auto);
    app.run("circle", serde_json::json!({"center": [0, 0], "radius": 1})).unwrap();
    let ctx = egui::Context::default();

    // One panic: the drawing has a whole recovery copy by the time the frame goes on.
    panic_in_a_nested_ui(&mut app, &ctx, "a bug in a panel");
    assert_eq!(app.fault.message.as_deref(), Some("a bug in a panel"));
    assert_eq!((app.fault.autosaved, app.fault.broken), (1, false));
    assert_eq!(recovery_copies(&auto), 1);
    let text = painted_text(&mut app, &ctx);
    assert!(text.contains("kept running") && text.contains("recovery copy"), "{text}");

    let report = std::fs::read_dir(&reports).unwrap().flatten().next().expect("a report").path();
    let report = std::fs::read_to_string(report).unwrap();
    assert!(report.contains("Message: a bug in a panel") && report.contains("ui_faults.rs"), "{report}");
    assert!(report.starts_with("CADCraft "), "{report}");

    // The app still works after it.
    app.run("circle", serde_json::json!({"center": [5, 0], "radius": 1})).unwrap();

    // Panics that keep coming: the interface stops running and only offers to close.
    panic_in_a_nested_ui(&mut app, &ctx, "again");
    panic_in_a_nested_ui(&mut app, &ctx, "and again");
    assert!(app.fault.broken);
    let mut ran = false;
    app.guarded(&ctx, |_| ran = true);
    assert!(!ran, "a broken interface isn't run again");
    assert!(painted_text(&mut app, &ctx).contains("Drawing Recovery offers them"));

    // Closing now keeps the copies for the next start.
    app.shutdown_autosave();
    assert_eq!(recovery_copies(&auto), 1);
    let _ = std::fs::remove_dir_all(&root);
}
