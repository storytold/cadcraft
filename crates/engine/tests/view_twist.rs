//! The view twist (VIEWTWIST): DVIEW TWist turns the Model tab's view, ZOOM Previous turns it
//! back, each VPORTS tile keeps its own (DXF VPORT group 51), and VIEW Save/Restore keep it.

use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Session, View, sysvars};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.viewport_px = (1200.0, 800.0);
    s.execute("line", &json!({ "points": [[0, 0], [100, 50]] })).unwrap();
    s.execute("view.set", &json!({ "center": [10, 10], "height": 20 })).unwrap();
    s
}

fn typed(s: &mut Session, lines: &[&str]) {
    for l in lines {
        s.cmdline(l).unwrap();
    }
}

fn view(s: &Session) -> View {
    s.state().unwrap().view()
}

fn twist_deg(s: &Session) -> f64 {
    view(s).twist.to_degrees()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn dview_twist_sets_viewtwist_and_zoom_previous_turns_it_back() {
    let mut s = session();
    assert_eq!(view(&s).twist, 0.0);
    typed(&mut s, &["dview", "", "tw", "30", ""]);
    assert!(near(twist_deg(&s), 30.0), "twist {}", twist_deg(&s));
    let vt = sysvars::get(&s, "VIEWTWIST").and_then(|v| v.as_f64()).unwrap();
    assert!(near(vt, 30.0), "VIEWTWIST {vt}");
    // Only the twist changed; the view is not an undo step.
    assert_eq!((view(&s).center, view(&s).height), (Vec2::new(10.0, 10.0), 20.0));
    assert_eq!(s.state().unwrap().undo.len(), 1, "just the line");
    // Zooming keeps the twist; ZOOM Previous steps back through it.
    s.execute("zoom", &json!({ "mode": "in" })).unwrap();
    assert!(near(twist_deg(&s), 30.0));
    s.execute("zoom", &json!({ "mode": "previous" })).unwrap();
    s.execute("zoom", &json!({ "mode": "previous" })).unwrap();
    assert_eq!(view(&s).twist, 0.0);
    assert!(sysvars::is_read_only("VIEWTWIST"));
    // JSON form; negative angles wrap into 0..360.
    s.execute("dview", &json!({ "twist": -90 })).unwrap();
    assert!(near(twist_deg(&s), 270.0));
    // The 3D options are not available yet; the command carries on.
    typed(&mut s, &["dview", "", "ca", ""]);
    assert!(s.current_prompt().is_none());
    // Not in a layout.
    let layout = s.doc().unwrap().layouts.first().map(|l| l.name.clone()).unwrap();
    s.execute("layout.set", &json!({ "name": layout })).unwrap();
    assert!(s.execute("dview", &json!({ "twist": 10 })).is_err());
}

const PAPER: &str = "This command is not available in Paper space.";

#[test]
fn dview_in_paper_space_says_it_is_not_available() {
    let mut s = session();
    let layout = s.doc().unwrap().layouts.first().map(|l| l.name.clone()).unwrap();
    s.execute("layout.set", &json!({ "name": layout })).unwrap();
    let e = s.execute("dview", &json!({ "twist": 10 })).unwrap_err();
    assert_eq!(e.to_string(), PAPER);
    // Typed, it is refused as it starts: no prompt is left open.
    let e = s.cmdline("dview").unwrap_err();
    assert_eq!(e.to_string(), PAPER);
    assert!(s.current_prompt().is_none());
    // view.set can't turn a layout's view either (an untwisted one is fine).
    let e = s.execute("view.set", &json!({ "center": [0, 0], "height": 5, "twist": 15 })).unwrap_err();
    assert_eq!(e.to_string(), PAPER);
    s.execute("view.set", &json!({ "center": [0, 0], "height": 5 })).unwrap();
    assert_eq!(view(&s).twist, 0.0);
}

#[test]
fn a_single_untiled_view_keeps_its_twist_through_a_dxf_round_trip() {
    let mut s = session();
    s.execute("dview", &json!({ "twist": 30 })).unwrap();
    assert!(s.model_tiles().len() <= 1, "untiled");
    let dxf = cadcraft_io::write_dxf(s.doc().unwrap());
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let written = lines.windows(2).any(|w| w[0] == "51" && w[1].parse::<f64>().is_ok_and(|a| near(a, 30.0)));
    assert!(written, "the lone *ACTIVE VPORT carries group 51");
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let mut o = Session::new();
    o.viewport_px = (1200.0, 800.0);
    o.open_drawing(back.clone(), "single", None);
    assert!(near(twist_deg(&o), 30.0), "twist {}", twist_deg(&o));
    assert!(o.model_tiles().len() <= 1, "still untiled");
    // And again: saved from the reopened session, it is still turned.
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(o.doc().unwrap()).as_bytes()).unwrap();
    let mut o2 = Session::new();
    o2.viewport_px = (1200.0, 800.0);
    o2.open_drawing(again, "again", None);
    assert!(near(twist_deg(&o2), 30.0));
    // Turned back (ZOOM Previous here), the drawing keeps no twist.
    s.execute("zoom", &json!({ "mode": "previous" })).unwrap();
    assert_eq!(view(&s).twist, 0.0);
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let mut o3 = Session::new();
    o3.viewport_px = (1200.0, 800.0);
    o3.open_drawing(back, "plain", None);
    assert_eq!(view(&o3).twist, 0.0);
}

#[test]
fn zoom_extents_fits_in_the_twisted_frame() {
    let mut s = session();
    s.execute("dview", &json!({ "twist": 90 })).unwrap();
    s.execute("zoom", &json!({ "mode": "extents" })).unwrap();
    let v = view(&s);
    // Turned a quarter, the 100 x 50 line's box stands 100 high on screen.
    assert!(near(v.twist.to_degrees(), 90.0));
    assert!((v.center - Vec2::new(50.0, 25.0)).len() < 1e-6, "{:?}", v.center);
    assert!(near(v.height, 105.0), "height {}", v.height);
}

#[test]
fn each_tile_keeps_its_twist_through_a_dxf_round_trip() {
    let mut s = session();
    s.execute("-vports", &json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    s.execute("dview", &json!({ "twist": 30 })).unwrap();
    let first = s.model_tiles().iter().position(|t| t.active).unwrap();
    s.activate_tile(1 - first).unwrap();
    assert_eq!(view(&s).twist, 0.0, "the other tile has its own twist");
    s.execute("dview", &json!({ "twist": 60 })).unwrap();
    let mut tw: Vec<f64> = s.model_tiles().iter().map(|t| t.view.twist.to_degrees()).collect();
    tw.sort_by(f64::total_cmp);
    assert!(near(tw[0], 30.0) && near(tw[1], 60.0), "{tw:?}");
    // Switching back shows the first tile's twist again.
    s.activate_tile(first).unwrap();
    assert!(near(twist_deg(&s), 30.0));
    // A saved configuration keeps both twists through DXF (VPORT group 51).
    s.execute("-vports", &json!({ "option": "save", "name": "Pair" })).unwrap();
    let dxf = cadcraft_io::write_dxf(s.doc().unwrap());
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let written = lines.windows(2).any(|w| w[0] == "51" && w[1].parse::<f64>().is_ok_and(|a| near(a, 30.0)));
    assert!(written, "group 51 written");
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let pair = back.vports.iter().find(|c| c.name == "Pair").unwrap();
    let mut tw: Vec<f64> = pair.tiles.iter().map(|t| t.twist.to_degrees()).collect();
    tw.sort_by(f64::total_cmp);
    assert!(near(tw[0], 30.0) && near(tw[1], 60.0), "{tw:?}");
    // Opened, restoring the configuration shows each tile turned as saved.
    let mut o = Session::new();
    o.viewport_px = (1200.0, 800.0);
    o.open_drawing(back, "pair", None);
    o.execute("-vports", &json!({ "option": "restore", "name": "Pair" })).unwrap();
    let mut tw: Vec<f64> = o.model_tiles().iter().map(|t| t.view.twist.to_degrees()).collect();
    tw.sort_by(f64::total_cmp);
    assert!(near(tw[0], 30.0) && near(tw[1], 60.0), "{tw:?}");
}

#[test]
fn opening_a_tiled_drawing_shows_the_current_tile_twisted() {
    let mut s = session();
    s.execute("dview", &json!({ "twist": 45 })).unwrap();
    s.execute("-vports", &json!({ "option": "2" })).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let mut o = Session::new();
    o.viewport_px = (1200.0, 800.0);
    o.open_drawing(back, "tiled", None);
    assert!(near(twist_deg(&o), 45.0), "twist {}", twist_deg(&o));
}

#[test]
fn view_save_and_restore_keep_the_twist() {
    let mut s = session();
    s.execute("dview", &json!({ "twist": 30 })).unwrap();
    s.execute("view", &json!({ "option": "save", "name": "Turned" })).unwrap();
    let saved = s.doc().unwrap().views.iter().find(|v| v.name == "Turned").map(|v| v.twist.to_degrees()).unwrap();
    assert!(near(saved, 30.0));
    s.execute("dview", &json!({ "twist": 0 })).unwrap();
    s.execute("view.set", &json!({ "center": [80, 40], "height": 3 })).unwrap();
    assert_eq!(view(&s).twist, 0.0);
    s.execute("view", &json!({ "option": "restore", "name": "Turned" })).unwrap();
    assert!(near(twist_deg(&s), 30.0));
    assert!((view(&s).center - Vec2::new(10.0, 10.0)).len() < 1e-9);
    // view.set keeps the twist unless given one.
    s.execute("view.set", &json!({ "center": [0, 0], "height": 5 })).unwrap();
    assert!(near(twist_deg(&s), 30.0));
    s.execute("view.set", &json!({ "center": [0, 0], "height": 5, "twist": 15 })).unwrap();
    assert!(near(twist_deg(&s), 15.0));
}

#[test]
fn zoom_fits_camera_projected_bounds_when_rolled_or_orbited() {
    // CodeRabbit regression case: a 100 x 0 horizontal segment in a 2:1 viewport (1600 x 800),
    // rolled 90° via camera roll (view.twist == 0), stands vertically on screen (0 x 100).
    // Fitting unprojected world bounds gave height = 100 / 2.0 = 50 (clipping half the line).
    let mut s = Session::new();
    s.viewport_px = (1600.0, 800.0);
    s.execute("limits", &json!({ "min": [0, 0], "max": [100, 0] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [100, 0]] })).unwrap();
    s.execute("view.roll", &json!({ "angle": 90.0, "inPlane": true })).unwrap();
    assert_eq!(view(&s).twist, 0.0);

    s.execute("zoom", &json!({ "mode": "extents" })).unwrap();
    assert!(near(view(&s).height, 105.0), "extents height {}", view(&s).height);

    s.execute("zoom", &json!({ "mode": "window", "p1": [0, 0], "p2": [100, 0] })).unwrap();
    assert!(near(view(&s).height, 100.0), "window height {}", view(&s).height);

    s.execute("zoom", &json!({ "mode": "all" })).unwrap();
    assert!(view(&s).height >= 100.0, "all height {} should not clip rolled horizontal segment", view(&s).height);

    s.execute("selectall", &json!({})).unwrap();
    s.execute("zoom", &json!({ "mode": "object" })).unwrap();
    assert!(view(&s).height >= 100.0, "object height {} should fit rolled horizontal segment", view(&s).height);
}

#[test]
fn zoom_extents_and_window_fit_isometric_and_edge_on_views() {
    use cadcraft_engine::geom::{Bounds2, Camera3D};

    let mut s = Session::new();
    s.viewport_px = (1600.0, 800.0);
    let b = Bounds2::new(Vec2::ZERO, Vec2::new(200.0, 100.0));
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [200, 100] })).unwrap();

    // Isometric view: all 4 projected corners of `b` stay inside the fitted viewport.
    s.execute("view.set", &json!({ "preset": "se_isometric" })).unwrap();
    s.execute("zoom", &json!({ "mode": "window", "p1": [0, 0], "p2": [200, 100] })).unwrap();
    let st = s.state().unwrap();
    let v = st.view();
    let aspect = s.viewport_px.0 / s.viewport_px.1;
    let half_h = v.height * 0.5 + 1e-6;
    let half_w = half_h * aspect;
    for corner in b.corners() {
        let uv = st.camera.project_xy(corner, v.center);
        assert!(uv.x.abs() <= half_w && uv.y.abs() <= half_h, "iso corner {corner:?} clipped at uv={uv:?} (half=({half_w}, {half_h}))");
    }

    s.execute("zoom", &json!({ "mode": "extents" })).unwrap();
    let v_ext = view(&s);
    assert!(near(v_ext.height, v.height * 1.05), "extents should add 5% margin over tight iso fit: ext={} vs win={}", v_ext.height, v.height);

    // Edge-on Front view: height collapses to 0, width = 200 -> height = 200 / aspect = 100 (or 105 with 5% margin).
    s.execute("view.set", &json!({ "preset": "front" })).unwrap();
    s.execute("zoom", &json!({ "mode": "extents" })).unwrap();
    assert!(near(view(&s).height, 105.0), "front extents height {}", view(&s).height);
    assert!((view(&s).center - Vec2::new(100.0, 50.0)).len() < 1e-9);

    // Exact inverse consistency for off-origin boxes under arbitrary roll/orbit cameras.
    let off = Bounds2::new(Vec2::new(-30.0, 15.0), Vec2::new(140.0, 95.0));
    for cam in [
        Camera3D::TOP.roll_in_plane(37.0),
        Camera3D::preset("sw_isometric").unwrap(),
        Camera3D::TOP.orbit(33.0, 56.0, true),
    ] {
        let (center, pw, ph) = View::projected_box(off, cam, 0.0);
        let projected = Bounds2::from_points(off.corners().map(|p| cam.project_xy(p, Vec2::ZERO)));
        assert!(near(pw, projected.width()) && near(ph, projected.height()));
        let round_trip = cam.project_xy(center, Vec2::ZERO);
        assert!((round_trip - projected.center()).len() < 1e-9, "round_trip={round_trip:?} projected={:?}", projected.center());
    }
}
