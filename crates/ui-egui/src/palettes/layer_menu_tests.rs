//! Popup regressions exercise real property/layer controls, including selection and row toggles.

use super::{layers_section, pick_menu, properties_section};
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
    h.get_by_label(last).click();
    h.run_steps(4);
    let handle = h.state().session.selection()[0];
    assert_eq!(h.state().session.doc().unwrap().entity(handle).unwrap().common.layer, last);
    assert!(h.query_by_label("Property Layer 01").is_none(), "selection closes the menu");
}

fn many_layers() -> CadApp {
    let mut app = CadApp::new(Session::default(), Services::default());
    for i in 0..60 {
        app.run("layer.new", json!({ "name": format!("Layer {i:02}") })).unwrap();
    }
    app.run("layer.current", json!({ "name": "Layer 00" })).unwrap();
    app.ui.show_layer_list = false;
    app
}

fn painted_rect(h: &Harness<'_, CadApp>, text: &str) -> Rect {
    let mut painted = Vec::new();
    for shape in &h.output().shapes {
        texts(&shape.shape, shape.clip_rect, &mut painted);
    }
    painted.iter().find(|(t, at, clip)| t == text && clip.contains_rect(*at)).unwrap_or_else(|| panic!("{text} is not visible: {painted:?}")).1
}

fn click_at(h: &Harness<'_, CadApp>, pos: egui::Pos2) {
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }
}

#[test]
fn current_property_layer_menu_can_select_its_last_layer() {
    let mut h = Harness::builder().with_size(vec2(400.0, 600.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            properties_section(app, ui);
        },
        many_layers(),
    );
    h.run_steps(4);
    h.get_by_label("Layer 00").click();
    h.run_steps(4);
    h.get_by_label("Layer 59").scroll_to_me();
    h.run_steps(4);
    painted_rect(&h, "Layer 59");
    h.get_by_label("Layer 59").click();
    h.run_steps(4);
    assert_eq!(h.state().session.doc().unwrap().header.str("CLAYER", ""), "Layer 59");
    assert!(h.query_by_label("Layer 01").is_none());
}

#[test]
fn layer_palette_scrolls_and_preserves_row_controls() {
    for scale in [1.0, 2.0] {
        let shots = std::env::var_os("CRAFT_UI_SUITE_SHOTS").map(std::path::PathBuf::from);
        let mut builder = Harness::builder().with_size(vec2(400.0, 240.0)).with_pixels_per_point(scale);
        if shots.is_some() {
            builder = builder.wgpu();
        }
        let mut h = builder.build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(&ui.ctx().clone());
                layers_section(app, ui);
            },
            many_layers(),
        );
        h.run_steps(4);
        click_at(&h, painted_rect(&h, "Layer 00").center());
        h.run_steps(6);
        h.get_by_label("Layer 59").scroll_to_me();
        h.run_steps(6);
        painted_rect(&h, "Layer 59");
        let y = h.get_by_label("Layer 59").rect().center().y;
        for label in ["On/Off", "Freeze/Thaw", "Lock/Unlock"] {
            let button = h.get_all_by_label(label).into_iter().find(|n| (n.rect().center().y - y).abs() < 1.0).unwrap();
            button.click();
            h.run_steps(4);
            assert!(h.query_by_label("Layer 59").is_some(), "toggling keeps the popup open");
        }
        let layer = h.state().session.doc().unwrap().layer("Layer 59").unwrap();
        assert!(!layer.on);
        assert!(layer.frozen);
        assert!(layer.locked);
        // A frozen layer cannot be made current: thaw it again before selecting it.
        h.get_all_by_label("Freeze/Thaw").into_iter().find(|n| (n.rect().center().y - y).abs() < 1.0).unwrap().click();
        h.run_steps(4);
        if let Some(directory) = shots {
            std::fs::create_dir_all(&directory).unwrap();
            h.render().unwrap().save(directory.join(format!("cadcraft-layer-popup-{scale}x.png"))).unwrap();
        }
        h.get_by_label("Layer 59").click();
        h.run_steps(4);
        assert_eq!(h.state().session.doc().unwrap().header.str("CLAYER", ""), "Layer 59");
    }
}

#[test]
fn property_linetype_menus_select_the_last_loaded_type() {
    for selected in [false, true] {
        let mut app = CadApp::new(Session::default(), Services::default());
        for i in 0..60 {
            app.session.doc_mut().unwrap().linetypes.push(cadcraft_doc::Linetype::simple(&format!("Dash {i:02}"), "", &[0.5, -0.25]));
        }
        app.run("linetype", json!({ "current": "Dash 00" })).unwrap();
        if selected {
            let line = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
            app.run("select", json!({ "handles": [line["handles"][0].clone()] })).unwrap();
        }
        let mut h = Harness::builder().with_size(vec2(400.0, 600.0)).build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(&ui.ctx().clone());
                properties_section(app, ui);
            },
            app,
        );
        h.run_steps(4);
        h.get_by_label("Dash 00").click();
        h.run_steps(4);
        h.get_by_label("Dash 59").scroll_to_me();
        h.run_steps(4);
        painted_rect(&h, "Dash 59");
        h.get_by_label("Dash 59").click();
        h.run_steps(4);
        let doc = h.state().session.doc().unwrap();
        if selected {
            assert_eq!(doc.entity(h.state().session.selection()[0]).unwrap().common.linetype, "Dash 59");
            assert_eq!(doc.header.str("CELTYPE", ""), "Dash 00", "editing an object must not change the drawing default");
        } else {
            assert_eq!(doc.header.str("CELTYPE", ""), "Dash 59");
        }
        assert!(h.query_by_label("Dash 01").is_none());
    }
}

#[test]
fn pick_menu_keeps_short_lists_small_and_selects_from_long_lists() {
    for count in [3, 60] {
        let mut h = Harness::builder().with_size(vec2(300.0, 200.0)).build_ui_state(
            move |ui, picked: &mut String| {
                if let Some(value) = pick_menu(ui, picked, (0..count).map(|i| format!("Item {i:02}"))) {
                    *picked = value;
                }
            },
            "Choose".to_string(),
        );
        h.get_by_label("Choose").click();
        h.run_steps(5);
        let last = format!("Item {:02}", count - 1);
        h.get_by_label(&last).scroll_to_me();
        h.run_steps(4);
        h.get_by_label(&last).click();
        h.run_steps(4);
        assert_eq!(h.state(), &last);
        assert!(h.query_by_label("Item 00").is_none());
    }
}
