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
    assert!(enabled(&app, "pasteclip") && enabled(&app, "pasteblock"), "the system clipboard may hold objects from another window");

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

/// The text painted in one frame.
fn painted(app: &mut CadApp, ctx: &egui::Context, mut events: Vec<egui::Event>, modifiers: egui::Modifiers) -> String {
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
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    let input =
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let mut text = String::new();
    for s in &output.shapes {
        collect(&s.shape, &mut text);
    }
    text
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

/// A quick right-click on the canvas at the insertion point of an INSERT the Blocks palette started
/// cancels it; the same click during a typed INSERT is Enter, which asks the point again.
#[test]
fn quick_right_click_cancels_a_palette_placement_only() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx, Vec::new(), egui::Modifiers::NONE);
    app.session.execute("line", &json!({ "points": [[0, 0], [4, 0]] })).unwrap();
    let handles: Vec<String> = app.session.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    app.session.execute("block", &json!({ "name": "BOLT", "base": [0, 0], "handles": handles, "keep": "delete" })).unwrap();
    let at = egui::pos2(800.0, 500.0);
    let click = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE };
    let right_click = |app: &mut CadApp| {
        painted(app, &ctx, vec![egui::Event::PointerMoved(at)], egui::Modifiers::NONE);
        painted(app, &ctx, vec![click(true), click(false)], egui::Modifiers::NONE);
        painted(app, &ctx, Vec::new(), egui::Modifiers::NONE);
    };

    app.cmdline("INSERT");
    app.cmdline("BOLT");
    assert!(app.session.prompt_text().contains("Specify insertion point"), "{}", app.session.prompt_text());
    right_click(&mut app);
    assert!(app.session.prompt_text().contains("Specify insertion point"), "a typed INSERT asks again: {}", app.session.prompt_text());
    app.session.cancel();

    app.cmdline("INSERT");
    app.cmdline("BOLT");
    app.canvas.palette_insert = Some(app.session.commands_started);
    app.cmd.buffer = "12,".into();
    right_click(&mut app);
    assert!(!app.session.prompting(), "the palette's placement ends: {}", app.session.prompt_text());
    assert!(app.cmd.buffer.is_empty(), "the half-typed point goes too: {}", app.cmd.buffer);
}

/// Edit > Paste, the context menu and a typed PASTECLIP (#3320) read the system clipboard first:
/// they are offered with nothing copied here, ask the window for its text, and paste what this
/// window holds when none arrives.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn menu_and_typed_paste_ask_the_window_for_the_system_clipboard() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let spec = cadcraft_engine::find_command("pasteclip").unwrap();
    assert!(app.session.clipboard.is_empty());
    assert!(cadcraft_ui_egui::clipboard::offered(&app.session, spec), "offered while the system clipboard may hold objects");
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h = app.session.doc().unwrap().model.handles();
    app.session.set_selection(h);
    app.run("copyclip", json!({})).unwrap();
    let ctx = egui::Context::default();
    let mut now = 0.0;
    let mut frame = |app: &mut CadApp, events: Vec<egui::Event>, dt: f64| {
        now += dt;
        ctx.begin_pass(egui::RawInput { events, time: Some(now), ..Default::default() });
        cadcraft_ui_egui::menus::shortcuts(app, &ctx);
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        out.viewport_output.into_values().flat_map(|v| v.commands).any(|c| c == egui::ViewportCommand::RequestPaste)
    };
    let running = |app: &CadApp| app.session.running.as_ref().map(|r| r.id.clone());

    app.start("pasteclip");
    assert_eq!(running(&app), None, "held until the system clipboard has been read");
    assert!(frame(&mut app, vec![], 0.01), "the window is asked for the clipboard text");
    assert_eq!(running(&app), None);
    assert!(!frame(&mut app, vec![egui::Event::Paste("text from another program".into())], 0.01));
    assert_eq!(running(&app).as_deref(), Some("pasteclip"), "the text arrived: paste runs");
    app.session.cancel();

    assert_eq!(app.cmdline("pasteblock"), None);
    assert_eq!(running(&app), None);
    assert!(frame(&mut app, vec![], 0.01));
    assert!(!frame(&mut app, vec![], 0.1));
    assert_eq!(running(&app), None, "still waiting for the window");
    frame(&mut app, vec![], 1.0);
    assert_eq!(running(&app).as_deref(), Some("pasteblock"), "no text came: this window's clipboard is pasted");
}

/// A held paste is dropped by Escape, completes on a `Paste` that arrives on the frame its wait
/// runs out, and a `Paste` answering a request already given up on does not paste again, while a
/// later Ctrl+V does.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_held_paste_is_cancelled_by_escape_and_runs_once() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h = app.session.doc().unwrap().model.handles();
    app.session.set_selection(h);
    app.run("copyclip", json!({})).unwrap();
    let ctx = egui::Context::default();
    let mut now = 0.0;
    let mut frame = |app: &mut CadApp, events: Vec<egui::Event>, dt: f64| {
        now += dt;
        ctx.begin_pass(egui::RawInput { events, time: Some(now), ..Default::default() });
        cadcraft_ui_egui::menus::shortcuts(app, &ctx);
        ctx.end_pass().textures_delta.clear();
    };
    let running = |app: &CadApp| app.session.running.as_ref().map(|r| r.id.clone());
    let paste = || vec![egui::Event::Paste("text from another program".into())];

    // Escape while held: the answer and the timeout paste nothing.
    app.start("pasteclip");
    frame(&mut app, vec![], 0.01);
    cadcraft_ui_egui::cmdline::escape(&mut app);
    frame(&mut app, paste(), 0.01);
    assert_eq!(running(&app), None, "an escaped paste does not run when the text arrives");
    app.start("pasteclip");
    frame(&mut app, vec![], 5.0);
    cadcraft_ui_egui::cmdline::escape(&mut app);
    frame(&mut app, vec![], 5.0);
    assert_eq!(running(&app), None, "an escaped paste does not run at the timeout");
    app.start("pasteclip");
    cadcraft_ui_egui::cmdline::escape(&mut app);
    frame(&mut app, vec![], 5.0);
    frame(&mut app, vec![], 5.0);
    assert_eq!(running(&app), None, "an escaped paste is dropped before it asks");

    // The text arrives on the frame the wait runs out: that completes the paste, once.
    app.start("pasteclip");
    frame(&mut app, vec![], 5.0);
    frame(&mut app, paste(), 0.6);
    assert_eq!(running(&app).as_deref(), Some("pasteclip"));
    app.session.cancel();
    frame(&mut app, vec![], 0.6);
    assert_eq!(running(&app), None, "one request pastes once");

    // The wait runs out first: this window's clipboard is pasted, and the late text is not a second paste.
    frame(&mut app, vec![], 2.0);
    app.start("pasteclip");
    frame(&mut app, vec![], 0.01);
    frame(&mut app, vec![], 0.6);
    assert_eq!(running(&app).as_deref(), Some("pasteclip"));
    app.session.cancel();
    frame(&mut app, paste(), 0.1);
    assert_eq!(running(&app), None, "the late answer does not paste again");

    // An ordinary Ctrl+V after that still pastes.
    frame(&mut app, paste(), 2.0);
    assert_eq!(running(&app).as_deref(), Some("pasteclip"));
    app.session.cancel();

    // A Ctrl+V that comes before the late answer takes its place: the two together paste once.
    app.start("pasteclip");
    frame(&mut app, vec![], 0.01);
    frame(&mut app, vec![], 0.6);
    app.session.cancel();
    frame(&mut app, paste(), 0.1);
    assert_eq!(running(&app), None, "the Ctrl+V is the answer");
    frame(&mut app, paste(), 0.1);
    assert_eq!(running(&app).as_deref(), Some("pasteclip"), "the answer is then the paste");
    app.session.cancel();

    // A slow answer (Wayland reads asynchronously) is still that request's, not a second paste.
    app.start("pasteclip");
    frame(&mut app, vec![], 0.01);
    frame(&mut app, vec![], 0.6);
    app.session.cancel();
    frame(&mut app, paste(), 2.0);
    assert_eq!(running(&app), None, "a late answer pastes nothing");

    // The window answers within its delivery budget or never: after that a `Paste` is a Ctrl+V.
    app.start("pasteclip");
    frame(&mut app, vec![], 0.01);
    frame(&mut app, vec![], 0.6);
    app.session.cancel();
    frame(&mut app, paste(), 10.0);
    assert_eq!(running(&app).as_deref(), Some("pasteclip"));
    app.session.cancel();

    // Typing another command, even one of the interface's own, drops a held paste.
    app.start("pasteclip");
    frame(&mut app, vec![], 0.01);
    app.cmdline("quickcalc");
    frame(&mut app, paste(), 0.1);
    assert_eq!(running(&app), None);
    app.start("pasteclip");
    app.cmdline("quickcalc");
    frame(&mut app, vec![], 5.0);
    frame(&mut app, vec![], 5.0);
    assert_eq!(running(&app), None);

    // A stale answer, to a request whose command was escaped, does not complete a newer held
    // command: that one keeps waiting and runs on its own answer, or at its own deadline.
    app.start("pasteclip");
    frame(&mut app, vec![], 0.01);
    cadcraft_ui_egui::cmdline::escape(&mut app);
    app.start("pasteblock");
    frame(&mut app, vec![], 0.01);
    frame(&mut app, paste(), 0.01);
    assert_eq!(running(&app), None, "the older request's answer is not the newer command's");
    frame(&mut app, paste(), 0.01);
    assert_eq!(running(&app).as_deref(), Some("pasteblock"), "its own answer runs it");
    app.session.cancel();
    app.start("pasteclip");
    frame(&mut app, vec![], 0.01);
    cadcraft_ui_egui::cmdline::escape(&mut app);
    app.start("pasteblock");
    frame(&mut app, vec![], 0.01);
    frame(&mut app, paste(), 0.01);
    assert_eq!(running(&app), None);
    frame(&mut app, vec![], 1.0);
    assert_eq!(running(&app).as_deref(), Some("pasteblock"), "its own deadline runs it");
}

/// Where the window answers in the next frame or not at all, a request stops being answerable
/// after that frame: the paste falls back at once and a later `Paste` is an ordinary Ctrl+V.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn without_late_answers_a_paste_after_the_next_frame_is_ordinary() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.set_clipboard_late_answers(false);
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h = app.session.doc().unwrap().model.handles();
    app.session.set_selection(h);
    app.run("copyclip", json!({})).unwrap();
    let ctx = egui::Context::default();
    let mut now = 0.0;
    let mut frame = |app: &mut CadApp, events: Vec<egui::Event>| {
        now += 0.01;
        ctx.begin_pass(egui::RawInput { events, time: Some(now), ..Default::default() });
        cadcraft_ui_egui::menus::shortcuts(app, &ctx);
        ctx.end_pass().textures_delta.clear();
    };
    let running = |app: &CadApp| app.session.running.as_ref().map(|r| r.id.clone());
    let paste = || vec![egui::Event::Paste("text from another program".into())];

    app.start("pasteclip");
    frame(&mut app, vec![]);
    assert_eq!(running(&app), None, "the answer may still come in the next frame");
    frame(&mut app, vec![]);
    assert_eq!(running(&app), None);
    frame(&mut app, vec![]);
    assert_eq!(running(&app).as_deref(), Some("pasteclip"), "no answer: pasted without waiting for the deadline");
    app.session.cancel();
    frame(&mut app, paste());
    assert_eq!(running(&app).as_deref(), Some("pasteclip"), "a Ctrl+V after that is not an answer");
    app.session.cancel();

    // An answer in the next frame completes the paste, once.
    app.start("pasteclip");
    frame(&mut app, vec![]);
    frame(&mut app, paste());
    assert_eq!(running(&app).as_deref(), Some("pasteclip"));
    app.session.cancel();
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    assert_eq!(running(&app), None);

    // A Ctrl+V in the frame the request goes out is not its answer.
    app.start("pasteblock");
    frame(&mut app, paste());
    assert_eq!(running(&app).as_deref(), Some("pasteclip"), "the Ctrl+V pastes");
}

/// A paste command the user bound to a key of its own reads the system clipboard first, like a
/// menu paste: the key press is not a `Paste` event, so nothing has been read when it fires.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_paste_bound_to_another_key_asks_the_window_for_the_system_clipboard() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h = app.session.doc().unwrap().model.handles();
    app.session.set_selection(h);
    app.run("copyclip", json!({})).unwrap();
    app.ui.shortcuts.insert("pasteclip".into(), "F9".into());
    let ctx = egui::Context::default();
    let mut now = 0.0;
    let mut frame = |app: &mut CadApp, events: Vec<egui::Event>| {
        now += 0.01;
        ctx.begin_pass(egui::RawInput { events, time: Some(now), ..Default::default() });
        cadcraft_ui_egui::menus::shortcuts(app, &ctx);
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        out.viewport_output.into_values().flat_map(|v| v.commands).any(|c| c == egui::ViewportCommand::RequestPaste)
    };
    let running = |app: &CadApp| app.session.running.as_ref().map(|r| r.id.clone());
    let f9 = egui::Event::Key { key: egui::Key::F9, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };

    let asked = frame(&mut app, vec![f9]);
    assert_eq!(running(&app), None, "held until the system clipboard has been read");
    assert!(asked || frame(&mut app, vec![]), "the window is asked for the clipboard text");
    frame(&mut app, vec![egui::Event::Paste("text from another program".into())]);
    assert_eq!(running(&app).as_deref(), Some("pasteclip"), "the text arrived: paste runs");
}
