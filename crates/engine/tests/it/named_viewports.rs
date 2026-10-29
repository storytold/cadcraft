//! View > Viewports > Named Viewports...: the saved model space viewport configurations, the one
//! the Model tab shows, and restoring or deleting one (its tiles in the Model tab, layout
//! viewports in a layout).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Space};
use serde_json::json;

/// A drawing with "Two" (two tiles side by side) and "Quad" (four tiles) saved, showing one view.
fn session() -> Session {
    let mut s = Session::new();
    s.viewport_px = (1200.0, 800.0);
    s.execute("line", &json!({ "points": [[0, 0], [100, 50]] })).unwrap();
    s.execute("-vports", &json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    s.execute("-vports", &json!({ "option": "save", "name": "Two" })).unwrap();
    s.execute("-vports", &json!({ "option": "4", "applyTo": "display" })).unwrap();
    s.execute("-vports", &json!({ "option": "save", "name": "Quad" })).unwrap();
    s.execute("-vports", &json!({ "option": "single" })).unwrap();
    s
}

fn rects(s: &Session) -> Vec<[f64; 4]> {
    let mut r: Vec<[f64; 4]> = s.model_tiles().iter().map(|t| [t.min.x, t.min.y, t.max.x, t.max.y]).collect();
    r.sort_by(|a, b| a.partial_cmp(b).unwrap());
    r
}

fn layout_viewports(s: &Session) -> usize {
    let d = s.doc().unwrap();
    d.space(&Space::Paper("Layout1".into())).unwrap().iter().filter(|e| matches!(e.kind, EntityKind::Viewport(_))).count()
}

#[test]
fn named_viewports_lists_restores_and_deletes_in_the_model_tab() {
    let mut s = session();
    let list = s.execute("vports.named", &json!({})).unwrap();
    assert_eq!(list["configurations"], json!([{ "name": "Two", "viewports": 2 }, { "name": "Quad", "viewports": 4 }]));
    assert_eq!(list["current"], json!(null), "one view matches no saved configuration");
    assert_eq!(s.viewport_configuration(), None);

    s.execute("vports.named", &json!({ "name": "two" })).unwrap();
    assert_eq!(rects(&s), vec![[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]]);
    assert_eq!(s.viewport_configuration().as_deref(), Some("Two"));
    assert_eq!(s.execute("vports.named", &json!({})).unwrap()["current"], json!("Two"));
    s.undo().unwrap();
    assert!(rects(&s).is_empty(), "restoring is one undo step");

    let r = s.execute("vports.named", &json!({ "name": "QUAD", "delete": true })).unwrap();
    assert_eq!(r["deleted"], json!("Quad"));
    let names: Vec<String> = s.doc().unwrap().vports.iter().filter(|c| !c.is_active()).map(|c| c.name.clone()).collect();
    assert_eq!(names, vec!["Two".to_string()]);
    s.undo().unwrap();
    assert!(s.doc().unwrap().vports.iter().any(|c| c.name == "Quad"), "deleting is one undo step");
}

#[test]
fn named_viewports_refuses_unknown_names_and_a_delete_that_is_not_a_boolean() {
    let mut s = session();
    for bad in [
        json!({ "name": "nope" }),
        json!({ "name": "*Active" }),
        json!({ "name": "Two", "delete": "yes" }),
        json!({ "name": "nope", "delete": true }),
    ] {
        assert!(s.execute("vports.named", &bad).is_err(), "{bad}");
    }
    // Only the exact name is deleted, never a wildcard match.
    assert!(s.execute("vports.named", &json!({ "name": "*", "delete": true })).is_err());
    assert_eq!(s.doc().unwrap().vports.iter().filter(|c| !c.is_active()).count(), 2);
}

#[test]
fn named_viewports_places_a_configuration_in_a_layout() {
    let mut s = session();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let before = layout_viewports(&s);
    let r = s.execute("vports.named", &json!({ "name": "Quad", "p1": [10, 10], "p2": [210, 110] })).unwrap();
    assert_eq!(r["configuration"], json!("Quad"));
    assert_eq!(layout_viewports(&s), before + 4);
    assert!(s.model_tiles().is_empty(), "the Model tab's tiles do not change");
}

#[test]
fn typed_named_viewports_asks_for_the_configuration() {
    let mut s = session();
    s.start("vports.named").unwrap();
    assert!(s.prompt_text().contains("Enter name of viewport configuration to restore"), "{}", s.prompt_text());
    s.cmdline("Two").unwrap();
    assert!(s.running.is_none());
    assert_eq!(rects(&s).len(), 2);

    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let before = layout_viewports(&s);
    s.start("vports.named").unwrap();
    assert!(s.prompt_text().contains("Enter viewport configuration name"), "{}", s.prompt_text());
    s.cmdline("Quad").unwrap();
    assert!(s.prompt_text().contains("Specify first corner"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(layout_viewports(&s), before + 4, "Enter fits the configuration to the printable area");
}
