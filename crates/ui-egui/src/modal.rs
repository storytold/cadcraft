//! Alerts that appear in the frame that asks for them.
//!
//! An `egui::Modal` normally shows a frame late and then fades in over `animation_time`
//! (0.2 s): its area's first frame only measures it and draws nothing, and the dim behind it
//! fades with it. An alert that answers a key press or menu choice should be there at once.

/// A modal with no fade-in. On its first show it asks egui to run the pass again with the
/// measured size, so the first frame presented already draws it.
pub fn instant(ctx: &egui::Context, id: egui::Id) -> egui::Modal {
    if egui::AreaState::load(ctx, id).is_none() {
        ctx.request_discard("an instant modal's first show only measures it");
    }
    egui::Modal::new(id).area(egui::Modal::default_area(id).fade_in(false))
}
