//! Window ▸ Blocks (#1249): a block library taller than the window scrolls down to its last block.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Rect, Shape, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// Painted texts, with the clip rectangle they are painted in.
fn texts(shape: &Shape, clip: Rect, out: &mut Vec<(String, Rect, Rect)>) {
    match shape {
        Shape::Text(t) => out.push((t.galley.text().to_string(), t.visual_bounding_rect(), clip)),
        Shape::Vec(v) => v.iter().for_each(|s| texts(s, clip, out)),
        _ => {}
    }
}

#[test]
fn a_long_block_list_scrolls_to_its_last_block() {
    let mut app = CadApp::new(Session::new(), Services::default());
    for i in 0..60 {
        let c = app.session.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
        let h = c["handle"].as_str().unwrap().to_string();
        app.session.execute("block", &json!({"name": format!("Block {i:02}"), "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
    }
    let last = app.session.doc().unwrap().blocks.keys().rfind(|k| !k.starts_with('*')).unwrap().clone();
    app.ui.dialog = Some("blocks".into());
    let mut h = Harness::builder().with_size(vec2(1200.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    let window = h.ctx.memory(|m| m.area_rect(egui::Id::new("Blocks"))).unwrap();
    h.get_by_label(&last).scroll_to_me();
    h.run_steps(5);
    let mut found = Vec::new();
    for s in &h.output().shapes {
        texts(&s.shape, s.clip_rect, &mut found);
    }
    let drawn = found.iter().find(|(t, _, _)| *t == last);
    let shown = drawn.is_some_and(|(_, at, clip)| window.contains_rect(*clip) && clip.contains_rect(*at));
    assert!(shown, "{last} is not drawn inside the Blocks window {window:?}: {drawn:?}");
}

#[test]
fn filtering_blocks_shows_only_matching_blocks() {
    let mut app = CadApp::new(Session::new(), Services::default());
    for name in ["Valve_DN50", "Pump_Centrifugal", "Damper_Fire"] {
        let c = app.session.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
        let h = c["handle"].as_str().unwrap().to_string();
        app.session.execute("block", &json!({"name": name, "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
    }
    app.ui.dialog = Some("blocks".into());
    let mut h = Harness::builder().with_size(vec2(1200.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    assert!(h.query_by_label("Valve_DN50").is_some());
    assert!(h.query_by_label("Pump_Centrifugal").is_some());

    let filter_id = egui::Id::new("blockfilter");
    h.ctx.data_mut(|d| d.insert_temp(filter_id, "pump".to_string()));
    h.run_steps(2);
    assert!(h.query_by_label("Pump_Centrifugal").is_some());
    assert!(h.query_by_label("Valve_DN50").is_none());
    assert!(h.query_by_label("Damper_Fire").is_none());
}

#[test]
fn blocks_menu_opens_the_dockable_blocks_palette() {
    let mut app = CadApp::new(Session::new(), Services::default());
    assert!(!app.ui.docking.layout.contains(&cadcraft_ui_egui::docking::Panel::Blocks));
    app.run("ui.dialog.blocks", json!({})).unwrap();
    assert!(app.ui.docking.layout.contains(&cadcraft_ui_egui::docking::Panel::Blocks));
}
