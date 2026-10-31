//! The dimension shortcut menu: Align Text, Dimension Style (the drawing's styles, then Save as
//! New Style), Precision and Remove Style Overrides.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind};
use cadcraft_ui_egui::menus::Entry;
use cadcraft_ui_egui::{CadApp, Services, context_menu};
use serde_json::json;

/// (label path, id, enabled) of the items, submenu items as "Sub/Item".
fn flatten(es: &[Entry]) -> Vec<(String, String, bool)> {
    fn walk(prefix: &str, es: &[Entry], out: &mut Vec<(String, String, bool)>) {
        for e in es {
            match e {
                Entry::Item { label, id, enabled, .. } => out.push((format!("{prefix}{label}"), id.clone(), *enabled)),
                Entry::Sub { label, children } => walk(&format!("{prefix}{label}/"), children, out),
            }
        }
    }
    let mut out = Vec::new();
    walk("", es, &mut out);
    out
}

fn dims(app: &CadApp) -> Vec<Dimension> {
    app.session.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d.clone()) } else { None }).collect()
}

/// A drawing with styles Standard and Big and a dimension and an angular dimension, all selected.
fn app() -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("dimstyle", json!({ "name": "Big", "current": false })).unwrap();
    app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    app.run("dimangular", json!({ "vertex": [0, 10], "p1": [10, 10], "p2": [0, 20], "at": [3, 13] })).unwrap();
    let all = app.session.doc().unwrap().model.handles();
    app.session.set_selection(all);
    app
}

#[test]
fn selected_dimensions_offer_their_style_precision_and_overrides() {
    let mut app = app();
    assert_eq!(dims(&app).len(), 2, "the angular dimension was made");
    let items = flatten(&context_menu::object_entries(&app));
    let has = |label: &str| items.iter().find(|(l, _, _)| l == label).unwrap_or_else(|| panic!("{label} missing from {items:?}"));
    for label in ["Align Text/Home", "Dimension Style/Standard", "Dimension Style/Big", "Dimension Style/Save as New Style...", "Precision/0.00"] {
        has(label);
    }
    // Nothing to remove until a dimension overrides its style.
    assert!(!has("Remove Style Overrides").2);
    // Precision ▸ 0.00: DIMDEC for the linear dimension, DIMADEC for the angular one.
    context_menu::choose(&mut app, &format!("{}2", context_menu::DIM_PRECISION));
    let d = dims(&app);
    assert_eq!(d[0].overrides.get("decimals"), Some(&json!(2)));
    assert_eq!(d[1].overrides.get("angularDecimals"), Some(&json!(2)));
    let items = flatten(&context_menu::object_entries(&app));
    assert!(items.iter().any(|(l, _, enabled)| l == "Remove Style Overrides" && *enabled));
    // Dimension Style ▸ Big.
    context_menu::choose(&mut app, &format!("{}Big", context_menu::DIM_STYLE));
    assert!(dims(&app).iter().all(|d| d.style == "Big"));
    // Remove Style Overrides acts on the selection.
    context_menu::choose(&mut app, "dimoverride.clear");
    assert!(app.session.running.is_none());
    assert!(dims(&app).iter().all(|d| d.overrides.is_empty()));
}

#[test]
fn the_dimension_menu_is_translated() {
    let app = app();
    let mut missing = Vec::new();
    for (label, id, _) in flatten(&context_menu::object_entries(&app)) {
        let segments: Vec<&str> = label.split('/').collect();
        // Style names are the drawing's own, precisions are numbers.
        let data = id.starts_with(context_menu::DIM_STYLE) || id.starts_with(context_menu::DIM_PRECISION);
        let n = segments.len() - usize::from(data);
        for s in &segments[..n] {
            if cadcraft_ui_egui::i18n::tr("uk", s) == *s {
                missing.push(s.to_string());
            }
        }
    }
    assert!(missing.is_empty(), "untranslated: {missing:?}");
}
