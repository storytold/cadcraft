//! Ribbon visibility is independent of drawing state and command execution.
use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, ribbon::TABS};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// Invalid remote customization must leave every UI preference unchanged.
#[test]
fn ui_set_rejects_invalid_ribbon_visibility_atomically() {
    use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
    let mut app = CadApp::new(Session::default(), Services::default());
    let ctx = egui::Context::default();
    let (request, _) = ControlRequest::new("ui.set", json!({"ribbonVisibility":{"hiddenTabs":["home"]}}));
    assert!(matches!(handle(&mut app, &ctx, &request), Outcome::Done(v) if v["ok"] == true));
    let before = serde_json::to_value(&app.ui).unwrap();
    for invalid in [
        json!({"hiddenTabs":["home","home"]}),
        json!({"hiddenTabs":["unknown"]}),
        json!({"hiddenTabs":vec!["home"; TABS.len() + 1]}),
        json!({"hiddenTabs":"home"}),
        json!({"hiddenPanels":[["home","Draw"],["home","Draw"]]}),
        json!({"hiddenPanels":[["home","unknown"]]}),
        json!({"unexpected":true}),
    ] {
        let (request, _) = ControlRequest::new("ui.set", json!({"ribbonVisibility":invalid,"showToolbar":!app.ui.show_toolbar}));
        assert!(matches!(handle(&mut app, &ctx, &request), Outcome::Done(v) if v["ok"] == false));
        assert_eq!(serde_json::to_value(&app.ui).unwrap(), before);
    }
    let mut restored = CadApp::new(Session::default(), Services::default());
    restored.load_prefs(&app.prefs_json());
    assert_eq!(restored.ui.ribbon_visibility.hidden_tabs, ["home"]);
}

#[test]
fn visibility_is_atomic_persistent_and_recoverable_without_cancelling_line() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.session.start("line").unwrap();
    let before = app.session.doc().unwrap().clone();
    app.run("ui.ribbon.visibility", json!({"tab":"home","panel":"Draw","on":false})).unwrap();
    assert!(!app.ui.ribbon_visibility.panel_visible("home", "Draw"));
    for tab in TABS {
        app.run("ui.ribbon.visibility", json!({"tab":tab.id,"on":false})).unwrap();
    }
    let hidden = app.ui.ribbon_visibility.clone();
    for params in [json!({"tab":"missing","on":true}), json!({"tab":"home","panel":"missing","on":true}), json!({"tab":"home","on":"false"})] {
        assert!(app.run("ui.ribbon.visibility", params).is_err());
        assert_eq!(app.ui.ribbon_visibility, hidden);
    }
    let mut restored = CadApp::new(Session::default(), Services::default());
    restored.load_prefs(&app.prefs_json());
    assert_eq!(restored.ui.ribbon_visibility, hidden);
    for malformed in [json!({"hiddenTabs":["home","home"]}), json!({"hiddenPanels":[["home","missing"]]}), json!({"hiddenTabs":"home"})] {
        restored.load_prefs(&json!({"ribbonVisibility":malformed}).to_string());
        assert_eq!(restored.ui.ribbon_visibility, hidden);
    }
    app.run("ui.resetpalettes", json!({})).unwrap();
    assert_eq!(app.ui.ribbon_visibility, hidden);
    app.run("ui.ribbon.visibility.reset", json!({})).unwrap();
    assert!(TABS.iter().all(|tab| app.ui.ribbon_visibility.tab_visible(tab.id)));
    assert!(app.ui.ribbon_visibility.panel_visible("home", "Draw"));
    assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    assert_eq!(app.session.doc().unwrap(), &before);
}

#[test]
fn customization_remains_reachable_with_all_tabs_hidden() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.show_ribbon = true;
    for tab in TABS {
        app.run("ui.ribbon.visibility", json!({"tab":tab.id,"on":false})).unwrap();
    }
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h.get_by_label("Ribbon Customization").click();
    h.run_steps(4);
    h.get_by_label("Show All Ribbon Items").click();
    h.run_steps(4);
    assert!(TABS.iter().all(|tab| h.state().ui.ribbon_visibility.tab_visible(tab.id)));
    assert!(h.state().ui.show_ribbon);
}

#[test]
fn customization_remains_reachable_beside_tabs_in_a_narrow_window() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.ui.show_ribbon = true;
    let mut h = Harness::builder().with_size(egui::vec2(500.0, 600.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h.get_by_label("Ribbon Customization").click();
    h.run_steps(4);
    assert!(h.query_by_label("Show All Ribbon Items").is_some());
    assert_eq!(h.state().ui.ribbon_tab, "home");
}
