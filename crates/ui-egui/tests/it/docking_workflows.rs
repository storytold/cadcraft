//! Panel moves use the real app shell, retain commands, and survive a complete UI-state reload.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, UiState, docking::Panel};
use egui::{Event, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(state: Option<UiState>, width: f32, scale: f32) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("rectang", json!({"p1":[30,30],"p2":[170,110]})).unwrap();
    app.run("circle", json!({"center":[100,70],"radius":26})).unwrap();
    app.run("line", json!({"points":[[30,30],[170,110]]})).unwrap();
    app.run("view.set", json!({"center":[100,70],"height":220})).unwrap();
    if let Some(state) = state {
        app.ui = state;
    }
    app.ui.in_window_menu = false;
    // These tests drive the Tool Sets palette; its tools also sit on the Tool Sets bar.
    app.ui.toolsets_bar.show = false;
    app.integrated_titlebar = true;
    let mut harness = Harness::builder().with_size(vec2(width, 760.0)).with_pixels_per_point(scale).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

fn drag(harness: &mut Harness<'_, CadApp>, from: egui::Pos2, to: egui::Pos2) {
    harness.event(Event::PointerMoved(from));
    harness.run_steps(2);
    harness.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    harness.run_steps(2);
    for step in 1..=8 {
        harness.event(Event::PointerMoved(from.lerp(to, step as f32 / 8.0)));
        harness.run_steps(1);
    }
    harness.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    harness.run_steps(4);
}

#[test]
fn real_panel_drag_float_close_reopen_and_full_workspace_reload() {
    let mut harness = harness(None, 1280.0, 1.0);
    let source = harness.get_by_role_and_label(egui::accesskit::Role::Tab, "Layers").rect().center();
    let target = harness.get_by_role_and_label(egui::accesskit::Role::Tab, "Tool Sets").rect().center();
    drag(&mut harness, source, target);
    let location = harness.state().ui.docking.layout.location(&Panel::Layers).unwrap();
    assert_eq!(location.anchor, Some(Panel::ToolSets));
    harness.get_by_role_and_label(egui::accesskit::Role::Tab, "Layers").click_button(PointerButton::Secondary);
    harness.run_steps(3);
    harness.get_by_label("Float panel").click();
    harness.run_steps(5);
    let rect = harness.state().ui.docking.layout.floating.iter().find(|group| group.panels.contains(&Panel::Layers)).unwrap().rect;
    harness.get_by_role_and_label(egui::accesskit::Role::Tab, "Layers").click_button(PointerButton::Secondary);
    harness.run_steps(3);
    harness.get_by_label("Close panel").click();
    harness.run_steps(4);
    assert!(!harness.state().ui.docking.layout.contains(&Panel::Layers));
    harness.state_mut().run("ui.toggle.palettes", json!({"on": false})).unwrap();
    harness.run_steps(3);
    harness.state_mut().run("ui.toggle.palettes", json!({"on": true})).unwrap();
    harness.run_steps(4);
    assert_eq!(harness.state().ui.docking.layout.floating.iter().find(|group| group.panels.contains(&Panel::Layers)).unwrap().rect, rect);
    let saved = serde_json::to_string(&harness.state().ui).unwrap();
    let expected = serde_json::to_value(&harness.state().ui.docking).unwrap();
    let mut reloaded = self::harness(Some(serde_json::from_str(&saved).unwrap()), 1280.0, 1.0);
    assert_eq!(serde_json::to_value(&reloaded.state().ui.docking).unwrap(), expected);
    reloaded.state_mut().run("ui.dock", json!({"operation":"move", "panel":"layers", "target":"toolSets", "zone":"center"})).unwrap();
    reloaded.run_steps(4);
    reloaded.get_by_role_and_label(egui::accesskit::Role::Tab, "Tool Sets").click();
    reloaded.run_steps(3);
    reloaded.get_by_label("Line").click();
    reloaded.run_steps(3);
    assert_eq!(reloaded.state().session.running.as_ref().map(|command| command.id.as_str()), Some("line"));
}

#[test]
fn invalid_requests_leave_document_and_panel_layout_unchanged() {
    let mut harness = harness(None, 1000.0, 1.0);
    let before = serde_json::to_value(&harness.state().ui.docking).unwrap();
    let drawing = harness.state().session.doc().unwrap().clone();
    for params in [
        json!({"operation":"float", "panel":"canvas", "rect":[100,100,400,300]}),
        json!({"operation":"move", "panel":"toolSets", "target":"canvas", "zone":"center"}),
        json!({"operation":"float", "panel":"layers", "rect":[0,0,-1,200]}),
        json!({"operation":"move", "panel":"missing", "target":"canvas", "zone":"right"}),
    ] {
        assert!(harness.state_mut().run("ui.dock", params).is_err());
        assert_eq!(serde_json::to_value(&harness.state().ui.docking).unwrap(), before);
        assert_eq!(harness.state().session.doc().unwrap(), &drawing);
    }
}

#[test]
fn docking_renders_default_float_and_redocked_at_normal_and_narrow_sizes() {
    for width in [900.0, 1280.0] {
        for scale in [1.0, 2.0] {
            let mut harness = harness(None, width, scale);
            for state in ["default", "floating", "redocked"] {
                if state == "floating" {
                    harness.state_mut().run("ui.dock", json!({"operation":"float", "panel":"layers", "rect":[160,100,300,380]})).unwrap();
                } else if state == "redocked" {
                    harness
                        .state_mut()
                        .run("ui.dock", json!({"operation":"move", "panel":"layers", "target":"properties", "zone":"center"}))
                        .unwrap();
                }
                harness.run_steps(5);
                harness.state().ui.docking.validate().unwrap();
                let content = harness.state().ui.docking.layout.panels();
                assert_eq!(content.len(), 4);
                assert!(content.contains(&&Panel::Canvas));
                if let Ok(directory) = std::env::var("CRAFT_UI_DOCKING_DIR") {
                    std::fs::create_dir_all(&directory).unwrap();
                    harness.render().unwrap().save(format!("{directory}/cad-{width}-{scale}-{state}.png")).unwrap();
                }
            }
        }
    }
}

#[test]
fn automation_reaches_split_stack_resize_and_tab_reorder_through_the_same_handler() {
    use craft_ui::{docking::Action, layout::SplitSize};
    let mut h = harness(None, 1280.0, 1.0);
    h.state_mut()
        .run(
            "ui.dock",
            json!({"action":serde_json::to_value(Action::<Panel>::ResizeSplit { path: vec![], size: SplitSize::FixedFirst(280.0) }).unwrap()}),
        )
        .unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"setStackOpen","panel":"layers","open":false})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"resizeStack","panel":"layers","height":160.0})).unwrap();
    assert_eq!(h.state().ui.docking.layout.location(&Panel::Layers).unwrap().stack, Some((false, Some(160.0))));
    h.state_mut().run("ui.dock", json!({"operation":"move","panel":"layers","target":"properties","zone":"center","before":null})).unwrap();
    h.run_steps(4);
    let panels = h.state().ui.docking.layout.panels();
    assert!(panels.iter().position(|p| **p == Panel::Properties).unwrap() < panels.iter().position(|p| **p == Panel::Layers).unwrap());
    assert_eq!(h.state().ui.docking.layout.location(&Panel::ToolSets).unwrap().size, Some(SplitSize::FixedFirst(280.0)));
}

#[test]
fn stack_boundary_resizes_by_pointer_and_keyboard_without_changing_the_drawing() {
    let mut h = harness(None, 1280.0, 1.0);
    let drawing = h.state().session.doc().unwrap().clone();
    let center = h
        .get_all_by_role_and_label(egui::accesskit::Role::Splitter, "Resize panels")
        .into_iter()
        .find(|node| node.rect().width() > node.rect().height())
        .unwrap()
        .rect()
        .center();
    drag(&mut h, center, center + vec2(0.0, -45.0));
    let height = h.state().ui.docking.layout.location(&Panel::Layers).unwrap().stack.unwrap().1.unwrap();
    h.get_all_by_role_and_label(egui::accesskit::Role::Splitter, "Resize panels")
        .into_iter()
        .find(|node| node.rect().width() > node.rect().height())
        .unwrap()
        .focus();
    h.run_steps(2);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(4);
    let after = h.state().ui.docking.layout.location(&Panel::Layers).unwrap().stack.unwrap().1.unwrap();
    assert!(after > height, "keyboard resize must reach the same panel command handler");
    assert_eq!(h.state().session.doc().unwrap(), &drawing);
}

#[test]
fn grouped_restore_preserves_order_and_rectangle_in_both_close_orders_after_reload() {
    use craft_ui::docking::{Action, Placement};
    for order in [[Panel::Layers, Panel::Properties], [Panel::Properties, Panel::Layers]] {
        let mut workspace = cadcraft_ui_egui::docking::Workspace::default();
        let rect = [50.0, 60.0, 320.0, 400.0];
        workspace.apply(Action::Float { panel: Panel::Layers, rect }).unwrap();
        workspace
            .apply(Action::Move { panel: Panel::Properties, anchor: Panel::Layers, placement: Placement::Tab { before: Some(Panel::Layers) } })
            .unwrap();
        let expected = workspace.layout.floating[0].panels.clone();
        for panel in order {
            workspace.set_visible(panel, false).unwrap();
        }
        let saved = serde_json::to_string(&workspace).unwrap();
        workspace = serde_json::from_str(&saved).unwrap();
        workspace.reveal_panel(Panel::Properties).unwrap();
        workspace.reveal_panel(Panel::Layers).unwrap();
        assert_eq!(workspace.layout.floating.len(), 1);
        assert_eq!(workspace.layout.floating[0].panels, expected);
        assert_eq!(workspace.layout.floating[0].rect, rect);
    }
}

#[test]
fn explicit_properties_open_restores_closed_panel_and_activates_inactive_tab() {
    let mut harness = harness(None, 1000.0, 1.0);
    harness.state_mut().run("ui.dock", json!({"operation":"close","panel":"properties"})).unwrap();
    harness.state_mut().cmdline("PROPERTIES");
    assert!(harness.state().ui.docking.layout.contains(&Panel::Properties));
    harness.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,250,300,350]})).unwrap();
    harness.state_mut().run("ui.dock", json!({"operation":"move","panel":"properties","target":"layers","zone":"center"})).unwrap();
    harness.state_mut().run("ui.dock", json!({"operation":"activate","panel":"layers"})).unwrap();
    harness.state_mut().run("ui.panel.properties", json!({})).unwrap();
    harness.run_steps(3);
    let location = harness.state().ui.docking.layout.location(&Panel::Properties).unwrap();
    assert_eq!(location.anchor, Some(Panel::Layers));
    let group = harness.state().ui.docking.layout.floating.iter().find(|group| group.panels.contains(&Panel::Properties)).unwrap();
    assert_eq!(group.panels.get(group.active), Some(&Panel::Properties));
}

#[test]
fn middle_press_over_floating_panel_does_not_pan_drawing() {
    let mut harness = harness(None, 1280.0, 1.0);
    harness.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,250,300,350]})).unwrap();
    harness.run_steps(4);
    let before = harness.state().session.state().unwrap().view();
    let at = egui::pos2(450.0, 330.0);
    harness.event(Event::PointerMoved(at));
    harness.event(Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: true, modifiers: Modifiers::NONE });
    harness.run_steps(2);
    harness.event(Event::PointerMoved(at + egui::vec2(40.0, 25.0)));
    harness.run_steps(3);
    let after = harness.state().session.state().unwrap().view();
    assert_eq!(after.center, before.center);
    assert_eq!(after.height, before.height);
    harness.event(Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: false, modifiers: Modifiers::NONE });
    harness.run_steps(2);
}

fn type_command(h: &mut Harness<'_, CadApp>, command: &str) {
    h.ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    h.event(Event::Text(command.into()));
    h.run_steps(1);
    assert_eq!(h.state().cmd.buffer, command);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().cmd.buffer.is_empty());
}

#[test]
fn properties_typed_in_rendered_command_line_reopens_and_selects_actual_workspace() {
    let mut h = harness(None, 1280.0, 1.0);
    h.state_mut().run("ui.dock", json!({"operation":"close","panel":"properties"})).unwrap();
    h.run_steps(3);
    let drawing = h.state().session.doc().unwrap().clone();
    let tools = h.state().ui.docking.layout.location(&Panel::ToolSets).unwrap();
    type_command(&mut h, "PROPERTIES");
    assert!(h.state().ui.docking.layout.contains(&Panel::Properties));
    assert!(h.state().ui.docking.layout.contains(&Panel::Layers));
    let restored_tools = h.state().ui.docking.layout.location(&Panel::ToolSets).unwrap();
    assert_eq!(restored_tools.anchor, tools.anchor);
    assert_eq!(restored_tools.placement, tools.placement);
    assert_eq!(restored_tools.size, tools.size);
    assert_eq!(restored_tools.floating, tools.floating);
    assert_eq!(restored_tools.stack, tools.stack);
    assert!(restored_tools.sibling.contains(&Panel::Properties));

    h.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,250,300,350]})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"move","panel":"properties","target":"layers","zone":"center"})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"activate","panel":"layers"})).unwrap();
    h.run_steps(3);
    type_command(&mut h, "PROPERTIES");
    let group = h.state().ui.docking.layout.floating.iter().find(|group| group.panels.contains(&Panel::Properties)).unwrap();
    assert_eq!(group.panels.get(group.active), Some(&Panel::Properties));
    assert_eq!(h.state().session.doc().unwrap(), &drawing);
}

#[test]
fn reset_typed_in_canvas_preserves_authoritative_workspace_replacement() {
    let mut h = harness(None, 1280.0, 1.0);
    h.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,200,300,350]})).unwrap();
    h.run_steps(3);
    let drawing = h.state().session.doc().unwrap().clone();
    type_command(&mut h, "ui.dock.reset");
    let expected = cadcraft_ui_egui::docking::Workspace::default();
    assert_eq!(h.state().ui.docking.layout, expected.layout);
    assert_eq!(h.state().ui.docking.hidden, expected.hidden);
    assert_eq!(h.state().session.doc().unwrap(), &drawing);
}

#[test]
fn canvas_reset_during_splitter_drag_discards_old_resize_and_ends_gesture() {
    use craft_ui::docking::Action;
    use craft_ui::layout::SplitSize;
    let mut h = harness(None, 1280.0, 1.0);
    // Give the old root divider a size different from Reset's 220px ToolSets pane.
    h.state_mut()
        .run(
            "ui.dock",
            json!({"action":Action::<Panel>::ResizeSplit {
                path: Vec::new(), size: SplitSize::FixedFirst(280.0),
            }}),
        )
        .unwrap();
    h.run_steps(3);
    let drawing = h.state().session.doc().unwrap().clone();
    let splitter = egui::Id::new("cadcraft-docking").with(("split", Vec::<bool>::new()));
    let from = h.ctx.read_response(splitter).expect("rendered root divider").rect.center();
    h.event(Event::PointerMoved(from));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    // Canvas is visited before the ancestor ResizeHandle. The command runs before
    // that ancestor queues this frame's ResizeSplit against the old immutable tree.
    assert_eq!(h.ctx.dragged_id(), Some(splitter), "actual divider owns the pointer");
    assert!(h.ctx.input(|input| input.pointer.primary_down()));
    // A ResizeHandle takes keyboard focus on press. Release only that keyboard focus
    // so the real Canvas command line can submit while the pointer remains captured.
    h.ctx.memory_mut(|memory| memory.surrender_focus(splitter));
    assert!(!h.ctx.egui_wants_keyboard_input());
    let to = from + vec2(36.0, 0.0);
    h.event(Event::PointerMoved(to));
    h.event(Event::Text("ui.dock.reset".into()));
    h.event(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
    let expected = cadcraft_ui_egui::docking::Workspace::default();
    assert!(h.state().cmd.buffer.is_empty(), "Canvas submitted the actual reset command");
    assert_eq!(h.state().ui.docking.layout, expected.layout, "old-frame resize must not modify Reset's tree");
    assert_eq!(h.state().ui.docking.hidden, expected.hidden);
    // Keep the physical button down: replacement must terminate the old divider gesture,
    // rather than letting its stable root ID continue resizing the new workspace.
    let later = to + vec2(24.0, 0.0);
    h.event(Event::PointerMoved(later));
    h.run_steps(2);
    assert_eq!(h.state().ui.docking.layout, expected.layout, "continued old drag must leave the replacement unchanged");
    h.event(Event::PointerButton { pos: later, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(h.state().ui.docking.layout, expected.layout);
    assert_eq!(h.state().session.doc().unwrap(), &drawing);
}

#[test]
fn collapsed_tool_sets_leave_a_visible_control_that_expands_them() {
    let mut h = harness(None, 1280.0, 1.0);
    h.get_by_label("Collapse Tool Sets").click();
    h.run_steps(4);
    assert!(!h.state().ui.show_toolsets);
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Tab, "Tool Sets").is_none());
    h.get_by_label("Expand Tool Sets").click();
    h.run_steps(4);
    assert!(h.state().ui.show_toolsets);
    assert!(h.query_by_label("Expand Tool Sets").is_none());
    h.get_by_role_and_label(egui::accesskit::Role::Tab, "Tool Sets").click();
    h.run_steps(3);
    h.get_by_label("Line").click();
    h.run_steps(3);
    assert_eq!(h.state().session.running.as_ref().map(|command| command.id.as_str()), Some("line"));
    h.state_mut().session.cancel();
    // Window > Tool Sets (Cmd+3) stays the keyboard route back.
    h.get_by_label("Collapse Tool Sets").click();
    h.run_steps(4);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Num3);
    h.run_steps(4);
    assert!(h.state().ui.show_toolsets);
}
