//! Interaction and optional offscreen evidence for the application's painted controls.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, icons, theme};
use egui::{Event, Key, Modifiers, PointerButton, Response, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn app() -> CadApp {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("ui.sample", serde_json::json!({})).unwrap();
    app
}

#[derive(Default)]
struct ButtonState {
    selected: bool,
    disabled: bool,
    clicks: usize,
    response: Option<Response>,
}

fn button_frame(ctx: &egui::Context, state: &mut ButtonState, events: Vec<Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
        let response = ui.add_enabled_ui(!state.disabled, |ui| icons::button(ui, icons::Icon::Line, 40.0, "Draw line", state.selected)).inner;
        state.clicks += usize::from(response.clicked());
        state.response = Some(response);
    });
    output.textures_delta.clear();
    output
}

fn key(key: Key, pressed: bool) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }
}

fn pointer(pos: egui::Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

#[test]
fn drafting_button_preserves_multicolor_selection_and_accessibility() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut state = ButtonState { selected: true, ..Default::default() };
    button_frame(&ctx, &mut state, vec![]);
    let center = state.response.as_ref().unwrap().rect.center();
    let out = button_frame(&ctx, &mut state, vec![Event::PointerMoved(center), pointer(center, true)]);
    let t = theme::Tokens::get();
    assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == t.tab_active)));
    assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == t.icon_accent)));
    assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Circle(c) if c.fill == t.icon_point)));
    let update = out.platform_output.accesskit_update.unwrap();
    let node = &update.nodes.iter().find(|(_, n)| n.label() == Some("Draw line")).unwrap().1;
    assert_eq!(node.role(), egui::accesskit::Role::Button);
    assert_eq!(node.toggled(), Some(egui::accesskit::Toggled::True));
    button_frame(&ctx, &mut state, vec![pointer(center, false)]);
    assert_eq!(state.clicks, 1);
    assert!(state.selected, "selection belongs to the caller");
}

#[test]
fn drafting_button_keyboard_disabled_state_and_click_cancellation() {
    for key_code in [Key::Enter, Key::Space] {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut state = ButtonState::default();
        button_frame(&ctx, &mut state, vec![]);
        button_frame(&ctx, &mut state, vec![]);
        let response = state.response.clone().unwrap();
        ctx.memory_mut(|m| m.request_focus(response.id));
        let out = button_frame(&ctx, &mut state, vec![]);
        assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r)
            if r.rect == response.rect.shrink(2.0) && r.stroke.color == theme::Tokens::get().accent)));
        button_frame(&ctx, &mut state, vec![key(key_code, true)]);
        button_frame(&ctx, &mut state, vec![key(key_code, false)]);
        assert_eq!(state.clicks, 1);
        let center = response.rect.center();
        button_frame(&ctx, &mut state, vec![Event::PointerMoved(center), pointer(center, true)]);
        button_frame(&ctx, &mut state, vec![Event::PointerMoved(pos2(300.0, 150.0))]);
        button_frame(&ctx, &mut state, vec![pointer(pos2(300.0, 150.0), false)]);
        assert_eq!(state.clicks, 1, "releasing away must not start a command");
        state.disabled = true;
        button_frame(&ctx, &mut state, vec![Event::PointerMoved(center), pointer(center, true), key(key_code, true)]);
        let out = button_frame(&ctx, &mut state, vec![pointer(center, false), key(key_code, false)]);
        assert_eq!(state.clicks, 1);
        assert!(!state.response.as_ref().unwrap().enabled());
        assert!(out.platform_output.accesskit_update.unwrap().nodes.iter().any(|(_, n)| n.label() == Some("Draw line") && n.is_disabled()));
    }
}

#[test]
fn drafting_palette_pointer_and_keyboard_start_the_same_commands() {
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app(),
    );
    h.run_steps(5);
    h.get_by_label("Line").click();
    h.run_steps(3);
    assert_eq!(h.state().session.running.as_ref().unwrap().id, "line");
    h.state_mut().session.cancel();
    for key in [Key::Enter, Key::Space] {
        h.get_by_label("Circle").focus();
        h.run_steps(2);
        assert!(h.get_by_label("Circle").is_focused());
        h.key_press(key);
        h.run_steps(3);
        assert_eq!(h.state().session.running.as_ref().unwrap().id, "circle");
        h.state_mut().session.cancel();
    }
}

#[test]
fn right_palettes_collapse_arrow_hides_menu() {
    // Install the production font chain before the first pass; the fixture draws the palette
    // directly and would otherwise fall back to egui's fonts instead of the app's chain.
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_context_setup(|ctx| theme::install_fonts(ctx, "en")).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::palettes::right_palettes(app, ui);
        },
        app(),
    );
    h.run_steps(5);
    assert!(h.state().ui.show_palettes);

    // Collapsing palettes on the right hides palettes.
    h.get_by_label("Collapse Palettes").click();
    h.run_steps(3);
    assert!(!h.state().ui.show_palettes);
}

#[test]
fn toolsets_collapse_arrow_hides_menu() {
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::palettes::toolsets(app, ui);
        },
        app(),
    );
    h.run_steps(5);
    assert!(h.state().ui.show_toolsets);

    // Collapsing toolsets on the left hides toolsets.
    h.get_by_label("Collapse Tool Sets").click();
    h.run_steps(3);
    assert!(!h.state().ui.show_toolsets);
}

#[test]
fn shared_controls_visuals() {
    let Some(directory) = std::env::var_os("CRAFT_UI_SUITE_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    for scale in [1.0, 2.0] {
        let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_pixels_per_point(scale).wgpu().build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            app(),
        );
        for (name, size) in [("normal", vec2(1600.0, 1000.0)), ("narrow", vec2(900.0, 600.0))] {
            h.set_size(size);
            h.run_steps(6);
            h.render().unwrap().save(directory.join(format!("cadcraft-{name}-dark-{scale}x.png"))).unwrap();
        }
        let mut controls = Harness::builder().with_size(vec2(340.0, 150.0)).with_pixels_per_point(scale).wgpu().build_ui_state(
            |ui, rects: &mut Vec<egui::Rect>| {
                theme::apply(ui.ctx(), egui::Theme::Dark, false);
                rects.clear();
                for enabled in [true, false] {
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.horizontal(|ui| {
                            for selected in [false, true] {
                                for icon in [icons::Icon::Line, icons::Icon::Circle, icons::Icon::Move] {
                                    rects.push(icons::button(ui, icon, 40.0, "Drafting control", selected).rect);
                                }
                            }
                        });
                    });
                }
            },
            Vec::new(),
        );
        controls.run_steps(4);
        controls.render().unwrap().save(directory.join(format!("cadcraft-controls-dark-{scale}x.png"))).unwrap();
        for (name, index, pressed) in [("hover", 0, false), ("selected-hover", 3, false), ("disabled-hover", 6, false), ("pressed", 0, true)] {
            let center = controls.state()[index].center();
            controls.event(Event::PointerMoved(center));
            if pressed {
                controls.event(pointer(center, true));
            }
            controls.run_steps(2);
            controls.render().unwrap().save(directory.join(format!("cadcraft-controls-{name}-dark-{scale}x.png"))).unwrap();
        }
        controls.event(pointer(controls.state()[0].center(), false));
        controls.event(Event::PointerGone);
        controls.run_steps(2);
        controls.key_press(Key::Tab);
        controls.run_steps(2);
        controls.render().unwrap().save(directory.join(format!("cadcraft-controls-focus-dark-{scale}x.png"))).unwrap();
    }
}
