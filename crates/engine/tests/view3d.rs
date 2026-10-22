//! VPOINT and PLAN: model space seen from a direction (a vector or a preset of View ▸ 3D Views),
//! zoomed to the extents of what the view shows; layouts stay in plan.

use cadcraft_engine::Session;
use serde_json::json;

fn session_with_a_column() -> Session {
    let mut s = Session::new();
    // A 2 × 2 square on the ground and a 10 high column at its corner.
    s.execute("line", &json!({"points": [[0, 0], [2, 0], [2, 2], [0, 2], [0, 0]]})).unwrap();
    s.execute("3dpoly", &json!({"points": [[0, 0, 0], [0, 0, 10]]})).unwrap();
    s
}

#[test]
fn a_preset_view_shows_heights_and_zooms_to_them() {
    let mut s = session_with_a_column();
    let r = s.execute("vpoint", &json!({"preset": "front"})).unwrap();
    assert_eq!(r["direction"], json!([0.0, -1.0, 0.0]));
    let st = s.state().unwrap();
    assert!(st.projection().is_some());
    // From the front the column is 10 high and the square 2 wide: the view fits the height.
    let v = st.view();
    assert!((v.center.y - 5.0).abs() < 1e-6 && v.height >= 10.0 && v.height < 12.0, "{v:?}");
    let g = s.execute("view.get", &json!({})).unwrap();
    assert_eq!(g["direction"], json!([0.0, -1.0, 0.0]));

    // An isometric view looks down at the square and up the column.
    s.execute("vpoint.seiso", &json!({})).unwrap();
    let d = s.state().unwrap().view_dir;
    assert!((d.x - d.z).abs() < 1e-12 && (d.x + d.y).abs() < 1e-12 && d.x > 0.0);

    // PLAN goes back to the plan view.
    s.execute("plan", &json!({})).unwrap();
    assert!(s.state().unwrap().projection().is_none());
    let v = s.state().unwrap().view();
    assert!((v.center.x - 1.0).abs() < 1e-6 && (v.center.y - 1.0).abs() < 1e-6, "{v:?}");
}

#[test]
fn bad_directions_and_layouts_are_refused() {
    let mut s = session_with_a_column();
    assert!(s.execute("vpoint", &json!({"direction": [0, 0, 0]})).is_err());
    assert!(s.execute("vpoint", &json!({"direction": [1, 2]})).is_err());
    assert!(s.execute("vpoint", &json!({"preset": "sideways"})).is_err());
    assert!(s.execute("vpoint", &json!({})).is_err());
    s.execute("vpoint", &json!({"direction": [1, 1, 1]})).unwrap();
    // A layout is drawn in plan whatever model space's view is.
    let name = s.doc().unwrap().layouts[0].name.clone();
    s.execute("layout.set", &json!({"name": name})).unwrap();
    assert!(s.state().unwrap().projection().is_none());
    assert!(s.execute("vpoint", &json!({"preset": "top"})).is_err());
}

#[test]
fn orbit_turns_the_view_about_the_centre_and_stops_short_of_the_poles() {
    let mut s = session_with_a_column();
    s.execute("vpoint", &json!({"preset": "front"})).unwrap();
    let before = s.state().unwrap().view();
    // A quarter turn about Z from the front: the view from the right.
    let r = s.execute("3dorbit", &json!({"azimuth": 90})).unwrap();
    let d = &r["direction"];
    assert!((d[0].as_f64().unwrap() - 1.0).abs() < 1e-9 && d[1].as_f64().unwrap().abs() < 1e-9, "{d}");
    assert_eq!(s.view_name(), "Right");
    assert_eq!(s.state().unwrap().view().height, before.height, "orbiting keeps the zoom");
    // Up past the pole: it stops just short of straight down at the model.
    s.execute("3dorbit", &json!({"elevation": 200})).unwrap();
    let z = s.state().unwrap().view_dir.z;
    assert!(z < 1.0 && z > 0.9999, "{z}");
    assert!(s.state().unwrap().projection().is_some());
}

#[test]
fn orbit_follows_drags_until_enter() {
    let mut s = session_with_a_column();
    s.viewport_px = (800.0, 600.0);
    s.cmdline("3dorbit").unwrap();
    assert!(s.orbit_active());
    // From the plan view, a drag starts looking from the south and tilts down to the model.
    s.orbit(0.0, -100.0).unwrap();
    let d = s.state().unwrap().view_dir;
    assert!(d.y < 0.0 && d.z > 0.0 && d.z < 1.0, "{d:?}");
    s.orbit(200.0, 0.0).unwrap();
    assert_ne!(s.state().unwrap().view_dir, d);
    s.cmdline("").unwrap();
    assert!(!s.orbit_active());
}

#[test]
fn the_compass_ring_turns_a_3d_view_and_leaves_the_plan_view() {
    let mut s = session_with_a_column();
    let plan = s.state().unwrap().view();
    s.rotate_view(std::f64::consts::FRAC_PI_2).unwrap();
    assert_eq!((s.view_name(), s.state().unwrap().view()), ("Top", plan));
    s.execute("vpoint", &json!({"preset": "front"})).unwrap();
    s.rotate_view(std::f64::consts::FRAC_PI_2).unwrap();
    assert_eq!(s.view_name(), "Right");
}

#[test]
fn a_drawing_saved_in_a_3d_view_opens_in_it() {
    use cadcraft_engine::cmd::file::{IoHooks, set_io};
    set_io(IoHooks {
        read: |bytes, _| cadcraft_io::read_dxf(bytes).map_err(|e| e.to_string()),
        write: |d, _| Ok(cadcraft_io::write_dxf(d).into_bytes()),
        plot: None,
    });
    let mut s = session_with_a_column();
    s.execute("vpoint", &json!({"preset": "neiso"})).unwrap();
    let path = std::env::temp_dir().join(format!("cadcraft-view3d-{}.dxf", std::process::id()));
    s.execute("saveas", &json!({"path": path.to_string_lossy()})).unwrap();
    assert!(!s.state().unwrap().is_dirty(), "the view doesn't change the drawing");
    s.execute("open", &json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(s.view_name(), "NE Isometric");
    // Saved from a layout, model space keeps its 3D view.
    let name = s.doc().unwrap().layouts[0].name.clone();
    s.execute("layout.set", &json!({"name": name})).unwrap();
    s.execute("saveas", &json!({"path": path.to_string_lossy()})).unwrap();
    s.execute("open", &json!({"path": path.to_string_lossy()})).unwrap();
    s.execute("layout.set", &json!({"name": "Model"})).unwrap();
    assert_eq!(s.view_name(), "NE Isometric");
    // Saved again in plan view, it opens in plan view.
    s.execute("plan", &json!({})).unwrap();
    s.execute("saveas", &json!({"path": path.to_string_lossy()})).unwrap();
    s.execute("open", &json!({"path": path.to_string_lossy()})).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(s.view_name(), "Top");
}

#[test]
fn tiled_model_viewports_stay_in_plan() {
    // Split model space: the tiles draw in plan, so a 3D view is refused, and so is an orbit.
    let mut s = session_with_a_column();
    s.execute("-vports", &json!({"option": "2"})).unwrap();
    assert!(s.model_tiled());
    for (cmd, p) in [("vpoint", json!({"preset": "swiso"})), ("vpoint.front", json!({})), ("3dorbit", json!({"azimuth": 30}))] {
        let e = s.execute(cmd, &p).unwrap_err();
        assert!(e.to_string().contains("single model viewport"), "{cmd}: {e}");
    }
    s.drag_view(100.0, 50.0).unwrap();
    s.rotate_view(0.5).unwrap();
    assert!(s.state().unwrap().projection().is_none(), "the ViewCube and its compass leave tiles in plan");
    // Back to plan is always allowed.
    s.execute("plan", &json!({})).unwrap();
    s.execute("vpoint.top", &json!({})).unwrap();
    // A single viewport takes a 3D view again; from there model space is not split.
    s.execute("-vports", &json!({"option": "single"})).unwrap();
    s.execute("vpoint", &json!({"preset": "swiso"})).unwrap();
    assert!(s.execute("-vports", &json!({"option": "2"})).is_err());
    assert!(!s.model_tiled());
    s.execute("plan", &json!({})).unwrap();
    s.execute("-vports", &json!({"option": "2"})).unwrap();
    assert!(s.model_tiled());
}

#[test]
fn zoom_all_and_an_empty_drawing_in_a_3d_view() {
    // ZOOM All in a 3D view is ZOOM Extents: the plan-view limits don't widen it.
    let mut s = session_with_a_column();
    s.execute("vpoint", &json!({"preset": "front"})).unwrap();
    s.execute("zoom", &json!({"mode": "extents"})).unwrap();
    let extents = s.state().unwrap().view();
    s.execute("zoom", &json!({"mode": "all"})).unwrap();
    assert_eq!(s.state().unwrap().view(), extents);
    // An empty drawing shows its limits (0,0 to 12,9 on the ground) as the view sees them: from
    // the front, a line along the ground.
    let mut s = cadcraft_engine::Session::new();
    s.execute("vpoint", &json!({"preset": "front"})).unwrap();
    let v = s.state().unwrap().view();
    assert!((v.center.x - 6.0).abs() < 1e-9 && v.center.y.abs() < 1e-9, "{v:?}");
}

#[test]
fn named_views_keep_their_3d_direction() {
    let view = |s: &mut Session, p: serde_json::Value| s.execute("view", &p).unwrap();
    let mut s = session_with_a_column();
    let top = s.state().unwrap().view();
    view(&mut s, json!({"option": "save", "name": "Top"}));
    s.execute("vpoint", &json!({"preset": "swiso"})).unwrap();
    let (iso_dir, iso) = (s.state().unwrap().view_dir, s.state().unwrap().view());
    view(&mut s, json!({"option": "save", "name": "Iso"}));
    let saved = s.doc().unwrap().views.iter().find(|v| v.name == "Iso").and_then(|v| v.view3d).unwrap();
    assert_eq!(saved.dir, iso_dir);
    assert!(s.doc().unwrap().views.iter().find(|v| v.name == "Top").unwrap().view3d.is_none());
    // A plan view restored from a 3D view goes back to plan, at its own centre.
    view(&mut s, json!({"option": "restore", "name": "Top"}));
    assert!(s.state().unwrap().projection().is_none());
    assert!((s.state().unwrap().view().center - top.center).len() < 1e-9);
    // A 3D view restored from plan turns the view back to its direction, at its centre.
    view(&mut s, json!({"option": "restore", "name": "Iso"}));
    let st = s.state().unwrap();
    assert_eq!(st.view_dir, iso_dir);
    assert!((st.view().center - iso.center).len() < 1e-9 && (st.view().height - iso.height).abs() < 1e-9, "{:?} vs {iso:?}", st.view());
    // A view kept in its target (as files store it): the centre moves to the editor's coordinates.
    // From the front, a target at (3, 0, 4) is at (3, 4).
    let d = s.doc_mut().unwrap();
    let v = d.views.iter_mut().find(|v| v.name == "Iso").unwrap();
    v.center = cadcraft_engine::geom::Vec2::ZERO;
    v.view3d = Some(cadcraft_engine::doc::ViewDir {
        dir: cadcraft_engine::geom::Vec3::new(0.0, -2.0, 0.0),
        target: cadcraft_engine::geom::Vec3::new(3.0, 0.0, 4.0),
    });
    view(&mut s, json!({"option": "restore", "name": "Iso"}));
    let st = s.state().unwrap();
    assert_eq!(st.view_dir, cadcraft_engine::geom::Vec3::new(0.0, -1.0, 0.0));
    assert!((st.view().center - cadcraft_engine::geom::Vec2::new(3.0, 4.0)).len() < 1e-9, "{:?}", st.view());
    // Tiled model viewports stay in plan: a 3D view isn't restored into them.
    view(&mut s, json!({"option": "restore", "name": "Top"}));
    s.execute("-vports", &json!({"option": "2"})).unwrap();
    assert!(s.execute("view", &json!({"option": "restore", "name": "Iso"})).is_err());
    assert!(s.state().unwrap().projection().is_none());
    // Refused from a layout too, before anything changes: the tab and the selection stay.
    let layout = s.doc().unwrap().layouts[0].name.clone();
    s.execute("layout.set", &json!({"name": layout})).unwrap();
    let sel = s.doc().unwrap().model.iter().map(|e| e.handle).take(1).collect::<Vec<_>>();
    s.set_selection(sel.clone());
    let before = s.layout_space();
    assert!(s.execute("view", &json!({"option": "restore", "name": "Iso"})).is_err());
    assert_eq!(s.layout_space(), before);
    assert_eq!(s.selection(), sel);
}
