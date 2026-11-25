//! Real key traversal of the custom painted controls, without assigning widget focus.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.integrated_titlebar = true;
    app.ui.in_window_menu = false;
    let mut h = Harness::builder().with_context_setup(cadcraft_ui_egui::framework::install_headless).with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            ui.ctx().set_os(egui::os::OperatingSystem::Mac);
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

/// Focuses the control labelled `label` through its accessibility focus action. Traversal starts from a focused
/// control: with nothing focused, the app's raw input hook turns Tab into command-line text.
fn focus(h: &mut Harness<'_, CadApp>, label: &str) {
    h.get_by_label(label).focus();
    h.run_steps(2);
    assert!(h.get_by_label(label).is_focused(), "{label} takes the focus");
}

fn tab_to(h: &mut Harness<'_, CadApp>, label: &str) {
    for _ in 0..160 {
        if h.query_by_label(label).is_some_and(|node| node.is_focused()) {
            return;
        }
        h.key_press(Key::Tab);
        h.run_steps(2);
    }
    panic!("Tab traversal did not reach {label}");
}

/// Presses Tab with `mods`.
fn press(h: &mut Harness<'_, CadApp>, mods: Modifiers) {
    h.key_press_modifiers(mods, Key::Tab);
    h.run_steps(2);
}

fn focused(h: &Harness<'_, CadApp>) -> Option<egui::Id> {
    h.ctx.memory(|memory| memory.focused())
}

#[test]
fn toolbar_tab_and_shift_tab_move_and_activate_once() {
    let mut h = harness();
    focus(&mut h, "New Drawing (⌘N)");
    h.key_press(Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label("Open (⌘O)").is_focused());
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label("New Drawing (⌘N)").is_focused());
    let before = h.state().session.docs.len();
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().session.docs.len(), before + 1);
}

#[test]
fn toolset_tabs_traverse_and_activate_without_drafting_commands() {
    let mut h = harness();
    focus(&mut h, "New Drawing (⌘N)");
    // From the document tabs, Tab goes over the drawing into the palettes, not back to the toolbar.
    tab_to(&mut h, "Close Drawing1");
    while !h.get_by_label("Drafting").is_focused() {
        press(&mut h, Modifiers::NONE);
        assert!(focused(&h).is_some() && !h.get_by_label("New Drawing (⌘N)").is_focused(), "Tab from the document tabs enters the palettes");
    }
    h.key_press(Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label("Modeling").is_focused());
    h.key_press(Key::Space);
    h.run_steps(3);
    assert_eq!(h.state().ui.toolset_tab, "Modeling");
    assert!(h.state().session.running.is_none());
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label("Drafting").is_focused());
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().ui.toolset_tab, "Drafting");
    tab_to(&mut h, "Line");
    h.key_press(Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label("Polyline").is_focused());
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label("Line").is_focused());
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().session.running.as_ref().map(|command| command.id.as_str()), Some("line"));
}

#[test]
fn document_tabs_have_distinct_keyboard_identity_and_activation() {
    let mut h = harness();
    h.state_mut().run("new", serde_json::json!({})).unwrap();
    h.run_steps(3);
    let title = h.state().session.docs[0].title.clone();
    focus(&mut h, "New Drawing (⌘N)");
    tab_to(&mut h, "Start");
    h.key_press(Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label(&title).is_focused());
    h.key_press(Key::Space);
    h.run_steps(3);
    assert_eq!(h.state().session.active, 0);
    assert!(!h.state().ui.start_tab);
}

/// Tab walks one loop over the whole window, never leaving the focus nowhere, and Shift+Tab walks it back.
#[test]
fn shift_tab_walks_the_tab_loop_backward() {
    let mut h = harness();
    focus(&mut h, "New Drawing (⌘N)");
    let start = focused(&h);
    let mut forward = vec![start];
    loop {
        press(&mut h, Modifiers::NONE);
        assert!(focused(&h).is_some(), "Tab {} keeps a focus", forward.len());
        if focused(&h) == start {
            break;
        }
        forward.push(focused(&h));
        assert!(forward.len() < 400, "Tab returns to its start");
    }
    let mut back = vec![start];
    for _ in 1..forward.len() {
        press(&mut h, Modifiers::SHIFT);
        back.push(focused(&h));
    }
    back[1..].reverse();
    assert_eq!(back, forward);
}

/// The drawing as the focused Tab stop: typing runs commands as with nothing focused.
#[test]
fn the_focused_drawing_leaves_typing_to_the_command_line() {
    let mut h = harness();
    focus(&mut h, "Close Drawing1");
    press(&mut h, Modifiers::NONE);
    let drawing = focused(&h);
    assert!(drawing.is_some(), "Tab from the document tabs stops at the drawing");
    h.event(egui::Event::Text("LINE".into()));
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().session.running.as_ref().map(|command| command.id.as_str()), Some("line"));
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().session.running.is_none(), "Escape cancels it");
    assert_eq!(focused(&h), drawing);
    press(&mut h, Modifiers::NONE);
    assert!(focused(&h).is_some() && focused(&h) != drawing, "Tab moves on from the drawing");
}
