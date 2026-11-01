//! A long drawing name is cut short with "…" inside its file tab instead of running under the
//! next tab and the close button (#513); the unsaved-changes mark stays visible.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Rect, Shape, vec2};
use egui_kittest::Harness;

/// Painted texts: the full source text, the painted bounds, and whether it was cut short.
fn texts(shape: &Shape, out: &mut Vec<(String, Rect, bool)>) {
    match shape {
        Shape::Text(t) => out.push((t.galley.text().to_string(), t.visual_bounding_rect(), t.galley.elided)),
        Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
        _ => {}
    }
}

#[test]
fn long_names_are_cut_short_inside_the_tab() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", serde_json::json!({})).unwrap();
    app.run("circle", serde_json::json!({"center": [0, 0], "radius": 1})).unwrap();
    app.session.state_mut().unwrap().title = "LICENCIA COFRE DE PEROTE REPOSICION FINAL.dwg".into();
    app.ui.start_tab = false;
    let mut h = Harness::builder().with_size(vec2(1200.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    let mut found = Vec::new();
    for s in &h.output().shapes {
        texts(&s.shape, &mut found);
    }
    // The window title shows the whole name too; the tab's is the lower one.
    let (_, rect, cut) =
        found.iter().filter(|(t, ..)| t.starts_with("LICENCIA")).max_by(|a, b| a.1.top().total_cmp(&b.1.top())).cloned().expect("the tab's name");
    assert!(cut, "the name ends in an ellipsis");
    // Tabs are 170 points wide: 8 of padding, then the name, the mark and the close button (24).
    assert!(rect.width() <= 170.0 - 8.0 - 24.0, "{} points wide", rect.width());
    let mark = found.iter().find(|(t, r, _)| t == "*" && (r.left() - rect.right()).abs() < 4.0);
    assert!(mark.is_some(), "the unsaved mark follows the shortened name");
    // A short name is not cut.
    assert!(found.iter().any(|(t, _, cut)| t == "Start" && !cut));
}
