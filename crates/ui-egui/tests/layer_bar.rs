//! Toolbar layer controls exercise the same engine and docking paths as the palettes.
use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, chrome, docking::Panel, host_panel};
use egui::vec2;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use serde_json::json;

fn app(drawing: bool, host: bool, model: bool) -> CadApp {
    let mut app = CadApp::new(Session::empty(), Services::default());
    if drawing {
        app.run("new", json!({})).unwrap();
        app.run("layer.new", json!({"name":"A", "current":true})).unwrap();
        app.ui.start_tab = false;
    }
    app.host_panel.enabled = host;
    if model {
        let model = host_panel::parse(&json!({"type":"cadcraft:panel", "panel":{"title":"Host", "sections":[]}})).unwrap().unwrap();
        app.receive_host_panel(Some(model));
        app.run("ui.dock", json!({"operation":"close", "panel":"host"})).unwrap();
    }
    app
}

fn harness(app: CadApp, width: f32) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(width, 800.0)).build_ui_state(|ui, app: &mut CadApp| chrome::title_and_toolbar(app, ui), app);
    h.run_steps(3);
    h
}

#[test]
fn current_layer_changes_are_undoable() {
    let mut h = harness(app(true, false, false), 2000.0);
    h.get_by_label("Current layer: A").click();
    h.run_steps(3);
    h.get_by_label("0").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().header.str("CLAYER", ""), "0");
    h.state_mut().run("u", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Current layer: A").is_some());
}

#[test]
fn ten_layer_buttons_dispatch_to_the_engine() {
    for (command, label) in [
        ("layoff", "Off"),
        ("layiso", "Isolate"),
        ("layfrz", "Freeze"),
        ("laylck", "Lock"),
        ("laymcur", "Make Current"),
        ("layon", "Turn All Layers On"),
        ("layuniso", "Unisolate"),
        ("laythw", "Thaw All Layers"),
        ("layulk", "Unlock"),
        ("laymch", "Match Layer"),
    ] {
        let mut expected = app(true, false, false);
        expected.start(command);
        let mut h = harness(app(true, false, false), 2000.0);
        h.get_by_label(&format!("{} ({command})", label)).click();
        h.run_steps(3);
        assert_eq!(h.state().session.last_command, expected.session.last_command, "{command}");
        assert_eq!(h.state().session.log, expected.session.log, "{command}");
        assert_eq!(h.state().session.running.as_ref().map(|r| &r.id), expected.session.running.as_ref().map(|r| &r.id), "{command}");
    }
}

#[test]
fn layer_properties_toggles_docking() {
    let mut h = harness(app(true, false, false), 2000.0);
    assert!(h.state().ui.docking.layout.contains(&Panel::Layers));
    for visible in [false, true] {
        h.get_by_label("Layer Properties").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.docking.layout.contains(&Panel::Layers), visible);
    }
}

#[test]
fn parameters_reopens_only_an_available_host_panel() {
    let mut h = harness(app(true, true, true), 2000.0);
    assert!(!h.get_by_label("Parameters").accesskit_node().is_disabled());
    h.get_by_label("Parameters").click();
    h.run_steps(3);
    assert!(h.state().ui.docking.layout.contains(&Panel::Host));
    let h = harness(app(true, true, false), 2000.0);
    assert!(h.get_by_label("Parameters").accesskit_node().is_disabled());
    let h = harness(app(true, false, false), 2000.0);
    assert!(h.query_by_label("Parameters").is_none());
}

#[test]
fn no_drawing_has_no_layer_bar() {
    let h = harness(app(false, true, false), 2000.0);
    assert!(h.query_by_label("Layer Properties").is_none());
    assert!(h.query_by_label("Current layer: A").is_none());
    assert!(h.query_by_label("Off (layoff)").is_none());
    assert!(h.query_by_label("Parameters").is_none());
}

#[test]
fn compact_toolbar_keeps_layer_controls_in_overflow() {
    let mut h = harness(app(true, true, true), 700.0);
    h.get_by_label("More toolbar commands").click();
    h.run_steps(3);
    h.get_by_label("Layer Properties").click();
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout.contains(&Panel::Layers));
}

#[test]
fn frozen_layer_is_disabled_until_thawed_and_toggles_are_undoable() {
    let mut state = app(true, false, false);
    state.run("layer.set", json!({"name":"0", "frozen":true})).unwrap();
    let mut h = harness(state, 2000.0);
    h.get_by_label("Current layer: A").click();
    h.run_steps(3);
    assert!(h.get_by_label("0").accesskit_node().is_disabled());
    let thaw = h.get_all_by_label("Freeze/Thaw").next().unwrap();
    thaw.click();
    h.run_steps(3);
    assert!(!h.state().session.doc().unwrap().layer("0").unwrap().frozen);
    h.state_mut().run("u", json!({})).unwrap();
    assert!(h.state().session.doc().unwrap().layer("0").unwrap().frozen);
}

#[test]
fn layer_bar_visual() {
    if let Ok(path) = std::env::var("CRAFT_LAYER_BAR_SNAPSHOT") {
        let mut h = Harness::builder().with_size(vec2(1500.0, 900.0)).wgpu().build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app(true, true, true),
        );
        h.run_steps(5);
        h.render().unwrap().save(path).unwrap();
    }
}

#[test]
fn overflow_picker_and_command_buttons_remain_interactive() {
    let mut h = harness(app(true, false, false), 700.0);
    h.get_by_label("More toolbar commands").click();
    h.run_steps(3);
    h.get_by_label("Current layer: A").click();
    h.run_steps(3);
    h.get_by_label("0").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().header.str("CLAYER", ""), "0");
    if h.query_by_label("Off (layoff)").is_none() {
        h.get_by_label("More toolbar commands").click();
        h.run_steps(3);
    }
    h.get_by_label("Off (layoff)").click();
    h.run_steps(3);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("layoff"));
}

#[test]
fn toolbar_and_palette_current_layer_pickers_do_not_share_popup_ids() {
    let mut h = Harness::builder().with_size(vec2(2000.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app(true, false, false),
    );
    h.run_steps(4);
    let pickers: Vec<_> = h.get_all_by_label("Current layer: A").collect();
    assert_eq!(pickers.len(), 2);
    pickers.iter().min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().click();
    h.run_steps(3);
    h.get_by_label("0").click();
    h.run_steps(3);
    assert_eq!(h.get_all_by_label("Current layer: 0").count(), 2);
}
