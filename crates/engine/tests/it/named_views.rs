//! Named views (#463): VIEW / -VIEW Save, Window, Restore, Delete, ? and the layer snapshot
//! setting; Zoom Realtime and Dynamic; ZOOMFACTOR. (The DXF round trip of views is tested in
//! `cadcraft-io`'s `view_tables_roundtrip`.)

use cadcraft_engine::doc::Space;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Session, View};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.viewport_px = (1200.0, 800.0);
    s.execute("line", &json!({ "points": [[0, 0], [100, 50]] })).unwrap();
    s
}

/// Type each line at the command line.
fn typed(s: &mut Session, lines: &[&str]) {
    for l in lines {
        s.cmdline(l).unwrap();
    }
}

fn view(s: &Session) -> View {
    s.state().unwrap().view()
}

fn set_view(s: &mut Session, cx: f64, cy: f64, h: f64) {
    s.execute("view.set", &json!({ "center": [cx, cy], "height": h })).unwrap();
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9 * (1.0 + a.abs().max(b.abs()))
}

#[test]
fn typed_save_restore_list_delete() {
    let mut s = session();
    set_view(&mut s, 10.0, 20.0, 30.0);
    typed(&mut s, &["-view", "s", "Front"]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let v = s.doc().unwrap().views.first().cloned().unwrap();
    assert_eq!((v.name.as_str(), v.center, v.height, v.layout.as_deref()), ("Front", Vec2::new(10.0, 20.0), 30.0, None));
    assert!(close(v.width, 45.0), "width {}", v.width);
    assert!(v.layer_state.is_some(), "a layer snapshot by default");
    // Saving is an undo step; a view change is not.
    set_view(&mut s, 0.0, 0.0, 500.0);
    typed(&mut s, &["view", "r", "front"]);
    assert_eq!(view(&s), View { center: Vec2::new(10.0, 20.0), height: 30.0 });
    // Zoom Previous goes back to the view before the restore.
    s.execute("zoom.previous", &json!({})).unwrap();
    assert_eq!(view(&s).height, 500.0);
    typed(&mut s, &["-view", "?", ""]);
    assert!(s.log.iter().any(|l| l.contains("Front") && l.contains("Model")), "{:?}", s.log);
    typed(&mut s, &["-view", "d", "fr*"]);
    assert!(s.doc().unwrap().views.is_empty());
    assert!(s.doc().unwrap().layer_states.is_empty(), "the snapshot goes with the view");
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().views.len(), 1);
}

#[test]
fn window_option_and_json_forms() {
    let mut s = session();
    typed(&mut s, &["-view", "w", "Plan", "0,0", "40,10"]);
    let v = s.doc().unwrap().views.first().cloned().unwrap();
    assert_eq!((v.center, v.height, v.width), (Vec2::new(20.0, 5.0), 10.0, 40.0));
    // Restoring fits the whole window: the width decides here (40 / 1.5 > 10).
    s.execute("view", &json!({ "option": "restore", "name": "PLAN" })).unwrap();
    let r = view(&s);
    assert_eq!(r.center, Vec2::new(20.0, 5.0));
    assert!(close(r.height, 40.0 / 1.5), "{}", r.height);
    // JSON save replaces a view of the same name; list reports it.
    s.execute("view", &json!({ "option": "save", "name": "plan", "layerSnapshot": false })).unwrap();
    let list = s.execute("view", &json!({ "option": "list" })).unwrap();
    let rows = list["views"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0]["name"].as_str(), rows[0]["layerSnapshot"].as_bool()), (Some("plan"), Some(false)));
    // Bad names and missing views are errors, not panics.
    for bad in [json!({ "option": "save", "name": "a*b" }), json!({ "option": "save" }), json!({ "option": "restore", "name": "nope" })] {
        assert!(s.execute("view", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("view", &json!({ "option": "window", "name": "Flat", "p1": [0, 0], "p2": [10, 0] })).is_err());
    assert!(s.execute("view", &json!({ "option": "orthographic" })).is_err());
}

#[test]
fn layer_snapshot_restores_layers_and_the_setting_turns_it_off() {
    let mut s = session();
    s.doc_mut().unwrap().layers.push(cadcraft_engine::doc::Layer { name: "Walls".into(), ..Default::default() });
    let walls_on = |s: &Session| s.doc().unwrap().layer("Walls").map(|l| l.on);
    typed(&mut s, &["-view", "s", "All"]);
    s.doc_mut().unwrap().layer_mut("Walls").unwrap().on = false;
    typed(&mut s, &["-view", "r", "All"]);
    assert_eq!(walls_on(&s), Some(true));
    // sEttings > Layer snapshot > No: views are saved without one.
    typed(&mut s, &["-view", "e", "l", "n"]);
    assert!(!s.settings.view_layer_snapshot);
    typed(&mut s, &["-view", "s", "Bare"]);
    let bare = s.doc().unwrap().views.iter().find(|v| v.name == "Bare").cloned().unwrap();
    assert_eq!(bare.layer_state, None);
}

#[test]
fn paper_space_view_switches_to_its_layout() {
    let mut s = session();
    let layout = s.doc().unwrap().layouts.first().map(|l| l.name.clone()).unwrap();
    s.execute("layout.set", &json!({ "name": layout })).unwrap();
    set_view(&mut s, 5.0, 4.0, 6.0);
    s.execute("view", &json!({ "option": "save", "name": "Sheet" })).unwrap();
    assert_eq!(s.doc().unwrap().views.first().and_then(|v| v.layout.clone()), Some(layout.clone()));
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    s.execute("view", &json!({ "option": "restore", "name": "Sheet" })).unwrap();
    assert_eq!(s.layout_space(), Space::Paper(layout));
    assert_eq!(view(&s), View { center: Vec2::new(5.0, 4.0), height: 6.0 });
}

#[test]
fn zoom_realtime_drags_and_enter_ends() {
    let mut s = session();
    set_view(&mut s, 0.0, 0.0, 100.0);
    s.state_mut().unwrap().view_history.clear();
    // Enter at ZOOM's first prompt starts Realtime.
    typed(&mut s, &["zoom", ""]);
    assert!(s.realtime_zoom_active(), "{}", s.prompt_text());
    // Up half the height (400 of 800 px): 2x in; then down a quarter: back by √2.
    s.realtime_zoom(-400.0).unwrap();
    assert!(close(view(&s).height, 50.0), "{}", view(&s).height);
    s.realtime_zoom(200.0).unwrap();
    assert!(close(view(&s).height, 50.0 * 2f64.sqrt()), "{}", view(&s).height);
    s.realtime_zoom(f64::NAN).unwrap();
    typed(&mut s, &[""]);
    assert!(s.running.is_none() && !s.realtime_zoom_active());
    // Outside Realtime a drag does nothing; Zoom Previous returns to the view before the drags.
    s.realtime_zoom(-400.0).unwrap();
    assert!(close(view(&s).height, 50.0 * 2f64.sqrt()));
    s.execute("zoom.previous", &json!({})).unwrap();
    assert_eq!(view(&s).height, 100.0);
    // The JSON form zooms by a factor about the centre.
    s.execute("zoom.realtime", &json!({ "factor": 4 })).unwrap();
    assert_eq!(view(&s), View { center: Vec2::ZERO, height: 25.0 });
}

#[test]
fn zoom_dynamic_places_and_sizes_the_view_box() {
    let mut s = session();
    set_view(&mut s, 10.0, 10.0, 20.0);
    typed(&mut s, &["zoom", "d"]);
    // The overview shows the extents (0,0 to 100,50) and the old view.
    let over = view(&s);
    assert!(over.height > 50.0, "{over:?}");
    assert!(!s.preview(Vec2::new(50.0, 25.0)).is_empty());
    // Place the box at 60,30, size it to 60 wide (left edge 60 - 15 = 45 → 105), accept.
    typed(&mut s, &["60,30", "105,0", ""]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let v = view(&s);
    assert!(close(v.center.x, 75.0) && close(v.center.y, 30.0), "{v:?}");
    assert!(close(v.height, 40.0), "{v:?}");
    // Zoom Previous skips the overview.
    s.execute("zoom.previous", &json!({})).unwrap();
    assert_eq!(view(&s), View { center: Vec2::new(10.0, 10.0), height: 20.0 });
    // JSON: like Center.
    s.execute("zoom.dynamic", &json!({ "center": [1, 2], "height": 3 })).unwrap();
    assert_eq!(view(&s), View { center: Vec2::new(1.0, 2.0), height: 3.0 });
}

#[test]
fn zoomfactor_scales_the_wheel() {
    let mut s = session();
    assert_eq!(s.execute("getvar", &json!({ "name": "ZOOMFACTOR" })).unwrap()["value"], json!(60));
    let at60 = s.wheel_zoom_factor(50.0);
    for bad in [json!(2), json!(101), json!("x")] {
        assert!(s.execute("setvar", &json!({ "name": "ZOOMFACTOR", "value": bad })).is_err());
    }
    s.execute("setvar", &json!({ "name": "ZOOMFACTOR", "value": 100 })).unwrap();
    let at100 = s.wheel_zoom_factor(50.0);
    s.execute("setvar", &json!({ "name": "ZOOMFACTOR", "value": 3 })).unwrap();
    let at3 = s.wheel_zoom_factor(50.0);
    assert!(at100 > at60 && at60 > at3 && at3 > 1.0, "{at3} {at60} {at100}");
    assert!(close(s.wheel_zoom_factor(-50.0), 1.0 / at3));
    assert_eq!(s.wheel_zoom_factor(f64::INFINITY), 1.0);
}
