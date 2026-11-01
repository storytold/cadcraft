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
    let xf = Xf { rect, center: Vec2::new(5.0, 0.0), scale: 10.0 };
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
    app.session.pending_window = Some(cadcraft_engine::PendingWindow { corner: Vec2::ZERO, during_command: true });
    let text = painted_text(&mut app, &ctx, at);
    assert!(text.contains(&format!("   {}  <", arch(c.len()))), "no {} in:\n{text}", arch(c.len()));
}
