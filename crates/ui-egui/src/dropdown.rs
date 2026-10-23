//! Overflow protection for popup lists, including rows with custom controls. Short lists shrink
//! to their contents without a scrollbar; long lists scroll within a viewport-bounded height.
//! This is a list body, not a replacement for popup placement or selection/closing behavior.

/// Show a list inside a popup. Callers must use `CloseOnClickOutside` and close explicitly on
/// selection so scrollbar clicks don't dismiss the popup.
///
/// Popups may move around their anchor, so use the full viewport height. Reset the maximum
/// height each frame so a popup can grow again after a window resize.
pub(crate) fn list<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> egui::scroll_area::ScrollAreaOutput<R> {
    let margin = egui::Frame::popup(ui.style()).total_margin().sum().y;
    let height = (ui.ctx().content_rect().height() - margin).clamp(0.0, 300.0);
    ui.set_max_height(height);
    egui::ScrollArea::vertical().max_height(height).min_scrolled_height(0.0).show(ui, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, Vec2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    struct State {
        bounded: bool,
        count: usize,
        popup: Rect,
        viewport: Rect,
        content: Vec2,
        last: Rect,
        last_clip: Rect,
        picked: Option<usize>,
    }

    fn rows(ui: &mut egui::Ui, state: &mut State) {
        for i in 0..state.count {
            let response = ui.button(format!("Choice {i:02}"));
            if i + 1 == state.count {
                state.last = response.rect;
                state.last_clip = ui.clip_rect();
            }
            if response.clicked() {
                state.picked = Some(i);
                ui.close();
            }
        }
    }

    /// Keep rows and popup configuration identical in both modes so comparisons with the plain
    /// menu isolate the scroll area's sizing effects, rather than differences in layout or controls.
    fn harness(bounded: bool, count: usize, size: Vec2, scale: f32) -> Harness<'static, State> {
        Harness::builder().with_size(size).with_pixels_per_point(scale).build_ui_state(
            |ui, state: &mut State| {
                let button = ui.button("Choose");
                let response = egui::Popup::menu(&button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    // Solid bars would reserve space if a short list unnecessarily showed a bar.
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                    if state.bounded {
                        let output = list(ui, |ui| rows(ui, state));
                        state.viewport = output.inner_rect;
                        state.content = output.content_size;
                    } else {
                        rows(ui, state);
                    }
                });
                if let Some(inner) = response {
                    state.popup = inner.response.rect;
                }
            },
            State {
                bounded,
                count,
                popup: Rect::NOTHING,
                viewport: Rect::NOTHING,
                content: Vec2::ZERO,
                last: Rect::NOTHING,
                last_clip: Rect::NOTHING,
                picked: None,
            },
        )
    }

    #[test]
    fn short_lists_keep_the_plain_menu_size_without_scrollbar_space() {
        for count in [1, 3] {
            let size = vec2(400.0, 600.0);
            let mut plain = harness(false, count, size, 1.0);
            plain.get_by_label("Choose").click();
            plain.run_steps(6);
            let mut bounded = harness(true, count, size, 1.0);
            bounded.get_by_label("Choose").click();
            bounded.run_steps(6);
            // ScrollArea rounds its viewport to pixels; it must not add bar-width or empty height.
            let difference = (bounded.state().popup.size() - plain.state().popup.size()).abs();
            assert!(difference.x <= 1.0 && difference.y <= 1.0, "short menu grew by {difference:?}");
            assert!(bounded.state().content.y <= bounded.state().viewport.height() + 1.0);
            assert!(bounded.state().last_clip.contains_rect(bounded.state().last));
        }
    }

    #[test]
    fn scrollbar_clicks_keep_the_popup_open_but_outside_clicks_close_it() {
        let mut h = harness(true, 60, vec2(400.0, 600.0), 1.0);
        h.get_by_label("Choose").click();
        h.run_steps(6);
        let viewport = h.state().viewport;
        let style = egui::style::ScrollStyle::solid();
        let x = viewport.right() + style.bar_inner_margin + style.bar_width / 2.0;
        let before = h.state().last.top();
        // Click the thumb without dragging, then the track below it.
        for y in [viewport.top() + 4.0, viewport.center().y] {
            let pos = egui::pos2(x, y);
            h.hover_at(pos);
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
            h.run_steps(4);
            assert!(h.query_by_label("Choice 59").is_some(), "scrollbar clicks must not close the popup");
            assert_eq!(h.state().picked, None);
        }
        assert!(h.state().last.top() < before - 1.0, "track click must scroll the list");
        let outside = egui::pos2(390.0, 590.0);
        assert!(!h.state().popup.contains(outside));
        h.hover_at(outside);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: outside, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.run_steps(4);
        assert!(h.query_by_label("Choice 59").is_none(), "outside click closes the menu");
        assert_eq!(h.state().picked, None);
    }

    #[test]
    fn scrollbar_drag_reaches_the_last_choice_without_closing_the_popup() {
        let mut h = harness(true, 60, vec2(400.0, 600.0), 1.0);
        h.get_by_label("Choose").click();
        h.run_steps(6);
        let viewport = h.state().viewport;
        let style = egui::style::ScrollStyle::solid();
        let x = viewport.right() + style.bar_inner_margin + style.bar_width / 2.0;
        let start = egui::pos2(x, viewport.top() + 4.0);
        let end = egui::pos2(x, viewport.bottom() - 4.0);
        h.hover_at(start);
        h.drag_at(start);
        h.run_steps(2);
        h.hover_at(end);
        h.run_steps(4);
        h.drop_at(end);
        h.run_steps(4);
        assert!(h.query_by_label("Choice 59").is_some(), "dragging must not close the popup");
        assert!(h.state().last_clip.contains_rect(h.state().last), "dragging the bar must reach the last choice");
        h.get_by_label("Choice 59").click();
        h.run_steps(4);
        assert_eq!(h.state().picked, Some(59));
    }

    #[test]
    fn long_lists_fit_small_viewports_and_grow_again_after_resize() {
        for scale in [1.0, 2.0] {
            let mut h = harness(true, 60, vec2(400.0, 600.0), scale);
            h.get_by_label("Choose").click();
            h.run_steps(6);
            let original_height = h.state().popup.height();
            assert!(h.state().content.y > h.state().viewport.height());
            assert!(!h.state().last_clip.contains_rect(h.state().last));
            for height in [180.0, 600.0] {
                h.set_size(vec2(400.0, height));
                h.run_steps(8);
                let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, height));
                assert!(screen.expand(1.0).contains_rect(h.state().popup), "popup {:?} outside {screen:?}", h.state().popup);
                if height == 600.0 {
                    assert!((h.state().popup.height() - original_height).abs() <= 1.0);
                }
                h.get_by_label("Choice 59").scroll_to_me();
                h.run_steps(6);
                assert!(h.state().last_clip.contains_rect(h.state().last), "last choice is clipped at {height}, {scale}x");
            }
            h.get_by_label("Choice 59").click();
            h.run_steps(4);
            assert_eq!(h.state().picked, Some(59));
            assert!(h.query_by_label("Choice 59").is_none(), "selection closes the menu");
        }
    }
}
