//! QuickCalc's menu and platform shortcut toggle only its panel, preserving drafting and calculator
//! state. Explicit opens remain idempotent and programmatic calculations do not open the palette.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, docking::Panel, menus};
use egui::{Key, Modifiers, accesskit::Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use serde_json::{Value, json};

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

/// In these tests QuickCalc floats alone; scope to its body to avoid the Properties text fields.
/// Its first text input is the expression, before conversion values and variable definitions.
fn expression<'a>(h: &'a Harness<'_, CadApp>) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(Role::Tab, "QuickCalc").parent().unwrap().get_all_by_role(Role::TextInput).next().unwrap()
}

#[test]
fn platform_shortcut_toggles_with_expression_focus_and_preserves_drafting() {
    for (os, modifiers) in [
        (egui::os::OperatingSystem::Windows, Modifiers { ctrl: true, command: true, ..Modifiers::NONE }),
        (egui::os::OperatingSystem::Mac, Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE }),
    ] {
        let mut h = harness();
        h.ctx.set_os(os);
        let drawing = h.state().session.doc().unwrap().clone();
        h.state_mut().start("line");
        h.state_mut().cmd.buffer = "12,34".into();
        h.key_press_modifiers(modifiers, Key::Num8);
        h.run_steps(5);
        assert!(h.state().ui.docking.layout.contains(&Panel::QuickCalc));
        let floating = h.state().ui.docking.layout.floating.iter().find(|group| group.contains(&Panel::QuickCalc)).unwrap().rect;
        expression(&h).click();
        h.run_steps(3);
        expression(&h).type_text("2+3");
        h.run_steps(3);
        assert!(h.ctx.egui_wants_keyboard_input());
        h.key_press_modifiers(modifiers, Key::Num8);
        h.run_steps(5);
        assert!(!h.state().ui.docking.layout.contains(&Panel::QuickCalc), "text focus must not swallow the toggle");
        h.key_press_modifiers(modifiers, Key::Num8);
        h.run_steps(5);
        assert_eq!(h.get(by().role(Role::TextInput).value("2+3")).value().as_deref(), Some("2+3"));
        assert_eq!(h.state().ui.docking.layout.floating.iter().find(|group| group.contains(&Panel::QuickCalc)).unwrap().rect, floating);
        assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
        assert_eq!(h.state().cmd.buffer, "12,34");
        assert_eq!(h.state().session.doc().unwrap(), &drawing);
    }
}

#[test]
fn window_menu_toggle_preserves_calculator_history_and_command_line_input() {
    let mut h = harness();
    let (id, _, _, shortcut) = menus::UI_COMMANDS.iter().find(|(_, label, path, _)| *label == "QuickCalc" && !path.is_empty()).unwrap();
    assert_eq!(*id, "ui.toggle.quickcalc");
    assert_eq!(*shortcut, Some("Cmd+8"));
    // Both the native menu and egui menu dispatch through activate().
    menus::activate(h.state_mut(), id);
    h.run_steps(5);
    expression(&h).click();
    h.run_steps(3);
    expression(&h).type_text("2+3");
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "=").click();
    h.run_steps(3);
    assert!(h.query_all(by().value("2+3")).next().is_some());
    h.state_mut().start("line");
    h.state_mut().cmd.buffer = "12,34".into();
    menus::activate(h.state_mut(), id);
    h.run_steps(5);
    assert!(!h.state().ui.docking.layout.contains(&Panel::QuickCalc));
    menus::activate(h.state_mut(), id);
    h.run_steps(5);
    assert!(h.query_all(by().value("2+3")).next().is_some(), "closing must not discard calculation history");
    assert_eq!(h.get(by().role(Role::TextInput).value("5")).value().as_deref(), Some("5"));
    assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
    assert_eq!(h.state().cmd.buffer, "12,34");
}

#[test]
fn toggle_restores_floating_or_docked_location_without_hiding_other_panels() {
    for floating in [false, true] {
        let mut app = CadApp::new(Session::default(), Services::default());
        app.run("ui.panel.quickcalc", Value::Null).unwrap();
        let params = if floating {
            json!({"operation":"float", "panel":"quickCalc", "rect":[80,120,360,480]})
        } else {
            json!({"operation":"move", "panel":"quickCalc", "target":"toolSets", "zone":"center"})
        };
        app.run("ui.dock", params).unwrap();
        let layout = app.ui.docking.layout.clone();
        app.run("ui.toggle.quickcalc", json!({})).unwrap();
        assert!(!app.ui.docking.layout.contains(&Panel::QuickCalc));
        for panel in [Panel::Canvas, Panel::Layers, Panel::Properties, Panel::ToolSets] {
            assert!(app.ui.docking.layout.contains(&panel));
        }
        app.run("ui.toggle.quickcalc", json!({})).unwrap();
        assert_eq!(app.ui.docking.layout, layout);
    }
}

#[test]
fn explicit_opens_still_reveal_and_calculations_do_not_open_the_palette() {
    let mut app = CadApp::new(Session::default(), Services::default());
    let value = app.run("cal", json!({"expr":"2+3"})).unwrap();
    assert_eq!(value["value"], 5.0);
    assert!(app.cmdline(r#"quickcalc {"expr":"2+3"}"#).is_none());
    assert!(!app.ui.docking.layout.contains(&Panel::QuickCalc));
    assert!(menus::is_ui_command("ui.panel.quickcalc"));
    assert!(app.cmdline("QUICKCALC").is_none());
    let layout = app.ui.docking.layout.clone();
    for command in ["QC", "QUICKCALC", "ui.panel.quickcalc"] {
        assert!(app.cmdline(command).is_none());
        assert_eq!(app.ui.docking.layout, layout, "{command} must not toggle an open palette");
    }
    app.run("ui.dock", json!({"operation":"move", "panel":"quickCalc", "target":"toolSets", "zone":"center"})).unwrap();
    app.run("ui.dock", json!({"operation":"activate", "panel":"toolSets"})).unwrap();
    app.run("ui.panel.quickcalc", Value::Null).unwrap();
    let mut workspace = app.ui.docking.clone();
    cadcraft_ui_egui::docking::reveal_panel(&mut workspace, Panel::QuickCalc).unwrap();
    assert_eq!(app.ui.docking.layout, workspace.layout, "explicit open must activate an inactive tab");
}

#[test]
fn quickcalc_shortcut_does_not_bypass_the_unsaved_drawing_prompt() {
    let mut h = harness();
    h.state_mut().run("circle", json!({"center":[0,0], "radius":1})).unwrap();
    assert!(h.state_mut().request_quit());
    assert!(h.state().closing.is_some());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num8);
    h.run_steps(4);
    assert!(!h.state().ui.docking.layout.contains(&Panel::QuickCalc));
    assert!(h.state().closing.is_some());
}
