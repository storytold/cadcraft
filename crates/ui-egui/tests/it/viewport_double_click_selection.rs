use cadcraft_doc::{Common, EntityKind, Space, Viewport};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use serde_json::json;

#[test]
fn double_click_viewport_does_not_leave_pending_window() {
    let mut session = Session::new();
    session.new_drawing(true);
    session.viewport_px = (1600.0, 1000.0);

    let paper = Space::Paper("Layout1".into());
    let vp = Viewport {
        id: 2,
        center: cadcraft_geom::Vec3::new(70.0, 60.0, 0.0),
        width: 100.0,
        height: 80.0,
        view_center: Vec2::new(500.0, 250.0),
        view_height: 500.0,
        ..Default::default()
    };
    session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    session.execute("layout.set", &json!({"name": "Layout1"})).unwrap();

    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;

    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);

    // Simulate double click at viewport center
    let screen_pos = h.state().canvas.xf.unwrap().to_screen(Vec2::new(70.0, 60.0));
    // First click sets pending window
    h.event(egui::Event::PointerMoved(screen_pos));
    h.event(egui::Event::PointerButton { pos: screen_pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: screen_pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);

    // Second click (egui triggers double click)
    h.event(egui::Event::PointerButton { pos: screen_pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: screen_pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(3);

    assert!(h.state().session.pending_window.is_none(), "double clicking a viewport must not leave a pending selection window");
}

#[test]
fn seamless_double_click_switches_paper_vp1_vp2_without_blank_frame() {
    let mut session = Session::new();
    session.new_drawing(true);
    session.viewport_px = (1600.0, 1000.0);

    let paper = Space::Paper("Layout1".into());
    let cam_iso = cadcraft_geom::Camera3D::preset("se_isometric").unwrap();
    let vp1 = Viewport {
        id: 2,
        center: cadcraft_geom::Vec3::new(50.0, 60.0, 0.0),
        width: 60.0,
        height: 60.0,
        view_center: Vec2::new(100.0, 100.0),
        view_height: 200.0,
        camera: cam_iso,
        ..Default::default()
    };
    let vp2 = Viewport {
        id: 3,
        center: cadcraft_geom::Vec3::new(150.0, 60.0, 0.0),
        width: 60.0,
        height: 60.0,
        view_center: Vec2::new(0.0, 0.0),
        view_height: 150.0,
        camera: cadcraft_geom::Camera3D::TOP,
        ..Default::default()
    };
    let h1 = session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp1)).unwrap();
    let h2 = session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp2)).unwrap();
    session.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    session.execute("view.set", &json!({"center": [100.0, 60.0], "height": 140.0})).unwrap();

    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(2);

    let xf_paper = h.state().canvas.xf.unwrap();
    let p_vp1 = xf_paper.to_screen(Vec2::new(50.0, 60.0));
    let p_vp2 = xf_paper.to_screen(Vec2::new(150.0, 60.0));
    let p_outside = xf_paper.to_screen(Vec2::new(100.0, 115.0));

    let dbl_click = |h: &mut Harness<CadApp>, pos: egui::Pos2| {
        // Let any previous multi-click timer expire cleanly before moving to the next target.
        h.run_steps(40);
        h.event(egui::Event::PointerMoved(pos));
        h.run_steps(1);
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
                h.run_steps(1);
            }
        }
    };

    // 1. Paper Space -> double-click VP1: enters MSPACE immediately with VP1's 3D camera and rebuilt list on the same frame.
    dbl_click(&mut h, p_vp1);
    assert_eq!(h.state().session.state().unwrap().mspace, Some(h1));
    assert!(h.state().canvas.list.is_some(), "same-frame rebuild must avoid empty flash when entering VP1");
    assert_eq!(h.state().canvas.xf.unwrap().cam, cam_iso);

    // 2. VP1 -> double-click VP2 directly: switches active viewport cleanly without manual PSPACE.
    dbl_click(&mut h, p_vp2);
    assert_eq!(h.state().session.state().unwrap().mspace, Some(h2));
    assert!(h.state().canvas.list.is_some(), "same-frame rebuild must avoid empty flash when switching VP1 -> VP2");
    assert_eq!(h.state().canvas.xf.unwrap().cam, cadcraft_geom::Camera3D::TOP);

    // 3. VP2 -> double-click outside all viewports: seamlessly returns to Paper Space on the same frame.
    dbl_click(&mut h, p_outside);
    assert_eq!(h.state().session.state().unwrap().mspace, None);
    assert!(h.state().canvas.list.is_some(), "same-frame rebuild must avoid empty flash when exiting to Paper Space");
}

#[test]
fn viewcube_fits_inside_active_viewport_and_scales_with_paper_zoom() {
    let mut session = Session::new();
    session.new_drawing(true);
    session.viewport_px = (1600.0, 1000.0);

    let paper = Space::Paper("Layout1".into());
    // Small left viewport (50x50 paper units) and large right viewport (120x100 paper units).
    let vp_small = Viewport {
        id: 2,
        center: cadcraft_geom::Vec3::new(40.0, 60.0, 0.0),
        width: 45.0,
        height: 45.0,
        view_center: Vec2::new(0.0, 0.0),
        view_height: 100.0,
        camera: cadcraft_geom::Camera3D::TOP,
        ..Default::default()
    };
    let vp_large = Viewport {
        id: 3,
        center: cadcraft_geom::Vec3::new(150.0, 65.0, 0.0),
        width: 120.0,
        height: 100.0,
        view_center: Vec2::new(0.0, 0.0),
        view_height: 100.0,
        camera: cadcraft_geom::Camera3D::TOP,
        ..Default::default()
    };
    let h_small = session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp_small.clone())).unwrap();
    let h_large = session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp_large.clone())).unwrap();
    session.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    session.execute("view.set", &json!({"center": [105.0, 65.0], "height": 180.0})).unwrap();

    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(2);

    let canvas_rect = h.state().canvas.rect.unwrap();
    assert!(cadcraft_ui_egui::canvas::active_viewcube_layout(h.state(), canvas_rect).is_none(), "ViewCube must be hidden in Paper Space");

    let xf_far = h.state().canvas.xf.unwrap();
    let vp_screen_rect = |xf: cadcraft_ui_egui::canvas::Xf, vp: &Viewport| {
        egui::Rect::from_two_pos(
            xf.to_screen(Vec2::new(vp.center.x - vp.width / 2.0, vp.center.y + vp.height / 2.0)),
            xf.to_screen(Vec2::new(vp.center.x + vp.width / 2.0, vp.center.y - vp.height / 2.0)),
        )
    };
    let small_screen_far = vp_screen_rect(xf_far, &vp_small);
    let large_screen_far = vp_screen_rect(xf_far, &vp_large);

    let dbl_click = |h: &mut Harness<CadApp>, pos: egui::Pos2| {
        h.run_steps(40);
        h.event(egui::Event::PointerMoved(pos));
        h.run_steps(1);
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
                h.run_steps(1);
            }
        }
        h.run_steps(2);
    };

    // Enter small viewport: ViewCube sits inside `small_screen_far` and scales down cleanly.
    dbl_click(&mut h, xf_far.to_screen(Vec2::new(40.0, 60.0)));
    assert_eq!(h.state().session.state().unwrap().mspace, Some(h_small));
    let l_small = cadcraft_ui_egui::canvas::active_viewcube_layout(h.state(), canvas_rect).unwrap();
    assert!(small_screen_far.contains(l_small.center), "ViewCube center {:?} must sit inside active small viewport {:?}", l_small.center, small_screen_far);
    assert!(small_screen_far.contains_rect(l_small.bounds), "ViewCube bounds {:?} must fit inside small viewport {:?}", l_small.bounds, small_screen_far);
    assert!(l_small.scale < 0.7, "expected reduced ViewCube scale inside small viewport, got {}", l_small.scale);

    // Click top-front-right corner (or right dir triangle) on the scaled ViewCube inside VP1 to verify interactivity!
    let (_t, _b, _l, dir_r) = l_small.dir_triangles(&cadcraft_geom::Camera3D::TOP);
    let tri_center = egui::pos2((dir_r[0].x + dir_r[1].x + dir_r[2].x) / 3.0, (dir_r[0].y + dir_r[1].y + dir_r[2].y) / 3.0);
    h.run_steps(40);
    h.event(egui::Event::PointerMoved(tri_center));
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: tri_center, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: tri_center, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(2);
    assert_ne!(h.state().canvas.xf.unwrap().cam, cadcraft_geom::Camera3D::TOP, "clicking scaled directional arrow inside viewport must orbit that viewport");

    // Switch directly to large viewport: ViewCube scales up larger inside `large_screen_far`.
    dbl_click(&mut h, xf_far.to_screen(Vec2::new(150.0, 65.0)));
    assert_eq!(h.state().session.state().unwrap().mspace, Some(h_large));
    let l_large = cadcraft_ui_egui::canvas::active_viewcube_layout(h.state(), canvas_rect).unwrap();
    assert!(large_screen_far.contains(l_large.center));
    assert!(l_large.scale > l_small.scale, "larger viewport must yield larger ViewCube ({} vs {})", l_large.scale, l_small.scale);

    // Exit to Paper Space, zoom in close onto `vp_small`, then re-enter `vp_small`:
    // ViewCube must scale noticeably larger than when entered while zoomed out!
    dbl_click(&mut h, xf_far.to_screen(Vec2::new(105.0, 145.0)));
    assert_eq!(h.state().session.state().unwrap().mspace, None);
    h.state_mut().session.execute("view.set", &json!({"center": [40.0, 60.0], "height": 60.0})).unwrap();
    h.run_steps(2);
    let xf_near = h.state().canvas.xf.unwrap();
    let small_screen_near = vp_screen_rect(xf_near, &vp_small);
    dbl_click(&mut h, xf_near.to_screen(Vec2::new(40.0, 60.0)));
    assert_eq!(h.state().session.state().unwrap().mspace, Some(h_small));
    let l_small_zoomed = cadcraft_ui_egui::canvas::active_viewcube_layout(h.state(), canvas_rect).unwrap();
    assert!(small_screen_near.contains(l_small_zoomed.center));
    assert!(
        l_small_zoomed.scale > l_small.scale,
        "zooming closer in Paper Space before entering MSPACE must enlarge ViewCube ({} -> {})",
        l_small.scale,
        l_small_zoomed.scale
    );
}
