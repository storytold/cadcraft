//! Ctrl+=, Ctrl++, Ctrl+- and Ctrl+0 (Cmd on macOS) change the interface scale of the Options dialog.

use cadcraft_engine::Session;
use cadcraft_ui_egui::docking::Panel;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, Modifiers, accesskit::Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::{Value, json};

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    let mut h = Harness::builder().with_context_setup(cadcraft_ui_egui::framework::install_headless).with_size(vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

/// QuickCalc's expression field.
fn expression<'a>(h: &'a Harness<'_, CadApp>) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(Role::TabPanel, "QuickCalc").get_all_by_role(Role::TextInput).next().unwrap()
}

/// The interface scale after a press of `key`.
fn press(h: &mut Harness<'_, CadApp>, modifiers: Modifiers, key: Key) -> f32 {
    h.key_press_modifiers(modifiers, key);
    h.run_steps(3);
    h.state().options.interface_scale
}

#[test]
fn the_zoom_keys_change_the_interface_scale() {
    for (os, command) in [
        (egui::os::OperatingSystem::Windows, Modifiers { ctrl: true, command: true, ..Modifiers::NONE }),
        (egui::os::OperatingSystem::Mac, Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE }),
    ] {
        let mut h = harness();
        h.ctx.set_os(os);
        h.state_mut().run("ui.panel.quickcalc", Value::Null).unwrap();
        h.run_steps(5);
        assert!(h.state().ui.docking.layout().contains(&Panel::QuickCalc));
        h.state_mut().start("line");
        h.state_mut().cmd.buffer = "12,34".into();

        // With the command line taking the keys.
        assert_eq!(press(&mut h, command, Key::Equals), 1.1);
        assert_eq!(press(&mut h, command | Modifiers::SHIFT, Key::Plus), 1.2);
        assert_eq!(press(&mut h, command, Key::Minus), 1.1);

        // A key the user gave to a command runs the command.
        let ortho = h.state().session.settings.orthomode;
        h.state_mut().run("ui.shortcuts", json!({"set": {"ortho": "Cmd+Minus"}})).unwrap();
        assert_eq!(press(&mut h, command, Key::Minus), 1.1);
        assert_eq!(h.state().session.settings.orthomode, !ortho);
        h.state_mut().run("ui.shortcuts", json!({"reset": true})).unwrap();

        // With a text field taking them.
        expression(&h).click();
        h.run_steps(3);
        expression(&h).type_text("2+3");
        h.run_steps(3);
        assert!(h.ctx.egui_wants_keyboard_input());
        assert_eq!(press(&mut h, command, Key::Equals), 1.2);

        // The window takes the new scale.
        let ctx = h.ctx.clone();
        h.state_mut().before_input(&ctx);
        h.run_steps(2);
        assert_eq!(ctx.zoom_factor(), 1.2);

        // Within the setting's limits, and back to the system's own size.
        h.state_mut().options.interface_scale = 3.0;
        assert_eq!(press(&mut h, command, Key::Equals), 3.0);
        h.state_mut().options.interface_scale = 0.5;
        assert_eq!(press(&mut h, command, Key::Minus), 0.5);
        assert_eq!(press(&mut h, command, Key::Num0), 1.0);

        // Nothing was typed.
        assert!(h.query_all(by().role(Role::TextInput).value("2+3")).next().is_some());
        assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
        assert_eq!(h.state().cmd.buffer, "12,34");
    }
}
