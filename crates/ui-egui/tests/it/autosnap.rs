//! AutoSnap settings (AUTOSNAP bits, APBOX): the magnet holds the crosshair on the snap point,
//! the aperture box frames it, the marker and the tooltip can be turned off, and Options ▸
//! Drafting offers them all.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::mode;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Pos2, Rect, Shape, vec2};
use egui_kittest::Harness;
use serde_json::{Value, json};

/// Two crossing lines in model space, zoomed to fill the view, Endpoint and Intersection running.
fn harness() -> Harness<'static, CadApp> {
    let mut d = Drawing::new_imperial();
    for (a, b) in [(Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0)), (Vec2::new(10.0, -10.0), Vec2::new(10.0, 10.0))] {
        d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: a.to3(0.0), b: b.to3(0.0) })).unwrap();
    }
    let mut session = Session::empty();
    session.open_drawing(d, "AutoSnap", None);
    session.execute("view.set", &json!({"center": [10, 0], "height": 30})).unwrap();
    session.settings.osmode = mode::END | mode::INT;
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = false;
    // `CADCRAFT_AUTOSNAP_SHOTS=dir` renders the frames to PNGs there (needs a GPU), for a look.
    let builder = Harness::builder().with_size(vec2(1600.0, 1000.0));
    let builder = if shots().is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn shots() -> Option<std::path::PathBuf> {
    std::env::var_os("CADCRAFT_AUTOSNAP_SHOTS").map(std::path::PathBuf::from)
}

fn shot(h: &mut Harness<'_, CadApp>, name: &str) {
    if let Some(dir) = shots() {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join(format!("{name}.png"))).unwrap();
    }
}

fn setvar(h: &mut Harness<'_, CadApp>, name: &str, value: Value) {
    h.state_mut().session.execute("setvar", &json!({"name": name, "value": value})).unwrap();
    h.run_steps(3);
}

/// Moves the pointer `off` pixels from `point`'s place on screen and returns where it is.
fn hover(h: &mut Harness<'_, CadApp>, point: Vec2, off: egui::Vec2) -> Pos2 {
    let pos = h.state().canvas.xf.unwrap().to_screen(point) + off;
    h.event(Event::PointerMoved(pos));
    h.run_steps(3);
    pos
}

fn collect(shape: &Shape, rects: &mut Vec<(Rect, egui::Color32)>, text: &mut String) {
    match shape {
        Shape::Rect(r) if r.fill == egui::Color32::TRANSPARENT && !r.stroke.is_empty() => rects.push((r.rect, r.stroke.color)),
        Shape::Text(s) => {
            text.push_str(s.galley.text());
            text.push('\n');
        }
        Shape::Vec(v) => v.iter().for_each(|s| collect(s, rects, text)),
        _ => {}
    }
}

/// The stroked, unfilled rectangles and the text painted in the last frame.
fn painted(h: &Harness<'_, CadApp>) -> (Vec<(Rect, egui::Color32)>, String) {
    let (mut rects, mut text) = (Vec::new(), String::new());
    h.output().shapes.iter().for_each(|s| collect(&s.shape, &mut rects, &mut text));
    (rects, text)
}

/// The aperture box: a square of 2 × APERTURE pixels near the pointer at `pos`.
fn aperture_box(h: &Harness<'_, CadApp>, pos: Pos2) -> Option<Rect> {
    let side = 2.0 * h.state().session.settings.aperture as f32;
    let (rects, _) = painted(h);
    rects.into_iter().map(|(r, _)| r).find(|r| (r.width() - side).abs() < 0.6 && (r.height() - side).abs() < 0.6 && r.center().distance(pos) < 60.0)
}

#[test]
fn magnet_holds_the_crosshair_on_the_snap_point_and_the_aperture_box_frames_it() {
    let mut h = harness();
    setvar(&mut h, "APBOX", json!(1));
    h.state_mut().cmdline("line");
    h.run_steps(2);
    let pos = hover(&mut h, Vec2::new(20.0, 0.0), vec2(4.0, 3.0));
    let hit = h.state().canvas.snap.expect("the endpoint is found");
    assert_eq!(hit.mode, mode::END);
    let snap_px = h.state().canvas.xf.unwrap().to_screen(hit.point);

    // Magnet on (the default): the aperture box, drawn around the crosshair, sits on the snap point.
    shot(&mut h, "magnet-on");
    let b = aperture_box(&h, pos).expect("APBOX draws the aperture box at a point prompt");
    assert!(b.center().distance(snap_px) < 0.75, "box {b:?} on the snap at {snap_px:?}");
    assert!(b.center().distance(pos) > 2.0, "not under the mouse at {pos:?}");

    // Magnet off: the crosshair stays under the mouse while the marker still shows the snap.
    let bits = h.state().session.settings.autosnap() & !4;
    setvar(&mut h, "AUTOSNAP", json!(bits));
    assert!(h.state().canvas.snap.is_some(), "the snap still holds the pick");
    shot(&mut h, "magnet-off");
    let b = aperture_box(&h, pos).expect("the box follows the crosshair");
    assert!(b.center().distance(pos) < 0.75, "box {b:?} under the mouse at {pos:?}");

    // No box without APBOX, and none outside a point prompt.
    setvar(&mut h, "APBOX", json!(0));
    assert!(aperture_box(&h, pos).is_none());
    setvar(&mut h, "APBOX", json!(1));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().session.running.is_none());
    h.event(Event::PointerMoved(pos + vec2(1.0, 0.0)));
    h.run_steps(3);
    assert!(aperture_box(&h, pos).is_none(), "no point prompt, no box");
}

#[test]
fn a_one_off_snap_decides_for_the_aperture_box() {
    let mut h = harness();
    setvar(&mut h, "APBOX", json!(1));
    h.state_mut().cmdline("line");
    h.run_steps(2);
    let near_end = |h: &mut Harness<'_, CadApp>| hover(h, Vec2::new(20.0, 0.0), vec2(4.0, 3.0));
    let pos = near_end(&mut h);
    assert!(aperture_box(&h, pos).is_some(), "running snaps on: the box");

    // NON while the running snaps are on: this pick takes no snap, so no box.
    h.state_mut().cmdline("non");
    h.run_steps(2);
    let pos = near_end(&mut h);
    assert!(aperture_box(&h, pos).is_none(), "NON: no box");
    h.state_mut().cmdline("5,5");
    h.run_steps(2);

    // F3 has the running snaps off: no box, until END asks for this pick's endpoint.
    h.state_mut().session.settings.osmode |= mode::OFF;
    let pos = near_end(&mut h);
    assert!(aperture_box(&h, pos).is_none(), "running snaps off: no box");
    h.state_mut().cmdline("end");
    h.run_steps(2);
    let pos = near_end(&mut h);
    assert!(aperture_box(&h, pos).is_some(), "END: the box for this pick");
}

#[test]
fn marker_and_tooltip_follow_their_autosnap_bits() {
    let mut h = harness();
    h.state_mut().cmdline("line");
    h.run_steps(2);
    let pos = hover(&mut h, Vec2::new(0.0, 0.0), vec2(3.0, 2.0));
    assert!(h.state().canvas.snap.is_some());
    let endpoints = |h: &Harness<'_, CadApp>| painted(h).1.matches("Endpoint").count();
    let marker_squares = |h: &Harness<'_, CadApp>| {
        let side = 2.0 * cadcraft_ui_egui::theme::snap_marker_half();
        painted(h).0.iter().filter(|(r, _)| (r.width() - side).abs() < 0.6 && r.center().distance(pos) < 30.0).count()
    };
    let with_both = endpoints(&h);
    assert!(with_both >= 1, "the tooltip names the snap");
    assert_eq!(marker_squares(&h), 1, "the Endpoint marker is a square");

    let bits = h.state().session.settings.autosnap();
    setvar(&mut h, "AUTOSNAP", json!(bits & !2));
    assert_eq!(endpoints(&h), with_both - 1, "AUTOSNAP without bit 2: no tooltip");
    assert_eq!(marker_squares(&h), 1, "the marker stays");

    setvar(&mut h, "AUTOSNAP", json!(bits & !3));
    assert_eq!(marker_squares(&h), 0, "AUTOSNAP without bit 1: no marker");
    assert!(h.state().canvas.snap.is_some(), "the pick still lands on the snap");
}

/// Two frames (windows fade in), returning the painted text of the last.
fn render(app: &mut CadApp, ctx: &egui::Context) -> String {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))), ..Default::default() };
    let mut text = String::new();
    for _ in 0..2 {
        app.logic(ctx);
        let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
        let (mut rects, mut t) = (Vec::new(), String::new());
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut rects, &mut t));
        text = t;
        output.textures_delta.clear();
    }
    text
}

#[test]
fn options_json_and_the_drafting_tab_offer_the_autosnap_settings() {
    let mut a = CadApp::new(Session::new(), Services::default());
    let r = a.run("options", json!({"AUTOSNAP": 36, "APBOX": true})).unwrap();
    assert_eq!((r["AUTOSNAP"].clone(), r["APBOX"].clone()), (json!(36), json!(1)));
    let s = &a.session.settings;
    assert!(s.autosnap_magnet && s.autotrack_tooltip && !s.autosnap_marker && !s.autosnap_tooltip && s.apbox);
    assert!(a.run("options", json!({"AUTOSNAP": 64})).is_err(), "0–63 only");
    assert!(a.run("options", json!({"APBOX": 2})).is_err(), "APBOX is 0 or 1");

    a.ui.dialog = Some("options:drafting".into());
    let text = render(&mut a, &egui::Context::default());
    for label in ["AutoSnap settings", "Marker", "Magnet", "Display AutoSnap tooltip", "Display AutoSnap aperture box", "Display AutoTrack tooltip"] {
        assert!(text.contains(label), "{label}: {text}");
    }
}
