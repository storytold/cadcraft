//! The canvas's shortcut menus (#512): the command menu while a command runs (Enter, Cancel,
//! Recent Input, the prompt's options, Snap Overrides, Pan/Zoom), the selection menu with the
//! selected object type's edit commands, Recent Commands, and time-sensitive right-click.

use cadcraft_engine::Session;
use cadcraft_ui_egui::menus::Entry;
use cadcraft_ui_egui::{CadApp, Services, cmdline, context_menu};
use serde_json::json;

/// (label, id, enabled) of the items, with submenu items prefixed by "Sub/".
fn flatten(groups: &[Vec<Entry>]) -> Vec<(String, String, bool)> {
    fn walk(prefix: &str, es: &[Entry], out: &mut Vec<(String, String, bool)>) {
        for e in es {
            match e {
                Entry::Item { label, id, enabled, .. } => out.push((format!("{prefix}{label}"), id.clone(), *enabled)),
                Entry::Sub { label, children } => walk(&format!("{prefix}{label}/"), children, out),
            }
        }
    }
    let mut out = Vec::new();
    for g in groups {
        walk("", g, &mut out);
    }
    out
}

fn labels(groups: &[Vec<Entry>]) -> Vec<String> {
    flatten(groups).into_iter().map(|(l, _, _)| l).collect()
}

fn ids(groups: &[Vec<Entry>]) -> Vec<String> {
    flatten(groups).into_iter().map(|(_, id, _)| id).collect()
}

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// Select everything in model space.
fn select_all(app: &mut CadApp) {
    let all = app.session.doc().unwrap().model.handles();
    app.session.set_selection(all);
}

fn type_line(app: &mut CadApp, text: &str) {
    app.cmd.buffer = text.into();
    cmdline::submit(app);
}

#[test]
fn command_menu_offers_enter_cancel_options_snaps_and_views() {
    let mut app = app();
    // Typed as is (Dynamic Input would make the second point relative).
    app.session.settings.dynmode = false;
    assert!(context_menu::command_entries(&app).is_none(), "no command, no command menu");
    type_line(&mut app, "LINE");
    type_line(&mut app, "0,0");
    type_line(&mut app, "10,0");
    let groups = context_menu::command_entries(&app).unwrap();
    let l = labels(&groups);
    for want in
        ["Enter", "Cancel", "Recent Input/10,0", "Recent Input/0,0", "Undo", "Snap Overrides/Endpoint", "Snap Overrides/From", "Zoom/Zoom Extents"]
    {
        assert!(l.iter().any(|x| x == want), "{want} missing from {l:?}");
    }
    let recent: Vec<_> = l.iter().filter(|x| x.starts_with("Recent Input/")).collect();
    assert_eq!(recent, ["Recent Input/10,0", "Recent Input/0,0"], "newest first");
    assert!(!l.iter().any(|x| x == "Recent Input/LINE"), "command names aren't input");

    // The prompt's option is the same input as typing it: LINE's Undo removes the segment.
    context_menu::choose(&mut app, "keyword:Undo");
    assert!(app.session.prompt_text().contains("next point"), "{}", app.session.prompt_text());
    // Recent Input types the value again: the segment comes back.
    context_menu::choose(&mut app, "type:10,0");
    // Zoom is transparent: the command keeps running.
    context_menu::choose(&mut app, "type:'zoom.extents");
    assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    // Enter ends LINE with its one segment.
    context_menu::choose(&mut app, context_menu::ENTER);
    assert!(app.session.running.is_none());
    assert_eq!(app.session.doc().unwrap().model.handles().len(), 1);

    // A snap override is typed at the point prompt; Cancel is Escape.
    type_line(&mut app, "CIRCLE");
    context_menu::choose(&mut app, "snap:_endp");
    assert_eq!(app.session.snap_override(), Some(cadcraft_engine::snap::mode::END));
    context_menu::choose(&mut app, context_menu::CANCEL);
    assert!(app.session.running.is_none());
}

#[test]
fn snap_overrides_only_at_point_prompts() {
    let mut app = app();
    app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    type_line(&mut app, "ERASE");
    let l = labels(&context_menu::command_entries(&app).unwrap());
    assert!(l.iter().any(|x| x == "Enter"));
    assert!(!l.iter().any(|x| x.starts_with("Snap Overrides/")), "Select objects isn't a point prompt: {l:?}");
}

#[test]
fn idle_menu_lists_recent_commands_and_selection_commands() {
    let mut app = app();
    let l = labels(&context_menu::entries(&app));
    assert!(!l.iter().any(|x| x.starts_with("Recent Commands/")), "nothing ran yet");
    assert!(!ids(&context_menu::entries(&app)).iter().any(|i| i == "erase"), "no selection");
    app.cmdline("LINE 0,0 10,0");
    app.cmdline("");
    app.cmdline("CIRCLE 5,5 2");
    let g = context_menu::entries(&app);
    let l = labels(&g);
    let recent: Vec<_> = l.iter().filter(|x| x.starts_with("Recent Commands/")).collect();
    assert_eq!(recent, ["Recent Commands/CIRCLE", "Recent Commands/LINE"]);
    assert_eq!(l.first().map(String::as_str), Some("Repeat CIRCLE"));

    select_all(&mut app);
    let g = context_menu::entries(&app);
    let ids = ids(&g);
    for id in ["erase", "move", "copy", "scale", "rotate", "draworder.front", "draworder.back", "group", "ai_deselect", "properties", "qselect"] {
        assert!(ids.iter().any(|x| x == id), "{id} missing from {ids:?}");
    }
    // Items for commands that don't exist are left out, not stubbed.
    for id in ids.iter().filter(|i| !i.contains(':')) {
        assert!(cadcraft_engine::find_command(id).is_some() || cadcraft_ui_egui::menus::is_ui_command(id), "{id} isn't a command");
    }
    // A line and a circle: no object-specific commands for a mixed selection.
    assert!(context_menu::object_entries(&app).is_empty());
    // Choosing an item starts the command, acting on the selection.
    context_menu::choose(&mut app, "erase");
    assert!(app.session.doc().unwrap().model.handles().is_empty());
}

#[test]
fn selection_menu_has_the_object_types_edit_commands() {
    let cases = [
        ("text", json!({ "at": [0, 0], "text": "hi" }), "textedit"),
        ("pline", json!({ "vertices": [[0, 0], [10, 0], [10, 5]] }), "pedit"),
        ("spline", json!({ "fit": [[0, 0], [5, 5], [10, 0]] }), "splinedit"),
    ];
    for (cmd, params, edit) in cases {
        let mut app = app();
        app.run(cmd, params).unwrap();
        select_all(&mut app);
        let g = vec![context_menu::object_entries(&app)];
        assert!(ids(&g).iter().any(|i| i == edit), "{cmd}: {edit} missing from {:?}", labels(&g));
        assert!(ids(&context_menu::entries(&app)).iter().any(|i| i == edit));
    }
    // Polyline Edit, not the command's bare "Polyline" label.
    let mut app = app();
    app.run("pline", json!({ "vertices": [[0, 0], [10, 0]] })).unwrap();
    select_all(&mut app);
    assert_eq!(labels(&[context_menu::object_entries(&app)]), ["Polyline Edit"]);
}

#[test]
fn a_selected_block_reference_offers_the_block_editor() {
    let mut app = app();
    let l = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone();
    app.run("block", json!({ "name": "Door", "base": [0, 0], "handles": [l] })).unwrap();
    select_all(&mut app);
    let g = vec![context_menu::object_entries(&app)];
    assert_eq!(labels(&g), ["Block Editor", "Edit Reference In-Place"]);
    // Choosing it opens the Block Editor on that block; its own menu then saves and closes it.
    cadcraft_ui_egui::menus::activate(&mut app, "bedit");
    assert_eq!(app.session.state().unwrap().block_editor(), Some("Door"));
    let menu = ids(&context_menu::entries(&app));
    assert!(["bsave", "bsaveas", "bclose"].iter().all(|id| menu.iter().any(|i| i == id)), "{menu:?}");
    cadcraft_ui_egui::menus::activate(&mut app, "bclose");
    assert_eq!(app.session.state().unwrap().block_editor(), None);
}

/// The text painted in one frame at `time` (seconds).
fn painted(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>, time: f64) -> String {
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

/// A quick right-click during a command is Enter; holding the button opens the command menu.
#[test]
fn time_sensitive_right_click() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx, Vec::new(), 0.0);
    let at = egui::pos2(800.0, 500.0);
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE };
    painted(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], 0.1);

    // Held: the menu opens and the command keeps running.
    app.cmdline("LINE 0,0 10,0");
    painted(&mut app, &ctx, vec![button(true)], 1.0);
    painted(&mut app, &ctx, Vec::new(), 1.2);
    painted(&mut app, &ctx, vec![button(false)], 1.0 + context_menu::HOLD_SECS + 0.1);
    let text = painted(&mut app, &ctx, Vec::new(), 1.5);
    for l in ["Enter", "Cancel", "Undo", "Snap Overrides", "Zoom"] {
        assert!(text.contains(l), "{l} missing from the painted menu:\n{text}");
    }
    assert!(app.session.prompt_text().contains("next point"), "no Enter: {}", app.session.prompt_text());
    // A quick click elsewhere closes the menu and is Enter.
    let away = egui::pos2(400.0, 300.0);
    let quick = |pressed| egui::Event::PointerButton { pos: away, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE };
    painted(&mut app, &ctx, vec![egui::Event::PointerMoved(away)], 2.0);
    painted(&mut app, &ctx, vec![quick(true)], 3.0);
    painted(&mut app, &ctx, vec![quick(false)], 3.05);
    painted(&mut app, &ctx, Vec::new(), 3.1);
    assert!(app.session.running.is_none(), "a quick right-click is Enter: {}", app.session.prompt_text());
    assert_eq!(app.session.doc().unwrap().model.handles().len(), 1);
}
