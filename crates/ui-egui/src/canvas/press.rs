//! Press-first clicks (#1001): a click on the canvas counts when the primary button goes down, at
//! the spot where it went down, however fast the mouse is moving. Where the pointer goes while the
//! button is held only decides whether the press also becomes a drag:
//!
//! - holding still, or staying within [`TOLERANCE`] of the press, never makes a drag;
//! - once the pointer leaves the tolerance a timer starts; a button still down [`DRAG_DELAY`]
//!   later is a drag (a selection window the press opened closes where the button comes up);
//! - a button that comes up sooner was a click (the window waits for the next press).
//!
//! The timer starts on leaving the tolerance, not on the press: pressing, holding still and then
//! moving and letting go quickly is still a click, as in AutoCAD.

use egui::{Modifiers, Pos2};

/// How far (points) the pointer may wander from the press before the timer starts.
pub(crate) const TOLERANCE: f32 = 6.0;
/// How long (seconds) the button must stay down after the pointer left [`TOLERANCE`] for the
/// press to be a drag.
pub(crate) const DRAG_DELAY: f64 = 0.25;

/// A primary-button press being held.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Press {
    origin: Pos2,
    /// When the pointer first went further than [`TOLERANCE`] from `origin` (seconds).
    left_at: Option<f64>,
    /// The press opened a selection window (set its first corner).
    pub(crate) opened_window: bool,
}

impl Press {
    pub(crate) fn new(origin: Pos2, opened_window: bool) -> Self {
        Self { origin, left_at: None, opened_window }
    }

    /// The pointer is at `pos` at time `now`.
    pub(crate) fn moved(&mut self, pos: Pos2, now: f64) {
        if self.left_at.is_none() && pos.distance(self.origin) > TOLERANCE {
            self.left_at = Some(now);
        }
    }

    /// Whether the button coming up at `pos` at time `now` ends a drag rather than a click.
    pub(crate) fn released_as_drag(&mut self, pos: Pos2, now: f64) -> bool {
        self.moved(pos, now);
        self.left_at.is_some_and(|t| now - t >= DRAG_DELAY)
    }
}

/// A primary-button event of the frame, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Step {
    Down(Pos2, Modifiers),
    Move(Pos2),
    Up(Pos2),
}

/// The primary-button presses, pointer moves and releases among a frame's events, in order. A slow
/// frame can hold several clicks; each press is its own step (#815).
pub(crate) fn steps(events: &[egui::Event]) -> Vec<Step> {
    events
        .iter()
        .filter_map(|e| match *e {
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers } => Some(Step::Down(pos, modifiers)),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => Some(Step::Up(pos)),
            egui::Event::PointerMoved(pos) => Some(Step::Move(pos)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn staying_within_the_tolerance_is_a_click_however_long_the_button_is_held() {
        let mut p = Press::new(pos2(100.0, 100.0), true);
        p.moved(pos2(104.0, 103.0), 0.5);
        assert!(!p.released_as_drag(pos2(105.0, 100.0), 5.0));
    }

    #[test]
    fn letting_go_soon_after_leaving_the_tolerance_is_a_click() {
        let mut p = Press::new(pos2(100.0, 100.0), true);
        p.moved(pos2(112.0, 100.0), 1.0);
        assert!(!p.released_as_drag(pos2(130.0, 100.0), 1.0 + DRAG_DELAY / 2.0));
    }

    #[test]
    fn holding_still_then_moving_and_letting_go_quickly_is_a_click() {
        // The timer starts when the pointer leaves the tolerance, not when the button goes down.
        let mut p = Press::new(pos2(100.0, 100.0), true);
        p.moved(pos2(101.0, 100.0), 2.0);
        assert!(!p.released_as_drag(pos2(140.0, 100.0), 2.05));
    }

    #[test]
    fn a_button_still_down_after_the_delay_is_a_drag() {
        let mut p = Press::new(pos2(100.0, 100.0), true);
        p.moved(pos2(110.0, 100.0), 1.0);
        p.moved(pos2(200.0, 150.0), 1.1);
        assert!(p.released_as_drag(pos2(250.0, 160.0), 1.0 + DRAG_DELAY));
    }

    #[test]
    fn steps_keep_every_press_of_a_slow_frame_in_order() {
        let at = |x| pos2(x, 10.0);
        let button =
            |x, pressed| egui::Event::PointerButton { pos: at(x), button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let events = vec![
            egui::Event::PointerMoved(at(1.0)),
            button(1.0, true),
            button(1.0, false),
            egui::Event::PointerButton { pos: at(2.0), button: egui::PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE },
            button(3.0, true),
            button(3.0, false),
        ];
        assert_eq!(
            steps(&events),
            vec![
                Step::Move(at(1.0)),
                Step::Down(at(1.0), Modifiers::NONE),
                Step::Up(at(1.0)),
                Step::Down(at(3.0), Modifiers::NONE),
                Step::Up(at(3.0))
            ]
        );
    }
}
