//! The drafting toggles on the status bar are transparent, as F7-F12 are: clicking Grid or Snap
//! while drawing a line changes the setting and leaves LINE running (#1108).

use cadcraft_engine::doc::{Drawing, EntityKind, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(Drawing::new_imperial(), "Status bar toggles", None);
    let app = CadApp::new(session, Services::default());
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn running(h: &Harness<'_, CadApp>) -> Option<String> {
    h.state().session.running.as_ref().map(|r| r.id.clone())
}

#[test]
fn status_readout_right_click_opens_customization() {
    let mut h = harness();
    h.state_mut().canvas.cursor = Some(Vec2::new(1.0, 2.0));
    h.run_steps(3);
    let at = egui::pos2(800.0, 987.0);
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(2);
    }
    assert!(h.query_by_label("Show All Status Items").is_some());
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Grid (F7)").click();
    h.run_steps(4);
    assert!(h.state().ui.hidden_status_items.iter().any(|id| id == "grid"));
    assert!(h.query_by_label("Show All Status Items").is_some(), "right-click customization stays open after toggling an item");
}

#[test]
fn status_visibility_is_atomic_persistent_and_separate_from_drafting_settings() {
    use serde_json::json;
    let mut h = harness();
    let settings = h.state().session.settings.clone();
    let dirty = h.state().session.state().unwrap().is_dirty();
    for (item, _) in cadcraft_ui_egui::chrome::status::ITEMS {
        h.state_mut().run("ui.status.item", json!({"item":item,"visible":false})).unwrap();
    }
    let before = h.state().ui.hidden_status_items.clone();
    for params in [json!({"item":"unknown"}), json!({"item":"grid","visible":"no"}), json!({"visible":true})] {
        assert!(h.state_mut().run("ui.status.item", params).is_err());
        assert_eq!(h.state().ui.hidden_status_items, before);
    }
    let restored: cadcraft_ui_egui::UiState = serde_json::from_value(serde_json::to_value(&h.state().ui).unwrap()).unwrap();
    assert_eq!(restored.hidden_status_items, before);
    let prefs = h.state().prefs_json();
    let mut restarted = CadApp::new(Session::new(), Services::default());
    restarted.load_prefs(&prefs);
    assert_eq!(restarted.ui.hidden_status_items, before);
    for invalid in [json!(["grid", "grid"]), json!(["unknown"]), json!([false]), json!("grid")] {
        restarted.load_prefs(&json!({"hiddenStatusItems":invalid}).to_string());
        assert_eq!(restarted.ui.hidden_status_items, before, "invalid preferences must be ignored atomically");
    }
    restarted.load_prefs("{\"hiddenStatusItems\":[]}");
    assert!(restarted.ui.hidden_status_items.is_empty());
    assert!(serde_json::from_value::<cadcraft_ui_egui::UiState>(json!({})).unwrap().hidden_status_items.is_empty());
    assert_eq!(h.state().session.settings.gridmode, settings.gridmode);
    assert_eq!(h.state().session.settings.snapmode, settings.snapmode);
    assert_eq!(h.state().session.state().unwrap().is_dirty(), dirty);
    h.run_steps(4);
    assert!(h.query_by_label("Grid (F7)").is_none());
    h.get_by_label("Status Bar Customization").click();
    h.run_steps(4);
    h.get_by_label("Show All Status Items").click();
    h.run_steps(4);
    assert!(h.state().ui.hidden_status_items.is_empty());
}

#[test]
fn status_customization_does_not_cancel_or_feed_a_point_to_a_running_line() {
    use egui::accesskit::Role;
    let mut h = harness();
    h.state_mut().session.start("line").unwrap();
    h.state_mut().session.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    h.run_steps(3);
    let grid = h.state().session.settings.gridmode;
    h.get_by_label("Status Bar Customization").click();
    h.run_steps(4);
    h.get_by_role_and_label(Role::CheckBox, "Grid (F7)").click();
    h.run_steps(4);
    assert!(h.state().ui.hidden_status_items.iter().any(|id| id == "grid"));
    assert_eq!(h.state().session.settings.gridmode, grid);
    assert_eq!(running(&h).as_deref(), Some("line"));
    h.get_by_label("Show All Status Items").click();
    h.run_steps(3);
    assert_eq!(running(&h).as_deref(), Some("line"));
    h.state_mut().session.input(Input::Point(Vec2::new(10.0, 0.0))).unwrap();
    let count = h.state().session.doc().unwrap().space(&Space::Model).unwrap().iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    assert_eq!(count, 1);
}

/// Resetting side palettes must not reset the independent status preferences.
#[test]
fn status_visibility_survives_palette_reset() {
    let mut h = harness();
    h.state_mut().run("ui.status.item", serde_json::json!({"item":"grid","visible":false})).unwrap();
    h.state_mut().run("ui.resetpalettes", serde_json::json!({})).unwrap();
    h.run_steps(4);
    assert_eq!(h.state().ui.hidden_status_items, ["grid"]);
    assert!(h.query_by_label("Grid (F7)").is_none());
    h.state_mut().run("ui.status.reset", serde_json::json!({})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("Grid (F7)").is_some());
}
/// The generic control route must reject an invalid list before changing any UI field.
#[test]
fn ui_set_rejects_invalid_status_items_atomically() {
    use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
    let mut app = CadApp::new(Session::default(), Services::default());
    let ctx = egui::Context::default();
    let (request, _) = ControlRequest::new("ui.set", json!({"hiddenStatusItems":["grid"]}));
    assert!(matches!(handle(&mut app, &ctx, &request), Outcome::Done(v) if v["ok"] == true));
    let before = serde_json::to_value(&app.ui).unwrap();
    for invalid in [json!(["grid", "grid"]), json!(["unknown"]), json!(vec!["grid"; 19]), json!("grid")] {
        let (request, _) = ControlRequest::new("ui.set", json!({"hiddenStatusItems":invalid,"showToolbar":!app.ui.show_toolbar}));
        assert!(matches!(handle(&mut app, &ctx, &request), Outcome::Done(v) if v["ok"] == false));
        assert_eq!(serde_json::to_value(&app.ui).unwrap(), before);
    }
    let mut restored = CadApp::new(Session::default(), Services::default());
    restored.load_prefs(&app.prefs_json());
    assert_eq!(restored.ui.hidden_status_items, ["grid"]);
}
