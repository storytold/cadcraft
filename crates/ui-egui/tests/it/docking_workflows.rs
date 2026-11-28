//! Panel moves use the real app shell, retain commands, and survive a complete UI-state reload.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, UiState, docking::Panel, menus};
use craft_behavior::dock::{Action, Column, DockEdge, Layout, Node};
use egui::{Event, Modifiers, PointerButton, Rect, vec2};
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
    app.integrated_titlebar = true;
    let mut harness = Harness::builder()
        .with_context_setup(cadcraft_ui_egui::framework::install_headless)
        .with_size(vec2(width, 760.0))
        .with_pixels_per_point(scale)
        .build_ui_state(
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

/// The rect of the tab the dock drew for the panel named `label`.
fn tab(h: &Harness<'_, CadApp>, label: &str) -> Rect {
    h.get_by_role_and_label(egui::accesskit::Role::Tab, label).rect()
}

/// The columns of the dock on `side`, from the window edge inwards.
fn columns(layout: &Layout<Panel>, side: DockEdge) -> Vec<Column<Panel>> {
    let mut nodes: Vec<&Node<Panel>> = layout.root.iter().collect();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Split { first, second, .. } => nodes.extend([&**first, &**second]),
            Node::Dock { side: s, columns, .. } if *s == side => return columns.clone(),
            _ => {}
        }
    }
    Vec::new()
}

/// The panels of each group of each column of the dock on `side`.
fn dock_panels(layout: &Layout<Panel>, side: DockEdge) -> Vec<Vec<Vec<Panel>>> {
    columns(layout, side).iter().map(|column| column.groups.iter().map(|group| group.panels.clone()).collect()).collect()
}

/// The tab groups outside the docks: the frames of the split tree.
fn frames(layout: &Layout<Panel>) -> Vec<Vec<Panel>> {
    let mut out = Vec::new();
    let mut nodes: Vec<&Node<Panel>> = layout.root.iter().collect();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Split { first, second, .. } => nodes.extend([&**first, &**second]),
            Node::Tabs { panels, .. } => out.push(panels.clone()),
            Node::Stack { entries, .. } => out.push(entries.iter().map(|entry| entry.panel).collect()),
            _ => {}
        }
    }
    out
}

#[test]
fn default_layout_is_tool_sets_left_and_the_palettes_right_in_dock_columns() {
    use cadcraft_ui_egui::docking::{PALETTES_WIDTH, TOOL_SETS_WIDTH, default_workspace, validate};
    let workspace = default_workspace();
    validate(&workspace).unwrap();
    let layout = workspace.layout();
    assert!(workspace.hidden().is_empty() && layout.floating.is_empty());
    assert_eq!(frames(layout), [[Panel::Canvas]]);
    let start = columns(layout, DockEdge::Start);
    let end = columns(layout, DockEdge::End);
    assert_eq!(dock_panels(layout, DockEdge::Start), [[[Panel::ToolSets]]]);
    assert_eq!(dock_panels(layout, DockEdge::End), [vec![vec![Panel::Layers], vec![Panel::Properties]]]);
    assert_eq!((start[0].width, end[0].width), (Some(TOOL_SETS_WIDTH), Some(PALETTES_WIDTH)));
    assert!(start.iter().chain(&end).all(|column| !column.iconic));
    assert!(start.iter().chain(&end).flat_map(|column| &column.groups).all(|group| group.height.is_none() && !group.minimized));
    assert!(!layout.contains(&Panel::QuickCalc) && !layout.contains(&Panel::ExternalRefs));

    // Drawn: Tool Sets left of the drawing, Layers above Properties right of it, Layers at its
    // own height and Properties taking the rest of the column.
    let h = harness(None, 1280.0, 1.0);
    let drawing = h.state().canvas.rect.expect("the drawing view is drawn");
    let tools = tab(&h, "Tool Sets");
    let layers = tab(&h, "Layers");
    let properties = tab(&h, "Properties");
    assert!(tools.right() <= drawing.left() + 1.0, "{tools:?} {drawing:?}");
    assert!(layers.left() >= drawing.right() - 1.0 && properties.left() >= drawing.right() - 1.0, "{layers:?} {properties:?} {drawing:?}");
    // Layers keeps most of its own height above Properties.
    assert!(properties.top() - layers.bottom() > 200.0, "{layers:?} {properties:?}");
}

#[test]
fn a_palette_column_collapses_to_icons_beside_the_drawing() {
    let mut h = harness(None, 1280.0, 1.0);
    h.state_mut().run("ui.dock", json!({"action":{"SetIconic":{"panel":"layers","iconic":true}}})).unwrap();
    h.run_steps(4);
    assert!(columns(h.state().ui.docking.layout(), DockEdge::End)[0].iconic);
    let drawing = h.state().canvas.rect.unwrap();
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Tab, "Layers").is_none(), "an iconic column shows no tabs");
    // The toolbar has a Properties button too: the icons are the ones right of the drawing.
    let icon = |label: &str| {
        h.query_all_by_label(label)
            .map(|node| node.rect())
            .find(|rect| rect.left() >= drawing.right() - 1.0)
            .unwrap_or_else(|| panic!("no {label} icon"))
    };
    let (layers, properties) = (icon("Layers"), icon("Properties"));
    assert!(layers.left() >= drawing.right() - 1.0 && layers.top() < properties.top(), "{layers:?} {properties:?}");
}

#[test]
fn real_panel_drag_float_close_reopen_and_full_workspace_reload() {
    let mut harness = harness(None, 1280.0, 1.0);
    let source = tab(&harness, "Layers").center();
    let target = tab(&harness, "Tool Sets").center();
    drag(&mut harness, source, target);
    let location = harness.state().ui.docking.layout().location(&Panel::Layers).unwrap();
    assert_eq!(location.anchor, Some(Panel::ToolSets));
    harness.state_mut().run("ui.dock", json!({"operation":"float", "panel":"layers", "rect":[400,250,300,350]})).unwrap();
    harness.run_steps(5);
    let rect = harness.state().ui.docking.layout().floating.iter().find(|group| group.contains(&Panel::Layers)).unwrap().rect;
    harness.get_by_label("Close Layers").click();
    harness.run_steps(4);
    assert!(!harness.state().ui.docking.layout().contains(&Panel::Layers));
    harness.state_mut().run("ui.toggle.palettes", json!({"on": false})).unwrap();
    harness.run_steps(3);
    harness.state_mut().run("ui.toggle.palettes", json!({"on": true})).unwrap();
    harness.run_steps(4);
    assert_eq!(harness.state().ui.docking.layout().floating.iter().find(|group| group.contains(&Panel::Layers)).unwrap().rect, rect);
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
        json!({"operation":"float", "panel":"layers", "rect":[1e39,0,300,200]}),
        json!({"operation":"resizeStack", "panel":"layers", "height":1e39}),
        json!({"operation":"resizeSplit", "path":[true,true,true,true,true,true,true,true], "size":{"Ratio":0.5}}),
        json!({"operation":"move", "panel":"layers", "target":"properties", "zone":"right", "before":null}),
        json!({"operation":"spin", "panel":"layers"}),
        json!({"action":{"Float":{"panel":"canvas","rect":[0,0,100,100]}}}),
        json!({"action":{"MovePanel":{"panel":"canvas","to":{"NewDock":{"side":"Start"}}}}}),
        json!({"action":{"Move":{"panel":"layers","anchor":"canvas","placement":{"Tab":{"before":null}}}}}),
        json!(null),
    ] {
        assert!(harness.state_mut().run("ui.dock", params.clone()).is_err(), "{params}");
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
                let content = harness.state().ui.docking.layout().panels();
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
fn automation_reaches_column_and_group_resizes_tab_reorder_and_minimize_through_the_same_handler() {
    let mut h = harness(None, 1280.0, 1.0);
    let act = |h: &mut Harness<'_, CadApp>, action: Action<Panel>| {
        h.state_mut().run("ui.dock", json!({"action": serde_json::to_value(action).unwrap()})).unwrap();
    };
    act(&mut h, Action::resize_column(Panel::ToolSets, 280.0));
    act(&mut h, Action::resize_group(Panel::Layers, Some(160.0)));
    let layout = h.state().ui.docking.layout();
    assert_eq!(columns(layout, DockEdge::Start)[0].width, Some(280.0));
    assert_eq!(columns(layout, DockEdge::End)[0].groups[0].height, Some(160.0));
    h.state_mut().run("ui.dock", json!({"operation":"move","panel":"layers","target":"properties","zone":"center","before":null})).unwrap();
    assert_eq!(dock_panels(h.state().ui.docking.layout(), DockEdge::End), [[[Panel::Properties, Panel::Layers]]]);
    act(&mut h, Action::set_minimized(Panel::Layers, true));
    h.run_steps(4);
    assert!(columns(h.state().ui.docking.layout(), DockEdge::End)[0].groups[0].minimized);
    assert_eq!(columns(h.state().ui.docking.layout(), DockEdge::Start)[0].width, Some(280.0));
}

#[test]
fn grouped_restore_preserves_order_and_rectangle_in_both_close_orders_after_reload() {
    use cadcraft_ui_egui::docking::{default_workspace, permissions, reveal_panel};
    use craft_behavior::dock::DockPlacement;
    for order in [[Panel::Layers, Panel::Properties], [Panel::Properties, Panel::Layers]] {
        let mut workspace = default_workspace();
        let rect = Rect::from_min_size(egui::pos2(50.0, 60.0), vec2(320.0, 400.0));
        workspace.apply(Action::float(Panel::Layers, rect), false, permissions).unwrap();
        workspace.apply(Action::move_beside(Panel::Properties, Panel::Layers, DockPlacement::tab(Some(Panel::Layers))), false, permissions).unwrap();
        let expected: Vec<Panel> = workspace.layout().floating[0].panels().copied().collect();
        for panel in order {
            workspace.set_visible(panel, false, None, false, permissions).unwrap();
        }
        let saved = serde_json::to_string(&workspace).unwrap();
        workspace = serde_json::from_str(&saved).unwrap();
        reveal_panel(&mut workspace, Panel::Properties).unwrap();
        reveal_panel(&mut workspace, Panel::Layers).unwrap();
        assert_eq!(workspace.layout().floating.len(), 1);
        assert_eq!(workspace.layout().floating[0].panels().copied().collect::<Vec<_>>(), expected);
        assert_eq!(workspace.layout().floating[0].rect, rect);
    }
}

#[test]
fn palettes_off_and_on_reopen_a_floating_group_as_one_window() {
    let mut h = harness(None, 1280.0, 1.0);
    h.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,250,300,350]})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"move","panel":"properties","target":"layers","zone":"center"})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"close","panel":"properties"})).unwrap();
    h.state_mut().run("ui.toggle.palettes", json!({"on": false})).unwrap();
    h.run_steps(3);
    h.state_mut().run("ui.toggle.palettes", json!({"on": true})).unwrap();
    h.run_steps(3);
    let floating = &h.state().ui.docking.layout().floating;
    assert_eq!(floating.len(), 1, "{floating:?}");
    assert!(floating[0].contains(&Panel::Layers) && floating[0].contains(&Panel::Properties), "{floating:?}");
}

#[test]
fn palettes_off_and_on_restore_the_right_dock_column() {
    let mut h = harness(None, 1280.0, 1.0);
    let before = h.state().ui.docking.layout().clone();
    h.state_mut().run("ui.toggle.palettes", json!({"on": false})).unwrap();
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout().contains(&Panel::Layers) && !h.state().ui.docking.layout().contains(&Panel::Properties));
    h.state_mut().run("ui.toggle.palettes", json!({"on": true})).unwrap();
    h.run_steps(3);
    let layout = h.state().ui.docking.layout();
    assert_eq!(dock_panels(layout, DockEdge::End), dock_panels(&before, DockEdge::End));
    assert_eq!(columns(layout, DockEdge::End)[0].width, columns(&before, DockEdge::End)[0].width);
}

#[test]
fn window_menu_reopens_closed_and_new_palettes_in_their_dock_groups() {
    use cadcraft_ui_egui::docking::{PALETTES_WIDTH, TOOL_SETS_WIDTH};
    let mut h = harness(None, 1280.0, 1.0);
    // A closed palette comes back where it was.
    h.state_mut().run("ui.dock", json!({"operation":"close","panel":"layers"})).unwrap();
    h.run_steps(3);
    menus::activate(h.state_mut(), "ui.panel.layers");
    h.run_steps(3);
    assert_eq!(dock_panels(h.state().ui.docking.layout(), DockEdge::End), [vec![vec![Panel::Layers], vec![Panel::Properties]]]);
    // Tool Sets turned off and on in the Window menu comes back in the left dock at its width.
    menus::activate(h.state_mut(), "ui.toggle.toolsets");
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout().contains(&Panel::ToolSets));
    menus::activate(h.state_mut(), "ui.toggle.toolsets");
    h.run_steps(3);
    let layout = h.state().ui.docking.layout();
    assert_eq!(dock_panels(layout, DockEdge::Start), [[[Panel::ToolSets]]]);
    assert_eq!(columns(layout, DockEdge::Start)[0].width, Some(TOOL_SETS_WIDTH));
    // After Reset nothing has a recorded place: External References opens below Properties.
    h.state_mut().run("ui.dock.reset", json!({})).unwrap();
    h.state_mut().run("xref", serde_json::Value::Null).unwrap();
    h.run_steps(3);
    assert_eq!(
        dock_panels(h.state().ui.docking.layout(), DockEdge::End),
        [vec![vec![Panel::Layers], vec![Panel::Properties], vec![Panel::ExternalRefs]]]
    );
    // Tool Sets with no recorded place and no left dock makes one at its width.
    let mut workspace = cadcraft_ui_egui::docking::default_workspace();
    workspace.apply(Action::close(Panel::ToolSets), false, cadcraft_ui_egui::docking::permissions).unwrap();
    // Nothing records where Tool Sets was.
    let workspace = cadcraft_ui_egui::docking::Workspace::from_parts(workspace.layout().clone(), Vec::new(), workspace.floated().to_vec());
    h.state_mut().ui.docking = workspace;
    h.state_mut().ui.show_toolsets = false;
    menus::activate(h.state_mut(), "ui.toggle.toolsets");
    h.run_steps(3);
    let layout = h.state().ui.docking.layout();
    assert_eq!(dock_panels(layout, DockEdge::Start), [[[Panel::ToolSets]]]);
    assert_eq!(columns(layout, DockEdge::Start)[0].width, Some(TOOL_SETS_WIDTH));
    // With the palettes off, External References makes a new right dock at the palettes' width.
    h.state_mut().run("ui.dock.reset", json!({})).unwrap();
    h.state_mut().run("ui.toggle.palettes", json!({"on": false})).unwrap();
    h.run_steps(3);
    h.state_mut().run("xref", serde_json::Value::Null).unwrap();
    h.run_steps(3);
    let layout = h.state().ui.docking.layout();
    assert_eq!(dock_panels(layout, DockEdge::End), [[[Panel::ExternalRefs]]]);
    assert_eq!(columns(layout, DockEdge::End)[0].width, Some(PALETTES_WIDTH));
    assert_eq!(frames(layout), [[Panel::Canvas]]);
}

#[test]
fn explicit_properties_open_restores_closed_panel_and_activates_inactive_tab() {
    let mut harness = harness(None, 1000.0, 1.0);
    harness.state_mut().run("ui.dock", json!({"operation":"close","panel":"properties"})).unwrap();
    harness.state_mut().cmdline("PROPERTIES");
    assert!(harness.state().ui.docking.layout().contains(&Panel::Properties));
    harness.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,250,300,350]})).unwrap();
    harness.state_mut().run("ui.dock", json!({"operation":"move","panel":"properties","target":"layers","zone":"center"})).unwrap();
    harness.state_mut().run("ui.dock", json!({"operation":"activate","panel":"layers"})).unwrap();
    harness.state_mut().run("ui.panel.properties", json!({})).unwrap();
    harness.run_steps(3);
    let location = harness.state().ui.docking.layout().location(&Panel::Properties).unwrap();
    assert_eq!(location.anchor, Some(Panel::Layers));
    assert_eq!(harness.state().ui.docking.layout().panel_state(&Panel::Properties), craft_behavior::dock::PanelState::Front);
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
    let tools = h.state().ui.docking.layout().location(&Panel::ToolSets).unwrap();
    type_command(&mut h, "PROPERTIES");
    assert_eq!(dock_panels(h.state().ui.docking.layout(), DockEdge::End), [vec![vec![Panel::Layers], vec![Panel::Properties]]]);
    assert_eq!(h.state().ui.docking.layout().location(&Panel::ToolSets).unwrap(), tools);

    h.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,250,300,350]})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"move","panel":"properties","target":"layers","zone":"center"})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"activate","panel":"layers"})).unwrap();
    h.run_steps(3);
    type_command(&mut h, "PROPERTIES");
    assert!(h.state().ui.docking.layout().floating.iter().any(|group| group.contains(&Panel::Properties)));
    assert_eq!(h.state().ui.docking.layout().panel_state(&Panel::Properties), craft_behavior::dock::PanelState::Front);
    assert_eq!(h.state().session.doc().unwrap(), &drawing);
}

#[test]
fn reset_typed_in_canvas_preserves_authoritative_workspace_replacement() {
    let mut h = harness(None, 1280.0, 1.0);
    h.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,200,300,350]})).unwrap();
    h.run_steps(3);
    let drawing = h.state().session.doc().unwrap().clone();
    type_command(&mut h, "ui.dock.reset");
    let expected = cadcraft_ui_egui::docking::default_workspace();
    assert_eq!(h.state().ui.docking.layout(), expected.layout());
    assert_eq!(h.state().ui.docking.hidden(), expected.hidden());
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

#[test]
fn palettes_dock_only_in_the_left_and_right_columns() {
    let mut h = harness(None, 1280.0, 1.0);
    let document = h.state().session.doc().unwrap().clone();
    let drawing = h.state().canvas.rect.expect("the drawing view is drawn");
    // Neither the drawing nor the bottom edge takes a drop: the palette floats there.
    for to in [drawing.center(), egui::pos2(drawing.center().x, drawing.bottom() - 4.0)] {
        h.state_mut().run("ui.dock.reset", json!({})).unwrap();
        h.run_steps(3);
        let from = tab(&h, "Layers").center();
        drag(&mut h, from, to);
        let layout = h.state().ui.docking.layout();
        assert_eq!(frames(layout), [[Panel::Canvas]], "{to:?}: {:?}", layout.root);
        assert_eq!(dock_panels(layout, DockEdge::End), [[[Panel::Properties]]], "{to:?}");
        assert!(layout.floating.iter().any(|window| window.contains(&Panel::Layers)), "{to:?}: {layout:?}");
    }
    // The left window edge, where Tool Sets docks: the drop leaves the drawing the only frame and Tool Sets in its
    // column. Whether the palette becomes a new outer column there is the dock's own edge-drop rule.
    h.state_mut().run("ui.dock.reset", json!({})).unwrap();
    h.run_steps(3);
    let from = tab(&h, "Layers").center();
    drag(&mut h, from, egui::pos2(3.0, drawing.center().y));
    let layout = h.state().ui.docking.layout();
    assert!(dock_panels(layout, DockEdge::Start).concat().concat().contains(&Panel::ToolSets), "{:?}", layout.root);
    assert_eq!(frames(layout), [[Panel::Canvas]]);
    // Without a left dock, the left window edge makes one.
    h.state_mut().run("ui.dock.reset", json!({})).unwrap();
    h.state_mut().run("ui.toggle.toolsets", json!({"on": false})).unwrap();
    h.run_steps(4);
    assert!(columns(h.state().ui.docking.layout(), DockEdge::Start).iter().all(|column| column.groups.is_empty()));
    let drawing = h.state().canvas.rect.unwrap();
    let from = tab(&h, "Layers").center();
    drag(&mut h, from, egui::pos2(drawing.left() + 3.0, drawing.center().y));
    let layout = h.state().ui.docking.layout();
    assert_eq!(dock_panels(layout, DockEdge::Start), [[[Panel::Layers]]], "{:?}", layout.root);
    assert_eq!(frames(layout), [[Panel::Canvas]]);
    assert_eq!(h.state().session.doc().unwrap(), &document);
}

/// Every closable panel's tab has a visible close button, docked and floating, and it closes the panel; the drawing
/// has none.
#[test]
fn every_closable_panel_tab_has_a_close_button_that_closes_it() {
    let mut h = harness(None, 1280.0, 1.0);
    assert!(h.query_by_label("Close Tool Sets").is_some(), "docked tabs show a close button");
    assert!(h.query_by_label("Close Drawing").is_none(), "the drawing view is not a closable panel");
    // External References opens as a tab; its button sits beside the title, not over it.
    h.state_mut().run("ui.dock", json!({"operation":"open","panel":"externalReferences"})).unwrap();
    h.run_steps(4);
    assert!(h.state().ui.docking.layout().contains(&Panel::ExternalRefs));
    let close = h.get_by_label("Close External References").rect();
    let title = tab(&h, "External References");
    assert!(close.width() < title.width(), "the close button does not cover the panel title");
    h.get_by_label("Close External References").click();
    h.run_steps(4);
    assert!(!h.state().ui.docking.layout().contains(&Panel::ExternalRefs));
    h.get_by_label("Close Layers").click();
    h.run_steps(4);
    assert!(!h.state().ui.docking.layout().contains(&Panel::Layers));
    // Floating panels get the same button.
    h.state_mut().run("ui.dock", json!({"operation":"float","panel":"properties","rect":[400,250,300,350]})).unwrap();
    h.run_steps(4);
    h.get_by_label("Close Properties").click();
    h.run_steps(4);
    assert!(!h.state().ui.docking.layout().contains(&Panel::Properties));
}
