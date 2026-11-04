use super::properties_section;
use crate::{CadApp, Services};
use cadcraft_engine::Session;
use egui::{Rect, Shape, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn texts(shape: &Shape, clip: Rect, out: &mut Vec<(String, Rect, Rect)>) {
    match shape {
        Shape::Text(t) => out.push((t.galley.text().to_string(), t.visual_bounding_rect(), clip)),
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, clip, out)),
        _ => {}
    }
}

#[test]
fn selected_entity_layer_menu_scrolls_to_its_last_layer() {
    let mut app = CadApp::new(Session::default(), Services::default());
    for i in 0..60 {
        app.run("layer.new", json!({ "name": format!("Property Layer {i:02}") })).unwrap();
    }
    app.run("layer.current", json!({ "name": "Property Layer 00" })).unwrap();
    let line = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    app.run("select", json!({ "handles": [line["handles"][0].clone()] })).unwrap();

    let shot_dir = std::env::var_os("CRAFT_UI_SUITE_SHOTS").map(std::path::PathBuf::from);
    let mut builder = Harness::builder().with_size(vec2(400.0, 600.0));
    if shot_dir.is_some() {
        builder = builder.wgpu();
    }
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            properties_section(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("Property Layer 00").click();
    h.run_steps(3);
    let last = "Property Layer 59";
    h.get_by_label(last).scroll_to_me();
    h.run_steps(4);

    let mut painted = Vec::new();
    for shape in &h.output().shapes {
        texts(&shape.shape, shape.clip_rect, &mut painted);
    }
    let shown = painted.iter().find(|(text, _, _)| text == last).is_some_and(|(_, at, clip)| clip.contains_rect(*at));
    assert!(shown, "{last} was not scrolled into the visible layer menu: {painted:?}");
    if let Some(directory) = shot_dir {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(directory.join("cadcraft-properties-layer-menu.png")).unwrap();
    }
}
