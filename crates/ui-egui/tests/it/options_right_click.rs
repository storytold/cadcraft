//! Options ▸ User Preferences ▸ Right-Click Customization: the time-sensitive right-click's
//! duration, and what a right-click does with nothing selected (Default mode), with a selection
//! (Edit mode) and while a command runs (Command mode), through the OPTIONS JSON form, the
//! drawing area and the dialog.

use cadcraft_engine::Session;
use cadcraft_ui_egui::options::{CommandClick, IdleClick};
use cadcraft_ui_egui::{CadApp, Services, menus};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// One frame at `time` (seconds) with `events`; returns the painted text.
fn frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>, time: f64) -> String {
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
    app.logic(ctx);
    let mut out = ctx.run_ui(input, |ui| app.ui(ui));
    out.textures_delta.clear();
    let mut text = String::new();
    out.shapes.iter().for_each(|s| collect(&s.shape, &mut text));
    text
}

const AT: egui::Pos2 = egui::pos2(800.0, 500.0);

fn button(pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: AT, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE }
}

/// A right-click on the drawing area from `t` held for `held` seconds; returns the text painted
/// just after.
fn right_click(app: &mut CadApp, ctx: &egui::Context, t: f64, held: f64) -> String {
    frame(app, ctx, vec![egui::Event::PointerMoved(AT)], t);
    frame(app, ctx, vec![button(true)], t + 0.01);
    frame(app, ctx, vec![button(false)], t + 0.01 + held);
    frame(app, ctx, Vec::new(), t + 0.05 + held)
}

#[test]
fn right_click_settings_are_checked_and_survive_a_restart() {
    let mut a = app();
    let all = a.run("options", json!({})).unwrap();
    assert_eq!(
        (all["rightClickDuration"].clone(), all["rightClickDefault"].clone(), all["rightClickEdit"].clone(), all["rightClickCommand"].clone()),
        (json!(250), json!("menu"), json!("menu"), json!("menu"))
    );
    let r = a
        .run(
            "options",
            json!({"rightClickDuration": 400, "rightClickDefault": "repeat", "rightClickEdit": "Repeat", "rightClickCommand": "menuWithOptions"}),
        )
        .unwrap();
    assert_eq!((r["rightClickDuration"].clone(), r["rightClickEdit"].clone()), (json!(400), json!("repeat")));
    assert_eq!(a.options.right_click_command, CommandClick::MenuWithOptions);

    let before = a.run("options", json!({})).unwrap();
    for bad in [
        json!({"rightClickDuration": 50}),
        json!({"rightClickDuration": 2001}),
        json!({"rightClickDuration": f64::NAN}),
        json!({"rightClickDuration": "long"}),
        json!({"rightClickDefault": "enter"}),
        json!({"rightClickEdit": 1}),
        json!({"rightClickCommand": "repeat", "PICKBOX": 9}),
    ] {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
        assert_eq!(a.run("options", json!({})).unwrap(), before, "{bad} changed nothing");
    }

    let mut b = app();
    b.load_profile(&a.profile_json());
    assert_eq!(b.options, a.options);
    let mut c = app();
    c.load_profile(r#"{"rightClickDuration": 99999, "rightClickDefault": "fly", "rightClickCommand": null}"#);
    assert_eq!(c.options, app().options, "hostile saved values are skipped");
    let r = a.run("options", json!({"reset": true})).unwrap();
    assert_eq!((r["rightClickDuration"].clone(), r["rightClickDefault"].clone()), (json!(250), json!("menu")));
}

#[test]
fn an_idle_right_click_can_repeat_the_last_command() {
    let mut a = app();
    a.run("options", json!({"rightClickDefault": "repeat"})).unwrap();
    a.cmdline("circle 0,0 5");
    assert!(!a.session.prompting());
    let ctx = egui::Context::default();
    frame(&mut a, &ctx, Vec::new(), 0.0);
    right_click(&mut a, &ctx, 0.1, 0.05);
    assert!(a.session.prompting(), "CIRCLE runs again: {}", a.session.prompt_text());
    a.cmdline("10,10 2");

    // With a selection, Edit mode decides: the menu (its default) opens instead.
    a.run("selectall", json!({})).unwrap();
    assert!(!a.session.selection().is_empty());
    let text = right_click(&mut a, &ctx, 1.0, 0.05);
    assert!(!a.session.prompting(), "no command started: {}", a.session.prompt_text());
    assert!(text.contains("Erase"), "the shortcut menu opens:\n{text}");
}

#[test]
fn command_mode_and_click_duration_decide_enter_or_the_menu() {
    let ctx = egui::Context::default();

    // A longer duration: a 0.5 s click is still Enter.
    let mut a = app();
    a.run("options", json!({"rightClickDuration": 1000})).unwrap();
    frame(&mut a, &ctx, Vec::new(), 0.0);
    a.cmdline("LINE 0,0 10,0");
    right_click(&mut a, &ctx, 0.1, 0.5);
    assert!(a.session.running.is_none(), "a click shorter than the duration is Enter: {}", a.session.prompt_text());

    // Time-sensitive off, Command mode Enter: every right-click is Enter.
    let ctx = egui::Context::default();
    let mut a = app();
    a.run("options", json!({"timeSensitiveRightClick": false, "rightClickCommand": "enter"})).unwrap();
    frame(&mut a, &ctx, Vec::new(), 0.0);
    a.cmdline("LINE 0,0 10,0");
    let text = right_click(&mut a, &ctx, 0.1, 0.6);
    assert!(a.session.running.is_none(), "Enter ends LINE: {}", a.session.prompt_text());
    assert!(!text.contains("Snap Overrides"), "no menu:\n{text}");

    // Command mode "enabled when command options are present": CIRCLE's first prompt has
    // options, so the menu opens and the command keeps running.
    a.run("options", json!({"rightClickCommand": "menuWithOptions"})).unwrap();
    a.cmdline("CIRCLE");
    let text = right_click(&mut a, &ctx, 2.0, 0.05);
    assert!(text.contains("Cancel"), "the command menu opens:\n{text}");
    assert!(a.session.prompting(), "CIRCLE still runs");
}

/// With `CADCRAFT_OPTIONS_SHOTS` set to a folder, the dialog is rendered there for a look.
const SHOTS: &str = "CADCRAFT_OPTIONS_SHOTS";

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let builder = Harness::builder().with_size(egui::vec2(1400.0, 900.0));
    let builder = if std::env::var_os(SHOTS).is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn shot(h: &mut Harness<'static, CadApp>, name: &str) {
    if let Some(dir) = std::env::var_os(SHOTS).map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}

#[test]
fn the_customization_window_sets_every_mode() {
    let mut a = app();
    menus::activate(&mut a, "options");
    a.ui.dialog = Some("options:preferences".into());
    let mut h = harness(a);
    h.get_by_label("Right-Click Customization...").click();
    h.run_steps(3);
    h.get_by_label("Longer click duration:");
    shot(&mut h, "options-right-click.png");

    // Default mode's "Repeat last command" comes first, Edit mode's second.
    if let Some(n) = h.get_all_by_label("Repeat last command").next() {
        n.click();
    }
    h.run_steps(3);
    assert_eq!(h.state().options.right_click_default, IdleClick::Repeat);
    assert_eq!(h.state().options.right_click_edit, IdleClick::Menu);

    // Command mode applies once the time-sensitive right-click is off.
    h.get_by_label("Turn on time-sensitive right-click").click();
    h.run_steps(3);
    assert!(!h.state().options.time_sensitive_right_click);
    h.get_by_label("Shortcut Menu: enabled when command options are present").click();
    h.run_steps(3);
    assert_eq!(h.state().options.right_click_command, CommandClick::MenuWithOptions);

    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed controls: {unnamed:?}");
    // The window's Close comes after the Options dialog's in the tree.
    if let Some(n) = h.get_all_by_label("Close").last() {
        n.click();
    }
    h.run_steps(3);
    assert!(h.query_by_label("Longer click duration:").is_none());
    assert_eq!(h.state().ui.dialog.as_deref(), Some("options:preferences"));
}
