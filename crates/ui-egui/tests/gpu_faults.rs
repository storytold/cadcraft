//! A wgpu error or device loss no longer closes the app (#602): it is recorded, unsaved drawings
//! are autosaved at once instead of SAVETIME minutes later, and a notice says what happened.
//! SAVEFILEPATH is process-wide, so everything runs in one test.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::gpu::GpuFaults;
use cadcraft_ui_egui::{CadApp, Services};
use egui_wgpu::wgpu::DeviceLostReason;

/// The text the notice paints (the second frame: a new window sizes itself in the first).
fn painted_text(app: &mut CadApp, ctx: &egui::Context) -> String {
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
    let mut text = String::new();
    for _ in 0..2 {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| cadcraft_ui_egui::autosave::gpu_notice(app, ui.ctx()));
        out.textures_delta.clear(); // no renderer here
        text.clear();
        out.shapes.iter().for_each(|s| collect(&s.shape, &mut text));
    }
    text
}

#[test]
fn faults_are_recorded_autosaved_and_shown() {
    // The registry reports each new batch once; dropping the device on purpose is not a fault.
    let f = GpuFaults::default();
    assert_eq!(f.take(), None);
    f.error("first".into());
    f.error("second".into());
    let got = f.take().unwrap();
    assert_eq!((got.errors, got.lost, got.message.as_str()), (2, false, "second"));
    assert_eq!(f.take(), None);
    f.device_lost(DeviceLostReason::Destroyed, "dropped".into());
    assert_eq!(f.take(), None);
    f.device_lost(DeviceLostReason::Unknown, "driver reset".into());
    assert!(f.take().is_some_and(|g| g.lost && g.message == "driver reset"));

    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let dir = std::env::temp_dir().join(format!("cadcraft-gpu-faults-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = CadApp::new(Session::new(), Services::default());
    app.enable_autosave(&dir);
    app.run("circle", serde_json::json!({"center": [0, 0], "radius": 1})).unwrap();
    let ctx = egui::Context::default();
    cadcraft_ui_egui::framework::install_headless(&ctx);

    app.gpu_fault_tick(&ctx);
    assert!(app.gpu_notice.is_none(), "no fault, no notice");

    app.gpu_faults.error("Buffer size 302884104 is greater than the maximum buffer size".into());
    app.gpu_fault_tick(&ctx);
    let n = app.gpu_notice.clone().unwrap();
    assert_eq!((n.fault.errors, n.fault.lost, n.autosaved), (1, false, 1));
    // The change was autosaved now, not SAVETIME minutes from now.
    let written = || std::fs::read_dir(&dir).map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "sv$")).count()).unwrap_or(0);
    for _ in 0..500 {
        if written() == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(written(), 1);
    let text = painted_text(&mut app, &ctx);
    assert!(text.contains("graphics driver reported an error") && text.contains("recovery copy"), "{text}");

    // The same error again (an invalid buffer used every frame): counted, nothing new to save.
    app.gpu_faults.error("again".into());
    app.gpu_fault_tick(&ctx);
    let n = app.gpu_notice.clone().unwrap();
    assert_eq!((n.fault.errors, n.autosaved), (2, 1));
    assert!(painted_text(&mut app, &ctx).contains("(2 times)"));

    app.gpu_faults.device_lost(DeviceLostReason::Unknown, "lost".into());
    app.gpu_fault_tick(&ctx);
    assert!(app.gpu_notice.as_ref().is_some_and(|n| n.fault.lost));
    assert!(painted_text(&mut app, &ctx).contains("can't draw again until it restarts"));

    app.shutdown_autosave();
    let _ = std::fs::remove_dir_all(&dir);
}
