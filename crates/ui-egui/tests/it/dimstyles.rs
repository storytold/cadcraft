//! The Dimension Style Manager and editor, and the dimension rows of the Properties palette.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
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

/// The style editor belongs to the drawing it was opened in: switching to another drawing
/// cancels it, so its OK can't write the first drawing's style into the second (#2034 review).
#[test]
fn the_style_editor_is_cancelled_when_another_drawing_becomes_active() {
    let mut h = harness(|app| {
        app.run("dimstyle", json!({ "name": "Standard", "textHeight": 7.0 })).unwrap();
        app.run("new", json!({})).unwrap();
        app.run("dimstyle", json!({ "name": "Standard", "textHeight": 3.0 })).unwrap();
        // The manager opens on the first of the two drawings.
        app.session.active = app.session.docs.len() - 2;
        app.ui.dialog = Some("dimstyle".into());
    });
    click(&mut h, "Standard");
    click(&mut h, "Modify...");
    assert!(h.query_by_label_contains("Modify Dimension Style:").is_some(), "the editor opens");
    // Another drawing becomes active (its file tab), the editor still on screen.
    let other = h.state().session.docs.len() - 1;
    h.state_mut().session.active = other;
    h.run_steps(4);
    assert!(h.query_by_label_contains("Modify Dimension Style:").is_none(), "the editor is cancelled");
    assert!(h.query_all_by_label("OK").next().is_none(), "no OK left to press");
    let second = h.state().session.doc().unwrap().dim_style("Standard").unwrap().text_height;
    assert_eq!(second, 3.0, "the second drawing keeps its own style");
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
