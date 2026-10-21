//! Window buttons where the desktop leaves the window frame to the app (GNOME on Wayland): the
//! title row carries them in the desktop's order until Options ▸ Display ▸ Use system title bar
//! gives the window back winit's frame, at once and without a restart. Ported from WordCraft
//! (storytold/wordcraft#3361).

use cadcraft_engine::Session;
use cadcraft_ui_egui::window_controls::{Button, Layout};
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(window_controls: Option<Layout>) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.window_controls = window_controls;
    let mut harness = Harness::builder().with_size(vec2(1280.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness
}

/// The `Decorations(on)` command the last frame sent, if any.
fn decorations_asked(h: &Harness<'static, CadApp>) -> Option<bool> {
    h.output().viewport_output.values().flat_map(|v| v.commands.iter()).find_map(|c| match c {
        egui::ViewportCommand::Decorations(on) => Some(*on),
        _ => None,
    })
}

/// Steps a few frames and reports the first `Decorations` command among them.
fn step_for_decorations(h: &mut Harness<'static, CadApp>) -> Option<bool> {
    let mut asked = None;
    for _ in 0..4 {
        h.step();
        asked = asked.or(decorations_asked(h));
    }
    asked
}

fn buttons(h: &Harness<'static, CadApp>, label: &str) -> usize {
    h.query_all_by_role_and_label(Role::Button, label).count()
}

#[test]
fn use_system_title_bar_swaps_the_window_buttons_for_the_frame() {
    let plain = harness(None);
    let base = |label| buttons(&plain, label);
    let mut h = harness(Some(Layout::default()));
    // The window opened without winit's frame, so the first frames ask for nothing.
    assert_eq!(step_for_decorations(&mut h), None);
    for label in ["Minimize", "Maximize", "Close"] {
        assert_eq!(buttons(&h, label), base(label) + 1, "the title row has {label}");
    }
    if let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(format!("{directory}/cad-window-buttons.png")).unwrap();
    }

    h.state_mut().ui.dialog = Some("options:display".into());
    h.run_steps(3);
    h.get_by_role_and_label(Role::CheckBox, "Use system title bar").click();
    assert_eq!(step_for_decorations(&mut h), Some(true), "the window gets its frame back");
    assert!(h.state().ui.system_title_bar);
    assert_eq!(buttons(&h, "Minimize"), base("Minimize"), "and loses the app's buttons");

    let r = h.state_mut().run("options", json!({})).unwrap();
    assert_eq!(r["systemTitleBar"], true, "OPTIONS reports it");
    assert!(h.state_mut().run("options", json!({"systemTitleBar": "no"})).is_err());
    assert!(h.state().ui.system_title_bar, "a bad value changes nothing");
    h.state_mut().run("options", json!({"systemTitleBar": false})).unwrap();
    assert_eq!(step_for_decorations(&mut h), Some(false), "the frame goes again");
    assert_eq!(buttons(&h, "Minimize"), base("Minimize") + 1);
}

/// GNOME's `button-layout` decides which buttons appear: Fedora's default shows only close.
#[test]
fn the_desktop_button_layout_decides_the_window_buttons() {
    let plain = harness(None);
    let h = harness(Some(Layout { left: vec![], right: vec![Button::Close] }));
    assert_eq!(buttons(&h, "Close"), buttons(&plain, "Close") + 1);
    assert_eq!(buttons(&h, "Minimize"), buttons(&plain, "Minimize"));
    assert_eq!(buttons(&h, "Maximize"), buttons(&plain, "Maximize"));
}

/// Where the desktop draws the frame there are no window buttons and nothing to choose.
#[test]
fn desktops_that_draw_the_frame_are_left_alone() {
    let mut h = harness(None);
    h.state_mut().ui.dialog = Some("options:display".into());
    h.run_steps(3);
    assert!(h.query_by_label("Use system title bar").is_none());
    h.state_mut().run("options", json!({"systemTitleBar": true})).unwrap();
    assert_eq!(step_for_decorations(&mut h), None);
}
