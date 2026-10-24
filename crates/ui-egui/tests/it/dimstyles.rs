//! The Dimension Style Manager and editor, the dimension rows of the Properties palette and the
//! dimension text grip's menu.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(setup: impl FnOnce(&mut CadApp)) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    setup(&mut app);
    let mut h = Harness::builder().with_size(vec2(1280.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn click(h: &mut Harness<'_, CadApp>, label: &str) {
    h.query_all_by_label(label).last().unwrap_or_else(|| panic!("no {label}")).click();
    h.run_steps(4);
}

#[test]
fn format_dimension_style_opens_the_manager_and_new_makes_a_style() {
    let mut h = harness(|app| app.start("dimstyle"));
    assert_eq!(h.state().ui.dialog.as_deref(), Some("dimstyle"));
    assert!(h.query_by_label_contains("Current dimension style:").is_some());
    click(&mut h, "New...");
    click(&mut h, "Continue");
    assert!(h.query_by_label_contains("New Dimension Style:").is_some(), "the editor opens");
    for tab in ["Lines", "Symbols and Arrows", "Text", "Fit", "Primary Units", "Alternate Units", "Tolerances"] {
        click(&mut h, tab);
    }
    click(&mut h, "OK");
    let doc = h.state().session.doc().unwrap();
    assert!(doc.dim_style("Copy of Standard").is_some(), "{:?}", doc.dim_styles.iter().map(|s| &s.name).collect::<Vec<_>>());
    assert_eq!(doc.header.str("DIMSTYLE", "Standard"), "Standard", "a new style is not made current");
}

#[test]
fn modify_and_set_current_and_compare() {
    let mut h = harness(|app| {
        app.run("dimstyle", json!({ "name": "ISO-25", "textHeight": 2.5, "current": false })).unwrap();
        app.ui.dialog = Some("dimstyle".into());
    });
    click(&mut h, "ISO-25");
    click(&mut h, "Set Current");
    assert_eq!(h.state().session.doc().unwrap().header.str("DIMSTYLE", ""), "ISO-25");
    click(&mut h, "Modify...");
    click(&mut h, "OK");
    assert_eq!(h.state().session.doc().unwrap().dim_style("ISO-25").unwrap().text_height, 2.5, "OK without changes keeps the style");
    click(&mut h, "Compare...");
    assert!(h.query_by_label_contains("The two styles are the same.").is_some());
}

#[test]
fn a_selected_dimension_shows_its_style_settings_in_properties() {
    let mut hx = String::new();
    let mut h = harness(|app| {
        hx = app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap()["handle"].as_str().unwrap().to_string();
        let handle = cadcraft_engine::doc::Handle::parse_hex(&hx).unwrap();
        app.session.set_selection(vec![handle]);
        // Room for the Properties palette's rows.
        app.run("ui.dock", json!({ "operation": "close", "panel": "layers" })).unwrap();
    });
    h.run_steps(3);
    // The rows' values: arrowheads, text placement, fit, units (row labels are painted).
    for value in ["Closed filled", "Centered", "Either text or arrows (best fit)", "Decimal"] {
        assert!(h.query_all_by_label_contains(value).next().is_some() || h.query_all_by_value(value).next().is_some(), "{value}");
    }
    // The same overrides the palette sets.
    h.state_mut().run("dimstyle.override", json!({ "handles": [hx], "arrowSize": 0.5 })).unwrap();
    let doc = h.state().session.doc().unwrap();
    let Some(EntityKind::Dimension(d)) = doc.model.iter().find(|e| e.handle.hex() == hx).map(|e| e.kind.clone()) else { panic!() };
    assert_eq!(d.overrides.get("arrowSize"), Some(&json!(0.5)));
}

#[test]
fn resting_on_the_text_grip_opens_its_menu() {
    let mut hx = String::new();
    let mut h = harness(|app| {
        hx = app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap()["handle"].as_str().unwrap().to_string();
        app.run("view.set", json!({ "center": [5, 3], "height": 20 })).unwrap();
        let handle = cadcraft_engine::doc::Handle::parse_hex(&hx).unwrap();
        app.session.set_selection(vec![handle]);
    });
    h.run_steps(3);
    let handle = cadcraft_engine::doc::Handle::parse_hex(&hx).unwrap();
    let grip = h.state().session.grips_of(handle).unwrap()[3];
    let at: Pos2 = h.state().canvas.xf.unwrap().to_screen(grip);
    h.event(Event::PointerMoved(at));
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    h.run_steps(2);
    assert!(h.query_by_label("Move Text Only").is_some(), "the menu opened on hover");
    let item = h.get_by_label("Move Text Only").rect().center();
    h.event(Event::PointerMoved(item));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: item, button: PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.event(Event::PointerButton { pos: item, button: PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
    assert!(h.state().session.running.is_some(), "the grip is hot, asking for the text location");
    assert!(h.state().session.prompt_text().contains("MOVE TEXT ONLY"), "{}", h.state().session.prompt_text());
}
