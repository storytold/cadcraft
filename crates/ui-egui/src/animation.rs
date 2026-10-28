//! Animation effects: the interface's fades and glides, and the choice to show them.
//!
//! None of the motion is CADCraft's own; it is egui's. Windows, menus, tooltips and popups fade in
//! over [`egui::Style::animation_time`], collapsing sections and toggles slide over it, scroll
//! areas glide to a scroll target, and each mouse-wheel notch is spread over several frames. With
//! animation effects off all of that happens at once.
//!
//! What tells the user about the work stays, on or off: rubber bands and previews, snap markers
//! and tracking's dwell, the text cursor's blink, spinners, and pan and zoom, which follow the
//! pointer and the wheel directly.
//!
//! [`AnimationPref::System`], the default, follows the operating system (Windows: Settings ▸
//! Accessibility ▸ Visual effects ▸ Animation effects; a browser: `prefers-reduced-motion`). The
//! host asks the OS and hands its answer to [`CadApp::system_animations`](crate::CadApp).

use serde::{Deserialize, Serialize};

/// Whether the interface shows animation effects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationPref {
    /// The default: follow the operating system's setting, live.
    #[default]
    System,
    On,
    Off,
}

impl AnimationPref {
    pub const ALL: [AnimationPref; 3] = [AnimationPref::System, AnimationPref::On, AnimationPref::Off];

    pub fn as_str(self) -> &'static str {
        match self {
            AnimationPref::System => "system",
            AnimationPref::On => "on",
            AnimationPref::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Option<AnimationPref> {
        AnimationPref::ALL.into_iter().find(|p| p.as_str().eq_ignore_ascii_case(s.trim()))
    }

    /// Whether to animate, given what the OS reports (`None`: it states nothing, or the host
    /// cannot ask it; `System` then animates, as egui does by default).
    pub fn resolve(self, system: Option<bool>) -> bool {
        match self {
            AnimationPref::System => system.unwrap_or(true),
            AnimationPref::On => true,
            AnimationPref::Off => false,
        }
    }
}

/// Turn egui's own animations on or off, in the styles of both themes.
pub fn apply(ctx: &egui::Context, animate: bool) {
    let animated = egui::Style::default();
    ctx.all_styles_mut(|s| {
        s.animation_time = if animate { animated.animation_time } else { 0.0 };
        s.scroll_animation = if animate { animated.scroll_animation } else { egui::style::ScrollAnimation::none() };
    });
}

/// One frame of mouse-wheel input, read from the raw wheel events as egui reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Wheel {
    /// Points to scroll by.
    scroll: egui::Vec2,
    /// The zoom of wheel turns with the zoom modifier held (⌘/Ctrl); 1 when there are none.
    zoom: f32,
}

/// Units convert to points as egui does (a line is `line_scroll_speed` points, a page the view's
/// height); the zoom modifier turns a scroll into a zoom, ⇧ into a sideways scroll.
fn wheel(events: &[egui::Event], opts: &egui::InputOptions, page: f32) -> Wheel {
    let (mut scroll, mut zoom_points) = (egui::Vec2::ZERO, 0.0);
    for e in events {
        let egui::Event::MouseWheel { unit, delta, phase: egui::TouchPhase::Move, modifiers, .. } = e else { continue };
        let per = match unit {
            egui::MouseWheelUnit::Point => 1.0,
            egui::MouseWheelUnit::Line => opts.line_scroll_speed,
            egui::MouseWheelUnit::Page => page,
        };
        let d = *delta * per;
        if modifiers.matches_any(opts.zoom_modifier) {
            zoom_points += d.x + d.y;
            continue;
        }
        let (sideways, upright) = (modifiers.matches_any(opts.horizontal_scroll_modifier), modifiers.matches_any(opts.vertical_scroll_modifier));
        scroll += match (sideways, upright) {
            (true, false) => egui::vec2(d.x + d.y, 0.0),
            (false, true) => egui::vec2(0.0, d.x + d.y),
            _ => d,
        };
    }
    let zoom = (opts.scroll_zoom_speed * zoom_points).exp();
    Wheel {
        scroll: if scroll.x.is_finite() && scroll.y.is_finite() { scroll } else { egui::Vec2::ZERO },
        zoom: if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 },
    }
}

/// With animation effects off, hand each wheel notch to the scroll areas whole, in the frame it
/// arrives. egui spreads a notch over several frames ([`egui::InputState::smooth_scroll_delta`])
/// and has no switch for it, so the smoothed delta is replaced by the frame's raw one; the part
/// egui still holds back is dropped the same way on the frames that follow. Call once a frame,
/// before any scroll area is shown.
pub fn unsmooth_scroll(ctx: &egui::Context) {
    let opts = ctx.options(|o| o.input_options);
    ctx.input_mut(|i| {
        let page = i.viewport_rect().height();
        i.smooth_scroll_delta = wheel(&i.raw.events, &opts, page).scroll;
    });
}

/// The zoom gesture this frame. With animation effects on this is egui's own
/// [`egui::InputState::zoom_delta`]; off, a ⌘/Ctrl+wheel notch (a touchpad pinch, on Windows)
/// zooms by its whole step at once instead of gliding there. A real pinch (touch screen, macOS
/// trackpad) follows the fingers either way.
pub fn zoom_delta(i: &egui::InputState, opts: &egui::InputOptions, animate: bool) -> f32 {
    if animate {
        return i.zoom_delta();
    }
    if let Some(touch) = i.multi_touch() {
        return touch.zoom_delta;
    }
    let pinch: f32 = i
        .raw
        .events
        .iter()
        .filter_map(|e| match e {
            egui::Event::Zoom(factor) => Some(*factor),
            _ => None,
        })
        .product();
    let zoom = pinch * wheel(&i.raw.events, opts, i.viewport_rect().height()).zoom;
    if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notch(unit: egui::MouseWheelUnit, delta: egui::Vec2, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::MouseWheel { unit, delta, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Unknown, modifiers }
    }

    #[test]
    fn the_choice_parses_and_resolves_against_the_os() {
        assert_eq!(AnimationPref::default(), AnimationPref::System, "the OS decides until the user does");
        for p in AnimationPref::ALL {
            assert_eq!(AnimationPref::parse(p.as_str()), Some(p));
        }
        assert_eq!(AnimationPref::parse(" OFF "), Some(AnimationPref::Off));
        assert_eq!(AnimationPref::parse("sometimes"), None);
        assert!(!AnimationPref::System.resolve(Some(false)));
        assert!(AnimationPref::System.resolve(Some(true)));
        assert!(AnimationPref::System.resolve(None), "an OS that states nothing keeps egui's animations");
        for system in [None, Some(true), Some(false)] {
            assert!(AnimationPref::On.resolve(system));
            assert!(!AnimationPref::Off.resolve(system));
        }
    }

    #[test]
    fn egui_stops_animating_and_starts_again() {
        let ctx = egui::Context::default();
        let animated = egui::Style::default();
        apply(&ctx, false);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let s = ctx.style_of(theme);
            assert_eq!(s.animation_time, 0.0, "{theme:?}: fades and slides end at once");
            assert_eq!(s.scroll_animation, egui::style::ScrollAnimation::none(), "{theme:?}");
        }
        apply(&ctx, true);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let s = ctx.style_of(theme);
            assert_eq!(s.animation_time, animated.animation_time, "{theme:?}");
            assert_eq!(s.scroll_animation, animated.scroll_animation, "{theme:?}");
        }
    }

    #[test]
    fn the_wheel_reads_as_egui_reads_it() {
        use egui::{Modifiers, MouseWheelUnit, vec2};
        let o = egui::InputOptions::default();
        let none = Modifiers::NONE;
        let plain = wheel(&[notch(MouseWheelUnit::Line, vec2(0.0, -1.0), none), notch(MouseWheelUnit::Point, vec2(3.0, 5.0), none)], &o, 500.0);
        assert_eq!(plain, Wheel { scroll: vec2(3.0, 5.0 - o.line_scroll_speed), zoom: 1.0 });
        assert_eq!(wheel(&[notch(MouseWheelUnit::Page, vec2(0.0, 1.0), none)], &o, 500.0).scroll, vec2(0.0, 500.0));
        // ⇧ scrolls sideways; ⌘/Ctrl zooms and scrolls nothing.
        assert_eq!(wheel(&[notch(MouseWheelUnit::Point, vec2(0.0, 12.0), Modifiers::SHIFT)], &o, 500.0).scroll, vec2(12.0, 0.0));
        let zoomed = wheel(&[notch(MouseWheelUnit::Line, vec2(0.0, 1.0), Modifiers::COMMAND)], &o, 500.0);
        assert_eq!(zoomed.scroll, egui::Vec2::ZERO);
        assert!((zoomed.zoom - (o.scroll_zoom_speed * o.line_scroll_speed).exp()).abs() < 1e-6 && zoomed.zoom > 1.0);
        // A touchpad's begin and end markers carry no scroll.
        let ended = egui::Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, 9.0),
            phase: egui::TouchPhase::End,
            source: egui::MouseWheelSource::Unknown,
            modifiers: none,
        };
        assert_eq!(wheel(&[ended], &o, 500.0), Wheel { scroll: egui::Vec2::ZERO, zoom: 1.0 });
        // Hostile deltas scroll and zoom nothing.
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(wheel(&[notch(MouseWheelUnit::Line, vec2(0.0, bad), none)], &o, 500.0).scroll, egui::Vec2::ZERO);
            assert_eq!(wheel(&[notch(MouseWheelUnit::Line, vec2(0.0, bad), Modifiers::COMMAND)], &o, 500.0).zoom, 1.0);
        }
    }

    /// One frame with `events`, returning what the scroll areas would scroll by and the zoom.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, animate: bool) -> (egui::Vec2, f32) {
        let o = ctx.options(|o| o.input_options);
        let raw =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 500.0))), events, ..Default::default() };
        let mut seen = (egui::Vec2::ZERO, 1.0);
        let mut out = ctx.run_ui(raw, |ui| {
            if !animate {
                unsmooth_scroll(ui.ctx());
            }
            seen = ui.input(|i| (i.smooth_scroll_delta, zoom_delta(i, &o, animate)));
        });
        out.textures_delta.clear();
        seen
    }

    #[test]
    fn a_wheel_notch_scrolls_whole_in_its_frame_with_animation_effects_off() {
        let line = egui::InputOptions::default().line_scroll_speed;
        let turn = || vec![notch(egui::MouseWheelUnit::Line, egui::vec2(0.0, -3.0), egui::Modifiers::NONE)];

        // egui glides: part of the notch now, the rest over the frames that follow.
        let ctx = egui::Context::default();
        let (first, _) = frame(&ctx, turn(), true);
        assert!(first.y < 0.0 && first.y > -3.0 * line, "egui's smoothed delta lags behind: {first:?}");
        assert!(frame(&ctx, Vec::new(), true).0.y < 0.0, "and keeps arriving");

        // Off: all of it at once, and nothing trickles in afterwards.
        let ctx = egui::Context::default();
        assert_eq!(frame(&ctx, turn(), false).0, egui::vec2(0.0, -3.0 * line));
        for _ in 0..4 {
            assert_eq!(frame(&ctx, Vec::new(), false).0, egui::Vec2::ZERO);
        }
    }

    #[test]
    fn a_zoom_notch_zooms_whole_in_its_frame_with_animation_effects_off() {
        let o = egui::InputOptions::default();
        let whole = (o.scroll_zoom_speed * o.line_scroll_speed).exp();
        let turn = || vec![notch(egui::MouseWheelUnit::Line, egui::vec2(0.0, 1.0), egui::Modifiers::COMMAND)];

        let ctx = egui::Context::default();
        let (_, first) = frame(&ctx, turn(), true);
        assert!(first > 1.0 && first < whole, "egui's zoom glides there: {first} of {whole}");

        let ctx = egui::Context::default();
        assert!((frame(&ctx, turn(), false).1 - whole).abs() < 1e-6);
        assert_eq!(frame(&ctx, Vec::new(), false).1, 1.0, "nothing trickles in afterwards");
        // A pinch gesture's own zoom events pass through.
        assert_eq!(frame(&ctx, vec![egui::Event::Zoom(1.25)], false).1, 1.25);
    }
}
