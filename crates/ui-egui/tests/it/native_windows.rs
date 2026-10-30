//! Floating panels in their own OS windows: a modal dialog blocks their input, and keys typed in
//! one run the main window's shortcuts and command line.

use cadcraft_engine::Session;
use cadcraft_engine::autosave::Recoverable;
use cadcraft_ui_egui::closing::Choice;
use cadcraft_ui_egui::docking::native_input_blocked;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers};
use serde_json::json;

fn frame(app: &mut CadApp, ctx: &egui::Context) {
    frame_with(app, ctx, Vec::new());
}

/// A frame whose main-window input carries `events`.
fn frame_with(app: &mut CadApp, ctx: &egui::Context, events: Vec<Event>) {
    let mut raw =
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), events, ..Default::default() };
    app.raw_input_hook(ctx, &mut raw);
    cadcraft_ui_egui::cmdline::capture_tab(ctx, &mut raw);
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
}

fn pump(app: &mut CadApp, ctx: &egui::Context) {
    for _ in 0..3 {
        frame(app, ctx);
    }
}

fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

fn running(app: &CadApp) -> Option<&str> {
    app.session.running.as_ref().map(|command| command.id.as_str())
}

#[test]
fn modal_dialogs_block_native_panel_windows() {
    let ctx = egui::Context::default();
    let mut app = CadApp::new(Session::new(), Services::default());
    pump(&mut app, &ctx);
    assert!(!native_input_blocked(&app, &ctx));

    // "Save changes?" while a dirty drawing closes.
    app.run("circle", json!({"center": [0, 0], "radius": 1})).unwrap();
    app.request_close(None);
    assert!(native_input_blocked(&app, &ctx), "close prompt");
    pump(&mut app, &ctx);
    assert!(native_input_blocked(&app, &ctx), "close prompt shown");
    app.answer_close(Choice::Cancel);
    pump(&mut app, &ctx);
    assert!(!native_input_blocked(&app, &ctx), "after Cancel");

    // Drawing Recovery.
    app.recovery.items.push(Recoverable { path: "missing.sv$".into(), drawing: "Plan.dxf".into(), modified: 0, bytes: 0 });
    assert!(native_input_blocked(&app, &ctx), "drawing recovery");
    app.recovery.items.clear();
    pump(&mut app, &ctx);
    assert!(!native_input_blocked(&app, &ctx));

    // Any other egui modal, from the frame after it is shown.
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        egui::Modal::new(egui::Id::unique("test_modal")).show(ui.ctx(), |ui| ui.label("modal"));
    });
    out.textures_delta.clear();
    assert!(native_input_blocked(&app, &ctx), "egui modal");
}

#[test]
fn keys_forwarded_from_a_native_window_reach_the_command_line_and_shortcuts() {
    let ctx = egui::Context::default();
    let mut app = CadApp::new(Session::new(), Services::default());
    pump(&mut app, &ctx);

    let typed = vec![Event::Text("LINE".into()), key(Key::Enter, true, Modifiers::NONE), key(Key::Enter, false, Modifiers::NONE)];
    app.forward_native_input(&ctx, typed, Some(Modifiers::NONE));
    pump(&mut app, &ctx);
    assert_eq!(running(&app), Some("line"));
    assert_eq!(app.cmd.history, ["LINE"]);

    let escape = vec![key(Key::Escape, true, Modifiers::NONE), key(Key::Escape, false, Modifiers::NONE)];
    app.forward_native_input(&ctx, escape, Some(Modifiers::NONE));
    pump(&mut app, &ctx);
    assert_eq!(running(&app), None);

    let ortho = app.session.settings.orthomode;
    app.forward_native_input(&ctx, vec![key(Key::F8, true, Modifiers::NONE), key(Key::F8, false, Modifiers::NONE)], Some(Modifiers::NONE));
    pump(&mut app, &ctx);
    assert_eq!(app.session.settings.orthomode, !ortho, "F8");

    // Shift held in the native window: Cmd/Ctrl+Shift+C arrives as Copy and runs COPYBASE. The
    // main window's own modifiers come back afterwards.
    app.forward_native_input(&ctx, vec![Event::Copy], Some(Modifiers::SHIFT | Modifiers::COMMAND));
    pump(&mut app, &ctx);
    assert_eq!(running(&app), Some("copybase"));
    assert_eq!(ctx.input(|i| i.modifiers), Modifiers::NONE);
}

#[test]
fn a_main_window_modifier_change_outlasts_keys_forwarded_meanwhile() {
    let ctx = egui::Context::default();
    let mut app = CadApp::new(Session::new(), Services::default());
    pump(&mut app, &ctx);
    frame_with(&mut app, &ctx, vec![Event::ModifiersChanged(Modifiers::SHIFT)]);
    app.forward_native_input(&ctx, vec![key(Key::F8, true, Modifiers::COMMAND), key(Key::F8, false, Modifiers::COMMAND)], Some(Modifiers::COMMAND));
    // Shift is released in the main window in the frame the forwarded keys go in.
    frame_with(&mut app, &ctx, vec![Event::ModifiersChanged(Modifiers::NONE)]);
    pump(&mut app, &ctx);
    assert_eq!(ctx.input(|i| i.modifiers), Modifiers::NONE, "the released Shift is not put back");
}
