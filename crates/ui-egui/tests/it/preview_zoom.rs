//! The Plot Preview window draws the sheet once, and zooming in on it draws the part in view again
//! at the screen's resolution (a `crop` of the sheet) once the view rests.

use std::sync::Mutex;

use cadcraft_doc::{Drawing, Space};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, MouseWheelUnit, vec2};
use egui_kittest::Harness;
use serde_json::{Value, json};

/// The options of every `plot` the preview made.
static PLOTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

/// A plot hook that records the options it is given.
fn recording_plot(d: &Drawing, space: &Space, opts: &Value) -> Result<Vec<u8>, String> {
    if let Ok(mut plots) = PLOTS.lock() {
        plots.push(opts.clone());
    }
    cadcraft_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())
}

/// The PNG plots so far: other tests in this process also plot (to PDF) through the global hook.
fn plots() -> Vec<Value> {
    PLOTS.lock().map(|p| p.iter().filter(|o| o["output"] == "png").cloned().collect()).unwrap_or_default()
}

/// Zooming in draws the part in view again at the screen's resolution.
#[test]
fn zooming_in_draws_the_part_in_view_at_the_screens_resolution() {
    let _preview = crate::PLOT_PREVIEW.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    set_io(IoHooks { read: |_, _| Err("no".into()), write: |_, _| Err("no".into()), plot: Some(recording_plot) });
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.run("line", json!({ "points": [[0, 0], [100, 60]] })).unwrap();
    app.run("mview", json!({ "layout": "Layout1" })).unwrap();
    app.run("layout.set", json!({ "name": "Layout1" })).unwrap();
    let mut h = Harness::builder().with_size(vec2(1280.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);

    PLOTS.lock().unwrap().clear();
    h.state_mut().start("preview");
    h.run_steps(5);
    let first = plots();
    assert_eq!(first.len(), 1, "the whole sheet is drawn once: {first:?}");
    assert!(first[0].get("crop").is_none());
    assert_eq!(first[0]["output"], "png");

    // Scroll up over the preview: zoom in. Rendered again only once the view rests.
    let over = egui::pos2(450.0, 380.0);
    h.hover_at(over);
    h.run_steps(2);
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 1000.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
    });
    h.run_steps(12);
    let after = plots();
    assert!(after.len() >= 2, "the part in view is drawn again: {} plots", after.len());
    let detail = after.last().unwrap();
    let crop: Vec<f64> = detail["crop"].as_array().expect("a crop").iter().filter_map(Value::as_f64).collect();
    assert_eq!(crop.len(), 4);
    assert!(crop[2] - crop[0] < 1.0 && crop[3] - crop[1] < 1.0, "a part of the sheet, not all of it: {crop:?}");
    assert!(detail["resolution"].as_f64().unwrap() > first[0]["resolution"].as_f64().unwrap(), "at a higher resolution than the whole sheet");

    // Zooming back out to the whole sheet needs no further drawing.
    let n = plots().len();
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -2000.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
    });
    h.run_steps(12);
    assert_eq!(plots().len(), n, "the whole sheet is shown from its first image");
}
