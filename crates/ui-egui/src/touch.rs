//! A virtual mouse for touch screens.
//!
//! A touch on the drawing canvas never points where the finger is: the canvas keeps a crosshair,
//! and touches drive it like a trackpad.
//!
//! - one-finger drag moves the crosshair by the finger's motion, with no button;
//! - a tap anywhere on the canvas clicks at the crosshair, once the double-tap window has passed;
//! - a double tap clicks twice at the crosshair (a double click);
//! - double tap and hold presses at the crosshair, the drag moves the crosshair with the button
//!   held, and lifting releases at the crosshair;
//! - two or more fingers make the sequence a gesture (pinch zoom and two-finger pan read the
//!   touch events themselves): no buttons until every finger has lifted;
//! - the crosshair stays where it is when the finger lifts.
//!
//! The web backend turns a touch into three streams: [`Event::Touch`], pointer events it
//! synthesizes from the touch events in the same callback (press on start, move, release and
//! gone on end, at the finger), and the browser's own pointer events for the touch (`pointerdown`,
//! `pointerup`), each delivered alone in a frame of its own, plus any mouse events the browser
//! emulates after the touch. [`NativeGate`] decides which browser events reach the backend at
//! all: the browser's pointer events for touches, and emulated mouse events right after a touch,
//! do not. [`TouchMouse::filter`] then drops the synthesized pointer events of canvas touch
//! sequences, drops lone pointer events that arrive during or just after one, and appends its
//! own pointer events at the crosshair instead.
//!
//! Touch sequences that start off the canvas (menus, panels, dialogs over the canvas) and mouse
//! input pass through unchanged.

use std::collections::VecDeque;

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, TouchId, TouchPhase, Vec2};

/// A finger that moves no further than this (points) can still be a tap.
pub const SLOP: f32 = 10.0;
/// A tap lasts less than this (seconds); the second touch of a double tap held this long
/// presses.
pub const TAP_TIME: f64 = 0.25;
/// A touch starting within this long after a tap (seconds) continues a double tap.
pub const DOUBLE_TIME: f64 = 0.3;
/// A one-finger touch moves nothing before this long (seconds), so a second finger landing a
/// little late still makes a gesture with nothing to undo.
pub const SETTLE_TIME: f64 = 0.08;
/// Lone pointer events this soon after a canvas touch sequence ends (seconds) belong to it.
pub const QUIET_TIME: f64 = 0.5;
/// Mouse events this soon after a touch event (seconds) are the browser's emulation of it.
pub const COMPAT_TIME: f64 = 1.0;

/// A browser event on the canvas, as [`NativeGate`] sorts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomEvent {
    /// `touchstart`, `touchmove`, `touchend`, `touchcancel`.
    Touch,
    /// `pointerdown`, `pointerup`, `pointermove`, `pointercancel`; `touch` when the pointer is a
    /// finger.
    Pointer { touch: bool },
    /// `mousedown`, `mouseup`, `mousemove`, `mouseleave`, `click`.
    Mouse,
}

/// Which browser events on the canvas reach the web backend. Touches reach it as touch events
/// only: the browser's pointer events for a finger, and mouse events it emulates after a touch,
/// are stopped.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeGate {
    last_touch: Option<f64>,
}

impl NativeGate {
    /// Whether the event `e` at time `now` (seconds) reaches the backend.
    pub fn admit(&mut self, e: DomEvent, now: f64) -> bool {
        match e {
            DomEvent::Touch | DomEvent::Pointer { touch: true } => {
                self.last_touch = Some(now);
                e == DomEvent::Touch
            }
            DomEvent::Pointer { touch: false } => true,
            DomEvent::Mouse => self.last_touch.is_none_or(|t| now - t > COMPAT_TIME),
        }
    }
}

/// The id of the page's command-line input on touch screens, which a touch on the command bar
/// focuses to open the on-screen keyboard.
pub const COMMAND_INPUT_ID: &str = "mobile_command_input";

/// Whether a stopped touch `pointerup` focuses the canvas, given the id of the element holding
/// focus: not while the page's command input holds it, so typing keeps reaching that input.
pub fn focus_canvas_on_lift(active_id: Option<&str>) -> bool {
    active_id != Some(COMMAND_INPUT_ID)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Not yet a drag: may still be a tap or become a gesture.
    Open,
    /// One finger moving the crosshair.
    Drag,
    /// One finger dragging with the primary button held (double tap and hold).
    Press,
    /// Two or more fingers: ignored until every finger lifts.
    Gesture,
}

#[derive(Clone, Copy, Debug)]
struct Seq {
    primary: TouchId,
    start_time: f64,
    start: Pos2,
    last: Pos2,
    moved: f32,
    class: Class,
    /// Began within [`DOUBLE_TIME`] of the previous tap.
    double: bool,
    /// This touch showed the first crosshair, so its tap only shows it.
    placed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Idle,
    Canvas,
    Pass,
}

/// The virtual-mouse state machine (see the module docs). Holds no egui context.
#[derive(Debug)]
pub struct TouchMouse {
    crosshair: Option<Pos2>,
    mode: Mode,
    fingers: Vec<TouchId>,
    seq: Option<Seq>,
    /// When the last tap lifted.
    last_tap: Option<f64>,
    /// A tap's click waiting out the double-tap window: when it fires.
    pending: Option<f64>,
    out: VecDeque<Event>,
    modifiers: Modifiers,
    /// The touch event being filtered ended a canvas sequence.
    ended_on_canvas: bool,
    /// Lone pointer events before this time belong to the canvas sequence that just ended.
    quiet_until: f64,
}

impl Default for TouchMouse {
    fn default() -> Self {
        Self {
            crosshair: None,
            mode: Mode::Idle,
            fingers: Vec::new(),
            seq: None,
            last_tap: None,
            pending: None,
            out: VecDeque::new(),
            modifiers: Modifiers::default(),
            ended_on_canvas: false,
            quiet_until: f64::NEG_INFINITY,
        }
    }
}

fn is_synthesized_pointer(e: &Event) -> bool {
    matches!(e, Event::PointerMoved(_) | Event::PointerGone | Event::PointerButton { button: PointerButton::Primary, .. })
}

impl TouchMouse {
    /// The crosshair the virtual mouse points at, once a canvas touch has shown it.
    pub fn crosshair(&self) -> Option<Pos2> {
        self.crosshair
    }

    /// Whether the touch sequence in progress started off the canvas, so its fingers belong to
    /// whatever they landed on (a panel, a menu) and never navigate the drawing.
    pub fn passing(&self) -> bool {
        self.mode == Mode::Pass
    }

    /// Whether frames must keep coming without input (a click waiting out the double-tap
    /// window, a double tap's second touch that may become a hold, or queued pointer events).
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.out.is_empty() || self.seq.is_some_and(|s| s.double && s.class == Class::Open)
    }

    /// Rewrite one frame's `events` at time `now` (seconds). `on_canvas` says whether a touch
    /// starting at a point starts on the canvas; `canvas` (when known) bounds the crosshair.
    pub fn filter(&mut self, events: &mut Vec<Event>, now: f64, canvas: Option<Rect>, on_canvas: &dyn Fn(Pos2) -> bool) {
        let input = std::mem::take(events);
        let mut held: Vec<Event> = Vec::new();
        for e in input {
            if let Event::ModifiersChanged(m) = e {
                self.modifiers = m;
            }
            if is_synthesized_pointer(&e) {
                held.push(e);
                continue;
            }
            if let Event::Touch { id, phase, pos, .. } = e {
                self.touch(id, phase, pos, now, canvas, on_canvas);
                // The pointer events the backend made for this touch precede it.
                if self.mode == Mode::Canvas || self.ended_on_canvas {
                    held.clear();
                } else {
                    events.append(&mut held);
                }
                self.ended_on_canvas = false;
            }
            events.push(e);
        }
        // Pointer events with no touch after them: during or just after a canvas sequence they
        // are the browser's own events for its fingers.
        if self.mode != Mode::Canvas && now >= self.quiet_until {
            events.append(&mut held);
        }
        self.expire(now);
        self.drain(events);
    }

    fn touch(&mut self, id: TouchId, phase: TouchPhase, pos: Pos2, now: f64, canvas: Option<Rect>, on_canvas: &dyn Fn(Pos2) -> bool) {
        match phase {
            TouchPhase::Start => {
                if self.fingers.is_empty() {
                    self.mode = if on_canvas(pos) { Mode::Canvas } else { Mode::Pass };
                    if self.mode == Mode::Canvas {
                        self.begin(id, pos, now, canvas);
                    }
                }
                if !self.fingers.contains(&id) {
                    self.fingers.push(id);
                }
                if self.mode == Mode::Canvas && self.fingers.len() > 1 {
                    self.gesture();
                }
            }
            TouchPhase::Move => {
                if self.mode == Mode::Canvas {
                    self.moved(id, pos, now, canvas);
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                self.fingers.retain(|f| *f != id);
                if self.mode == Mode::Canvas && self.seq.is_some_and(|s| s.primary == id) {
                    self.lift(pos, now, phase == TouchPhase::Cancel, canvas);
                }
                if self.fingers.is_empty() {
                    if self.mode == Mode::Canvas {
                        self.ended_on_canvas = true;
                        self.quiet_until = now + QUIET_TIME;
                    }
                    self.mode = Mode::Idle;
                    self.seq = None;
                }
            }
        }
    }

    fn begin(&mut self, id: TouchId, pos: Pos2, now: f64, canvas: Option<Rect>) {
        let placed = self.crosshair.is_none();
        // The canvas may have shrunk since the crosshair was placed (an on-screen keyboard).
        let cross = match (self.crosshair, canvas) {
            (Some(c), r) => clamp(c, r),
            (None, Some(r)) if r.is_positive() => r.center(),
            (None, _) => pos,
        };
        self.crosshair = Some(cross);
        // Show the crosshair, and restore hover after a touch elsewhere.
        self.out.push_back(Event::PointerMoved(cross));
        let double = self.last_tap.is_some_and(|t| now - t <= DOUBLE_TIME);
        if !double {
            self.fire_pending();
        }
        self.seq = Some(Seq { primary: id, start_time: now, start: pos, last: pos, moved: 0.0, class: Class::Open, double, placed });
    }

    fn gesture(&mut self) {
        let Some(seq) = self.seq.as_mut() else { return };
        let was = seq.class;
        seq.class = Class::Gesture;
        match was {
            // A double tap's first click still stands.
            Class::Open => self.fire_pending(),
            Class::Press => self.button(false),
            Class::Drag | Class::Gesture => {}
        }
        self.last_tap = None;
    }

    fn moved(&mut self, id: TouchId, pos: Pos2, now: f64, canvas: Option<Rect>) {
        let Some(mut seq) = self.seq else { return };
        if seq.primary != id || seq.class == Class::Gesture {
            return;
        }
        seq.moved = seq.moved.max(seq.start.distance(pos));
        let delta = match seq.class {
            Class::Open if seq.moved > SLOP && now - seq.start_time >= SETTLE_TIME => {
                if seq.double {
                    self.press();
                    seq.class = Class::Press;
                } else {
                    seq.class = Class::Drag;
                }
                pos - seq.start
            }
            Class::Open | Class::Gesture => Vec2::ZERO,
            Class::Drag | Class::Press => pos - seq.last,
        };
        if seq.class != Class::Open {
            seq.last = pos;
        }
        self.seq = Some(seq);
        if delta != Vec2::ZERO {
            self.nudge(delta, canvas);
        }
    }

    fn lift(&mut self, pos: Pos2, now: f64, cancelled: bool, canvas: Option<Rect>) {
        let Some(seq) = self.seq else { return };
        match seq.class {
            Class::Gesture => {}
            Class::Press => {
                self.button(false);
                self.last_tap = None;
            }
            Class::Drag => self.last_tap = None,
            Class::Open => {
                let tap = !cancelled && seq.moved.max(seq.start.distance(pos)) <= SLOP && now - seq.start_time < TAP_TIME;
                self.fire_pending();
                if !tap {
                    // A one-finger drag that lifts before it settled still moves the crosshair.
                    let moved = seq.moved.max(seq.start.distance(pos));
                    if !cancelled && !seq.double && moved > SLOP {
                        self.nudge(pos - seq.start, canvas);
                    }
                    self.last_tap = None;
                } else if seq.double {
                    // The second tap of a double tap: its click follows the first.
                    self.click();
                    self.last_tap = None;
                } else {
                    if !seq.placed {
                        self.pending = Some(now + DOUBLE_TIME);
                    }
                    self.last_tap = Some(now);
                }
            }
        }
    }

    /// Press at the crosshair in place of a waiting first-tap click (double tap and hold).
    fn press(&mut self) {
        self.pending = None;
        self.button(true);
    }

    fn nudge(&mut self, delta: Vec2, canvas: Option<Rect>) {
        let Some(c) = self.crosshair else { return };
        let c = clamp(c + delta, canvas);
        self.crosshair = Some(c);
        self.out.push_back(Event::PointerMoved(c));
    }

    fn button(&mut self, pressed: bool) {
        let Some(pos) = self.crosshair else { return };
        self.out.push_back(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: self.modifiers });
    }

    fn click(&mut self) {
        self.button(true);
        self.button(false);
    }

    fn fire_pending(&mut self) {
        if self.pending.take().is_some() {
            self.click();
        }
    }

    /// Press for a double tap's second touch held past [`TAP_TIME`]; fire a waiting click once
    /// its window has passed, unless a touch that may continue the double tap is still down.
    fn expire(&mut self, now: f64) {
        if let Some(mut seq) = self.seq
            && seq.double
            && seq.class == Class::Open
            && now - seq.start_time >= TAP_TIME
        {
            self.press();
            seq.class = Class::Press;
            self.seq = Some(seq);
        }
        let deciding = self.seq.is_some_and(|s| s.double && s.class == Class::Open);
        if !deciding && self.pending.is_some_and(|t| now >= t) {
            self.fire_pending();
        }
    }

    /// Move queued events into this frame: at most one button event per frame, so a press and
    /// its release always reach egui in separate frames.
    fn drain(&mut self, events: &mut Vec<Event>) {
        let mut buttons = 0;
        while let Some(e) = self.out.front() {
            if matches!(e, Event::PointerButton { .. }) {
                if buttons > 0 {
                    break;
                }
                buttons += 1;
            }
            if let Some(e) = self.out.pop_front() {
                if let Event::PointerButton { pos, .. } = e {
                    events.push(Event::PointerMoved(pos));
                }
                events.push(e);
            }
        }
    }
}

fn clamp(p: Pos2, canvas: Option<Rect>) -> Pos2 {
    match canvas {
        Some(r) if r.is_positive() => p.clamp(r.min, r.max),
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: Rect = Rect { min: Pos2::new(0.0, 0.0), max: Pos2::new(800.0, 600.0) };
    const CENTRE: Pos2 = Pos2::new(400.0, 300.0);

    fn on_canvas(p: Pos2) -> bool {
        CANVAS.contains(p)
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::default() }
    }

    /// Drives a [`TouchMouse`] with every event the browser and the web backend produce for a
    /// touch: the browser's own pointer events (each run as a frame of its own), the touch events
    /// with the pointer events the backend synthesizes from them, and the mouse events the
    /// browser emulates after a lift, all through a [`NativeGate`] unless `gated` is off.
    struct Rig {
        m: TouchMouse,
        gate: NativeGate,
        gated: bool,
        canvas: Rect,
        t: f64,
        out: Vec<Event>,
        /// The backend's primary touch: its id and last position.
        primary: Option<(u64, Pos2)>,
        down: Vec<u64>,
    }

    impl Rig {
        fn new() -> Self {
            Self {
                m: TouchMouse::default(),
                gate: NativeGate::default(),
                gated: true,
                canvas: CANVAS,
                t: 10.0,
                out: Vec::new(),
                primary: None,
                down: Vec::new(),
            }
        }
        fn frame(&mut self, mut events: Vec<Event>) {
            self.m.filter(&mut events, self.t, Some(self.canvas), &on_canvas);
            self.out.extend(events);
        }
        /// A browser event: when admitted, the backend's events for it.
        fn dom(&mut self, kind: DomEvent, events: Vec<Event>) {
            if !self.gated || self.gate.admit(kind, self.t) {
                self.frame(events);
            }
        }
        fn wait(&mut self, secs: f64) {
            // Frames keep coming at 60 Hz.
            let end = self.t + secs;
            while self.t < end {
                self.t += 1.0 / 60.0;
                self.frame(Vec::new());
            }
        }
        fn touch(id: u64, phase: TouchPhase, pos: Pos2) -> Event {
            Event::Touch { device_id: egui::TouchDeviceId(0), id: TouchId(id), phase, pos, force: None }
        }
        fn primary_pos(&self, id: u64, pos: Pos2) -> Pos2 {
            match self.primary {
                Some((p, at)) if p != id => at,
                _ => pos,
            }
        }
        fn down(&mut self, id: u64, x: f32, y: f32) {
            let pos = Pos2::new(x, y);
            self.dom(DomEvent::Pointer { touch: true }, vec![button(pos, true)]);
            if self.primary.is_none() {
                self.primary = Some((id, pos));
            }
            self.down.push(id);
            let at = self.primary_pos(id, pos);
            self.dom(DomEvent::Touch, vec![button(at, true), Self::touch(id, TouchPhase::Start, pos)]);
        }
        fn mv(&mut self, id: u64, x: f32, y: f32) {
            let pos = Pos2::new(x, y);
            if self.primary.is_some_and(|(p, _)| p == id) {
                self.primary = Some((id, pos));
            }
            let at = self.primary_pos(id, pos);
            self.dom(DomEvent::Touch, vec![Event::PointerMoved(at), Self::touch(id, TouchPhase::Move, pos)]);
        }
        fn up(&mut self, id: u64, x: f32, y: f32) {
            let pos = Pos2::new(x, y);
            self.dom(DomEvent::Pointer { touch: true }, vec![button(pos, false)]);
            let at = self.primary_pos(id, pos);
            self.dom(DomEvent::Touch, vec![button(at, false), Event::PointerGone, Self::touch(id, TouchPhase::End, pos)]);
            if self.primary.is_some_and(|(p, _)| p == id) {
                self.primary = None;
            }
            self.down.retain(|d| *d != id);
            if self.down.is_empty() {
                // The emulated mouse events after the last finger lifts.
                self.t += 0.01;
                self.dom(DomEvent::Mouse, vec![Event::PointerMoved(pos)]);
                self.dom(DomEvent::Mouse, vec![]);
                self.dom(DomEvent::Mouse, vec![]);
            }
        }
        fn tap(&mut self, x: f32, y: f32) {
            self.down(1, x, y);
            self.t += 0.1;
            self.up(1, x, y);
        }
        fn drag(&mut self, from: Pos2, by: Vec2) {
            self.down(1, from.x, from.y);
            for i in 1..=10 {
                self.t += 0.02;
                let p = from + by * (i as f32 / 10.0);
                self.mv(1, p.x, p.y);
            }
            self.t += 0.02;
            self.up(1, from.x + by.x, from.y + by.y);
        }
        /// Every button event that reached egui so far: (pressed, pos).
        fn buttons(&self) -> Vec<(bool, Pos2)> {
            self.out
                .iter()
                .filter_map(|e| match e {
                    Event::PointerButton { pressed, pos, .. } => Some((*pressed, *pos)),
                    _ => None,
                })
                .collect()
        }
        fn clicks(&self) -> usize {
            self.buttons().iter().filter(|(p, _)| *p).count()
        }
        /// Every position egui's pointer was put at (moves and buttons both move it).
        fn pointer_positions(&self) -> Vec<Pos2> {
            self.out
                .iter()
                .filter_map(|e| match e {
                    Event::PointerMoved(p) | Event::PointerButton { pos: p, .. } => Some(*p),
                    _ => None,
                })
                .collect()
        }
        /// Where egui's pointer is now, or `None` if it was told the pointer is gone.
        fn pointer(&self) -> Option<Pos2> {
            self.out.iter().rev().find_map(|e| match e {
                Event::PointerMoved(p) | Event::PointerButton { pos: p, .. } => Some(Some(*p)),
                Event::PointerGone => Some(None),
                _ => None,
            })?
        }
        /// A rig whose crosshair is shown at the canvas centre.
        fn shown() -> Self {
            let mut r = Self::new();
            r.tap(100.0, 100.0);
            r.wait(0.5);
            r.out.clear();
            r
        }
    }

    #[test]
    fn first_touch_shows_crosshair_at_the_canvas_centre_without_clicking() {
        let mut r = Rig::new();
        r.tap(100.0, 100.0);
        r.wait(0.5);
        assert_eq!(r.clicks(), 0);
        assert_eq!(r.m.crosshair(), Some(CENTRE));
        assert_eq!(r.pointer(), Some(CENTRE), "the crosshair stays drawn after a lift");
    }

    #[test]
    fn one_finger_drag_moves_crosshair_relatively_without_buttons() {
        let mut r = Rig::shown();
        r.drag(Pos2::new(600.0, 500.0), Vec2::new(50.0, -30.0));
        r.wait(0.5);
        assert!(r.buttons().is_empty(), "{:?}", r.buttons());
        assert_eq!(r.m.crosshair(), Some(Pos2::new(450.0, 270.0)));
    }

    #[test]
    fn a_quick_drag_lifting_before_it_settles_still_moves_the_crosshair() {
        let mut r = Rig::shown();
        r.down(1, 600.0, 500.0);
        r.t += 0.02;
        r.mv(1, 630.0, 480.0);
        r.t += 0.02;
        r.up(1, 640.0, 470.0);
        r.wait(0.5);
        let end = CENTRE + Vec2::new(40.0, -30.0);
        assert!(r.buttons().is_empty(), "{:?}", r.buttons());
        assert_eq!(r.m.crosshair(), Some(end));
        assert_eq!(r.pointer(), Some(end));
    }

    #[test]
    fn a_touch_starting_off_the_canvas_passes_until_every_finger_lifts() {
        let mut m = TouchMouse::default();
        let on = |p: Pos2| p.y < 500.0;
        let panel = Pos2::new(50.0, 550.0);
        let canvas = Pos2::new(300.0, 300.0);
        let mut ev = vec![Rig::touch(1, TouchPhase::Start, panel)];
        m.filter(&mut ev, 1.0, Some(CANVAS), &on);
        assert!(m.passing());
        let mut ev = vec![Rig::touch(2, TouchPhase::Start, canvas), Rig::touch(1, TouchPhase::Move, canvas)];
        m.filter(&mut ev, 1.05, Some(CANVAS), &on);
        assert!(m.passing(), "a second finger on the canvas does not take the gesture over");
        let mut ev = vec![Rig::touch(1, TouchPhase::End, canvas), Rig::touch(2, TouchPhase::End, canvas)];
        m.filter(&mut ev, 1.1, Some(CANVAS), &on);
        assert!(!m.passing());
        let mut ev = vec![Rig::touch(3, TouchPhase::Start, canvas)];
        m.filter(&mut ev, 1.2, Some(CANVAS), &on);
        assert!(!m.passing(), "a canvas touch is the canvas's own");
    }

    #[test]
    fn a_drag_never_reaches_egui_as_a_press_at_the_finger() {
        let mut r = Rig::shown();
        r.drag(Pos2::new(600.0, 500.0), Vec2::new(50.0, -30.0));
        r.wait(0.5);
        let finger = |p: &Pos2| p.distance(Pos2::new(600.0, 500.0)) < 60.0;
        assert!(!r.pointer_positions().iter().any(finger), "egui's pointer went to the finger: {:?}", r.pointer_positions());
        assert!(r.buttons().is_empty());
    }

    #[test]
    fn crosshair_stays_where_dragged_not_where_the_finger_lifts() {
        let mut r = Rig::shown();
        r.drag(Pos2::new(600.0, 500.0), Vec2::new(-200.0, 50.0));
        r.wait(0.5);
        let c = Pos2::new(200.0, 350.0);
        assert_eq!(r.m.crosshair(), Some(c));
        assert_eq!(r.pointer(), Some(c), "egui's pointer ends on the crosshair, not the lift point (400, 550)");
    }

    #[test]
    fn the_ungated_backend_stream_presses_at_the_finger() {
        // Why the gate exists: the browser's own pointer events for a touch arrive alone, in frames
        // of their own, before the touch events that would identify them.
        let mut r = Rig::shown();
        r.gated = false;
        r.drag(Pos2::new(600.0, 500.0), Vec2::new(-200.0, 50.0));
        assert_eq!(r.buttons().first(), Some(&(true, Pos2::new(600.0, 500.0))));
    }

    #[test]
    fn crosshair_never_moves_to_a_tap() {
        let mut r = Rig::shown();
        r.tap(700.0, 50.0);
        r.wait(0.5);
        assert_eq!(r.m.crosshair(), Some(CENTRE));
        assert!(r.pointer_positions().iter().all(|p| *p == CENTRE), "{:?}", r.pointer_positions());
    }

    #[test]
    fn a_tap_anywhere_clicks_at_the_crosshair_after_the_window() {
        let mut r = Rig::shown();
        r.tap(700.0, 50.0);
        assert_eq!(r.clicks(), 0, "deferred by the double-tap window");
        assert!(r.m.busy());
        r.wait(0.4);
        assert_eq!(r.buttons(), vec![(true, CENTRE), (false, CENTRE)]);
        assert!(!r.m.busy());
    }

    #[test]
    fn a_tap_after_the_canvas_shrinks_clicks_inside_it() {
        let mut r = Rig::shown();
        r.canvas = Rect { min: Pos2::new(0.0, 0.0), max: Pos2::new(800.0, 200.0) };
        r.tap(100.0, 100.0);
        r.wait(0.4);
        let inside = Pos2::new(400.0, 200.0);
        assert_eq!(r.m.crosshair(), Some(inside));
        assert_eq!(r.buttons(), vec![(true, inside), (false, inside)]);
        assert!(r.pointer_positions().iter().all(|p| r.canvas.contains(*p)), "{:?}", r.pointer_positions());
    }

    #[test]
    fn a_long_press_without_moving_does_nothing() {
        let mut r = Rig::shown();
        r.down(1, 700.0, 50.0);
        r.wait(0.6);
        r.up(1, 702.0, 50.0);
        r.wait(0.5);
        assert!(r.buttons().is_empty());
        assert_eq!(r.m.crosshair(), Some(CENTRE));
    }

    #[test]
    fn press_and_release_reach_egui_in_separate_frames() {
        let mut r = Rig::shown();
        r.tap(100.0, 100.0);
        r.t += 0.4;
        let mut frames = Vec::new();
        for _ in 0..3 {
            let mut ev = Vec::new();
            r.m.filter(&mut ev, r.t, Some(CANVAS), &on_canvas);
            frames.push(ev.iter().filter(|e| matches!(e, Event::PointerButton { .. })).count());
        }
        assert_eq!(frames, vec![1, 1, 0]);
    }

    #[test]
    fn crosshair_stays_on_the_canvas() {
        let mut r = Rig::shown();
        r.drag(Pos2::new(700.0, 400.0), Vec2::new(-700.0, 0.0));
        assert_eq!(r.m.crosshair(), Some(Pos2::new(0.0, 300.0)));
    }

    #[test]
    fn double_tap_and_hold_drags_with_the_button_held_and_releases_at_the_crosshair() {
        let mut r = Rig::shown();
        r.tap(600.0, 500.0);
        r.t += 0.1;
        r.drag(Pos2::new(650.0, 450.0), Vec2::new(100.0, 50.0));
        r.wait(0.5);
        let end = CENTRE + Vec2::new(100.0, 50.0);
        assert_eq!(r.buttons(), vec![(true, CENTRE), (false, end)], "the drag replaces the first tap's click");
        assert_eq!(r.m.crosshair(), Some(end));
        assert_eq!(r.pointer(), Some(end));
        // The press comes before any drag motion.
        let press = r.out.iter().position(|e| matches!(e, Event::PointerButton { pressed: true, .. }));
        let moved = r.out.iter().position(|e| matches!(e, Event::PointerMoved(p) if p.x > CENTRE.x));
        assert!(press < moved);
    }

    #[test]
    fn double_tap_and_hold_presses_before_the_finger_moves() {
        let mut r = Rig::shown();
        r.tap(600.0, 500.0);
        r.t += 0.1;
        r.down(1, 600.0, 500.0);
        r.wait(0.4);
        assert_eq!(r.buttons(), vec![(true, CENTRE)]);
        r.mv(1, 640.0, 500.0);
        r.t += 0.02;
        r.mv(1, 660.0, 500.0);
        r.up(1, 660.0, 500.0);
        r.wait(0.5);
        let end = CENTRE + Vec2::new(60.0, 0.0);
        assert_eq!(r.buttons(), vec![(true, CENTRE), (false, end)]);
        assert_eq!(r.pointer(), Some(end));
    }

    #[test]
    fn double_tap_double_clicks_at_the_crosshair() {
        let mut r = Rig::shown();
        r.tap(600.0, 500.0);
        r.t += 0.1;
        r.tap(100.0, 50.0);
        r.wait(0.5);
        assert_eq!(r.buttons(), vec![(true, CENTRE), (false, CENTRE), (true, CENTRE), (false, CENTRE)]);
    }

    #[test]
    fn pinch_with_a_late_second_finger_and_staggered_lifts_never_clicks() {
        let mut r = Rig::shown();
        r.down(1, 300.0, 300.0);
        r.t += 0.05;
        r.mv(1, 306.0, 300.0);
        r.down(2, 500.0, 300.0);
        for i in 1..=10 {
            r.t += 0.02;
            r.mv(1, 306.0 - 10.0 * i as f32, 300.0);
            r.mv(2, 500.0 + 10.0 * i as f32, 300.0);
        }
        r.t += 0.05;
        r.up(1, 206.0, 300.0);
        r.t += 0.15;
        r.mv(2, 590.0, 300.0);
        r.up(2, 600.0, 300.0);
        r.wait(0.5);
        assert!(r.buttons().is_empty());
        assert_eq!(r.m.crosshair(), Some(CENTRE), "a gesture leaves the crosshair alone");
        assert_eq!(r.pointer(), Some(CENTRE));
        let touches = r.out.iter().filter(|e| matches!(e, Event::Touch { .. })).count();
        assert_eq!(touches, 26, "every touch event still reaches egui for pinch and pan");
    }

    #[test]
    fn last_finger_tapping_up_after_a_gesture_does_not_click() {
        let mut r = Rig::shown();
        r.down(1, 100.0, 100.0);
        r.down(2, 200.0, 100.0);
        r.t += 0.05;
        r.up(2, 200.0, 100.0);
        r.t += 0.05;
        r.up(1, 100.0, 100.0);
        r.wait(0.5);
        assert!(r.buttons().is_empty());
    }

    #[test]
    fn tap_click_pan_drag_tap_click_is_exactly_two_clicks() {
        // DIMLINEAR: click, pan elsewhere with two fingers, drag the crosshair, click again.
        let mut r = Rig::shown();
        r.tap(100.0, 100.0);
        r.t += 0.15;
        r.down(1, 110.0, 110.0);
        r.down(2, 300.0, 110.0);
        for i in 1..=10 {
            r.t += 0.02;
            r.mv(1, 110.0 + 20.0 * i as f32, 110.0);
            r.mv(2, 300.0 + 20.0 * i as f32, 110.0);
        }
        r.up(1, 310.0, 110.0);
        r.up(2, 500.0, 110.0);
        r.wait(0.5);
        r.drag(Pos2::new(500.0, 400.0), Vec2::new(-150.0, 80.0));
        r.wait(0.5);
        r.tap(505.0, 400.0);
        r.wait(0.5);
        let b = r.buttons();
        assert_eq!(r.clicks(), 2, "{b:?}");
        assert_eq!(b.first().map(|x| x.1), Some(CENTRE));
        assert_eq!(b.last().map(|x| x.1), Some(Pos2::new(250.0, 380.0)));
    }

    #[test]
    fn lone_pointer_events_during_and_just_after_a_canvas_touch_are_dropped() {
        let mut m = TouchMouse::default();
        let mut run = |events: Vec<Event>, t: f64| {
            let mut ev = events;
            m.filter(&mut ev, t, Some(CANVAS), &on_canvas);
            ev
        };
        let p = Pos2::new(600.0, 500.0);
        run(vec![button(p, true), Rig::touch(1, TouchPhase::Start, p)], 1.0);
        assert!(run(vec![button(p, true)], 1.05).iter().all(|e| !matches!(e, Event::PointerButton { .. })));
        run(vec![button(p, false), Event::PointerGone, Rig::touch(1, TouchPhase::End, p)], 1.1);
        let late = run(vec![button(p, false), Event::PointerMoved(p)], 1.2);
        assert!(late.is_empty(), "{late:?}");
        let mouse = vec![Event::PointerMoved(Pos2::new(5.0, 5.0))];
        assert_eq!(run(mouse.clone(), 2.0), mouse, "mouse input after the quiet time passes");
    }

    #[test]
    fn a_menu_tap_right_after_a_canvas_touch_passes_through() {
        let mut m = TouchMouse::default();
        let on = |p: Pos2| p.y < 500.0;
        let p = Pos2::new(100.0, 100.0);
        let mut ev = vec![button(p, true), Rig::touch(1, TouchPhase::Start, p)];
        m.filter(&mut ev, 1.0, Some(CANVAS), &on);
        let mut ev = vec![button(p, false), Event::PointerGone, Rig::touch(1, TouchPhase::End, p)];
        m.filter(&mut ev, 1.05, Some(CANVAS), &on);
        let menu = Pos2::new(50.0, 550.0);
        let press = button(menu, true);
        let mut ev = vec![press.clone(), Rig::touch(2, TouchPhase::Start, menu)];
        m.filter(&mut ev, 1.1, Some(CANVAS), &on);
        assert!(ev.contains(&press));
    }

    #[test]
    fn touches_off_the_canvas_and_mouse_input_pass_through() {
        let mut m = TouchMouse::default();
        let on = |p: Pos2| p.y < 500.0;
        let pos = Pos2::new(50.0, 550.0);
        let press = button(pos, true);
        let mut ev = vec![press.clone(), Rig::touch(1, TouchPhase::Start, pos)];
        m.filter(&mut ev, 1.0, Some(CANVAS), &on);
        assert!(ev.contains(&press));
        let mut mouse = vec![Event::PointerMoved(Pos2::new(10.0, 10.0)), press.clone(), Event::PointerGone];
        let before = mouse.clone();
        let mut m = TouchMouse::default();
        m.filter(&mut mouse, 1.0, Some(CANVAS), &on);
        assert_eq!(mouse, before);
    }

    #[test]
    fn gate_stops_finger_pointer_events_and_emulated_mouse_events() {
        let mut g = NativeGate::default();
        assert!(g.admit(DomEvent::Mouse, 1.0), "mouse with no touch");
        assert!(g.admit(DomEvent::Pointer { touch: false }, 1.0));
        assert!(!g.admit(DomEvent::Pointer { touch: true }, 2.0));
        assert!(g.admit(DomEvent::Touch, 2.0));
        assert!(!g.admit(DomEvent::Mouse, 2.5), "emulated after a touch");
        assert!(g.admit(DomEvent::Pointer { touch: false }, 2.5), "a real mouse's pointer events pass");
        assert!(g.admit(DomEvent::Mouse, 3.5));
    }

    #[test]
    fn a_lift_keeps_focus_on_the_command_input() {
        assert!(!focus_canvas_on_lift(Some(COMMAND_INPUT_ID)));
        assert!(focus_canvas_on_lift(Some("cadcraft_canvas")));
        assert!(focus_canvas_on_lift(Some("")));
        assert!(focus_canvas_on_lift(None));
    }
}
