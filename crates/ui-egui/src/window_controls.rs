//! Window buttons and resize edges for desktops that leave the window frame to the app.
//!
//! GNOME on Wayland has no server-side decorations (the compositor offers no
//! `zxdg_decoration_manager_v1`), so winit drew its own imitation frame above CADCraft's tool bar.
//! Instead the app turns that frame off and draws a title row as GNOME's header bar
//! ([`crate::chrome::title_and_toolbar`]): the drawing's name, and the minimize, maximize and close
//! buttons in the desktop's order (GNOME's `button-layout`), while thin zones along the window's
//! edges resize it. Desktops with server-side decorations (KDE, Sway, Hyprland, X11) keep their
//! own frame and never get here.
//!
//! Ported from WordCraft (storytold/wordcraft#3361).

use crate::theme::Tokens;
use egui::viewport::ResizeDirection;
use egui::{Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, Vec2};

/// A window button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Minimize,
    Maximize,
    Close,
}

/// Which buttons go on which end of the header bar, outermost last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub left: Vec<Button>,
    pub right: Vec<Button>,
}

impl Default for Layout {
    /// What a desktop that names no layout gets: all three, on the right.
    fn default() -> Self {
        Self { left: Vec::new(), right: vec![Button::Minimize, Button::Maximize, Button::Close] }
    }
}

impl Layout {
    /// No buttons on either end.
    pub fn none() -> Self {
        Self { left: Vec::new(), right: Vec::new() }
    }

    /// GNOME's `org.gnome.desktop.wm.preferences button-layout`: `left:right`, each a comma list
    /// such as `appmenu:minimize,maximize,close` (Fedora's default is `appmenu:close`). Without a
    /// colon everything is on the left, as in GTK. Names this app has no button for (`appmenu`,
    /// `icon`, `menu`, `spacer`) are skipped, and a button named twice appears once. `gsettings`
    /// prints the value quoted; the quotes are dropped.
    pub fn parse(value: &str) -> Self {
        let value = value.trim().trim_matches(|c| c == '\'' || c == '"');
        let (left, right) = value.split_once(':').unwrap_or((value, ""));
        let mut seen = Vec::new();
        let mut side = |list: &str| {
            let mut out = Vec::new();
            for name in list.split(',') {
                let button = match name.trim() {
                    "minimize" => Button::Minimize,
                    "maximize" => Button::Maximize,
                    "close" => Button::Close,
                    _ => continue,
                };
                if !seen.contains(&button) {
                    seen.push(button);
                    out.push(button);
                }
            }
            out
        };
        let left = side(left);
        let right = side(right);
        Self { left, right }
    }

    /// The layout to draw for the desktop's `button-layout` (`None` when it names none): minimize,
    /// maximize and close on the right when it names none, or names one without a single button,
    /// which would leave no way to close the window by mouse.
    pub fn or_default(value: Option<&str>) -> Self {
        match value.map(Self::parse) {
            Some(layout) if !layout.left.is_empty() || !layout.right.is_empty() => layout,
            _ => Self::default(),
        }
    }
}

/// Height of the title row the app draws as the header bar.
pub const HEADER: f32 = 36.0;
/// The round button's diameter, as in Adwaita.
pub const BUTTON: f32 = 24.0;
/// Between two window buttons.
pub const GAP: f32 = 6.0;
/// Between the window buttons and the rest of the bar.
pub const SIDE_GAP: f32 = 10.0;

/// The width `buttons` take in the bar, without the gap to the rest of it.
pub fn width(buttons: &[Button]) -> f32 {
    match buttons.len() {
        0 => 0.0,
        n => n as f32 * BUTTON + (n - 1) as f32 * GAP,
    }
}

/// Draw the window buttons for one end of the bar, in reading order, inside a horizontal layout
/// running left to right; the caller adds [`SIDE_GAP`] where the bar continues.
pub fn buttons(ui: &mut egui::Ui, buttons: &[Button]) {
    let t = Tokens::get();
    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
    ui.spacing_mut().item_spacing.x = 0.0;
    for (n, &button) in buttons.iter().enumerate() {
        if n > 0 {
            ui.add_space(GAP);
        }
        let label = match button {
            Button::Minimize => crate::tl!("Minimize"),
            Button::Maximize if maximized => crate::tl!("Restore"),
            Button::Maximize => crate::tl!("Maximize"),
            Button::Close => crate::tl!("Close"),
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        let response = response.on_hover_text(label);
        if ui.is_rect_visible(rect) {
            paint(ui.painter(), rect, button, maximized, &t, &response);
        }
        if response.clicked() {
            let command = match button {
                Button::Minimize => egui::ViewportCommand::Minimized(true),
                Button::Maximize => egui::ViewportCommand::Maximized(!maximized),
                // The same path as the desktop's close: unsaved documents still ask first.
                Button::Close => egui::ViewportCommand::Close,
            };
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

/// Adwaita's look: a round wash of the text colour, stronger on hover and press, and a 16-point
/// symbolic glyph drawn with lines (no icon files).
fn paint(painter: &egui::Painter, rect: Rect, button: Button, maximized: bool, t: &Tokens, response: &egui::Response) {
    let alpha = if response.is_pointer_button_down_on() {
        0.28
    } else if response.hovered() {
        0.16
    } else {
        0.10
    };
    painter.circle_filled(rect.center(), BUTTON / 2.0, t.text.gamma_multiply(alpha));
    if response.has_focus() {
        painter.circle_stroke(rect.center(), BUTTON / 2.0 - 0.5, Stroke::new(1.0, t.accent));
    }
    let stroke = Stroke::new(1.25, t.text);
    // The glyph's 16-point square, its corners on pixel centres so lines stay crisp.
    let c = rect.center();
    let at = |x: f32, y: f32| Pos2::new((c.x - 8.0 + x).round() + 0.5, (c.y - 8.0 + y).round() + 0.5);
    match button {
        Button::Minimize => {
            painter.line_segment([at(4.0, 11.0), at(11.0, 11.0)], stroke);
        }
        Button::Maximize if maximized => {
            // Restore: a smaller window in front of another.
            painter.rect_stroke(Rect::from_two_pos(at(4.0, 6.0), at(10.0, 12.0)), 1.0, stroke, egui::StrokeKind::Middle);
            painter.line_segment([at(6.0, 6.0), at(6.0, 4.0)], stroke);
            painter.line_segment([at(6.0, 4.0), at(12.0, 4.0)], stroke);
            painter.line_segment([at(12.0, 4.0), at(12.0, 10.0)], stroke);
            painter.line_segment([at(12.0, 10.0), at(10.0, 10.0)], stroke);
        }
        Button::Maximize => {
            painter.rect_stroke(Rect::from_two_pos(at(4.0, 4.0), at(11.0, 11.0)), 1.0, stroke, egui::StrokeKind::Middle);
        }
        Button::Close => {
            painter.line_segment([at(4.0, 4.0), at(11.0, 11.0)], stroke);
            painter.line_segment([at(11.0, 4.0), at(4.0, 11.0)], stroke);
        }
    }
}

/// Where [`desktop_grab`] notes that the desktop holds the pointer.
fn grab_id() -> egui::Id {
    egui::Id::unique("window_controls_desktop_grab")
}

/// Hand the pointer to the desktop to move (`StartDrag`) or resize (`BeginResize`) the window.
/// The desktop keeps the button's release (on Wayland the compositor grabs the pointer), so egui
/// would still think the button held and miss the next press: an edge or the title bar then
/// wouldn't start another resize or move until a click inside the window. The release is given
/// back on the next input ([`release_after_desktop_grab`]).
pub fn desktop_grab(ctx: &egui::Context, command: egui::ViewportCommand) {
    ctx.send_viewport_cmd(command);
    mark_desktop_grab(ctx);
}

/// Note a move or resize someone else asked the desktop for (the title row's `empty_area`), as
/// [`desktop_grab`] does for its own.
pub fn mark_desktop_grab(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(grab_id(), ()));
    ctx.request_repaint();
}

/// After [`desktop_grab`], add the primary button's release to the next input, unless it brings
/// one. Called from the app's raw input hook, before egui reads the input.
pub fn release_after_desktop_grab(ctx: &egui::Context, raw: &mut egui::RawInput) {
    if ctx.data_mut(|d| d.remove_temp::<()>(grab_id())).is_none() {
        return;
    }
    let released = raw.events.iter().any(|e| matches!(e, egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, .. }));
    if !released {
        let (pos, modifiers) = ctx.input(|i| (i.pointer.interact_pos().unwrap_or_default(), i.modifiers));
        raw.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers });
    }
}

/// How far into the window an edge resizes it.
const EDGE: f32 = 5.0;
/// How far along an edge a corner reaches, so corners are easy to hit.
const CORNER: f32 = 16.0;

/// The resize zones of a window filling `rect`: edges `edge` deep, and each corner an L of two
/// strips reaching `corner` along both edges (a square would reach into the content).
fn zones(rect: Rect, edge: f32, corner: f32) -> Vec<(ResizeDirection, Rect)> {
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    // Never inverted, however small the window.
    let zone = |x0: f32, x1: f32, y0: f32, y1: f32| Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1.max(x0), y1.max(y0)));
    vec![
        (ResizeDirection::NorthWest, zone(l, l + corner, t, t + edge)),
        (ResizeDirection::NorthWest, zone(l, l + edge, t + edge, t + corner)),
        (ResizeDirection::NorthEast, zone(r - corner, r, t, t + edge)),
        (ResizeDirection::NorthEast, zone(r - edge, r, t + edge, t + corner)),
        (ResizeDirection::SouthWest, zone(l, l + corner, b - edge, b)),
        (ResizeDirection::SouthWest, zone(l, l + edge, b - corner, b - edge)),
        (ResizeDirection::SouthEast, zone(r - corner, r, b - edge, b)),
        (ResizeDirection::SouthEast, zone(r - edge, r, b - corner, b - edge)),
        (ResizeDirection::North, zone(l + corner, r - corner, t, t + edge)),
        (ResizeDirection::South, zone(l + corner, r - corner, b - edge, b)),
        (ResizeDirection::West, zone(l, l + edge, t + corner, b - corner)),
        (ResizeDirection::East, zone(r - edge, r, t + corner, b - corner)),
    ]
}

fn cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

/// Resize zones along the window's edges and a hairline border, while the window is neither
/// maximized nor full screen. The zones sit above everything else (in the foreground order), so a
/// press there resizes instead of reaching the panel or canvas under it.
pub fn resize_edges(ctx: &egui::Context) {
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if maximized || fullscreen {
        return;
    }
    let screen = ctx.content_rect();
    let dark = crate::theme::active() == egui::Theme::Dark;
    egui::Area::new(egui::Id::unique("window_resize_edges"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .pivot(Align2::LEFT_TOP)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_min_size(Vec2::ZERO);
            for (n, (direction, rect)) in zones(screen, EDGE, CORNER).into_iter().enumerate() {
                if rect.width() <= 0.0 || rect.height() <= 0.0 {
                    continue;
                }
                let response = ui.interact(rect, ui.scope_id().with(("edge", n)), Sense::drag()).on_hover_cursor(cursor(direction));
                // Sensing only drags, a drag starts on the press itself.
                if response.drag_started() {
                    desktop_grab(ui.ctx(), egui::ViewportCommand::BeginResize(direction));
                }
            }
            // Without the desktop's frame or shadow, a hairline keeps the window apart from
            // what's behind it.
            ui.painter().rect_stroke(screen, 0.0, Stroke::new(1.0, Color32::from_black_alpha(if dark { 160 } else { 60 })), egui::StrokeKind::Inside);
        });
}

#[cfg(test)]
mod tests {
    use super::{Button, Layout, ResizeDirection, zones};
    use egui::{Pos2, Rect};

    /// After the desktop takes the pointer for a move or resize, the next input releases the
    /// button egui still thinks held, once, unless that input brings the release itself.
    #[test]
    fn a_desktop_grab_gives_the_button_release_back() {
        let primary_releases = |raw: &egui::RawInput| {
            raw.events.iter().filter(|e| matches!(e, egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, .. })).count()
        };
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        super::release_after_desktop_grab(&ctx, &mut raw);
        assert_eq!(primary_releases(&raw), 0, "nothing without a grab");
        super::desktop_grab(&ctx, egui::ViewportCommand::BeginResize(ResizeDirection::East));
        super::release_after_desktop_grab(&ctx, &mut raw);
        assert_eq!(primary_releases(&raw), 1, "the release the desktop kept");
        let mut next = egui::RawInput::default();
        super::release_after_desktop_grab(&ctx, &mut next);
        assert_eq!(primary_releases(&next), 0, "only once");
        super::desktop_grab(&ctx, egui::ViewportCommand::StartDrag);
        let mut with_release = egui::RawInput::default();
        with_release.events.push(egui::Event::PointerButton {
            pos: Pos2::ZERO,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        super::release_after_desktop_grab(&ctx, &mut with_release);
        assert_eq!(primary_releases(&with_release), 1, "not doubled when the release arrives");
    }

    #[test]
    fn gnome_button_layouts_parse() {
        let close = Layout { left: vec![], right: vec![Button::Close] };
        assert_eq!(Layout::parse("'appmenu:close'"), close);
        assert_eq!(Layout::parse("appmenu:close\n"), close);
        assert_eq!(Layout::parse(":minimize,maximize,close"), Layout::default());
        assert_eq!(
            Layout::parse("close,minimize,maximize:appmenu"),
            Layout { left: vec![Button::Close, Button::Minimize, Button::Maximize], right: vec![] }
        );
        // No colon: the left side, as GTK reads it.
        assert_eq!(Layout::parse("close"), Layout { left: vec![Button::Close], right: vec![] });
        // Unknown names, spaces and repeats.
        assert_eq!(Layout::parse("icon,spacer: close , close,bogus"), close);
        assert_eq!(Layout::parse(""), Layout::none());
    }

    #[test]
    fn widths_cover_the_buttons_and_their_gaps() {
        assert_eq!(super::width(&[]), 0.0);
        assert_eq!(super::width(&[Button::Close]), super::BUTTON);
        assert_eq!(super::width(&Layout::default().right), 3.0 * super::BUTTON + 2.0 * super::GAP);
    }

    #[test]
    fn resize_zones_line_the_edges_without_overlapping() {
        let window = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let zones = zones(window, 5.0, 16.0);
        let at = |x: f32, y: f32| zones.iter().filter(|(_, r)| r.contains(Pos2::new(x, y))).map(|(d, _)| *d).collect::<Vec<_>>();
        assert_eq!(at(1.0, 1.0), [ResizeDirection::NorthWest]);
        assert_eq!(at(2.0, 12.0), [ResizeDirection::NorthWest]);
        assert_eq!(at(400.0, 2.0), [ResizeDirection::North]);
        assert_eq!(at(799.0, 300.0), [ResizeDirection::East]);
        assert_eq!(at(400.0, 599.0), [ResizeDirection::South]);
        assert_eq!(at(1.0, 300.0), [ResizeDirection::West]);
        assert_eq!(at(799.0, 599.0), [ResizeDirection::SouthEast]);
        assert!(at(400.0, 300.0).is_empty(), "the middle of the window is left alone");
        assert!(at(12.0, 12.0).is_empty(), "corners stay along the edges");
        // Strips meet without overlapping.
        for y in [0.5, 4.5, 5.5, 15.5, 16.5, 595.5] {
            for x in [0.5, 4.5, 5.5, 15.5, 16.5, 795.5, 799.5] {
                assert!(at(x, y).len() <= 1, "({x}, {y}): {:?}", at(x, y));
            }
        }
        // A window smaller than its corners gets empty (not inverted) edge zones.
        for (_, r) in super::zones(Rect::from_min_size(Pos2::ZERO, egui::vec2(20.0, 20.0)), 5.0, 16.0) {
            assert!(r.width() >= 0.0 && r.height() >= 0.0);
        }
    }
}
