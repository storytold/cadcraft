//! The External References palette (#525): XREF opens it; its rows and buttons act through the
//! engine's -XREF commands.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Drawing, Entity, EntityKind, Handle, Layer, Line, XrefStatus};
use cadcraft_ui_egui::{CadApp, Services, docking::Panel};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

/// Test drawings: any file holds one line on layer Walls.
fn read(_bytes: &[u8], _name: &str) -> Result<Drawing, String> {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Walls".into(), ..Layer::default() });
    let mut e = Entity::new(Handle(0x60), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(50.0, 20.0, 0.0) }));
    e.common.layer = "Walls".into();
    d.model.push(e);
    Ok(d)
}

fn status(app: &CadApp) -> Option<XrefStatus> {
    app.session.doc().ok()?.block("site").map(|b| b.xref_status)
}

#[test]
fn xref_opens_the_palette_and_its_buttons_manage_the_xref() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    let mut app = CadApp::new(Session::default(), Services::default());
    let dir = TempDir::new("xref-palette");
    app.run("xattach", json!({ "path": dir.file("site.dxf", "site"), "at": [10, 10] })).unwrap();
    assert_eq!(status(&app), Some(XrefStatus::Loaded));
    // Typed (by its alias) it opens the palette instead of listing.
    assert!(app.cmdline("XR").is_none());
    assert!(app.ui.docking.layout.contains(&Panel::ExternalRefs));
    app.ui.in_window_menu = false;
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.get_by_label("site");
    h.get_by_label("Loaded");
    h.get_by_label("Unload").click();
    h.run_steps(3);
    assert_eq!(status(h.state()), Some(XrefStatus::Unloaded));
    h.get_by_label("Unloaded");
    h.get_by_label("Reload").click();
    h.run_steps(3);
    assert_eq!(status(h.state()), Some(XrefStatus::Loaded));
    h.get_by_label("Overlay").click();
    h.run_steps(3);
    assert!(h.state().session.doc().unwrap().block("site").unwrap().xref_overlay);
    h.get_by_label("Detach").click();
    h.run_steps(3);
    assert_eq!(status(h.state()), None);
    h.get_by_label("No external references are attached.");
    // Undo brings it back, and the palette lists it again.
    h.state_mut().session.undo().unwrap();
    h.run_steps(3);
    h.get_by_label("site");
}

#[test]
fn json_xref_lists_and_never_opens_the_palette() {
    let mut app = CadApp::new(Session::default(), Services::default());
    let r = app.run("xref", json!({})).unwrap();
    assert_eq!(r["xrefs"], json!([]));
    assert!(!app.ui.docking.layout.contains(&Panel::ExternalRefs));
}
