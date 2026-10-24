//! Integration tests for the WBLOCK dialog's confirm() path: verify that the file is written and
//! contains the expected entities for Entire Drawing and Objects modes.

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::Session;
use cadcraft_geom::Vec3;
use cadcraft_ui_egui::{CadApp, Services, wblock};
use serde_json::json;

fn setup_io() {
    // Writes a minimal JSON blob so tests can read back entity counts, line endpoints and INSBASE.
    set_io(IoHooks {
        read: |_, _| Ok(Drawing::new_imperial()),
        write: |d, _| {
            let model_n = d.model.len();
            let layout_n: usize = d.layouts.iter().map(|l| l.entities.len()).sum();
            let insbase = d.header.point("INSBASE").unwrap_or_default();
            // Record endpoints of every Line in model space so the base-point translation is
            // verifiable without a full DXF round-trip.
            let lines: Vec<_> = d
                .model
                .iter()
                .filter_map(|e| {
                    if let EntityKind::Line(l) = &e.kind {
                        Some(json!([[l.a.x, l.a.y], [l.b.x, l.b.y]]))
                    } else {
                        None
                    }
                })
                .collect();
            Ok(serde_json::json!({
                "model": model_n,
                "layouts": layout_n,
                "insbase": [insbase.x, insbase.y],
                "lines": lines,
            })
            .to_string()
            .into_bytes())
        },
        plot: None,
    });
}

fn app_with_two_entities() -> (CadApp, cadcraft_doc::Handle, cadcraft_doc::Handle) {
    let mut s = Session::new();
    // h1: line from (0,0) to (10,0) — used in the Objects test.
    let r1 = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let h1 = cadcraft_doc::Handle::parse_hex(r1["handles"][0].as_str().unwrap()).unwrap();
    // h2: second model-space line.
    let r2 = s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
    let h2 = cadcraft_doc::Handle::parse_hex(r2["handles"][0].as_str().unwrap()).unwrap();
    // Add one paper-space entity to verify the whole-drawing mode writes layouts too.
    s.doc_mut()
        .unwrap()
        .add(
            &Space::Paper("Layout1".into()),
            Common::default(),
            EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }),
        )
        .unwrap();
    s.set_selection(Vec::new());
    (CadApp::new(s, Services::default()), h1, h2)
}

fn out_path(label: &str) -> String {
    std::env::temp_dir()
        .join(format!("cadcraft-wblock-{label}-{}.json", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

fn read_json(path: &str) -> serde_json::Value {
    let bytes = std::fs::read(path).expect("wblock should have written the file");
    serde_json::from_slice(&bytes).expect("write hook should produce valid JSON")
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn confirm_entire_drawing_writes_all_entities_and_preserves_insbase() {
    setup_io();
    let (mut app, _h1, _h2) = app_with_two_entities();
    // Set a nonzero INSBASE on the source drawing — whole-drawing mode must preserve it.
    app.run("base", json!({ "at": [7.0, 3.0] })).unwrap();
    let path = out_path("entire");
    app.wblock.source = wblock::Source::EntireDrawing;
    app.wblock.path = path.clone();
    // Exercise confirm() — the same code path the dialog's OK button takes.
    assert!(wblock::confirm(&mut app), "confirm() should succeed: {:?}", app.wblock.error);
    let j = read_json(&path);
    assert_eq!(j["model"].as_u64(), Some(2), "entire-drawing mode must write all model entities");
    assert_eq!(j["layouts"].as_u64(), Some(1), "entire-drawing mode must write layout entities");
    // INSBASE must be preserved as-is (not reset to 0,0).
    let insbase = j["insbase"].as_array().unwrap();
    assert!(approx_eq(insbase[0].as_f64().unwrap(), 7.0), "INSBASE x must be preserved");
    assert!(approx_eq(insbase[1].as_f64().unwrap(), 3.0), "INSBASE y must be preserved");
}

#[test]
fn confirm_objects_with_base_point_translates_entity_coordinates() {
    setup_io();
    let (mut app, h1, _h2) = app_with_two_entities();
    let path = out_path("objects");
    // h1 runs from (0,0) to (10,0). With base point (5,3) the written coords are (-5,-3)→(5,-3).
    app.wblock.source = wblock::Source::Objects;
    app.wblock.picked_handles = vec![h1.hex()];
    app.wblock.base_x = "5".into();
    app.wblock.base_y = "3".into();
    app.wblock.path = path.clone();
    // Exercise confirm() — the same code path the dialog's OK button takes.
    assert!(wblock::confirm(&mut app), "confirm() should succeed: {:?}", app.wblock.error);
    let j = read_json(&path);
    assert_eq!(j["model"].as_u64(), Some(1), "objects mode must write only the selected handle");
    // INSBASE is reset to 0,0 in objects mode (the base subtraction is the offset).
    let insbase = j["insbase"].as_array().unwrap();
    assert!(approx_eq(insbase[0].as_f64().unwrap(), 0.0));
    assert!(approx_eq(insbase[1].as_f64().unwrap(), 0.0));
    // Verify that the base-point translation was actually applied to the entity's coordinates.
    let lines = j["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 1, "one line expected in the written file");
    let a = &lines[0][0];
    let b = &lines[0][1];
    assert!(approx_eq(a[0].as_f64().unwrap(), -5.0), "line.a.x should be 0-5 = -5");
    assert!(approx_eq(a[1].as_f64().unwrap(), -3.0), "line.a.y should be 0-3 = -3");
    assert!(approx_eq(b[0].as_f64().unwrap(),  5.0), "line.b.x should be 10-5 = 5");
    assert!(approx_eq(b[1].as_f64().unwrap(), -3.0), "line.b.y should be 0-3 = -3");
}
