//! Touches through `CadApp` as the web app delivers them: a tap places a point at the crosshair,
//! and two fingers pan the drawing.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Drawing;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::touch::{DOUBLE_TIME, SETTLE_TIME};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, TouchDeviceId, TouchId, TouchPhase, vec2};
use egui_kittest::Harness;
use serde_json::json;

const STEP: f64 = 1.0 / 60.0;

struct Touches {
    h: Harness<'static, CadApp>,
    now: f64,
    /// Whether events pass through the virtual mouse, as on the web.
    hooked: bool,
}

impl Touches {
    fn new(hooked: bool) -> Self {
        let mut session = Session::empty();
        session.open_drawing(Drawing::new_imperial(), "Touch input", None);
        session.execute("view.set", &json!({"center":[50,50],"height":100})).unwrap();
        session.settings.osmode = 0;
        session.settings.snapmode = false;
        session.settings.orthomode = false;
        let mut app = CadApp::new(session, Services::default());
        app.canvas.zoom_pending = false;
        let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(5);
        Self { h, now: 100.0, hooked }
    }

    /// One frame carrying `events`, after the hook the web app installs when `hooked`.
    fn frame(&mut self, events: Vec<Event>) {
        self.now += STEP;
        let mut raw = egui::RawInput { time: Some(self.now), events, ..Default::default() };
        if self.hooked {
            let ctx = self.h.ctx.clone();
            self.h.state_mut().touch_hook(&ctx, &mut raw);
        }
        for event in raw.events {
            self.h.event(event);
        }
        self.h.run_steps(1);
    }

    fn wait(&mut self, secs: f64) {
        let until = self.now + secs;
        while self.now < until {
            self.frame(Vec::new());
        }
    }

    fn tap(&mut self, at: Pos2) {
        self.frame(vec![touch(1, TouchPhase::Start, at)]);
        self.frame(vec![touch(1, TouchPhase::End, at)]);
        self.wait(DOUBLE_TIME + 3.0 * STEP);
    }

    fn crosshair(&self) -> Pos2 {
        self.h.state().touch.crosshair().expect("a canvas touch shows the crosshair")
    }

    fn canvas_point(&self, at: Pos2) -> Vec2 {
        self.h.state().canvas.xf.unwrap().to_world(at)
    }

    fn screen_of(&self, p: Vec2) -> Pos2 {
        self.h.state().canvas.xf.unwrap().to_screen(p)
    }

    fn set_twist(&mut self, twist: f64) {
        let st = self.h.state_mut().session.state_mut().unwrap();
        let view = st.view();
        st.set_view_quiet(cadcraft_engine::View { twist, ..view });
        self.h.run_steps(2);
    }

    /// Two fingers land around `at`, and move together by `by` over several frames.
    fn two_finger_pan(&mut self, at: Pos2, by: egui::Vec2) {
        let (a, b) = (at - vec2(50.0, 0.0), at + vec2(50.0, 0.0));
        // A native backend also makes the first finger a left press there.
        let press = Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE };
        let native = if self.hooked { Vec::new() } else { vec![Event::PointerMoved(a), press] };
        self.frame([vec![touch(1, TouchPhase::Start, a)], native].concat());
        self.frame(vec![touch(2, TouchPhase::Start, b)]);
        self.wait(SETTLE_TIME);
        for i in 1..=4 {
            let d = by * (i as f32 / 4.0);
            self.frame(vec![touch(1, TouchPhase::Move, a + d), touch(2, TouchPhase::Move, b + d)]);
        }
        self.frame(vec![touch(1, TouchPhase::End, a + by), touch(2, TouchPhase::End, b + by)]);
        self.h.run_steps(2);
    }
}

fn touch(id: u64, phase: TouchPhase, pos: Pos2) -> Event {
    Event::Touch { device_id: TouchDeviceId(0), id: TouchId(id), phase, pos, force: None }
}

#[test]
fn a_tap_at_a_line_prompt_places_the_point_at_the_crosshair() {
    let mut t = Touches::new(true);
    let at = t.screen_of(Vec2::new(30.0, 70.0));
    t.h.state_mut().start("line");
    t.h.run_steps(2);
    assert_eq!(t.h.state().session.running.as_ref().unwrap().id, "line");

    // The first touch only shows the crosshair.
    t.tap(at);
    assert!(t.h.state().session.current_prompt().unwrap().base.is_none(), "showing the crosshair places nothing");

    let crosshair = t.crosshair();
    let expected = t.canvas_point(crosshair);
    t.tap(at);
    let base = t.h.state().session.current_prompt().unwrap().base.expect("the tap placed the first point");
    assert!(base.near(expected, 1e-6), "placed at the crosshair {expected:?}, not at the finger: {base:?}");
    assert!(!base.near(t.canvas_point(at), 1.0), "the finger was elsewhere");
}

#[test]
fn two_fingers_drag_the_drawing_with_them_in_a_twisted_view() {
    for twist in [0.0, std::f64::consts::FRAC_PI_2, 0.5] {
        let mut t = Touches::new(true);
        t.set_twist(twist);
        let at = t.screen_of(Vec2::new(50.0, 50.0));
        let world = t.canvas_point(at);
        let by = vec2(60.0, -25.0);
        t.two_finger_pan(at, by);
        let moved = t.screen_of(world);
        assert!((moved - (at + by)).length() < 1.0, "twist {twist}: the point under the fingers follows them to {:?}, is at {moved:?}", at + by);
        assert_eq!(t.h.state().session.state().unwrap().view().twist, twist, "panning keeps the twist");
    }
}

#[test]
fn without_the_virtual_mouse_two_fingers_do_not_pan() {
    let mut t = Touches::new(false);
    let at = t.screen_of(Vec2::new(50.0, 50.0));
    let before = t.h.state().session.state().unwrap().view().center;
    t.two_finger_pan(at, vec2(60.0, -25.0));
    assert!(t.h.state().session.state().unwrap().view().center.near(before, 1e-9), "the first finger is the backend's left drag there");
}
