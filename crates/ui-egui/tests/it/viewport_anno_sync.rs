//! The status bar inside a layout viewport (gaps F12): the annotation scale list shows the
//! viewport's annotation scale, and when it differs from the viewport scale a button synchronizes
//! them.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn scale(app: &CadApp, h: Handle) -> f64 {
    match &app.session.doc().unwrap().layout("Layout1").unwrap().entities.get(h).unwrap().kind {
        EntityKind::Viewport(v) => v.height / v.view_height,
        _ => panic!("not a viewport"),
    }
}

#[test]
fn the_sync_button_brings_the_viewport_scale_to_the_annotation_scale() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.ui.start_tab = false;
    app.run("line", json!({"points": [[0, 0], [1000, 0]]})).unwrap();
    app.run("layout.set", json!({"name": "Layout1"})).unwrap();
    let r = app.run("mview", json!({"p1": [20, 20], "p2": [120, 100]})).unwrap();
    let vp = Handle::parse_hex(r["viewports"][0].as_str().unwrap()).unwrap();
    app.run("viewport.set", json!({"handle": vp.hex(), "scale": "1:50"})).unwrap();
    app.run("mspace", json!({"handle": vp.hex()})).unwrap();
    let mut h = harness(app);
    // In step: the list shows the viewport's scale and there is nothing to synchronize.
    assert!(h.query_by_role_and_label(Role::Button, "Annotation scale").is_some());
    assert!(h.query_by_label("Synchronize Viewport Scale").is_none());

    h.state_mut().run("viewport.set", json!({"annoScale": "1:100"})).unwrap();
    h.run_steps(3);
    if let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(format!("{directory}/cad-viewport-anno-sync.png")).unwrap();
    }
    h.get_by_label("Synchronize Viewport Scale").click();
    h.run_steps(3);
    assert!((scale(h.state(), vp) - 0.01).abs() < 1e-12, "{}", scale(h.state(), vp));
    assert!(h.state().session.state().unwrap().active_viewport().is_some(), "still inside the viewport");
    assert!(h.query_by_label("Synchronize Viewport Scale").is_none());

    // On paper (no active viewport) the annotation scale list isn't shown.
    h.state_mut().run("pspace", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Annotation scale").is_none());
}
