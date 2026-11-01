use cadcraft_color::Color;
use cadcraft_doc::Handle;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{HatchPicks, PickMode, Session};
use serde_json::json;

use super::*;

fn session_with_rect() -> (Session, Handle) {
    let mut s = Session::new();
    let r = s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    (s, Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap())
}

use crate::{CadApp, Services};

fn app_with_rect() -> (CadApp, Handle) {
    let (s, r) = session_with_rect();
    (CadApp::new(s, Services::default()), r)
}

#[test]
fn pick_points_hides_the_dialog_and_brings_it_back_with_the_points() {
    let (mut app, _) = app_with_rect();
    open_create(&mut app, false);
    start_pick(&mut app, PickMode::Points);
    assert_eq!(app.ui.dialog, None, "hidden while picking");
    assert!(app.session.prompt_text().contains("Pick internal point"));
    resume(&mut app);
    assert_eq!(app.ui.dialog, None, "still picking");
    app.session.input(cadcraft_engine::Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    app.session.input(cadcraft_engine::Input::Enter).unwrap();
    resume(&mut app);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    let st = app.hatch_dlg.clone().unwrap();
    assert_eq!((st.points, st.picking), (vec![[5.0, 5.0]], None));
}

#[test]
fn escape_brings_the_dialog_back_unchanged() {
    let (mut app, r) = app_with_rect();
    open_create(&mut app, false);
    if let Some(st) = app.hatch_dlg.as_mut() {
        st.handles.push(r);
    }
    let before = app.hatch_dlg.clone();
    start_pick(&mut app, PickMode::Points);
    app.session.input(cadcraft_engine::Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    app.session.cancel();
    resume(&mut app);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    assert_eq!(app.hatch_dlg, before);
}

#[test]
fn switching_drawings_drops_the_dialog() {
    let (mut app, _) = app_with_rect();
    open_create(&mut app, false);
    start_pick(&mut app, PickMode::Points);
    app.session.input(cadcraft_engine::Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    app.session.input(cadcraft_engine::Input::Enter).unwrap();
    app.session.new_drawing(false);
    resume(&mut app);
    assert_eq!(app.ui.dialog, None);
    assert_eq!(app.hatch_dlg, None);
}

#[test]
fn hatch_pick_opens_edit_mode_or_nothing() {
    let (mut app, _) = app_with_rect();
    let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap()["handle"].as_str().unwrap().to_string();
    open_create(&mut app, false);
    start_pick(&mut app, PickMode::Hatch);
    app.session.input(cadcraft_engine::Input::Pick(vec![Handle::parse_hex(&hx).unwrap()])).unwrap();
    resume(&mut app);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    assert_eq!(app.hatch_dlg.clone().unwrap().edit, vec![Handle::parse_hex(&hx).unwrap()]);
    // Nothing picked: no dialog.
    start_pick(&mut app, PickMode::Hatch);
    app.session.input(cadcraft_engine::Input::Enter).unwrap();
    resume(&mut app);
    assert_eq!((app.ui.dialog.clone(), app.hatch_dlg.clone()), (None, None));
}

#[test]
fn create_state_reads_the_header_and_survives_bad_values() {
    let (mut s, _) = session_with_rect();
    for (k, v) in [("HPNAME", json!("BRICK")), ("HPSCALE", json!(2.5)), ("HPANG", json!(30)), ("HPSEPARATE", json!(1)), ("HPASSOC", json!(0))] {
        s.execute("setvar", &json!({ "name": k, "value": v })).unwrap();
    }
    let st = create_state(s.doc().unwrap(), 7, false);
    let f = &st.fill;
    assert_eq!((f.pattern.as_str(), f.scale, f.associative, st.separate, st.uid), ("BRICK", 2.5, false, true, 7));
    assert!((f.angle - 30.0).abs() < 1e-9);
    assert!(st.edit.is_empty() && st.points.is_empty() && st.handles.is_empty());
    assert_eq!(st.fill, st.opened);
    // Hostile header values from a file.
    let h = &mut s.doc_mut().unwrap().header;
    h.set_f64("HPSCALE", f64::NAN);
    h.set_f64("HPANG", f64::INFINITY);
    h.set_i64("HPORIGINMODE", i64::MIN);
    h.set_i64("HPISLANDDETECTION", -5);
    let st = create_state(s.doc().unwrap(), 7, true);
    // An HPORIGINMODE out of 0–5 is "use current", as HATCH reads it.
    assert_eq!((st.fill.scale, st.fill.angle, st.fill.style, st.fill.specified), (1.0, 0.0, 0, false));
    assert!(ORIGIN_MODES.contains(&st.fill.origin_mode.as_str()));
    assert!(st.fill.gradient);
    s.doc_mut().unwrap().header.set_f64("HPSCALE", 0.0);
    s.doc_mut().unwrap().header.set_i64("HPORIGINMODE", 99);
    let st = create_state(s.doc().unwrap(), 7, false);
    assert_eq!((st.fill.scale, st.fill.specified), (1.0, false));
    assert!(ORIGIN_MODES.contains(&st.fill.origin_mode.as_str()));
    s.doc_mut().unwrap().header.set_i64("HPORIGINMODE", 3);
    let st = create_state(s.doc().unwrap(), 7, false);
    assert_eq!((st.fill.specified, st.fill.origin_mode.as_str()), (true, "topRight"));
}

#[test]
fn create_ok_hatches_points_and_objects_in_one_call() {
    let (s, r) = session_with_rect();
    let mut st = create_state(s.doc().unwrap(), 1, false);
    assert_eq!(ok_call(&st), None, "no boundary yet");
    st.points.push([5.0, 5.0]);
    st.handles.push(r);
    st.fill.pattern = "ANSI37".into();
    st.fill.scale = 2.0;
    st.fill.angle = 15.0;
    st.fill.style = 1;
    let (cmd, p) = ok_call(&st).unwrap();
    assert_eq!(cmd, "hatch");
    assert_eq!(p["points"], json!([[5.0, 5.0]]));
    assert_eq!(p["handles"], json!([r.hex()]));
    assert_eq!((p["pattern"].as_str(), p["scale"].as_f64(), p["angle"].as_f64()), (Some("ANSI37"), Some(2.0), Some(15.0)));
    assert_eq!((p["style"].as_str(), p["originMode"].as_str()), (Some("outer"), Some("current")));
    // A clicked origin, then a corner of the extents.
    st.fill.specified = true;
    st.fill.origin = Some([1.0, 2.0]);
    assert_eq!(ok_call(&st).unwrap().1["origin"], json!([1.0, 2.0]));
    st.fill.origin = None;
    st.fill.origin_mode = "topRight".into();
    assert_eq!(ok_call(&st).unwrap().1["originMode"], json!("topRight"));
    // Gradient.
    st.fill.gradient = true;
    st.fill.shape = "SPHERICAL".into();
    st.fill.inverted = true;
    st.fill.color1 = Color::Index(1);
    st.fill.centered = false;
    let (cmd, p) = ok_call(&st).unwrap();
    assert_eq!(cmd, "gradient");
    assert_eq!((p["name"].as_str(), p["color1"].as_str(), p["centered"].as_bool()), (Some("INVSPHERICAL"), Some("Red"), Some(false)));
    assert!(p.get("pattern").is_none());
}

#[test]
fn create_ok_runs_and_makes_one_hatch() {
    let (mut s, _) = session_with_rect();
    s.execute("circle", &json!({ "center": [30, 5], "radius": 3 })).unwrap();
    let c = s.doc().unwrap().model.iter().last().unwrap().handle;
    let mut st = create_state(s.doc().unwrap(), 1, false);
    st.points.push([5.0, 5.0]);
    st.handles.push(c);
    let (cmd, p) = ok_call(&st).unwrap();
    s.execute(cmd, &p).unwrap();
    let n = s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, cadcraft_doc::EntityKind::Hatch(_))).count();
    assert_eq!(n, 1);
}

#[test]
fn edit_ok_sends_only_what_changed() {
    let (mut s, _) = session_with_rect();
    let h = s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "ANSI31", "scale": 2 })).unwrap()["handle"].as_str().unwrap().to_string();
    let h = Handle::parse_hex(&h).unwrap();
    let mut st = edit_state(s.doc().unwrap(), 1, vec![h]).unwrap();
    assert_eq!((st.fill.pattern.as_str(), st.fill.scale, st.edit.clone()), ("ANSI31", 2.0, vec![h]));
    assert_eq!(ok_call(&st), None, "nothing changed: OK runs nothing");
    st.fill.scale = 3.0;
    let (cmd, p) = ok_call(&st).unwrap();
    assert_eq!(cmd, "hatchedit");
    assert_eq!(p, json!({ "handles": [h.hex()], "scale": 3.0 }));
    // To a gradient: the whole gradient, no pattern fields.
    st.fill.scale = 2.0;
    st.fill.gradient = true;
    let (_, p) = ok_call(&st).unwrap();
    assert_eq!(p["gradient"]["name"], json!("LINEAR"));
    assert!(p.get("pattern").is_none() && p.get("scale").is_none());
    s.execute("hatchedit", &p).unwrap();
    // Back from a gradient hatch: the pattern is sent even if its name didn't change.
    let mut st = edit_state(s.doc().unwrap(), 1, vec![h]).unwrap();
    assert!(st.fill.gradient);
    st.fill.gradient = false;
    st.fill.pattern = "ANSI31".into();
    assert_eq!(ok_call(&st).unwrap().1["pattern"], json!("ANSI31"));
    // Not a hatch: no edit state.
    let line = s.execute("line", &json!({ "points": [[20, 0], [30, 0]] })).unwrap()["handles"][0].as_str().unwrap().to_string();
    assert!(edit_state(s.doc().unwrap(), 1, vec![Handle::parse_hex(&line).unwrap()]).is_none());
}

#[test]
fn edit_dialog_sends_boundary_additions_and_removals() {
    let (mut s, outer) = session_with_rect();
    let island = s.execute("rectang", &json!({ "p1": [2, 2], "p2": [4, 4] })).unwrap()["handle"].as_str().unwrap().to_string();
    let h = s.execute("hatch", &json!({ "handles": [outer.hex()] })).unwrap()["handle"].as_str().unwrap().to_string();
    let h = Handle::parse_hex(&h).unwrap();
    let mut st = edit_state(s.doc().unwrap(), 1, vec![h]).unwrap();
    assert_eq!(st.loop_outer, vec![true]);
    merge_picks(&mut st, HatchPicks { mode: PickMode::Objects, handles: vec![Handle::parse_hex(&island).unwrap()], ..HatchPicks::default() });
    let (cmd, p) = ok_call(&st).unwrap();
    assert_eq!(cmd, "hatchedit");
    assert_eq!(p["addBoundary"], json!([island]));
    s.execute(cmd, &p).unwrap();
    assert_eq!(edit_state(s.doc().unwrap(), 1, vec![h]).unwrap().loop_outer, vec![true, false]);

    let mut st = edit_state(s.doc().unwrap(), 1, vec![h]).unwrap();
    st.remove_loops.push(1);
    let (_, p) = ok_call(&st).unwrap();
    assert_eq!(p["removeLoops"], json!([1]));
    s.execute("hatchedit", &p).unwrap();
    assert_eq!(edit_state(s.doc().unwrap(), 1, vec![h]).unwrap().loop_outer, vec![true]);
}

#[test]
fn picks_merge_into_the_state() {
    let (s, r) = session_with_rect();
    let mut st = create_state(s.doc().unwrap(), 1, false);
    st.error = Some("old".into());
    merge_picks(&mut st, HatchPicks { mode: PickMode::Points, points: vec![Vec2::new(5.0, 5.0)], ..HatchPicks::default() });
    merge_picks(&mut st, HatchPicks { mode: PickMode::Objects, handles: vec![r, r], ..HatchPicks::default() });
    merge_picks(&mut st, HatchPicks { mode: PickMode::Objects, handles: vec![r], ..HatchPicks::default() });
    assert_eq!((st.points.clone(), st.handles.clone(), st.error.clone()), (vec![[5.0, 5.0]], vec![r], None));
    merge_picks(&mut st, HatchPicks { mode: PickMode::Origin, origin: Some(Vec2::new(1.0, 2.0)), ..HatchPicks::default() });
    assert_eq!((st.fill.specified, st.fill.origin), (true, Some([1.0, 2.0])));
}

fn frames(app: &mut CadApp) -> egui::Context {
    let ctx = egui::Context::default();
    app.logic(&ctx);
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    // Enough frames for a window that grows each frame to show it.
    for _ in 0..30 {
        let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
        output.textures_delta.clear();
    }
    ctx
}

#[test]
fn the_dialog_fits_on_screen_with_its_buttons() {
    for gradient in [false, true] {
        let (mut app, _) = app_with_rect();
        open_create(&mut app, gradient);
        let ctx = frames(&mut app);
        let r = ctx.memory(|m| m.area_rect(egui::Id::unique("hatch_dialog"))).unwrap();
        // The screen is 1000 points high: the window keeps to its content.
        assert!(r.height() < 700.0, "gradient {gradient}: {r:?}");
    }
}

#[test]
fn the_dialog_draws_both_modes_and_tabs_and_stays_open() {
    let (mut app, _) = app_with_rect();
    // Opened by name with no state: create mode from the header.
    app.ui.dialog = Some("hatch".into());
    frames(&mut app);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    assert!(app.hatch_dlg.as_ref().is_some_and(|s| s.edit.is_empty()));
    if let Some(st) = app.hatch_dlg.as_mut() {
        st.fill.gradient = true;
    }
    frames(&mut app);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap()["handle"].as_str().unwrap().to_string();
    let st = app.session.state().unwrap();
    app.hatch_dlg = edit_state(&st.doc, st.uid, vec![Handle::parse_hex(&hx).unwrap()]);
    frames(&mut app);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
}

#[test]
fn ok_makes_the_hatch_and_closes() {
    let (mut app, _) = app_with_rect();
    open_create(&mut app, false);
    if let Some(st) = app.hatch_dlg.as_mut() {
        st.points.push([5.0, 5.0]);
    }
    assert!(ok(&mut app));
    assert_eq!(app.hatch_dlg, None);
    assert!(app.session.doc().unwrap().model.iter().any(|e| matches!(e.kind, cadcraft_doc::EntityKind::Hatch(_))));
}

#[test]
fn a_failed_ok_keeps_the_dialog_open_with_the_error() {
    let (mut app, r) = app_with_rect();
    open_create(&mut app, false);
    if let Some(st) = app.hatch_dlg.as_mut() {
        st.handles.push(r);
    }
    // The picked boundary is erased before OK.
    app.session.execute("erase", &json!({ "handles": [r.hex()] })).unwrap();
    let n = app.session.doc().unwrap().model.len();
    assert!(!ok(&mut app));
    let st = app.hatch_dlg.clone().unwrap();
    assert!(st.error.is_some_and(|e| !e.is_empty()));
    assert_eq!(app.session.doc().unwrap().model.len(), n, "drawing unchanged");
}

#[test]
fn typed_and_menu_hatch_open_the_dialog_but_dash_and_json_do_not() {
    let (mut app, _) = app_with_rect();
    app.cmdline("hatch");
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    assert!(app.session.running.is_none());
    assert!(!app.hatch_dlg.clone().unwrap().fill.gradient);
    for (typed, gradient) in [("h", false), ("bh", false), ("gradient", true), ("gd", true)] {
        app.ui.dialog = None;
        app.cmdline(typed);
        assert_eq!(app.ui.dialog.as_deref(), Some("hatch"), "{typed}");
        assert_eq!(app.hatch_dlg.clone().unwrap().fill.gradient, gradient, "{typed}");
    }
    app.ui.dialog = None;
    app.start("hatch");
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"), "menu");
    // -HATCH: the command line.
    app.ui.dialog = None;
    app.cmdline("-hatch");
    assert_eq!(app.ui.dialog, None);
    assert!(app.session.prompt_text().contains("Pick internal point"), "{}", app.session.prompt_text());
    app.session.cancel();
    // JSON: runs, never opens.
    app.run("hatch", json!({ "points": [[5, 5]] })).unwrap();
    assert_eq!(app.ui.dialog, None);
    app.cmdline("hatch {\"points\": [[5, 5]]}");
    assert_eq!(app.ui.dialog, None);
    // HPDLGMODE 0: the command line.
    app.run("setvar", json!({ "name": "HPDLGMODE", "value": 0 })).unwrap();
    app.cmdline("hatch");
    assert_eq!(app.ui.dialog, None);
    assert!(app.session.running.is_some());
}

#[test]
fn hatchedit_opens_edit_mode_on_the_selection_or_asks_for_a_hatch() {
    let (mut app, _) = app_with_rect();
    let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]] })).unwrap()["handle"].as_str().unwrap().to_string();
    let h = Handle::parse_hex(&hx).unwrap();
    // Nothing selected: pick one first.
    app.cmdline("hatchedit");
    assert_eq!(app.ui.dialog, None);
    assert!(app.session.prompt_text().contains("Select hatch object"), "{}", app.session.prompt_text());
    app.session.input(cadcraft_engine::Input::Pick(vec![h])).unwrap();
    resume(&mut app);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    assert_eq!(app.hatch_dlg.clone().unwrap().edit, vec![h]);
    // Selected (right-click Hatch Edit, typed HE), also with HPDLGMODE 0.
    app.ui.dialog = None;
    app.hatch_dlg = None;
    app.run("setvar", json!({ "name": "HPDLGMODE", "value": 0 })).unwrap();
    app.session.set_selection(vec![h]);
    app.start("he");
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    // OK applies the change.
    if let Some(st) = app.hatch_dlg.as_mut() {
        st.fill.pattern = "BRICK".into();
    }
    assert!(ok(&mut app));
    let d = app.session.doc().unwrap();
    assert!(matches!(&d.entity(h).unwrap().kind, cadcraft_doc::EntityKind::Hatch(x) if x.pattern == "BRICK"));
    // -HATCHEDIT: the command line (the window, not `ok`, closes the dialog).
    app.ui.dialog = None;
    app.cmdline("-hatchedit");
    assert_eq!(app.ui.dialog, None);
    assert!(app.session.running.is_some());
}

#[test]
fn edit_mode_on_an_out_of_range_hatch_changes_nothing_by_itself() {
    let (mut app, _) = app_with_rect();
    let hx =
        app.session.execute("hatch", &json!({ "points": [[5, 5]], "scale": 5.0e6, "angle": 400 })).unwrap()["handle"].as_str().unwrap().to_string();
    app.session.set_selection(vec![Handle::parse_hex(&hx).unwrap()]);
    open_edit(&mut app);
    frames(&mut app);
    assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
    assert_eq!(app.hatch_dlg.as_ref().and_then(ok_call), None, "the widgets must not clamp the hatch's own values");
}

#[test]
fn switching_to_a_layout_drops_the_picks() {
    let (mut app, r) = app_with_rect();
    open_create(&mut app, false);
    start_pick(&mut app, PickMode::Objects);
    app.session.input(cadcraft_engine::Input::Pick(vec![r])).unwrap();
    app.session.input(cadcraft_engine::Input::Enter).unwrap();
    app.run("layout", json!({ "option": "set", "name": "Layout1" })).unwrap();
    resume(&mut app);
    assert_eq!((app.ui.dialog.clone(), app.hatch_dlg.clone()), (None, None), "model-space picks never hatch a layout");
    // An open dialog closes too.
    app.run("layout", json!({ "option": "set", "name": "Model" })).unwrap();
    open_create(&mut app, false);
    app.run("layout", json!({ "option": "set", "name": "Layout1" })).unwrap();
    frames(&mut app);
    assert_eq!(app.hatch_dlg, None);
}

#[test]
fn a_gradient_hatch_offers_the_current_pattern_on_the_pattern_tab() {
    let (mut s, _) = session_with_rect();
    let h = s.execute("gradient", &json!({ "points": [[5, 5]] })).unwrap()["handle"].as_str().unwrap().to_string();
    let h = Handle::parse_hex(&h).unwrap();
    let mut st = edit_state(s.doc().unwrap(), 1, vec![h]).unwrap();
    assert_eq!(st.fill.pattern, "ANSI31", "HPNAME, not the gradient's SOLID");
    st.fill.gradient = false;
    let (cmd, p) = ok_call(&st).unwrap();
    s.execute(cmd, &p).unwrap();
    match &s.doc().unwrap().entity(h).unwrap().kind {
        cadcraft_doc::EntityKind::Hatch(x) => assert!(x.gradient.is_none() && x.pattern == "ANSI31"),
        k => panic!("{k:?}"),
    }
}
