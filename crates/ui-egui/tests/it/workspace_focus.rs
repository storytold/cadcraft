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
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
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

#[test]
fn toolbar_tab_and_shift_tab_skip_title_drag_and_activate_once() {
    let mut h = harness();
    h.key_press(Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label("New Drawing (⌘N)").is_focused(), "the title drag region is not a keyboard control");
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
    tab_to(&mut h, "Drafting");
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
    tab_to(&mut h, "Start");
    h.key_press(Key::Tab);
    h.run_steps(2);
    assert!(h.get_by_label(&title).is_focused());
    h.key_press(Key::Space);
    h.run_steps(3);
    assert_eq!(h.state().session.active, 0);
    assert!(!h.state().ui.start_tab);
}
