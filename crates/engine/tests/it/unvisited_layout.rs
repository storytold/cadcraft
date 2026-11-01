use cadcraft_doc::{Drawing, EntityKind, Space};
use cadcraft_engine::Session;
use serde_json::json;

fn session(metric: bool) -> Session {
    let mut s = Session::empty();
    s.new_drawing(metric);
    s.execute("line", &json!({"points": [[10, 20], [110, 70]]})).unwrap();
    s.doc_mut().unwrap().layouts[0].needs_initialization = true;
    s
}

#[test]
fn first_activation_sets_a_landscape_page_and_fits_the_model() {
    for metric in [false, true] {
        let mut s = session(metric);
        let model = s.doc().unwrap().model.clone();
        s.execute("layout.set", &json!({"name": "layout1"})).unwrap();
        let d = s.doc().unwrap();
        let l = &d.layouts[0];
        assert!(!l.needs_initialization);
        assert!(l.page.landscape);
        assert_eq!(l.page.width_mm, if metric { 210.0 } else { 215.9 });
        assert_eq!(d.model, model);
        assert_eq!(l.entities.len(), 1);
        let EntityKind::Viewport(vp) = &l.entities.iter().next().unwrap().kind else { panic!("expected a viewport") };
        assert_eq!(vp.id, 2);
        assert!((vp.view_center.x - 60.0).abs() < 1e-9);
        assert!((vp.view_center.y - 45.0).abs() < 1e-9);
        assert!(vp.view_height >= 50.0);
        assert!(vp.width > vp.height);
        assert!(s.state().unwrap().is_dirty());
        let list = cadcraft_render::build(d, &Space::Paper("Layout1".into()), &cadcraft_render::Options::default());
        assert!(!list.prims.is_empty(), "the model is visible through the viewport");
        s.execute("layout.set", &json!({"name": "Model"})).unwrap();
        s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        assert_eq!(s.doc().unwrap().layouts[0].entities.len(), 1);
    }
}

#[test]
fn an_erased_viewport_is_not_recreated_on_activation() {
    let mut s = session(true);
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    let h = s.doc().unwrap().layouts[0].entities.iter().next().unwrap().handle;
    s.execute("erase", &json!({"handles": [h.hex()]})).unwrap();
    s.execute("layout.set", &json!({"name": "Model"})).unwrap();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    assert!(s.doc().unwrap().layouts[0].entities.is_empty());
}

#[test]
fn initialized_layout_and_its_visible_viewport_survive_saving() {
    let mut s = session(true);
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    let mut formats = vec!["saved.dxf"];
    if !cfg!(target_arch = "wasm32") {
        formats.push("saved.dwg");
    }
    for name in formats {
        let bytes = cadcraft_io::write(s.doc().unwrap(), name).unwrap();
        let d = cadcraft_io::read(&bytes, name).unwrap();
        assert!(!d.layouts[0].needs_initialization, "{name}");
        let mut reopened = Session::new();
        *reopened.doc_mut().unwrap() = d;
        reopened.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        let d = reopened.doc().unwrap();
        assert_eq!(d.layouts[0].entities.len(), 1, "{name}");
        let list = cadcraft_render::build(d, &Space::Paper("Layout1".into()), &cadcraft_render::Options::default());
        assert!(!list.prims.is_empty(), "{name}: the viewport must still show the model");
    }
}

#[test]
fn configured_empty_layouts_are_left_empty_and_custom_pages_are_kept() {
    let mut s = Session::new();
    s.viewport_px = (900.0, 1000.0);
    s.execute("pagesetup", &json!({"layout": "Layout1", "width": 600, "height": 400, "landscape": true})).unwrap();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.layouts[0].entities.is_empty());
    let sheet = cadcraft_render::sheet(d, "Layout1").unwrap();
    assert!(sheet.size.x > sheet.size.y);
    assert_eq!(s.state().unwrap().paper_view().center, sheet.bounds().center());
    assert!(s.state().unwrap().paper_view().height >= sheet.size.x * 1000.0 / 900.0, "the entire sheet fits a narrow drawing area");
    // Previous saved/serialized drawings have no initialization field and remain configured.
    let value = serde_json::to_value(Drawing::new_metric().layouts[0].clone()).unwrap();
    let mut value = value.as_object().unwrap().clone();
    value.remove("needsInitialization");
    let l: cadcraft_doc::Layout = serde_json::from_value(value.into()).unwrap();
    assert!(!l.needs_initialization);
}
