//! The macOS menu bar takes the Edit menu's key equivalents before the window does: with a text
//! field focused they edit the field, and only without one do they run the drawing command.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, menus};
use egui::{accesskit::Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

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
    h.ctx.set_os(egui::os::OperatingSystem::Mac);
    h.run_steps(5);
    h
}

/// A native menu choice, then the frames that deliver what it queued for the main window.
fn menu(h: &mut Harness<'_, CadApp>, id: &str) {
    let ctx = h.ctx.clone();
    menus::activate_native(h.state_mut(), &ctx, id);
    let mut raw = egui::RawInput::default();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    h.input_mut().events.extend(raw.events);
    h.run_steps(3);
}

fn expression<'a>(h: &'a Harness<'_, CadApp>) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(Role::Tab, "QuickCalc").parent().unwrap().get_all_by_role(Role::TextInput).next().unwrap()
}

#[test]
fn edit_menu_keys_edit_the_focused_text_field_and_otherwise_the_drawing() {
    let mut h = harness();
    h.state_mut().run("line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let drawing = h.state().session.doc().unwrap().clone();
    menus::activate(h.state_mut(), "ui.toggle.quickcalc");
    h.run_steps(5);
    expression(&h).click();
    h.run_steps(3);
    expression(&h).type_text("2+3");
    h.run_steps(3);
    assert_eq!(menus::text_field_window(&h.ctx), Some(egui::ViewportId::ROOT));
    assert!(menus::edits_text("pasteclip") && !menus::edits_text("line"));
    menu(&mut h, "selectall");
    menu(&mut h, "cutclip");
    assert_eq!(expression(&h).value().as_deref(), Some(""), "Select All and Cut act on the field");
    assert!(h.state().session.running.is_none());
    assert_eq!(h.state().session.doc().unwrap(), &drawing);

    h.ctx.memory_mut(|m| m.stop_text_input());
    h.run_steps(3);
    assert_eq!(menus::text_field_window(&h.ctx), None);
    menu(&mut h, "u");
    assert_ne!(h.state().session.doc().unwrap(), &drawing, "without a focused field, Undo undoes the line");
}
