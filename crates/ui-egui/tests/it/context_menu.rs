//! The canvas's right-click menu (#325): Repeat, the Clipboard commands with AutoCAD's rules for
//! what is available, and the selection commands.

use cadcraft_engine::Session;
use cadcraft_ui_egui::menus::Entry;
use cadcraft_ui_egui::{CadApp, Services, context_menu};
use serde_json::json;

/// (label, id, enabled) of the items, with submenu items prefixed by "Sub/".
fn items(app: &CadApp) -> Vec<(String, String, bool)> {
    fn walk(prefix: &str, es: &[Entry], out: &mut Vec<(String, String, bool)>) {
        for e in es {
            match e {
                Entry::Item { label, id, enabled, .. } => out.push((format!("{prefix}{label}"), id.clone(), *enabled)),
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

fn enabled(app: &CadApp, id: &str) -> bool {
    items(app).iter().find(|(_, i, _)| i == id).is_some_and(|(_, _, e)| *e)
}

#[test]
fn context_menu_offers_the_clipboard() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _, _)| l).collect();
    for l in [
        "Clipboard/Cut",
        "Clipboard/Copy",
        "Clipboard/Copy with Base Point",
        "Clipboard/Paste",
        "Clipboard/Paste as Block",
        "Clipboard/Paste to Original Coordinates",
        "Undo",
        "Redo",
        "Pan",
        "Zoom",
    ] {
        assert!(labels.iter().any(|x| x == l), "{l} missing from {labels:?}");
    }
    assert!(!labels.iter().any(|l| l.starts_with("Repeat")), "no command yet");
    assert!(!enabled(&app, "pasteclip"), "nothing to paste");

    app.cmdline("LINE 0,0 10,0");
    app.cmdline("");
    app.run("copyclip", json!({ "handles": app.session.doc().unwrap().model.handles().iter().map(|h| h.hex()).collect::<Vec<_>>() })).unwrap();
    let repeat = items(&app).into_iter().next().unwrap();
    assert_eq!((repeat.0.as_str(), repeat.1.as_str()), ("Repeat LINE", "line"));
    assert!(enabled(&app, "pasteclip") && enabled(&app, "pasteblock"));
    assert!(!enabled(&app, "pasteorig"), "Paste to Original Coordinates pastes into another drawing");

    app.run("new", json!({})).unwrap();
    assert!(enabled(&app, "pasteorig"));
    context_menu_selection(&mut app);
}

/// With a selection, the editing commands and Deselect All.
fn context_menu_selection(app: &mut CadApp) {
    assert!(!items(app).iter().any(|(_, id, _)| id == "ai_deselect"));
    app.run("pasteorig", json!({})).unwrap();
    assert_eq!(app.session.selection().len(), 1, "pasted objects are selected");
    let ids: Vec<String> = items(app).into_iter().map(|(_, id, _)| id).collect();
    for id in ["erase", "move", "copy", "ai_deselect", "properties"] {
        assert!(ids.iter().any(|x| x == id), "{id} missing from {ids:?}");
    }
    app.run("ai_deselect", json!({})).unwrap();
    assert!(app.session.selection().is_empty());
}

/// egui delivers Cmd/Ctrl+C, X and V as clipboard events, not key presses: they run the
/// clipboard commands.
#[test]
fn clipboard_events_run_the_clipboard_commands() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h = app.session.doc().unwrap().model.handles();
    app.session.set_selection(h);
    let ctx = egui::Context::default();
    let frame = |app: &mut CadApp, events: Vec<egui::Event>| {
        ctx.begin_pass(egui::RawInput { events, ..Default::default() });
        cadcraft_ui_egui::menus::shortcuts(app, &ctx);
        ctx.end_pass().textures_delta.clear();
    };
    frame(&mut app, vec![egui::Event::Copy]);
    assert_eq!(app.session.clipboard.len(), 1, "Copy with a selection copies it");
    frame(&mut app, vec![egui::Event::Paste("text from another program".into())]);
    assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("pasteclip"));
    assert!(app.session.prompt_text().contains("insertion point"));
}

/// (label, id, enabled) of the object snap menu's items (#393).
fn snap_items(app: &CadApp) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for g in context_menu::snap_entries(app) {
        for e in g {
            match e {
                Entry::Item { label, id, enabled, .. } => out.push((label, id, enabled)),
                Entry::Sub { label, children } => {
                    for c in children {
                        if let Entry::Item { label: l, id, enabled, .. } = c {
                            out.push((format!("{label}/{l}"), id, enabled));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Shift/Ctrl+right-click: the object snap menu types the same overrides as the command line.
#[test]
fn snap_menu_types_point_modifiers_and_snap_overrides() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let items = snap_items(&app);
    let labels: Vec<&str> = items.iter().map(|(l, _, _)| l.as_str()).collect();
    for l in [
        "Temporary track point",
        "From",
        "Mid Between 2 Points",
        "Point Filters/.X",
        "Point Filters/.YZ",
        "Endpoint",
        "Midpoint",
        "Center",
        "Nearest",
        "None",
        "Osnap Settings...",
    ] {
        assert!(labels.contains(&l), "{l} missing from {labels:?}");
    }
    // No point prompt: only the settings item is available.
    assert!(items.iter().all(|(l, _, on)| *on == (l == "Osnap Settings...")), "{items:?}");

    app.cmdline("LINE 0,0 10,0");
    app.cmdline("");
    app.cmdline("LINE");
    assert!(snap_items(&app).iter().all(|(_, _, on)| *on), "everything at a point prompt");
    context_menu::choose_snap(&mut app, "_endp");
    assert_eq!(app.session.snap_override(), Some(cadcraft_engine::snap::mode::END));
    assert!(app.session.prompt_text().ends_with("Endpoint of:"), "{}", app.session.prompt_text());
    app.session.input(cadcraft_engine::Input::Point(cadcraft_geom::Vec2::new(9.98, 0.02))).unwrap();
    assert_eq!(app.session.last_point, cadcraft_geom::Vec2::new(10.0, 0.0));
    context_menu::choose_snap(&mut app, "_non");
    assert_eq!(app.session.snap_override(), Some(0));
    context_menu::choose_snap(&mut app, "_from");
    assert!(app.session.prompt_text().ends_with("Base point:"), "{}", app.session.prompt_text());
    app.cmdline("");
    context_menu::choose_snap(&mut app, "_tt");
    assert!(app.session.prompt_text().ends_with("Specify temporary OTRACK point:"), "{}", app.session.prompt_text());
}

/// The texts painted in one frame and where each was painted, so a test can click one.
fn painted_texts(app: &mut CadApp, ctx: &egui::Context, mut events: Vec<egui::Event>, modifiers: egui::Modifiers) -> Vec<(String, egui::Rect)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(s) => out.push((s.galley.text().to_string(), s.visual_bounding_rect())),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    let input =
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let mut texts = Vec::new();
    for s in &output.shapes {
        collect(&s.shape, &mut texts);
    }
    texts
}

/// The text painted in one frame, one painted text per line.
fn painted(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>, modifiers: egui::Modifiers) -> String {
    painted_texts(app, ctx, events, modifiers).into_iter().map(|(text, _)| text + "\n").collect()
}

/// Shift+right-click on the canvas during a command opens the snap menu instead of acting as Enter.
#[test]
fn shift_right_click_opens_the_snap_menu() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx, Vec::new(), egui::Modifiers::NONE);
    app.cmdline("LINE 0,0");
    let at = egui::pos2(800.0, 500.0);
    let shift = egui::Modifiers::SHIFT;
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: shift };
    let before = painted(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], shift);
    assert!(!before.contains("Mid Between 2 Points"));
    painted(&mut app, &ctx, vec![button(true)], shift);
    painted(&mut app, &ctx, vec![button(false)], shift);
    let text = painted(&mut app, &ctx, Vec::new(), egui::Modifiers::NONE);
    for l in ["From", "Mid Between 2 Points", "Endpoint", "Perpendicular", "Osnap Settings..."] {
        assert!(text.contains(l), "{l} missing from the painted menu:\n{text}");
    }
    assert!(app.session.prompt_text().contains("Specify next point"), "no Enter: {}", app.session.prompt_text());
}

/// MBUTTONPAN 0 (#2720): a middle click opens the snap menu, and dragging with the middle button
/// no longer pans. Choosing an item from that menu sets the snap override for the next point.
/// With MBUTTONPAN 1 (the default) the same drag pans and no menu opens.
#[test]
fn middle_button_pans_or_opens_the_snap_menu_by_mbuttonpan() {
    let none = egui::Modifiers::NONE;
    let button = |button, at, pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: none };
    let middle = |at, pressed| button(egui::PointerButton::Middle, at, pressed);
    let (at, to) = (egui::pos2(800.0, 500.0), egui::pos2(900.0, 560.0));
    // A middle drag from `at` to `to` during LINE. Returns the app and its context, so the menu
    // can be clicked, the texts painted after the drag, and whether the view's centre moved.
    let drag = |mbuttonpan: &str| {
        let mut app = CadApp::new(Session::new(), Services::default());
        let ctx = egui::Context::default();
        app.logic(&ctx);
        painted(&mut app, &ctx, Vec::new(), none);
        app.cmdline(&format!("MBUTTONPAN {mbuttonpan}"));
        app.cmdline("LINE 0,0");
        let before = app.session.state().unwrap().view().center;
        painted(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], none);
        painted(&mut app, &ctx, vec![middle(at, true)], none);
        painted(&mut app, &ctx, vec![egui::Event::PointerMoved(to)], none);
        painted(&mut app, &ctx, vec![middle(to, false)], none);
        let texts = painted_texts(&mut app, &ctx, Vec::new(), none);
        let moved = app.session.state().unwrap().view().center.dist(before) > 1e-9;
        (app, ctx, texts, moved)
    };
    let has = |texts: &[(String, egui::Rect)], label: &str| texts.iter().any(|(text, _)| text == label);

    let (mut app, ctx, texts, moved) = drag("0");
    for l in ["From", "Mid Between 2 Points", "Endpoint", "Osnap Settings..."] {
        assert!(has(&texts, l), "{l} missing from the painted menu:\n{texts:?}");
    }
    assert!(!moved, "MBUTTONPAN 0: the middle button doesn't pan");
    assert!(app.session.prompt_text().contains("Specify next point"), "the command keeps its prompt: {}", app.session.prompt_text());

    // Clicking Endpoint in the menu sets the override; the click doesn't reach LINE as a point.
    let endpoint = texts.iter().find(|(text, _)| text == "Endpoint").map(|(_, rect)| rect.center()).unwrap();
    painted(&mut app, &ctx, vec![egui::Event::PointerMoved(endpoint)], none);
    painted(&mut app, &ctx, vec![button(egui::PointerButton::Primary, endpoint, true)], none);
    painted(&mut app, &ctx, vec![button(egui::PointerButton::Primary, endpoint, false)], none);
    let after = painted_texts(&mut app, &ctx, Vec::new(), none);
    assert_eq!(app.session.snap_override(), Some(cadcraft_engine::snap::mode::END));
    assert!(app.session.prompt_text().ends_with("Endpoint of:"), "{}", app.session.prompt_text());
    assert!(!has(&after, "Mid Between 2 Points"), "choosing an item closes the menu:\n{after:?}");

    let (_, _, texts, moved) = drag("1");
    assert!(!has(&texts, "Mid Between 2 Points"), "MBUTTONPAN 1 opens no menu:\n{texts:?}");
    assert!(moved, "MBUTTONPAN 1: the middle button pans");
}
