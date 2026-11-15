use super::*;
use cadcraft_render::DPrim;

fn list(handles: &[u64]) -> DisplayList {
    let prim =
        |h| DPrim { handle: Handle(h), color: Rgb(255, 255, 255), aci7: true, lw: 0.0, transparency: 0, kind: Kind::Polyline, start: 0, len: 0 };
    DisplayList { prims: handles.iter().map(|&h| prim(h)).collect(), ..Default::default() }
}

fn hs(v: &[u64]) -> Vec<Handle> {
    v.iter().map(|&h| Handle(h)).collect()
}

/// A dragged object keeps its transparency in the GPU preview when TRANSPARENCYDISPLAY is
/// on, and is opaque when it is off.
#[test]
fn dragged_objects_keep_their_transparency() {
    let mut d = cadcraft_doc::Drawing::new_metric();
    let common = cadcraft_doc::Common { transparency: cadcraft_doc::Transparency::Percent(50), ..Default::default() };
    let line = cadcraft_doc::Line { a: cadcraft_geom::Vec3::ZERO, b: cadcraft_geom::Vec3::new(10.0, 0.0, 0.0) };
    let h = d.add(&cadcraft_doc::Space::Model, common, cadcraft_doc::EntityKind::Line(line)).unwrap();
    let drag = cadcraft_engine::Drag { handles: vec![h], matrix: cadcraft_geom::Mat3::IDENTITY, extra: Vec::new(), clipboard: false };
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 100.0));
    let xf = Xf { rect, center: Vec2::new(5.0, 0.0), scale: 10.0, twist: 0.0, target_z: 0.0, cam: cadcraft_geom::Camera3D::TOP };
    let painter = egui::Painter::new(egui::Context::default(), egui::LayerId::background(), rect);
    // One canvas across the toggle: the cached drag mesh must be rebuilt.
    let mut c = CanvasState::default();
    let mut alpha = |transparency: bool| {
        draw_drag_gpu(&mut c, &painter, &xf, &d, &drag, (&[], 0), Rgb(0, 0, 0), transparency);
        let mesh = c.drag_slot.lock().unwrap().take().unwrap();
        // The first line vertex's colour alpha (after its 2 × f32 position).
        mesh.lines[11]
    };
    assert_eq!(alpha(true), 128);
    assert_eq!(alpha(false), 255);
    assert_eq!(alpha(true), 128);
}

/// Previews the session rebuilds with the current properties (the rubber band, copies of
/// objects) stay opaque even when CLAYER is transparent: they were opaque before transparency
/// was drawn, and CLAYER's transparency isn't the objects'.
#[test]
fn rebuilt_previews_stay_opaque_on_a_transparent_current_layer() {
    let mut s = cadcraft_engine::Session::new();
    let d = s.doc_mut().unwrap();
    d.layers.push(cadcraft_doc::Layer { name: "Glass".into(), transparency: 40, ..Default::default() });
    d.header.set_str("CLAYER", "Glass");
    assert!(s.settings.transparency_display);
    // LINE: the segment from the first point to the cursor.
    s.cmdline("line").unwrap();
    s.cmdline("0,0").unwrap();
    let ents = s.preview(Vec2::new(10.0, 5.0));
    assert!(!ents.is_empty());
    assert_eq!(ents[0].common.layer, "Glass");
    let doc = s.doc().unwrap();
    // Built with transparency, the preview would take CLAYER's 40 %.
    let with = cadcraft_render::build_entities(doc, ents.iter(), &cadcraft_render::Options { transparency: true, ..preview_options(1.0) });
    assert!(with.prims.iter().all(|p| p.transparency == 40));
    let list = rebuilt_preview(doc, ents.iter(), 1.0);
    assert!(!list.prims.is_empty() && list.prims.iter().all(|p| p.transparency == 0));
    s.cancel();
    // MOVE's base-to-cursor rubber band (a drag extra).
    s.execute("line", &serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap();
    s.cmdline("move").unwrap();
    s.cmdline("all").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("0,0").unwrap();
    let (_, extra) = s.drag(Vec2::new(4.0, 4.0)).unwrap();
    assert!(!extra.is_empty());
    let list = rebuilt_preview(s.doc().unwrap(), extra.iter(), 1.0);
    assert!(!list.prims.is_empty() && list.prims.iter().all(|p| p.transparency == 0));
}

#[test]
fn viewport_highlight_tolerance_is_constant_within_a_zoom_band() {
    assert_eq!(viewport_highlight_tolerance(4.0), viewport_highlight_tolerance(3.0));
    assert_ne!(viewport_highlight_tolerance(4.0), viewport_highlight_tolerance(1.5));
    for s in [0.3, 1.0, 3.0, 4.0, 250.0] {
        assert!(viewport_highlight_tolerance(s) <= 0.5 / s);
    }
}

#[test]
fn selection_area_fill_follows_the_area_effect_settings() {
    let mut s = cadcraft_engine::Settings::default();
    // Window: ACI 150 at 25 %; crossing: ACI 100.
    let rgb = |c: Rgb| Color32::from_rgb(c.0, c.1, c.2);
    assert_eq!(selection_area_fill(&s, false), Some(rgb(cadcraft_color::aci_rgb(150)).gamma_multiply(0.25)));
    assert_eq!(selection_area_fill(&s, true), Some(rgb(cadcraft_color::aci_rgb(100)).gamma_multiply(0.25)));
    s.selectionareaopacity = 100;
    assert_eq!(selection_area_fill(&s, false), Some(rgb(cadcraft_color::aci_rgb(150))));
    s.selectionarea = false;
    assert_eq!(selection_area_fill(&s, false), None, "Indicate selection area off: no fill");
}

#[test]
fn viewport_highlight_is_tessellated_once_per_selection_and_zoom_band() {
    use cadcraft_doc::{Circle, Common, Drawing, EntityKind, Space};
    let mut d = Drawing::new_imperial();
    let circle = |r| EntityKind::Circle(Circle { center: cadcraft_geom::Vec3::new(0.0, 0.0, 0.0), radius: r });
    let a = d.add(&Space::Model, Common::default(), circle(10.0)).unwrap();
    let b = d.add(&Space::Model, Common::default(), circle(5.0)).unwrap();
    let st = cadcraft_engine::DocState::new(d, "highlight", None);
    let points = |l: &DisplayList| l.prims.iter().map(|p| l.points(p).len()).sum::<usize>();
    let (mut sel, mut hover) = (VpHighlight::default(), VpHighlight::default());
    sel.update(&st, &[a], 10.0);
    let (first, segments) = (sel.list_gen, points(&sel.list));
    assert!(!sel.list.prims.is_empty());
    // Every frame of a pan, zooming within the 2x band, and the hover moving: the same list.
    sel.update(&st, &[a], 10.0);
    sel.update(&st, &[a], 12.0);
    hover.update(&st, &[b], 12.0);
    hover.update(&st, &[a], 12.0);
    assert_eq!(sel.list_gen, first);
    // Zooming in past the band tessellates finer.
    sel.update(&st, &[a], 40.0);
    assert_ne!(sel.list_gen, first);
    assert!(points(&sel.list) > segments);
    // A different selection is rebuilt.
    let zoomed = sel.list_gen;
    sel.update(&st, &[a, b], 40.0);
    assert_ne!(sel.list_gen, zoomed);
    assert_eq!(sel.list.prims.iter().map(|p| p.handle).collect::<std::collections::BTreeSet<_>>().len(), 2);
}

#[test]
fn prim_index_matches_a_scan_and_follows_list_rebuilds() {
    let a = list(&[3, 1, 3, 2, 7, 3]);
    let mut ix = PrimIndex::default();
    assert_eq!(ix.prims(1, &a, &hs(&[3])), vec![0, 2, 5]);
    assert_eq!(ix.prims(1, &a, &hs(&[3])), prims_of(&a, &hs(&[3])));
    assert_eq!(ix.prims(1, &a, &hs(&[2, 1, 2])), vec![1, 3]);
    assert!(ix.prims(1, &a, &hs(&[9])).is_empty());
    assert!(ix.prims(1, &a, &[]).is_empty());
    // A rebuilt list (new generation) is re-indexed.
    let b = list(&[3, 9]);
    assert_eq!(ix.prims(2, &b, &hs(&[9, 3])), vec![0, 1]);
}

fn wheel(unit: egui::MouseWheelUnit, dy: f32, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::MouseWheel { unit, delta: vec2(0.0, dy), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Unknown, modifiers }
}

/// Wheel events convert to points as egui does; ⌘/Ctrl (egui's pinch zoom) and ⇧ (a sideways
/// scroll) don't zoom here, and every notch of the frame counts (#660).
#[test]
fn wheel_zoom_reads_every_notch_of_the_frame() {
    use egui::{Modifiers, MouseWheelUnit as U};
    let o = egui::InputOptions::default();
    let none = Modifiers::NONE;
    assert_eq!(wheel_zoom_points(&[wheel(U::Line, 1.0, none)], &o, 500.0), o.line_scroll_speed);
    assert_eq!(wheel_zoom_points(&[wheel(U::Line, -2.0, none)], &o, 500.0), -2.0 * o.line_scroll_speed);
    assert_eq!(wheel_zoom_points(&[wheel(U::Point, 7.5, none)], &o, 500.0), 7.5);
    assert_eq!(wheel_zoom_points(&[wheel(U::Page, 1.0, none)], &o, 500.0), 500.0);
    // Three quick notches in one frame zoom three steps at once.
    let three = [wheel(U::Line, 1.0, none), wheel(U::Line, 1.0, none), wheel(U::Line, 1.0, none)];
    assert_eq!(wheel_zoom_points(&three, &o, 500.0), 3.0 * o.line_scroll_speed);
    assert_eq!(wheel_zoom_points(&[wheel(U::Line, 1.0, Modifiers::COMMAND)], &o, 500.0), 0.0);
    assert_eq!(wheel_zoom_points(&[wheel(U::Line, 1.0, Modifiers::SHIFT)], &o, 500.0), 0.0);
    assert_eq!(wheel_zoom_points(&[wheel(U::Line, f32::NAN, none)], &o, 500.0), 0.0);
    assert_eq!(wheel_zoom_points(&[egui::Event::PointerGone], &o, 500.0), 0.0);
}

/// The frame a notch arrives, egui's smoothed scroll holds only part of it (the rest trickles
/// in over later frames, which made quick notches lag), while the raw reading zooms the
/// whole notch at once (#660).
#[test]
fn a_wheel_notch_zooms_whole_in_its_own_frame() {
    let ctx = egui::Context::default();
    let o = ctx.options(|o| o.input_options);
    let input = egui::RawInput { events: vec![wheel(egui::MouseWheelUnit::Line, 1.0, egui::Modifiers::NONE)], ..Default::default() };
    let mut seen = (0.0, 0.0);
    ctx.run_ui(input, |ui| {
        seen = ui.input(|i| (i.smooth_scroll_delta.y, wheel_zoom_points(&i.raw.events, &o, 500.0)));
    })
    .textures_delta
    .clear();
    let (smoothed, raw) = seen;
    assert_eq!(raw, o.line_scroll_speed, "the whole notch");
    assert!(smoothed < raw, "egui's smoothed delta lags behind: {smoothed} of {raw}");
}

/// The text painted by one frame of the app at `pointer`.
pub(crate) fn painted_text(app: &mut CadApp, ctx: &egui::Context, pointer: Pos2) -> String {
    fn collect(shape: &Shape, text: &mut String) {
        match shape {
            Shape::Text(s) => {
                text.push_str(s.galley.text());
                text.push('\n');
            }
            Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
        events: vec![egui::Event::PointerMoved(pointer)],
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    let mut text = String::new();
    out.shapes.iter().for_each(|s| collect(&s.shape, &mut text));
    text
}

/// The Dynamic Input distance box shows the cursor's live distance in the drawing's length
/// units (LUNITS/LUPREC), as DIST and the status bar do (#841).
#[test]
fn dynamic_input_distance_follows_the_drawing_units() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("units", &serde_json::json!({"lunits": 4, "luprec": 4})).unwrap();
    let ctx = egui::Context::default();
    painted_text(&mut app, &ctx, pos2(700.0, 450.0));
    app.session.cmdline("line").unwrap();
    app.session.input(Input::Point(Vec2::ZERO)).unwrap();
    let at = app.canvas.xf.unwrap().to_screen(Vec2::ZERO) + vec2(130.0, -70.0);
    painted_text(&mut app, &ctx, at);
    let text = painted_text(&mut app, &ctx, at);
    let c = app.canvas.cursor.unwrap();
    let arch = |v: f64| cadcraft_engine::units::format_distance(v, 4, 4);
    // The status bar shows X and Y in the same units: the distance must be neither.
    assert!(arch(c.len()) != arch(c.x) && arch(c.len()) != arch(c.y), "{c:?}");
    assert!(text.contains(&arch(c.len())), "no {} in:\n{text}", arch(c.len()));
    // Without the value boxes (a window corner pending) the tooltip shows the distance too.
    app.session.pending_window = Some(cadcraft_engine::PendingWindow::new(Vec2::ZERO, true));
    let text = painted_text(&mut app, &ctx, at);
    assert!(text.contains(&format!("   {}  <", arch(c.len()))), "no {} in:\n{text}", arch(c.len()));
}

/// A hot grip whose object was hidden since (HIDEOBJECTS, a layer turned off) is no longer
/// painted, so clicking another hot grip must not stretch the hidden object along with it.
#[test]
fn clicking_a_hot_grip_leaves_hidden_objects_out_of_the_stretch() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    for y in [0, 30] {
        app.run("line", serde_json::json!({ "points": [[0, y], [100, y]] })).unwrap();
    }
    let handles = app.session.doc().unwrap().model.handles();
    let (shown, hidden) = (handles[0], handles[1]);
    app.session.set_selection(vec![shown, hidden]);
    app.canvas.hot_grips = vec![(shown, 0), (hidden, 0)];
    app.run("hideobjects", serde_json::json!({ "handles": [hidden.hex()] })).unwrap();
    app.session.set_selection(vec![shown, hidden]);
    click_grip(&mut app, (shown, 0), false);
    assert_eq!(app.canvas.hot_grips, vec![(shown, 0)], "the hidden object's grip must leave the hot set");
}

#[test]
fn edge_on_to_world_preserves_depth_axis() {
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
    let cam = cadcraft_geom::Camera3D::preset("front").unwrap();
    let xf = Xf { rect, center: Vec2::new(10.0, 20.0), scale: 2.0, twist: 0.0, target_z: 0.0, cam };

    assert!(xf.try_to_world(rect.center()).is_none());
    assert!(xf.to_world(rect.center()).near(xf.center, 1e-9));

    let up = xf.to_world(rect.center() + vec2(12.0, -30.0));
    let down = xf.to_world(rect.center() + vec2(12.0, 30.0));
    assert!((up.x - down.x).abs() < 1e-9, "same horizontal screen offset keeps X: up={up:?}, down={down:?}");
    assert!((up.y - down.y).abs() > 29.0, "vertical screen motion must preserve depth in XY: up={up:?}, down={down:?}");
}

#[test]
fn shallow_3d_screen_delta_to_world_inverts_to_screen() {
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
    // Elevation ~5.7° (`|cam.z.z| == 0.1`, inside 0.01 < |z.z| <= 0.15): must still track 1:1.
    let z = cadcraft_geom::Vec3::new(0.0, 0.995, -0.1).normalized();
    let cam = cadcraft_geom::Camera3D::from_dir_up(z, cadcraft_geom::Vec3::Z);
    assert!(cam.z.z.abs() > 0.01 && cam.z.z.abs() <= 0.15);
    let xf = Xf { rect, center: Vec2::new(10.0, 20.0), scale: 1.5, twist: 0.0, target_z: 0.0, cam };

    let delta = vec2(24.0, -40.0);
    let world_offset = xf.screen_delta_to_world(delta);
    let roundtrip = xf.to_screen(xf.center + world_offset) - rect.center();
    assert!((roundtrip.x - delta.x).abs() < 1e-4 && (roundtrip.y - delta.y).abs() < 1e-4, "roundtrip={roundtrip:?}");
}

#[test]
fn paper_space_and_locked_mspace_block_orbit_while_unlocked_mspace_orbits_own_camera() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.run("line", serde_json::json!({ "points": [[0, 0], [100, 100]] })).unwrap();
    app.run("view.set", serde_json::json!({ "preset": "se_isometric" })).unwrap();
    let iso_cam = app.session.state().unwrap().camera;
    assert!(!iso_cam.is_top_plan());

    app.run("layout.set", serde_json::json!({ "name": "Layout1" })).unwrap();
    assert_eq!(app.session.state().unwrap().space, cadcraft_doc::Space::Paper("Layout1".into()));
    let ctx = egui::Context::default();
    let run_frame = |app: &mut CadApp, events: Vec<egui::Event>| {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        };
        ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
        .textures_delta
        .clear();
    };

    // Paper Space: stays TOP and blocks Shift+MMB orbit.
    run_frame(&mut app, vec![egui::Event::PointerMoved(pos2(700.0, 450.0))]);
    assert_eq!(app.canvas.xf.unwrap().cam, cadcraft_geom::Camera3D::TOP);
    assert!(begin_canvas_orbit(&app, pos2(700.0, 450.0), None).is_none());

    // Unlocked MSPACE: Shift+MMB orbits ONLY the active viewport's own 3D camera; Model tab
    // keeps `iso_cam` untouched.
    let vp_res = app.run("mview", serde_json::json!({ "p1": [10.0, 10.0], "p2": [150.0, 120.0] })).unwrap();
    let vp_handle = vp_res["viewports"][0].as_str().unwrap().to_string();
    app.run("mspace", serde_json::json!({})).unwrap();
    let shift = egui::Modifiers::SHIFT;
    run_frame(&mut app, vec![egui::Event::PointerMoved(pos2(700.0, 450.0)), egui::Event::ModifiersChanged(shift)]);
    let press = egui::Event::PointerButton {
        pos: pos2(700.0, 450.0),
        button: egui::PointerButton::Middle,
        pressed: true,
        modifiers: shift,
    };
    run_frame(&mut app, vec![egui::Event::ModifiersChanged(shift), egui::Event::PointerMoved(pos2(700.0, 450.0)), press]);
    run_frame(&mut app, vec![egui::Event::ModifiersChanged(shift), egui::Event::PointerMoved(pos2(760.0, 410.0))]);
    let release = egui::Event::PointerButton {
        pos: pos2(760.0, 410.0),
        button: egui::PointerButton::Middle,
        pressed: false,
        modifiers: shift,
    };
    run_frame(&mut app, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE), release]);

    let st = app.session.state().unwrap();
    let vp_cam_orbited = model_camera(st);
    assert!(!vp_cam_orbited.is_top_plan(), "unlocked MSPACE viewport must orbit in 3D");
    assert_eq!(app.canvas.xf.unwrap().cam, vp_cam_orbited);
    assert_eq!(st.camera, iso_cam, "Model tab 3D camera must remain independent");

    // Lock the viewport: further Shift+MMB orbit must be blocked cleanly.
    app.run("viewport.set", serde_json::json!({ "handle": vp_handle, "locked": true })).unwrap();
    assert!(begin_canvas_orbit(&app, pos2(700.0, 450.0), None).is_none());
    run_frame(&mut app, vec![egui::Event::PointerMoved(pos2(700.0, 450.0)), egui::Event::ModifiersChanged(shift)]);
    let press2 = egui::Event::PointerButton {
        pos: pos2(700.0, 450.0),
        button: egui::PointerButton::Middle,
        pressed: true,
        modifiers: shift,
    };
    run_frame(&mut app, vec![egui::Event::ModifiersChanged(shift), egui::Event::PointerMoved(pos2(700.0, 450.0)), press2]);
    run_frame(&mut app, vec![egui::Event::ModifiersChanged(shift), egui::Event::PointerMoved(pos2(790.0, 380.0))]);
    let release2 = egui::Event::PointerButton {
        pos: pos2(790.0, 380.0),
        button: egui::PointerButton::Middle,
        pressed: false,
        modifiers: shift,
    };
    run_frame(&mut app, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE), release2]);
    assert_eq!(model_camera(app.session.state().unwrap()), vp_cam_orbited, "locked MSPACE viewport must keep its camera");
}

#[test]
fn edge_on_view_allows_fully_locked_dynamic_input_click_and_keeps_buffer_on_reject() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.run("view.set", serde_json::json!({ "preset": "front" })).unwrap();
    let ctx = egui::Context::default();
    let run_events = |app: &mut CadApp, events: Vec<egui::Event>| {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        };
        ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
        .textures_delta
        .clear();
    };
    let hover = pos2(700.0, 450.0);
    run_events(&mut app, vec![egui::Event::PointerMoved(hover)]);

    app.session.cmdline("line").unwrap();
    app.cmd.buffer = "40,".into();
    // Clicking when only the first Dynamic Input coordinate is locked stays blocked in an edge-on
    // view and preserves the partially typed buffer.
    let click_at = |app: &mut CadApp, pos: Pos2| {
        run_events(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        run_events(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    };
    click_at(&mut app, hover);
    assert_eq!(app.cmd.buffer, "40,");
    assert_eq!(app.session.current_prompt().unwrap().base, None);

    // Type the second coordinate and press Tab to lock both values, then click to place (40, 50).
    run_events(&mut app, vec![egui::Event::PointerMoved(hover), egui::Event::Text("50".into())]);
    run_events(&mut app, vec![egui::Event::PointerMoved(hover), egui::Event::Text("\t".into())]);
    click_at(&mut app, hover);
    assert!(app.cmd.buffer.is_empty(), "placed buffer should be cleared, got {:?}", app.cmd.buffer);
    assert_eq!(app.session.current_prompt().unwrap().base, Some(Vec2::new(40.0, 50.0)));
}

#[test]
fn interactive_3dorbit_drags_anywhere_to_orbit_in_model_space() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.session.cmdline("3dorbit").unwrap();
    assert!(app.session.orbit_active());

    let ctx = egui::Context::default();
    let run_events = |app: &mut CadApp, events: Vec<egui::Event>| {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        };
        ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
        .textures_delta
        .clear();
    };
    // Initial frame so `app.canvas.rect` is populated.
    run_events(&mut app, vec![egui::Event::PointerMoved(pos2(700.0, 450.0))]);
    let center = app.canvas.rect.unwrap().center();

    let drag_primary = |app: &mut CadApp, start: Pos2, end: Pos2| {
        run_events(
            app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        run_events(app, vec![egui::Event::PointerMoved(end)]);
        run_events(
            app,
            vec![
                egui::Event::PointerMoved(end),
                egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    };
    drag_primary(&mut app, center, center + vec2(40.0, -30.0));
    let orbited = app.session.state().unwrap().camera;
    assert!(!orbited.is_top_plan(), "primary drag during 3dorbit must orbit camera");
    assert!(orbited.x.z.abs() < 1e-6, "constrained orbit keeps horizon level");

    // Dragging near the edge also performs constrained orbit (no roll zone).
    drag_primary(&mut app, center + vec2(320.0, 0.0), center + vec2(260.0, -20.0));
    let orbited2 = app.session.state().unwrap().camera;
    assert!((orbited2.z - orbited.z).len() > 0.05, "outer drag also orbits viewing direction");
    assert!(orbited2.x.z.abs() < 1e-6, "constrained orbit keeps horizon level");

    // Right-click exits 3DORBIT cleanly.
    run_events(
        &mut app,
        vec![
            egui::Event::PointerMoved(center),
            egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(!app.session.orbit_active());
}

#[test]
fn orbit_drag_from_3d_entity_keeps_pivot_fixed_on_screen_and_respects_orbitautotarget() {
    use cadcraft_geom::Vec3;

    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    let h = {
        let d = app.session.doc_mut().unwrap();
        let line = cadcraft_doc::Line { a: Vec3::new(100.0, 0.0, 15.0), b: Vec3::new(200.0, 0.0, 15.0) };
        d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Line(line)).unwrap()
    };
    assert!(app.session.doc().unwrap().entity(h).is_some());
    app.run("view.set", serde_json::json!({ "preset": "se_isometric" })).unwrap();
    app.run("zoom.extents", serde_json::json!({})).unwrap();

    let ctx = egui::Context::default();
    let run_events = |app: &mut CadApp, events: Vec<egui::Event>| {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        };
        ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
        .textures_delta
        .clear();
    };
    run_events(&mut app, vec![egui::Event::PointerMoved(pos2(700.0, 450.0))]);
    let xf0 = app.canvas.xf.unwrap();
    let pivot_world = Vec3::new(150.0, 0.0, 15.0);
    let hit_pos = xf0.to_screen_3d(pivot_world);
    assert!(app.canvas.rect.unwrap().contains(hit_pos), "hit_pos {hit_pos:?} must lie inside canvas rect");

    app.session.cmdline("3dorbit").unwrap();
    run_events(
        &mut app,
        vec![
            egui::Event::PointerMoved(hit_pos),
            egui::Event::PointerButton {
                pos: hit_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let dragged_pos = hit_pos + vec2(60.0, -35.0);
    run_events(&mut app, vec![egui::Event::PointerMoved(dragged_pos)]);
    run_events(
        &mut app,
        vec![
            egui::Event::PointerMoved(dragged_pos),
            egui::Event::PointerButton {
                pos: dragged_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );

    let xf1 = app.canvas.xf.unwrap();
    let screen_after = xf1.to_screen_3d(pivot_world);
    assert!(
        (screen_after - hit_pos).length() < 0.5,
        "3D ray-hit pivot must remain pinned on screen: before={hit_pos:?}, after={screen_after:?}"
    );
    let v1 = app.session.state().unwrap().view();
    assert!(v1.target_z.abs() > 1e-3, "orbiting around off-target Z=15 line must update target_z: {v1:?}");

    // Turning ORBITAUTOTARGET off forces orbiting around the fixed view target instead of ray hits.
    run_events(
        &mut app,
        vec![
            egui::Event::PointerMoved(dragged_pos),
            egui::Event::PointerButton {
                pos: dragged_pos,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: dragged_pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(!app.session.orbit_active());

    app.run("setvar", serde_json::json!({ "name": "ORBITAUTOTARGET", "value": 0 })).unwrap();
    let st_before = app.session.state().unwrap();
    let cam_before = st_before.camera;
    let target_before = st_before.view().target();

    app.session.cmdline("3dorbit").unwrap();
    let start2 = xf1.to_screen_3d(pivot_world);
    run_events(
        &mut app,
        vec![
            egui::Event::PointerMoved(start2),
            egui::Event::PointerButton {
                pos: start2,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let end2 = start2 + vec2(50.0, -25.0);
    run_events(&mut app, vec![egui::Event::PointerMoved(end2)]);
    run_events(
        &mut app,
        vec![
            egui::Event::PointerMoved(end2),
            egui::Event::PointerButton {
                pos: end2,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let st_after = app.session.state().unwrap();
    let target_after = st_after.view().target();
    assert!((st_after.camera.z - cam_before.z).len() > 0.05, "orbit drag with ORBITAUTOTARGET=0 must orbit camera");
    assert!(
        (target_after - target_before).len() < 1e-9,
        "ORBITAUTOTARGET=0 keeps fixed view target: before={target_before:?}, after={target_after:?}"
    );
}

#[test]
fn viewcube_drag_rigidly_orbits_selected_3d_object_pivot() {
    use cadcraft_geom::Vec3;

    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    let h_sel = {
        let d = app.session.doc_mut().unwrap();
        let a = cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(20.0, 0.0, 0.0) };
        let b = cadcraft_doc::Line { a: Vec3::new(70.0, 40.0, 30.0), b: Vec3::new(90.0, 40.0, 30.0) };
        let _ = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Line(a)).unwrap();
        d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Line(b)).unwrap()
    };
    app.session.set_selection(vec![h_sel]);
    app.run("view.set", serde_json::json!({ "preset": "se_isometric" })).unwrap();
    app.run("zoom.extents", serde_json::json!({})).unwrap();

    let ctx = egui::Context::default();
    let run_events = |app: &mut CadApp, events: Vec<egui::Event>| {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        };
        ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
        .textures_delta
        .clear();
    };
    run_events(&mut app, Vec::new());

    let rect = app.canvas.rect.unwrap();
    let xf0 = app.canvas.xf.unwrap();
    let pivot = Vec3::new(80.0, 40.0, 30.0);
    let pivot_screen0 = xf0.to_screen_3d(pivot);
    let cube_center = crate::viewcube::Layout::new(rect).center;

    run_events(
        &mut app,
        vec![
            egui::Event::PointerMoved(cube_center),
            egui::Event::PointerButton {
                pos: cube_center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let end = cube_center + vec2(45.0, -30.0);
    run_events(&mut app, vec![egui::Event::PointerMoved(end)]);
    run_events(
        &mut app,
        vec![
            egui::Event::PointerMoved(end),
            egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );

    let xf1 = app.canvas.xf.unwrap();
    let pivot_screen1 = xf1.to_screen_3d(pivot);
    assert!(
        (pivot_screen1 - pivot_screen0).length() < 0.5,
        "ViewCube drag must orbit around selected 3D object pivot: before={pivot_screen0:?}, after={pivot_screen1:?}"
    );
}

#[test]
fn scroll_zoom_in_3d_and_front_view_keeps_point_under_cursor() {
    use cadcraft_geom::Vec3;

    for preset in ["se_isometric", "front"] {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.run("view.set", serde_json::json!({ "preset": preset })).unwrap();
        let v0 = cadcraft_engine::View { center: Vec2::new(10.0, 20.0), height: 100.0, twist: 0.0, target_z: 35.0 };
        app.session.state_mut().unwrap().set_view_quiet(v0);

        let ctx = egui::Context::default();
        let run_events = |app: &mut CadApp, events: Vec<egui::Event>| {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                focused: true,
                events,
                ..Default::default()
            };
            ctx.run_ui(raw, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            })
            .textures_delta
            .clear();
        };

        let cursor = pos2(820.0, 310.0);
        run_events(&mut app, vec![egui::Event::PointerMoved(cursor)]);
        let xf0 = app.canvas.xf.unwrap();
        let cam = xf0.cam;
        let anchor: Vec3 = xf0.zoom_anchor_3d(cursor);

        run_events(&mut app, vec![egui::Event::PointerMoved(cursor), wheel(egui::MouseWheelUnit::Line, 2.0, egui::Modifiers::NONE)]);
        let xf1 = app.canvas.xf.unwrap();
        let v1 = app.session.state().unwrap().view();
        assert!(xf1.scale > xf0.scale, "{preset}: wheel up must zoom in");
        assert!(
            (xf1.to_screen_3d(anchor) - cursor).length() < 0.5,
            "{preset}: anchor point under cursor must stay fixed (got {:?} vs {cursor:?})",
            xf1.to_screen_3d(anchor)
        );
        assert!(
            (v1.target() - v0.target()).dot(cam.z).abs() < 1e-9,
            "{preset}: scroll zoom must preserve camera depth on target plane"
        );
    }
}

fn drive_canvas_events(ctx: &egui::Context, app: &mut CadApp, events: Vec<egui::Event>) {
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
        focused: true,
        events,
        ..Default::default()
    };
    ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
    .textures_delta
    .clear();
}

#[test]
fn screen_delta_to_world_3d_roundtrips_at_nonzero_target_z_and_shallow_angles() {
    use cadcraft_geom::Vec3;

    let rect = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
    let shallow_z = Vec3::new(0.0, 0.995, -0.1).normalized();
    let cameras = [
        ("sw_iso", cadcraft_geom::Camera3D::preset("sw_isometric").unwrap()),
        ("front", cadcraft_geom::Camera3D::preset("front").unwrap()),
        ("shallow", cadcraft_geom::Camera3D::from_dir_up(shallow_z, Vec3::Z)),
    ];
    let delta = vec2(36.0, -28.0);
    let sample_3d = Vec3::new(15.0, 25.0, 42.0);
    let sample_2d = Vec2::new(15.0, 25.0);

    for (name, cam) in cameras {
        let xf0 = Xf { rect, center: Vec2::new(10.0, 20.0), scale: 1.5, twist: 0.0, target_z: 45.0, cam };
        // Zero pixel delta must produce exact zero offset even when target_z != 0.
        assert_eq!(xf0.screen_delta_to_world_3d(egui::Vec2::ZERO), Vec3::ZERO, "{name}: zero 3d delta");
        assert!(xf0.screen_delta_to_world(egui::Vec2::ZERO).len() < 1e-12, "{name}: zero ground delta");

        let offset = xf0.screen_delta_to_world_3d(delta);
        let t1 = xf0.target() - offset;
        let xf1 = Xf { center: t1.xy(), target_z: t1.z, ..xf0 };
        let diff3 = xf1.to_screen_3d(sample_3d) - xf0.to_screen_3d(sample_3d);
        let diff2 = xf1.to_screen(sample_2d) - xf0.to_screen(sample_2d);
        assert!((diff3 - delta).length() < 1e-4, "{name}: 3D point must shift by delta={delta:?}, got {diff3:?}");
        assert!((diff2 - delta).length() < 1e-4, "{name}: 2D point must shift by delta={delta:?}, got {diff2:?}");
    }
}

#[test]
fn mmb_pan_after_3d_orbit_stays_stationary_and_tracks_cursor_without_drift() {
    use cadcraft_geom::Vec3;

    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.run("view.set", serde_json::json!({ "preset": "se_isometric" })).unwrap();
    // Target with non-zero Z (typical after 3D orbit/zoom about 3D geometry).
    let v0 = cadcraft_engine::View { center: Vec2::new(10.0, 20.0), height: 100.0, twist: 0.0, target_z: 40.0 };
    app.session.state_mut().unwrap().set_view_quiet(v0);

    let ctx = egui::Context::default();
    let start = pos2(700.0, 450.0);
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(start)]);

    // Press MMB and hold it still for several frames: view must NOT drift.
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Middle,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for _ in 0..5 {
        drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(start)]);
    }
    let held = app.session.state().unwrap().view();
    assert!(
        (held.target() - v0.target()).len() < 1e-9,
        "holding MMB stationary with target_z != 0 must not drift: before={:?}, after={:?}",
        v0.target(),
        held.target()
    );

    // Drag MMB across canvas: a 3D world point must follow the cursor delta 1:1.
    let obj_3d = Vec3::new(14.0, 18.0, 42.0);
    let screen_before = app.canvas.xf.unwrap().to_screen_3d(obj_3d);
    let drag_delta = vec2(90.0, -55.0);
    let end = start + drag_delta;
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(end),
            egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Middle,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let screen_after = app.canvas.xf.unwrap().to_screen_3d(obj_3d);
    assert!(
        ((screen_after - screen_before) - drag_delta).length() < 0.5,
        "MMB 3D pan must move geometry by drag_delta={drag_delta:?}, got {:?}",
        screen_after - screen_before
    );
}

#[test]
fn mmb_pan_in_front_view_moves_target_z_vertically() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.run("view.set", serde_json::json!({ "preset": "front", "center": [10.0, 20.0], "height": 90.0 })).unwrap();

    let ctx = egui::Context::default();
    let start = pos2(700.0, 450.0);
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(start)]);
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Middle,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let xf0 = app.canvas.xf.unwrap();
    let dy = 90.0_f32;
    let end = start + vec2(0.0, dy);
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
    let v1 = app.session.state().unwrap().view();
    let expected_z = f64::from(dy) / xf0.scale;
    assert!(
        (v1.target_z - expected_z).abs() < 1e-4,
        "vertical MMB pan in Front view must lift target_z by dy/scale ({expected_z}), got {}",
        v1.target_z
    );
    assert!(
        v1.center.near(Vec2::new(10.0, 20.0), 1e-6),
        "pure vertical MMB pan in Front view must keep XY center unchanged, got {:?}",
        v1.center
    );
}

#[test]
fn front_view_hover_and_click_pick_entities_across_depth() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.run("line", serde_json::json!({ "points": [[0, -300], [10, -300]] })).unwrap();
    app.run("line", serde_json::json!({ "points": [[20, 400], [30, 400]] })).unwrap();
    let handles = app.session.doc().unwrap().model.handles();
    let (h_back, h_front) = (handles[0], handles[1]);
    app.run("view.set", serde_json::json!({ "preset": "front", "center": [15.0, 0.0], "height": 40.0 })).unwrap();

    let ctx = egui::Context::default();
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(pos2(500.0, 400.0))]);
    let xf = app.canvas.xf.unwrap();
    let sp_back = xf.to_screen(Vec2::new(5.0, -300.0));
    let sp_front = xf.to_screen(Vec2::new(25.0, 400.0));

    // Hovering and clicking h_back (Y = -300) picks it in edge-on Front view.
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(sp_back)]);
    assert_eq!(app.canvas.hover, Some(h_back));
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(sp_back),
            egui::Event::PointerButton { pos: sp_back, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            egui::Event::PointerButton { pos: sp_back, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
        ],
    );
    assert_eq!(app.session.selection(), vec![h_back]);

    // Hovering and clicking h_front (Y = +400) also picks it accurately.
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(sp_front)]);
    assert_eq!(app.canvas.hover, Some(h_front));
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(sp_front),
            egui::Event::PointerButton { pos: sp_front, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            egui::Event::PointerButton { pos: sp_front, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
        ],
    );
    assert_eq!(app.session.selection(), vec![h_back, h_front]);
}

#[test]
fn front_view_window_crossing_and_wp_fence_select_and_render() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.run("line", serde_json::json!({ "points": [[0, -200], [10, -200]] })).unwrap();
    app.run("line", serde_json::json!({ "points": [[20, 300], [30, 300]] })).unwrap();
    let handles = app.session.doc().unwrap().model.handles();
    let (h_left, _h_right) = (handles[0], handles[1]);
    app.run("view.set", serde_json::json!({ "preset": "front", "center": [15.0, 0.0], "height": 40.0 })).unwrap();

    let ctx = egui::Context::default();
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(pos2(500.0, 400.0))]);
    let xf = app.canvas.xf.unwrap();

    // Drag left-to-right Window around h_left (U in [-17..-3], V in [-5..+5]); vertical span must survive edge-on unproject!
    let p_a = xf.screen_uv_to_screen(Vec2::new(-17.0, 5.0));
    let p_b = xf.screen_uv_to_screen(Vec2::new(-3.0, -5.0));
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(p_a),
            egui::Event::PointerButton { pos: p_a, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
        ],
    );
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(p_b)]);
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![egui::Event::PointerButton { pos: p_b, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }],
    );
    assert_eq!(app.session.selection(), vec![h_left]);
    app.session.set_selection(Vec::new());

    // Start ERASE -> WP (concave WPolygon) around h_right (U in [4..16], V in [-4..+4]), render overlay & commit.
    app.start("erase");
    app.session.input(cadcraft_engine::Input::Keyword("WP".into())).unwrap();
    let poly_uvs = [Vec2::new(4.0, 4.0), Vec2::new(10.0, 1.5), Vec2::new(16.0, 4.0), Vec2::new(16.0, -4.0), Vec2::new(4.0, -4.0)];
    for &uv in &poly_uvs {
        let sp = xf.screen_uv_to_screen(uv);
        drive_canvas_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(sp),
                egui::Event::PointerButton { pos: sp, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
                egui::Event::PointerButton { pos: sp, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
            ],
        );
    }
    assert!(app.session.select_mode.is_some());
    app.session.input(cadcraft_engine::Input::Enter).unwrap();
    app.session.input(cadcraft_engine::Input::Enter).unwrap();
    let remaining = app.session.doc().unwrap().model.handles();
    assert_eq!(remaining, vec![h_left]);
}

#[test]
fn live_selection_preview_highlights_window_polygon_and_shift_remove() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.run("line", serde_json::json!({ "points": [[0, -200], [10, -200]] })).unwrap();
    app.run("line", serde_json::json!({ "points": [[20, 300], [30, 300]] })).unwrap();
    let handles = app.session.doc().unwrap().model.handles();
    let (h_left, h_right) = (handles[0], handles[1]);
    app.run("view.set", serde_json::json!({ "preset": "front", "center": [15.0, 0.0], "height": 40.0 })).unwrap();

    let ctx = egui::Context::default();
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(pos2(500.0, 400.0))]);
    let xf = app.canvas.xf.unwrap();

    // Drag a left-to-right PendingWindow in Front view and check live preview_handles BEFORE release.
    let p_a = xf.screen_uv_to_screen(Vec2::new(-17.0, 5.0));
    let p_b = xf.screen_uv_to_screen(Vec2::new(-3.0, -5.0));
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(p_a),
            egui::Event::PointerButton { pos: p_a, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
        ],
    );
    // Tiny < 3px movement during press-drag stays below drag threshold.
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(p_a + vec2(1.0, 0.0))]);
    assert!(app.canvas.preview_handles.is_empty(), "sub-3px drag must not flash preview");

    // Drag across h_left updates preview_handles immediately.
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(p_b)]);
    assert_eq!(app.canvas.preview_handles, vec![h_left]);
    assert_eq!(app.canvas.preview_action, cadcraft_engine::PreviewAction::Add);
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![egui::Event::PointerButton { pos: p_b, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }],
    );
    assert_eq!(app.session.selection(), vec![h_left]);

    // Shift + idle window over both lines shows PreviewAction::Remove only for already-selected h_left.
    let p_all_a = xf.screen_uv_to_screen(Vec2::new(-17.0, 5.0));
    let p_all_b = xf.screen_uv_to_screen(Vec2::new(17.0, -5.0));
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(p_all_a),
            egui::Event::PointerButton { pos: p_all_a, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::SHIFT },
            egui::Event::PointerButton { pos: p_all_a, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::SHIFT },
        ],
    );
    assert!(app.session.pending_window.is_some());
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::ModifiersChanged(egui::Modifiers::SHIFT), egui::Event::PointerMoved(p_all_b)]);
    assert_eq!(app.canvas.preview_action, cadcraft_engine::PreviewAction::Remove);
    assert_eq!(app.canvas.preview_handles, vec![h_left]);
    // Finish removal, then release Shift in the following frame (as a real user does).
    drive_canvas_events(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerButton { pos: p_all_b, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::SHIFT },
            egui::Event::PointerButton { pos: p_all_b, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::SHIFT },
        ],
    );
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)]);
    assert!(app.session.selection().is_empty(), "Shift+window must remove h_left");

    // ERASE -> WP live polygon preview updates as vertices form a closed region over h_right.
    app.start("erase");
    app.session.input(cadcraft_engine::Input::Keyword("WP".into())).unwrap();
    let v0 = xf.screen_uv_to_screen(Vec2::new(4.0, 5.0));
    let v1 = xf.screen_uv_to_screen(Vec2::new(16.0, 5.0));
    let v2 = xf.screen_uv_to_screen(Vec2::new(16.0, -5.0));
    let v3_cursor = xf.screen_uv_to_screen(Vec2::new(4.0, -5.0));
    for v in [v0, v1, v2] {
        drive_canvas_events(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(v),
                egui::Event::PointerButton { pos: v, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
                egui::Event::PointerButton { pos: v, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
            ],
        );
    }
    drive_canvas_events(&ctx, &mut app, vec![egui::Event::PointerMoved(v3_cursor)]);
    assert_eq!(app.canvas.preview_handles, vec![h_right], "polygon preview highlights inside-polygon entity dynamically");
    app.session.input(cadcraft_engine::Input::Cancel).unwrap();
}

#[test]
fn orbit_pivot_marker_hidden_in_free_camera_and_shown_when_disabled() {
    let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
    app.session.cmdline("3dorbit").unwrap();
    let ctx = egui::Context::default();
    let has_pivot_circle = |shapes: &[egui::epaint::ClippedShape]| {
        shapes.iter().any(|cs| {
            matches!(
                &cs.shape,
                egui::Shape::Circle(c) if c.fill == Color32::from_rgb(0x3a, 0xb5, 0x4a)
            )
        })
    };
    let render_frame = |app: &mut CadApp| {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            focused: true,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        std::mem::take(&mut out.shapes)
    };

    assert!(app.session.settings.orbit_free_camera);
    let free_shapes = render_frame(&mut app);
    assert!(!has_pivot_circle(&free_shapes), "free camera mode hides the green orbit pivot dot");

    app.run("setvar", serde_json::json!({ "name": "ORBITFREECAM", "value": 0 })).unwrap();
    let autocad_shapes = render_frame(&mut app);
    assert!(has_pivot_circle(&autocad_shapes), "non-free camera mode shows the green orbit pivot dot");
}
