//! F3 or the status bar's Object Snap button with no running snap set (OSMODE 0): Drafting
//! Settings opens to pick the modes, as AutoCAD shows its Object Snap settings, and the running
//! command keeps going. Picking a mode there turns object snap on.

use cadcraft_engine::Session;
use cadcraft_engine::snap::mode;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let mut app = CadApp::new(s, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn osmode(h: &Harness<'_, CadApp>) -> u32 {
    h.state().session.settings.osmode
}

fn osnap_button_on(h: &Harness<'_, CadApp>) -> bool {
    h.get_by_label("Object Snap (F3)").accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

fn running(h: &Harness<'_, CadApp>) -> Option<String> {
    h.state().session.running.as_ref().map(|r| r.id.clone())
}

#[test]
fn f3_with_modes_still_toggles() {
    let mut h = harness();
    h.state_mut().start("line");
    h.run_steps(2);
    assert!(osnap_button_on(&h));
    h.key_press(Key::F3);
    h.run_steps(2);
    assert_eq!(osmode(&h), 4133 | mode::OFF);
    assert!(!osnap_button_on(&h));
    assert_eq!(h.state().ui.dialog, None, "no dialog when modes are set");
    h.key_press(Key::F3);
    h.run_steps(2);
    assert_eq!(osmode(&h), 4133);
    assert!(osnap_button_on(&h));
    assert_eq!(running(&h).as_deref(), Some("line"));
}

#[test]
fn f3_with_no_running_snaps_opens_drafting_settings() {
    let mut h = harness();
    h.state_mut().start("line");
    h.run_steps(2);
    h.state_mut().run("setvar", json!({"name": "OSMODE", "value": 0})).unwrap();
    h.run_steps(2);
    assert!(!osnap_button_on(&h), "no running snap: shown off");
    // F3 asks to turn object snap on: with nothing to snap to, Drafting Settings opens to pick
    // the modes; LINE keeps running.
    h.key_press(Key::F3);
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("dsettings"));
    assert_eq!(osmode(&h), 0);
    assert_eq!(running(&h).as_deref(), Some("line"));
    // The status bar's button does the same.
    h.state_mut().ui.dialog = None;
    h.run_steps(2);
    h.get_by_label("Object Snap (F3)").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("dsettings"));
    // Picking Endpoint there turns snapping on.
    h.get_by_label("Endpoint").click();
    h.run_steps(3);
    assert_eq!(osmode(&h) & mode::END, mode::END);
    assert!(osnap_button_on(&h));
}

fn dialog_osnap_on(h: &Harness<'_, CadApp>) -> bool {
    h.get_by_label("Object Snap On (F3)").accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn drafting_settings_object_snap_on_follows_the_status_bar() {
    let mut h = harness();
    h.state_mut().run("setvar", json!({"name": "OSMODE", "value": 0})).unwrap();
    h.run_steps(2);
    let n = h.state().session.log.len();
    h.key_press(Key::F3);
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("dsettings"));
    // The command line says it in words, without the JSON form's syntax.
    let echoed = h.state().session.log[n..].to_vec();
    assert!(echoed.iter().any(|l| l.contains("No running object snaps")), "{echoed:?}");
    assert!(echoed.iter().all(|l| !l.contains(['{', '['])), "{echoed:?}");
    // OSMODE 0: the dialog's "Object Snap On (F3)" is off, like the status bar's button.
    assert!(!osnap_button_on(&h));
    assert!(!dialog_osnap_on(&h), "no running mode: not shown on");
    // Picking a mode turns it on, in the dialog too.
    h.get_by_label("Endpoint").click();
    h.run_steps(3);
    assert_eq!(osmode(&h), mode::END);
    assert!(dialog_osnap_on(&h));
    assert!(osnap_button_on(&h));
    // Unticking it sets the F3 bit and keeps the mode; ticking it again clears it.
    h.get_by_label("Object Snap On (F3)").click();
    h.run_steps(3);
    assert_eq!(osmode(&h), mode::END | mode::OFF);
    assert!(!dialog_osnap_on(&h));
    assert!(!osnap_button_on(&h));
    h.get_by_label("Object Snap On (F3)").click();
    h.run_steps(3);
    assert_eq!(osmode(&h), mode::END);
    assert!(dialog_osnap_on(&h));
}
