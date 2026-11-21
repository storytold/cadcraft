//! The base and detail in Plot Preview must represent the same document revision.
use cadcraft_engine::{
    Session,
    cmd::file::{IoHooks, set_io},
};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

static RENDERS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn preview_rebuilds_after_a_drawing_switch_and_content_edit() {
    set_io(IoHooks {
        read: |_, _| Err("no reader".into()),
        write: |_, _| Err("no writer".into()),
        plot: Some(|_, _, _| {
            RENDERS.fetch_add(1, Ordering::SeqCst);
            let image = image::RgbaImage::from_pixel(1600, 1000, image::Rgba([255, 255, 255, 255]));
            let mut data = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image).write_to(&mut data, image::ImageFormat::Png).unwrap();
            Ok(data.into_inner())
        }),
    });
    let mut app = CadApp::new(Session::new(), Services::default());
    app.start("preview");
    let ctx = egui::Context::default();
    let frame = |app: &mut CadApp| {
        let mut output = ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), ..Default::default() },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
    };
    frame(&mut app);
    frame(&mut app);
    let initial = RENDERS.load(Ordering::SeqCst);
    app.run("new", json!({})).unwrap();
    frame(&mut app);
    assert_eq!(RENDERS.load(Ordering::SeqCst), initial + 1, "switch invalidates the base");
    app.run("line", json!({ "points": [[0,0],[10,0]] })).unwrap();
    frame(&mut app);
    assert_eq!(RENDERS.load(Ordering::SeqCst), initial + 2, "content edits invalidate the base too");
}
