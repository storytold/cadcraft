//! Panel moves use the real app shell, retain commands, and survive a complete UI-state reload.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, UiState, docking::Panel, menus};
use craft_ui::docking::{Column, Divider, DockArea, Hit, Layout, Node, Side};
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

fn area() -> egui::Id {
    egui::Id::unique("cadcraft-docking")
}

/// The rect the dock area drew last frame for `hit`.
fn hit(h: &Harness<'_, CadApp>, hit: Hit<Panel>) -> egui::Rect {
    let hits = DockArea::<Panel>::hit_rects(&h.ctx, area());
    hits.iter().find(|(drawn, _)| *drawn == hit).map(|(_, rect)| *rect).unwrap_or_else(|| panic!("{hit:?} is not drawn: {hits:?}"))
}

/// The columns of the dock on `side`, from the window edge inwards.
fn columns(layout: &Layout<Panel>, side: Side) -> Vec<Column<Panel>> {
    let mut nodes: Vec<&Node<Panel>> = layout.root.iter().collect();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Split { first, second, .. } => nodes.extend([&**first, &**second]),
            Node::Dock { side: s, columns } if *s == side => return columns.clone(),
            _ => {}
        }
    }
    Vec::new()
}

/// The panels of each group of each column of the dock on `side`.
fn dock_panels(layout: &Layout<Panel>, side: Side) -> Vec<Vec<Vec<Panel>>> {
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
            Node::Stack { entries } => out.push(entries.iter().map(|entry| entry.panel).collect()),
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
    assert!(workspace.hidden.is_empty() && workspace.layout.floating.is_empty());
    assert_eq!(frames(&workspace.layout), [[Panel::Canvas]]);
    let start = columns(&workspace.layout, Side::Start);
    let end = columns(&workspace.layout, Side::End);
    assert_eq!(dock_panels(&workspace.layout, Side::Start), [[[Panel::ToolSets]]]);
    assert_eq!(dock_panels(&workspace.layout, Side::End), [vec![vec![Panel::Layers], vec![Panel::Properties]]]);
    assert_eq!((start[0].width, end[0].width), (TOOL_SETS_WIDTH, PALETTES_WIDTH));
    assert!(start.iter().chain(&end).all(|column| !column.iconic));
    assert!(start.iter().chain(&end).flat_map(|column| &column.groups).all(|group| group.height.is_none() && !group.minimized));
    assert!(!workspace.layout.contains(&Panel::QuickCalc) && !workspace.layout.contains(&Panel::ExternalRefs));

    // Drawn: Tool Sets left of the drawing, Layers above Properties right of it, Layers at its
    // own height and Properties taking the rest of the column.
    let h = harness(None, 1280.0, 1.0);
    let drawing = h.state().canvas.rect.expect("the drawing view is drawn");
    let tools = hit(&h, Hit::Tab(Panel::ToolSets));
    let layers = hit(&h, Hit::Tab(Panel::Layers));
    let properties = hit(&h, Hit::Tab(Panel::Properties));
    assert!(tools.right() <= drawing.left() + 1.0, "{tools:?} {drawing:?}");
    assert!(layers.left() >= drawing.right() - 1.0 && properties.left() >= drawing.right() - 1.0, "{layers:?} {properties:?} {drawing:?}");
    let seam = hit(&h, Hit::Divider(Divider::Group(Panel::Layers)));
    assert!((seam.center().y - layers.bottom() - 280.0).abs() < 8.0, "{layers:?} {seam:?}");
    assert!(properties.top() > seam.top());
}

#[test]
fn palette_icons_are_painted_in_code_and_shown_in_an_iconic_column() {
    use cadcraft_ui_egui::docking::Panels;
    let mut app = CadApp::new(Session::default(), Services::default());
    let ctx = egui::Context::default();
    let mut paint = |panel: Panel| {
        let mut painted = false;
        let mut content = Panels { app: &mut app };
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(20.0, 20.0));
            painted = craft_ui::docking::DockContent::paint_icon(&mut content, &painter, rect, &panel, egui::Color32::RED);
        });
        output.textures_delta.clear();
        (painted, output.shapes.len())
    };
    for panel in [Panel::ToolSets, Panel::QuickCalc, Panel::Layers, Panel::Properties, Panel::ExternalRefs] {
        let (painted, shapes) = paint(panel);
        assert!(painted && shapes > 0, "{panel:?}: {painted} {shapes}");
    }
    assert_eq!(paint(Panel::Canvas), (false, 0));

    let mut h = harness(None, 1280.0, 1.0);
    h.state_mut().run("ui.dock", json!({"action":{"SetIconic":{"panel":"layers","iconic":true}}})).unwrap();
    h.run_steps(4);
    let drawing = h.state().canvas.rect.unwrap();
    let layers = hit(&h, Hit::Icon(Panel::Layers));
    let properties = hit(&h, Hit::Icon(Panel::Properties));
    assert!(layers.left() >= drawing.right() - 1.0 && layers.top() < properties.top(), "{layers:?} {properties:?}");
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
    harness.get_by_label("Float Panel").click();
    harness.run_steps(5);
    let rect = harness.state().ui.docking.layout.floating.iter().find(|group| group.contains(&Panel::Layers)).unwrap().rect;
    harness.get_by_role_and_label(egui::accesskit::Role::Tab, "Layers").click_button(PointerButton::Secondary);
    harness.run_steps(3);
    harness.get_by_label("Close Panel").click();
    harness.run_steps(4);
    assert!(!harness.state().ui.docking.layout.contains(&Panel::Layers));
    harness.state_mut().run("ui.toggle.palettes", json!({"on": false})).unwrap();
    harness.run_steps(3);
    harness.state_mut().run("ui.toggle.palettes", json!({"on": true})).unwrap();
    harness.run_steps(4);
    assert_eq!(harness.state().ui.docking.layout.floating.iter().find(|group| group.contains(&Panel::Layers)).unwrap().rect, rect);
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
fn automation_reaches_column_and_group_resizes_tab_reorder_and_minimize_through_the_same_handler() {
    use craft_ui::docking::Action;
    let mut h = harness(None, 1280.0, 1.0);
    let act = |h: &mut Harness<'_, CadApp>, action: Action<Panel>| {
        h.state_mut().run("ui.dock", json!({"action": serde_json::to_value(action).unwrap()})).unwrap();
    };
    act(&mut h, Action::ResizeColumn { panel: Panel::ToolSets, width: 280.0 });
    act(&mut h, Action::ResizeGroup { panel: Panel::Layers, height: Some(160.0) });
    let layout = &h.state().ui.docking.layout;
    assert_eq!(columns(layout, Side::Start)[0].width, 280.0);
    assert_eq!(columns(layout, Side::End)[0].groups[0].height, Some(160.0));
    h.state_mut().run("ui.dock", json!({"operation":"move","panel":"layers","target":"properties","zone":"center","before":null})).unwrap();
    assert_eq!(dock_panels(&h.state().ui.docking.layout, Side::End), [[[Panel::Properties, Panel::Layers]]]);
    act(&mut h, Action::SetMinimized { panel: Panel::Layers, minimized: true });
    h.run_steps(4);
    assert!(columns(&h.state().ui.docking.layout, Side::End)[0].groups[0].minimized);
    assert_eq!(columns(&h.state().ui.docking.layout, Side::Start)[0].width, 280.0);
}

#[test]
fn group_divider_resizes_by_pointer_and_keyboard_without_changing_the_drawing() {
    let mut h = harness(None, 1280.0, 1.0);
    let drawing = h.state().session.doc().unwrap().clone();
    let layers_height = |h: &Harness<'_, CadApp>| columns(&h.state().ui.docking.layout, Side::End)[0].groups[0].height;
    let center = hit(&h, Hit::Divider(Divider::Group(Panel::Layers))).center();
    drag(&mut h, center, center + vec2(0.0, -45.0));
    let height = layers_height(&h).expect("the drag sets the Layers group's height");
    h.ctx.memory_mut(|memory| memory.request_focus(DockArea::<Panel>::hit_id(area(), &Hit::Divider(Divider::Group(Panel::Layers)))));
    h.run_steps(2);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(4);
    let after = layers_height(&h).unwrap();
    assert!(after > height, "keyboard resize must reach the same panel command handler: {height} {after}");
    assert_eq!(h.state().session.doc().unwrap(), &drawing);
}

#[test]
fn grouped_restore_preserves_order_and_rectangle_in_both_close_orders_after_reload() {
    use cadcraft_ui_egui::docking::{default_workspace, permissions, reveal_panel};
    use craft_ui::docking::{Action, Placement};
    for order in [[Panel::Layers, Panel::Properties], [Panel::Properties, Panel::Layers]] {
        let mut workspace = default_workspace();
        let rect = [50.0, 60.0, 320.0, 400.0];
        workspace.apply(Action::Float { panel: Panel::Layers, rect }, permissions).unwrap();
        workspace
            .apply(
                Action::Move { panel: Panel::Properties, anchor: Panel::Layers, placement: Placement::Tab { before: Some(Panel::Layers) } },
                permissions,
            )
            .unwrap();
        let expected: Vec<Panel> = workspace.layout.floating[0].panels().copied().collect();
        for panel in order {
            workspace.set_visible(panel, false, None, permissions).unwrap();
        }
        let saved = serde_json::to_string(&workspace).unwrap();
        workspace = serde_json::from_str(&saved).unwrap();
        reveal_panel(&mut workspace, Panel::Properties).unwrap();
        reveal_panel(&mut workspace, Panel::Layers).unwrap();
        assert_eq!(workspace.layout.floating.len(), 1);
        assert_eq!(workspace.layout.floating[0].panels().copied().collect::<Vec<_>>(), expected);
        assert_eq!(workspace.layout.floating[0].rect, rect);
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
    let floating = &h.state().ui.docking.layout.floating;
    assert_eq!(floating.len(), 1, "{floating:?}");
    assert!(floating[0].contains(&Panel::Layers) && floating[0].contains(&Panel::Properties), "{floating:?}");
}

#[test]
fn palettes_off_and_on_restore_the_right_dock_column() {
    let mut h = harness(None, 1280.0, 1.0);
    let before = h.state().ui.docking.layout.clone();
    h.state_mut().run("ui.toggle.palettes", json!({"on": false})).unwrap();
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout.contains(&Panel::Layers) && !h.state().ui.docking.layout.contains(&Panel::Properties));
    h.state_mut().run("ui.toggle.palettes", json!({"on": true})).unwrap();
    h.run_steps(3);
    let layout = &h.state().ui.docking.layout;
    assert_eq!(dock_panels(layout, Side::End), dock_panels(&before, Side::End));
    assert_eq!(columns(layout, Side::End)[0].width, columns(&before, Side::End)[0].width);
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
    assert_eq!(dock_panels(&h.state().ui.docking.layout, Side::End), [vec![vec![Panel::Layers], vec![Panel::Properties]]]);
    // Tool Sets turned off and on in the Window menu comes back in the left dock at its width.
    menus::activate(h.state_mut(), "ui.toggle.toolsets");
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout.contains(&Panel::ToolSets));
    menus::activate(h.state_mut(), "ui.toggle.toolsets");
    h.run_steps(3);
    let layout = &h.state().ui.docking.layout;
    assert_eq!(dock_panels(layout, Side::Start), [[[Panel::ToolSets]]]);
    assert_eq!(columns(layout, Side::Start)[0].width, TOOL_SETS_WIDTH);
    // After Reset nothing has a recorded place: External References opens below Properties.
    h.state_mut().run("ui.dock.reset", json!({})).unwrap();
    h.state_mut().run("xref", serde_json::Value::Null).unwrap();
    h.run_steps(3);
    assert_eq!(dock_panels(&h.state().ui.docking.layout, Side::End), [vec![vec![Panel::Layers], vec![Panel::Properties], vec![Panel::ExternalRefs]]]);
    // Tool Sets with no recorded place and no left dock makes one at its width.
    let mut workspace = cadcraft_ui_egui::docking::default_workspace();
    workspace.apply(craft_ui::docking::Action::Close { panel: Panel::ToolSets }, cadcraft_ui_egui::docking::permissions).unwrap();
    workspace.hidden.clear();
    h.state_mut().ui.docking = workspace;
    h.state_mut().ui.show_toolsets = false;
    menus::activate(h.state_mut(), "ui.toggle.toolsets");
    h.run_steps(3);
    let layout = &h.state().ui.docking.layout;
    assert_eq!(dock_panels(layout, Side::Start), [[[Panel::ToolSets]]]);
    assert_eq!(columns(layout, Side::Start)[0].width, TOOL_SETS_WIDTH);
    // With the palettes off, External References makes a new right dock at the palettes' width.
    h.state_mut().run("ui.dock.reset", json!({})).unwrap();
    h.state_mut().run("ui.toggle.palettes", json!({"on": false})).unwrap();
    h.run_steps(3);
    h.state_mut().run("xref", serde_json::Value::Null).unwrap();
    h.run_steps(3);
    let layout = &h.state().ui.docking.layout;
    assert_eq!(dock_panels(layout, Side::End), [[[Panel::ExternalRefs]]]);
    assert_eq!(columns(layout, Side::End)[0].width, PALETTES_WIDTH);
    assert_eq!(frames(layout), [[Panel::Canvas]]);
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
    assert_eq!(harness.state().ui.docking.layout.panel_state(&Panel::Properties), craft_ui::docking::PanelState::Front);
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
    assert_eq!(dock_panels(&h.state().ui.docking.layout, Side::End), [vec![vec![Panel::Layers], vec![Panel::Properties]]]);
    assert_eq!(h.state().ui.docking.layout.location(&Panel::ToolSets).unwrap(), tools);

    h.state_mut().run("ui.dock", json!({"operation":"float","panel":"layers","rect":[400,250,300,350]})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"move","panel":"properties","target":"layers","zone":"center"})).unwrap();
    h.state_mut().run("ui.dock", json!({"operation":"activate","panel":"layers"})).unwrap();
    h.run_steps(3);
    type_command(&mut h, "PROPERTIES");
    assert!(h.state().ui.docking.layout.floating.iter().any(|group| group.contains(&Panel::Properties)));
    assert_eq!(h.state().ui.docking.layout.panel_state(&Panel::Properties), craft_ui::docking::PanelState::Front);
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
    assert_eq!(h.state().ui.docking.layout, expected.layout);
    assert_eq!(h.state().ui.docking.hidden, expected.hidden);
    assert_eq!(h.state().session.doc().unwrap(), &drawing);
}

#[test]
fn canvas_reset_during_column_drag_discards_old_resize_and_ends_gesture() {
    use craft_ui::docking::Action;
    let mut h = harness(None, 1280.0, 1.0);
    // Give the Tool Sets column a width different from Reset's.
    h.state_mut()
        .run("ui.dock", json!({"action": serde_json::to_value(Action::ResizeColumn { panel: Panel::ToolSets, width: 280.0 }).unwrap()}))
        .unwrap();
    h.run_steps(3);
    let drawing = h.state().session.doc().unwrap().clone();
    let divider = Hit::Divider(Divider::Column(Panel::ToolSets));
    let handle = DockArea::<Panel>::hit_id(area(), &divider);
    let from = hit(&h, divider).center();
    h.event(Event::PointerMoved(from));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    assert_eq!(h.ctx.dragged_id(), Some(handle), "the column's divider owns the pointer");
    assert!(h.ctx.input(|input| input.pointer.primary_down()));
    // A divider takes keyboard focus on press. Release only that keyboard focus so the real
    // Canvas command line can submit while the pointer remains captured.
    h.ctx.memory_mut(|memory| memory.surrender_focus(handle));
    assert!(!h.ctx.egui_wants_keyboard_input());
    let to = from + vec2(36.0, 0.0);
    h.event(Event::PointerMoved(to));
    h.event(Event::Text("ui.dock.reset".into()));
    h.event(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
    let expected = cadcraft_ui_egui::docking::default_workspace();
    assert!(h.state().cmd.buffer.is_empty(), "Canvas submitted the actual reset command");
    assert_eq!(h.state().ui.docking.layout, expected.layout, "old-frame resize must not modify Reset's layout");
    assert_eq!(h.state().ui.docking.hidden, expected.hidden);
    // Keep the physical button down: replacement must end the old divider gesture rather than
    // letting it go on resizing the new workspace.
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

#[test]
fn palettes_dock_only_in_the_left_and_right_columns() {
    let mut h = harness(None, 1280.0, 1.0);
    let document = h.state().session.doc().unwrap().clone();
    let drawing = h.state().canvas.rect.expect("the drawing view is drawn");
    // Neither the drawing nor the bottom edge takes a drop: the palette floats there.
    for to in [drawing.center(), egui::pos2(drawing.center().x, drawing.bottom() - 4.0)] {
        h.state_mut().run("ui.dock.reset", json!({})).unwrap();
        h.run_steps(3);
        let from = hit(&h, Hit::Tab(Panel::Layers)).center();
        drag(&mut h, from, to);
        let layout = &h.state().ui.docking.layout;
        assert_eq!(frames(layout), [[Panel::Canvas]], "{to:?}: {:?}", layout.root);
        assert_eq!(dock_panels(layout, Side::End), [[[Panel::Properties]]], "{to:?}");
        assert!(layout.floating.iter().any(|window| window.contains(&Panel::Layers)), "{to:?}: {layout:?}");
    }
    // The left window edge, where Tool Sets docks, takes the palette as a new outer column.
    h.state_mut().run("ui.dock.reset", json!({})).unwrap();
    h.run_steps(3);
    let column = hit(&h, Hit::ColumnBar(Panel::ToolSets));
    let from = hit(&h, Hit::Tab(Panel::Layers)).center();
    drag(&mut h, from, egui::pos2(column.left() + 3.0, drawing.center().y));
    let layout = &h.state().ui.docking.layout;
    assert_eq!(dock_panels(layout, Side::Start), [[[Panel::Layers]], [[Panel::ToolSets]]], "{:?}", layout.root);
    assert_eq!(frames(layout), [[Panel::Canvas]]);
    // Without a left dock, the left window edge makes one.
    h.state_mut().run("ui.dock.reset", json!({})).unwrap();
    h.state_mut().run("ui.toggle.toolsets", json!({"on": false})).unwrap();
    h.run_steps(4);
    assert!(columns(&h.state().ui.docking.layout, Side::Start).iter().all(|column| column.groups.is_empty()));
    let drawing = h.state().canvas.rect.unwrap();
    let from = hit(&h, Hit::Tab(Panel::Layers)).center();
    drag(&mut h, from, egui::pos2(drawing.left() + 3.0, drawing.center().y));
    let layout = &h.state().ui.docking.layout;
    assert_eq!(dock_panels(layout, Side::Start), [[[Panel::Layers]]], "{:?}", layout.root);
    assert_eq!(frames(layout), [[Panel::Canvas]]);
    assert_eq!(h.state().session.doc().unwrap(), &document);
}

#[test]
fn group_drag_bar_moves_the_whole_group_into_the_other_dock() {
    let mut h = harness(None, 1280.0, 1.0);
    h.state_mut().run("ui.dock", json!({"operation":"move", "panel":"layers", "target":"toolSets", "zone":"center", "before":null})).unwrap();
    h.run_steps(4);
    let bar = DockArea::<Panel>::hit_rects(&h.ctx, area())
        .into_iter()
        .find(|(hit, _)| matches!(hit, Hit::DragBar(panel) if *panel == Panel::ToolSets || *panel == Panel::Layers))
        .map(|(_, rect)| rect)
        .expect("the Tool Sets group draws a drag bar");
    // Into the middle of the Properties group, as tabs.
    let properties = hit(&h, Hit::Tab(Panel::Properties));
    let column = hit(&h, Hit::ColumnBar(Panel::Properties));
    let to = egui::pos2(column.center().x, properties.bottom() + 120.0);
    let document = h.state().session.doc().unwrap().clone();
    drag(&mut h, bar.center(), to);
    let layout = &h.state().ui.docking.layout;
    assert!(layout.floating.is_empty(), "{layout:?}");
    let end = dock_panels(layout, Side::End);
    assert_eq!(end.len(), 1, "{end:?}");
    assert!(end[0].iter().any(|group| group.contains(&Panel::ToolSets) && group.contains(&Panel::Layers)), "{end:?}");
    assert_eq!(frames(layout), [[Panel::Canvas]]);
    assert_eq!(h.state().session.doc().unwrap(), &document);
}
