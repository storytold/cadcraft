//! The real app's painted geometry at native and compact window sizes. Layout tabs stay anchored
//! to the status bar independently of palette visibility and docking, and remain interactive.

use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Rect, Shape, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness(width: f32, height: f32, scale: f32, message: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", serde_json::json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    if message {
        app.set_status("Drawing saved successfully with a long status message");
    }
    let mut h = Harness::builder().with_size(vec2(width, height)).with_pixels_per_point(scale).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.canvas.cursor = Some(Vec2::new(1234.5, -9876.25));
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn visible_text(shape: &Shape, clip: Rect, result: &mut Vec<(String, Rect)>) {
    match shape {
        Shape::Text(text) => {
            let rect = text.visual_bounding_rect().intersect(clip);
            if rect.is_positive() {
                result.push((text.galley.text().to_string(), rect));
            }
        }
        Shape::Vec(shapes) => {
            for shape in shapes {
                visible_text(shape, clip, result);
            }
        }
        _ => {}
    }
}

fn check_status_labels_case(width: f32, height: f32, scale: f32, message: bool, overlaps: &mut Vec<String>) {
    let mut h = harness(width, height, scale, message);
    if let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(format!("{directory}/cad-{width}-{scale}-{message}.png")).unwrap();
    }
    let mut texts = Vec::new();
    for shape in &h.output().shapes {
        visible_text(&shape.shape, shape.clip_rect, &mut texts);
    }
    texts.retain(|(_, rect)| rect.center().y > height - 26.0);
    // The current tab is always drawn; the others move to the layout list when the bar is narrow.
    assert!(texts.iter().any(|(text, _)| text == "Model"));
    if width >= 1200.0 {
        assert!(texts.iter().any(|(text, _)| text == "Layout2"));
    }
    for (i, (first, first_rect)) in texts.iter().enumerate() {
        for (second, second_rect) in texts.iter().skip(i + 1) {
            if first_rect.intersects(*second_rect) {
                overlaps.push(format!("{width}px @{scale}: {first:?} {first_rect:?} overlaps {second:?} {second_rect:?}"));
            }
        }
    }
}

#[test]
fn status_labels_never_overlap_at_standard_and_compact_sizes() {
    let mut overlaps = Vec::new();
    for (width, height) in [(1200.0, 760.0), (900.0, 560.0)] {
        for scale in [1.0, 2.0] {
            for message in [false, true] {
                check_status_labels_case(width, height, scale, message, &mut overlaps);
            }
        }
    }
    assert!(overlaps.is_empty(), "{}", overlaps.join("\n"));
}

#[test]
fn layout_tabs_stay_anchored_when_toolsets_toggle_resize_or_move() {
    for scale in [1.0, 2.0] {
        let mut h = harness(1200.0, 760.0, scale, false);
        let tab_rects =
            |h: &Harness<'_, CadApp>| ["Model", "Layout1", "Layout2"].map(|name| h.get_by_role_and_label(egui::accesskit::Role::Tab, name).rect());
        let original = tab_rects(&h);
        for (command, params) in [
            ("ui.toggle.toolsets", serde_json::json!({})),
            ("ui.toggle.toolsets", serde_json::json!({})),
            ("ui.dock", serde_json::json!({"operation":"resizeSplit", "path":[], "size":{"FixedFirst":360.0}})),
            ("ui.dock", serde_json::json!({"operation":"float", "panel":"toolSets", "rect":[400,120,260,480]})),
            ("ui.dock", serde_json::json!({"operation":"move", "panel":"toolSets", "target":"canvas", "zone":"right"})),
            ("ui.toggle.toolsets", serde_json::json!({})),
            ("ui.toggle.toolsets", serde_json::json!({})),
            ("ui.dock.reset", serde_json::json!({})),
        ] {
            h.state_mut().run(command, params.clone()).unwrap();
            h.run_steps(5);
            assert_eq!(tab_rects(&h), original, "{command} {params} at scale {scale} must not move the tabs");
        }
        // A fixed but oversized blank reservation would stop the jump without fixing the layout.
        let gap = original[0].left() - h.get_by_label("Layout list").rect().right();
        assert!((0.0..=16.0).contains(&gap), "tabs must follow the layout actions, not reserve palette space: {gap}");
    }
}

#[test]
fn layout_tabs_and_add_button_work_with_toolsets_shown_or_hidden() {
    for visible in [true, false] {
        let mut h = harness(1400.0, 760.0, 1.0, false);
        h.state_mut().run("ui.toggle.toolsets", serde_json::json!({"on":visible})).unwrap();
        h.run_steps(5);
        h.get_by_role_and_label(egui::accesskit::Role::Tab, "Layout1").click();
        h.run_steps(3);
        assert_eq!(h.state().session.state().unwrap().space, cadcraft_engine::doc::Space::Paper("Layout1".into()));
        h.get_by_role_and_label(egui::accesskit::Role::Tab, "Model").click();
        h.run_steps(3);
        assert_eq!(h.state().session.state().unwrap().space, cadcraft_engine::doc::Space::Model);
        // There are two add buttons; exercise the one between Model and the paper-layout tabs.
        let model = h.get_by_role_and_label(egui::accesskit::Role::Tab, "Model").rect();
        h.get_all_by_label("New layout").find(|node| node.rect().left() > model.right()).unwrap().click();
        h.run_steps(3);
        // The new layout first asks for the model area it shows; Enter takes the whole model.
        assert_eq!(h.state().session.state().unwrap().space, cadcraft_engine::doc::Space::Model);
        h.state_mut().cmdline("");
        h.run_steps(3);
        assert_eq!(h.state().session.state().unwrap().space, cadcraft_engine::doc::Space::Paper("Layout3".into()));
        assert_eq!(h.state().session.doc().unwrap().layouts.len(), 3);
        h.get_by_role_and_label(egui::accesskit::Role::Tab, "Layout2").click();
        h.run_steps(3);
        assert_eq!(h.state().session.state().unwrap().space, cadcraft_engine::doc::Space::Paper("Layout2".into()));
    }
}

#[test]
fn compact_toolbar_overflow_keeps_help_reachable() {
    let mut h = harness(900.0, 560.0, 1.0, false);
    h.get_by_label("More toolbar commands").click();
    h.run_steps(3);
    h.get_by_label("Help").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("about"));
}

#[test]
fn toolbar_plot_preview_opens_dialog() {
    let mut h = harness(1200.0, 760.0, 1.0, false);
    h.get_by_label("Plot Preview").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("preview"));
}
