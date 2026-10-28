//! Additive migration of persisted panel sizes from the 0.36 root-id key to 0.37's
//! `parent_ui.scope_id().with_salt(name)` key.
//!
//! CADCraft disables egui memory persistence today, but the app's own saved workspace or a host
//! that enables persistence must not lose a resized panel once. The old key is retained for
//! rollback; a value already stored under the new key always wins.

/// The top-level panel id salts used by CADCraft. Keep these strings stable: they are names in
/// saved test fixtures and in `ui.elements` expectations, not disposable implementation details.
pub(crate) const PANEL_IDS: &[&str] = &["cc_title_toolbar", "cc_menubar", "cc_status", "cc_file_tabs", "cc_toolsets_strip", "cc_palettes"];

/// Copy an old-key `PanelState` to the 0.37 key before panels are shown. Call once on the first
/// pass, while the root `Ui` scope id is available.
pub(crate) fn migrate(ui: &egui::Ui) {
    let ctx = ui.ctx();
    let parent = ui.scope_id();
    for name in PANEL_IDS {
        let old = egui::Id::unique(name);
        let new = parent.with_salt(egui::IdSalt::new(name));
        if old == new {
            continue;
        }
        let Some(state) = ctx.data_mut(|data| data.get_persisted::<egui::PanelState>(old)) else {
            continue;
        };
        ctx.data_mut(|data| {
            if data.get_persisted::<egui::PanelState>(new).is_none() {
                data.insert_persisted(new, state);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test panel state `width` points wide and 26 points high.
    fn state(width: f32) -> egui::PanelState {
        egui::PanelState { outer_rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 26.0)) }
    }

    /// `migrate` copies the old root-id panel state into the 0.37 scoped key and leaves the old key in place for rollback.
    #[test]
    fn old_key_is_copied_to_the_scoped_key_once() {
        let ctx = egui::Context::default();
        let old = egui::Id::unique("cc_status");
        ctx.data_mut(|data| data.insert_persisted(old, state(41.0)));
        let mut new_key = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            migrate(ui);
            let new = ui.scope_id().with_salt(egui::IdSalt::new("cc_status"));
            new_key = Some(new);
            assert_eq!(egui::PanelState::load(ui.ctx(), new).map(|state| state.size().x), Some(41.0));
        });
        output.textures_delta.clear();
        let new_key = new_key.expect("a root Ui");
        assert_eq!(egui::PanelState::load(&ctx, new_key).map(|state| state.size().x), Some(41.0));
        assert_eq!(egui::PanelState::load(&ctx, old).map(|state| state.size().x), Some(41.0), "the old key is kept for rollback");
    }

    /// `migrate` never replaces a value already stored under the new scoped key.
    #[test]
    fn a_new_key_is_never_overwritten() {
        let ctx = egui::Context::default();
        let old = egui::Id::unique("cc_status");
        ctx.data_mut(|data| data.insert_persisted(old, state(41.0)));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let new = ui.scope_id().with_salt(egui::IdSalt::new("cc_status"));
            ctx.data_mut(|data| data.insert_persisted(new, state(64.0)));
            migrate(ui);
            assert_eq!(egui::PanelState::load(ui.ctx(), new).map(|state| state.size().x), Some(64.0));
        });
        output.textures_delta.clear();
    }
}
