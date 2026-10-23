//! The About window: a compact, fixed-width window that opens from Help.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, Rect, Shape, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness(scale: f32) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", serde_json::json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    let mut h = Harness::builder().with_size(vec2(1200.0, 760.0)).with_pixels_per_point(scale).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn open_about(h: &mut Harness<'static, CadApp>) {
    cadcraft_ui_egui::menus::activate(h.state_mut(), "ui.dialog.about");
    h.run_steps(5);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("about"));
}

fn window(h: &Harness<'static, CadApp>) -> Rect {
    h.ctx.memory(|m| m.area_rect(egui::Id::new("About CADCraft"))).unwrap()
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

#[test]
fn about_opens_from_help_with_build_details_and_no_tabs() {
    let mut h = harness(1.0);
    h.get_by_label("Help").click();
    h.run_steps(5);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("about"));
    for label in ["Architecture", "Built", "License", "Website", "Discord", "GitHub", "Not affiliated with Autodesk, Inc."] {
        h.get_by_label(label);
    }
    for tab in ["Contributors", "Models"] {
        assert!(h.query_by_label(tab).is_none(), "{tab} is not part of the About window");
    }
}

#[test]
fn about_window_is_400_wide_and_centred() {
    for scale in [1.0, 2.0] {
        let mut h = harness(scale);
        open_about(&mut h);
        let rect = window(&h);
        assert!((rect.width() - 400.0).abs() <= 1.0, "@{scale}: {rect:?}");
        assert!((rect.center().x - 600.0).abs() <= 1.0, "@{scale}: {rect:?}");
    }
}

#[test]
fn about_has_no_overlapping_text() {
    let mut overlaps = Vec::new();
    for scale in [1.0, 2.0] {
        let mut h = harness(scale);
        open_about(&mut h);
        let inside = window(&h);
        let mut texts = Vec::new();
        for shape in &h.output().shapes {
            visible_text(&shape.shape, shape.clip_rect, &mut texts);
        }
        texts.retain(|(_, rect)| inside.contains_rect(*rect));
        assert!(texts.iter().any(|(text, _)| text == "Architecture"));
        for (text, rect) in &texts {
            assert!(rect.left() >= inside.left() && rect.right() <= inside.right(), "@{scale}: {text:?} leaves the window");
        }
        for (i, (first, first_rect)) in texts.iter().enumerate() {
            for (second, second_rect) in texts.iter().skip(i + 1) {
                if first_rect.intersects(*second_rect) {
                    overlaps.push(format!("@{scale}: {first:?} {first_rect:?} overlaps {second:?} {second_rect:?}"));
                }
            }
        }
    }
    assert!(overlaps.is_empty(), "{}", overlaps.join("\n"));
}

#[test]
fn escape_closes_about_and_keeps_the_running_command() {
    let mut h = harness(1.0);
    h.state_mut().start("line");
    h.run_steps(3);
    assert!(h.state().session.current_prompt().is_some());
    open_about(&mut h);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog, None);
    assert!(h.state().session.current_prompt().is_some(), "Escape cancelled the command behind the About window");
}
