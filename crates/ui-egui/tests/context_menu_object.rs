//! Right-click menus for #1366: a selected object's shortcut menu, and the command menu at a
//! prompt with options (AutoCAD shows the options as a popup at the cursor; before, a quick
//! right-click there was just Enter and the options stayed in the command line).

use cadcraft_engine::Session;
use cadcraft_ui_egui::menus::Entry;
use cadcraft_ui_egui::{CadApp, Services, context_menu};
use serde_json::json;

/// The text painted in one frame.
fn painted(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>, time: f64) -> String {
    /// Append every text shape in a frame, one line each, so a test can assert on what the user
    /// sees (the menu labels, not just the menu's model).
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(s) => {
                text.push_str(s.galley.text());
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
        time: Some(time),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let mut text = String::new();
    for s in &output.shapes {
        collect(&s.shape, &mut text);
    }
    text
}

/// A secondary-button press (`pressed`) or release at `at`; the frame it is fed to does the rest.
fn button(at: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE }
}

/// A quick right-click (well under the time-sensitive hold) at a point on the canvas.
fn right_click(app: &mut CadApp, ctx: &egui::Context, at: egui::Pos2, time: f64) {
    painted(app, ctx, vec![egui::Event::PointerMoved(at)], time);
    painted(app, ctx, vec![button(at, true)], time + 0.02);
    painted(app, ctx, vec![button(at, false)], time + 0.04);
}

/// Every item in the canvas menus as (label path, id), submenu labels joined by `/`; the ids are
/// the ones [`context_menu::choose`] accepts.
fn items(app: &CadApp) -> Vec<(String, String)> {
    /// Flatten `es` into (prefixed label, id) pairs, descending into submenus.
    fn walk(prefix: &str, es: &[Entry], out: &mut Vec<(String, String)>) {
        for e in es {
            match e {
                Entry::Item { label, id, .. } => out.push((format!("{prefix}{label}"), id.clone())),
                Entry::Sub { label, children } => walk(&format!("{prefix}{label}/"), children, out),
            }
        }
    }
    let mut out = Vec::new();
    for g in context_menu::entries(app) {
        walk("", &g, &mut out);
    }
    out
}

/// Right-clicking a selected object opens its shortcut menu at the cursor, and an item runs.
#[test]
fn right_click_on_a_selected_object_shows_its_menu() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx, Vec::new(), 0.0);
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    app.run("view.set", json!({ "center": [5, 0], "height": 20 })).unwrap();
    let h = app.session.doc().unwrap().model.handles()[0];
    app.session.set_selection(vec![h]);
    painted(&mut app, &ctx, Vec::new(), 0.1);
    let at = app.canvas.xf.expect("canvas transform").to_screen(cadcraft_geom::Vec2::new(3.0, 0.0));

    right_click(&mut app, &ctx, at, 0.2);
    let text = painted(&mut app, &ctx, Vec::new(), 0.3);
    for label in ["Erase", "Move", "Copy Selection", "Rotate", "Draw Order", "Select Similar", "Deselect All", "Properties"] {
        assert!(text.contains(label), "{label} missing from the popup at the cursor:\n{text}");
    }

    // Choosing an item acts on the selection, like the command itself.
    assert!(items(&app).iter().any(|(_, id)| id == "erase"));
    context_menu::choose(&mut app, "erase");
    assert!(app.session.doc().unwrap().model.handles().is_empty(), "Erase from the popup removes the selected line");
}

/// A quick right-click at a prompt with options opens the command menu at the cursor instead of
/// being Enter, so the options are not only in the command line (AutoCAD's command shortcut menu).
#[test]
fn quick_right_click_at_an_options_prompt_shows_the_menu() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx, Vec::new(), 0.0);
    app.run("circle", json!({ "center": [0, 0], "radius": 50 })).unwrap();
    app.run("view.set", json!({ "center": [0, 0], "height": 200 })).unwrap();
    let h = app.session.doc().unwrap().model.handles()[0];
    app.session.set_selection(vec![h]);
    app.cmdline("ARRAY");
    assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Enter array type".into()), "{}", app.session.prompt_text());
    painted(&mut app, &ctx, Vec::new(), 0.1);
    let at = app.canvas.xf.expect("canvas transform").to_screen(cadcraft_geom::Vec2::new(20.0, 20.0));

    right_click(&mut app, &ctx, at, 0.2);
    let text = painted(&mut app, &ctx, Vec::new(), 0.3);
    // "Cancel" is a menu item only; PAth also appears in the command line.
    assert!(text.contains("Cancel"), "no command menu at the cursor:\n{text}");
    assert!(text.matches("PAth").count() >= 2, "the prompt's options are not in the menu:\n{text}");
    assert_eq!(
        app.session.current_prompt().map(|p| p.message),
        Some("Enter array type".into()),
        "a quick right-click at an options prompt is not Enter"
    );

    // Choosing the option gives the command the same input as typing it.
    context_menu::choose(&mut app, "keyword:POlar");
    assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("array"));
    assert_ne!(app.session.current_prompt().map(|p| p.message), Some("Enter array type".into()), "POlar moved the command on");
}

/// A quick right-click at a prompt without options is still Enter.
#[test]
fn quick_right_click_at_a_prompt_without_options_is_enter() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx, Vec::new(), 0.0);
    app.cmdline("LINE");
    assert!(app.session.current_prompt().is_some_and(|p| p.keywords.is_empty()), "{}", app.session.prompt_text());
    painted(&mut app, &ctx, Vec::new(), 0.1);
    let at = app.canvas.xf.expect("canvas transform").to_screen(cadcraft_geom::Vec2::new(5.0, 5.0));

    right_click(&mut app, &ctx, at, 0.2);
    let text = painted(&mut app, &ctx, Vec::new(), 0.3);
    assert!(!text.contains("Cancel"), "no menu without options:\n{text}");
    assert!(
        app.session.current_prompt().is_none_or(|p| p.message != "Specify first point"),
        "the click was not Enter: {}",
        app.session.prompt_text()
    );
}
