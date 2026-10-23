//! The real app's painted geometry at native and compact window sizes.

use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Rect, Shape, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// `pc`: the Windows layout (Ribbon); else the Mac layout (Tool Bar).
fn harness(width: f32, height: f32, scale: f32, message: bool, pc: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", serde_json::json!({})).unwrap();
    app.run(if pc { "ui.layout.pc" } else { "ui.layout.mac" }, serde_json::json!({})).unwrap();
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

#[test]
fn status_labels_never_overlap_at_standard_and_compact_sizes() {
    let mut overlaps = Vec::new();
    for (width, height) in [(1200.0, 760.0), (900.0, 560.0)] {
        for scale in [1.0, 2.0] {
            for (message, pc) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut h = harness(width, height, scale, message, pc);
                if let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") {
                    std::fs::create_dir_all(&directory).unwrap();
                    h.render().unwrap().save(format!("{directory}/cad-{width}-{scale}-{message}-{}.png", if pc { "pc" } else { "mac" })).unwrap();
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
        }
    }
    assert!(overlaps.is_empty(), "{}", overlaps.join("\n"));
}

#[test]
fn compact_toolbar_overflow_keeps_help_reachable() {
    let mut h = harness(900.0, 560.0, 1.0, false, false);
    h.get_by_label("More toolbar commands").click();
    h.run_steps(3);
    h.get_by_label("Help").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("about"));
}

#[test]
fn toolbar_plot_preview_opens_dialog() {
    let mut h = harness(1200.0, 760.0, 1.0, false, false);
    h.get_by_label("Plot Preview").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("preview"));
}

#[test]
fn ribbon_output_tab_preview_opens_dialog() {
    let mut h = harness(1200.0, 760.0, 1.0, false, true);
    h.get_by_label("Output").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, "output");
    h.get_by_label("Preview").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("preview"));
}

#[test]
fn ribbon_home_tab_starts_line() {
    let mut h = harness(1200.0, 760.0, 1.0, false, true);
    h.get_by_label("Line").click();
    h.run_steps(3);
    assert!(h.state().session.running.is_some(), "LINE should be prompting");
}

#[test]
fn the_in_window_menu_bar_sits_above_the_ribbon() {
    let mut h = harness(1200.0, 760.0, 1.0, false, true);
    // Windows and Linux draw the menu bar in the window (macOS uses the system one).
    h.state_mut().ui.in_window_menu = true;
    h.run_steps(5);
    let mut texts = Vec::new();
    for shape in &h.output().shapes {
        visible_text(&shape.shape, shape.clip_rect, &mut texts);
    }
    let top = |label: &str| texts.iter().find(|(text, _)| text == label).map(|(_, rect)| rect.top()).unwrap_or_else(|| panic!("no {label:?} drawn"));
    // The menu bar's File, then the Ribbon's Home tab under it.
    assert!(top("File") < top("Home"), "File at {}, Home at {}", top("File"), top("Home"));
}

#[test]
fn the_windows_layout_has_no_collapsed_tool_sets_strip() {
    // The Ribbon holds the tools: no 22 px Tool Sets strip beside the Properties palette.
    let h = harness(1200.0, 760.0, 1.0, false, true);
    assert!(h.query_by_label("Expand Tool Sets").is_none());
    // The Mac layout with Tool Sets hidden keeps its strip to bring them back.
    let mut h = harness(1200.0, 760.0, 1.0, false, false);
    h.state_mut().ui.show_toolsets = false;
    h.run_steps(3);
    assert!(h.query_by_label("Expand Tool Sets").is_some());
}
