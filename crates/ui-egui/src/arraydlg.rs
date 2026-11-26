//! The Array window (ARRAYRECT, ARRAYPOLAR): a second view of the **running** command. The command
//! line keeps its prompts and the canvas keeps its preview; the window reads and writes the live
//! parameters ([`cadcraft_engine::ArrayLive`]), so an edit on either side shows on the other.

use cadcraft_doc::Space;
use cadcraft_engine::{ArrayLive, ArrayLiveKind};
use cadcraft_geom::Vec2;

use crate::CadApp;
use crate::theme::Tokens;

/// The window while it is open.
#[derive(Clone, Debug, Default)]
pub struct ArrayDlgState {
    /// The last refused value, shown in the window.
    pub error: Option<String>,
    /// The error was set in the current frame and has not been painted yet.
    pub error_new: bool,
    /// The error has been painted at least once (so the user has seen it).
    pub error_seen: bool,
    /// Raw (untranslated) prompt message of a refused OK, shown under the fields.
    pub hint: Option<String>,
    /// The drawing the window belongs to (`DocState::uid`).
    pub uid: u64,
    /// The space it belongs to (`DocState::edit_space`).
    pub space: Space,
}

impl ArrayDlgState {
    /// End-of-frame error lifecycle: a blur clears an error the user has already seen.
    fn end_frame(&mut self, lost_focus: bool) {
        if lost_focus && self.error_seen {
            self.error = None;
            self.error_new = false;
            self.error_seen = false;
        }
        if self.error_new {
            self.error_new = false;
            self.error_seen = true;
        }
    }
}

/// The command each array kind runs.
fn command(kind: ArrayLiveKind) -> &'static str {
    match kind {
        ArrayLiveKind::Rectangular => "arrayrect",
        ArrayLiveKind::Polar => "arraypolar",
    }
}

/// Start the real command and open the window on it.
pub fn open(app: &mut CadApp, kind: ArrayLiveKind) {
    let (uid, space) = {
        let Ok(st) = app.session.state() else { return };
        (st.uid, st.edit_space())
    };
    // The session, not `CadApp::start`: that one would come back here through the UI commands.
    if let Err(e) = app.session.start(command(kind)) {
        app.session.echo(e.to_string());
        return;
    }
    app.array_dlg = Some(ArrayDlgState { error: None, hint: None, error_new: false, error_seen: false, uid, space });
    app.ui.dialog = Some("array".into());
}

/// OK: finish the running array. True when the window closes.
pub fn ok(app: &mut CadApp) -> bool {
    if let Some(st) = app.array_dlg.as_mut() {
        st.hint = None;
    }
    if app.session.array_live().is_none() {
        return true;
    }
    // An error is set: do not commit yet; the window stays open and the error is shown.
    if app.array_dlg.as_ref().is_some_and(|st| st.error.is_some()) {
        return false;
    }
    match app.session.commit_array() {
        Ok(true) => {
            app.array_dlg = None;
            true
        }
        Ok(false) => {
            // Say what the command is waiting for (the polar centre), not the keyword list.
            if let Some(p) = app.session.current_prompt()
                && !p.accept.select
                && let Some(st) = app.array_dlg.as_mut()
            {
                st.hint = Some(p.message.clone());
            }
            false
        }
        Err(e) => {
            if let Some(st) = app.array_dlg.as_mut() {
                st.error = Some(e.to_string());
            }
            false
        }
    }
}

enum Action {
    None,
    Ok,
    Cancel,
    PickCenter,
}

/// A clamped 1..=1_000_000 DragValue; returns the widget response so the caller can label it.
fn count_field(ui: &mut egui::Ui, v: &mut u32) -> egui::Response {
    ui.add(egui::DragValue::new(v).range(1..=1_000_000))
}

/// The fields both kinds have; the counts come back through the caller's locals.
fn common_fields(ui: &mut egui::Ui, rows: &mut u32, levels: &mut u32, live: &mut ArrayLive) -> (bool, bool) {
    let mut changed = false;
    let mut lost_focus = false;
    egui::Grid::new("array_common").num_columns(2).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
        let label = ui.label(crate::tl!("Rows"));
        let r = count_field(ui, rows).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        let label = ui.label(crate::tl!("Levels"));
        let r = count_field(ui, levels).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        let label = ui.label(crate::tl!("Row spacing"));
        let r = ui.add(egui::DragValue::new(&mut live.row_spacing).speed(0.1)).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        let label = ui.label(crate::tl!("Level spacing"));
        let r = ui.add(egui::DragValue::new(&mut live.level_spacing).speed(0.1)).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        let label = ui.label(crate::tl!("Row elevation"));
        let r = ui.add(egui::DragValue::new(&mut live.row_elevation).speed(0.1)).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
    });
    let r = ui.checkbox(&mut live.associative, crate::tl!("Associative"));
    changed |= r.changed();
    lost_focus |= r.lost_focus();
    (changed, lost_focus)
}

/// The rectangular-only fields.
fn rect_fields(ui: &mut egui::Ui, cols: &mut u32, live: &mut ArrayLive) -> (bool, bool) {
    let mut changed = false;
    let mut lost_focus = false;
    egui::Grid::new("array_rect").num_columns(2).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
        let label = ui.label(crate::tl!("Columns"));
        let r = count_field(ui, cols).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        let label = ui.label(crate::tl!("Column spacing"));
        let r = ui.add(egui::DragValue::new(&mut live.col_spacing).speed(0.1)).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
    });
    (changed, lost_focus)
}

/// The polar-only fields. "Pick center" asks the running command for the point; the window stays.
fn polar_fields(ui: &mut egui::Ui, center: &mut Vec2, items: &mut u32, live: &mut ArrayLive, action: &mut Action) -> (bool, bool) {
    let mut changed = false;
    let mut lost_focus = false;
    egui::Grid::new("array_polar").num_columns(2).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
        let label = ui.label(crate::tl!("Center X"));
        let r = ui.add(egui::DragValue::new(&mut center.x).speed(0.1)).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        let label = ui.label(crate::tl!("Center Y"));
        let r = ui.add(egui::DragValue::new(&mut center.y).speed(0.1)).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        ui.label("");
        let r = ui.button(crate::tl!("Pick center"));
        lost_focus |= r.lost_focus();
        if r.clicked() {
            *action = Action::PickCenter;
        }
        ui.end_row();
        let label = ui.label(crate::tl!("Items"));
        let r = count_field(ui, items).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        let mut between = live.between.unwrap_or(0.0);
        let label = ui.label(crate::tl!("Angle between"));
        let r = ui.add(egui::DragValue::new(&mut between).speed(0.1).suffix("°")).labelled_by(label.id);
        if r.changed() {
            live.between = Some(between);
            changed = true;
        }
        lost_focus |= r.lost_focus();
        ui.end_row();
        let label = ui.label(crate::tl!("Fill angle"));
        let r = ui.add(egui::DragValue::new(&mut live.fill).speed(1.0).suffix("°")).labelled_by(label.id);
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
        ui.label("");
        let r = ui.checkbox(&mut live.rotate, crate::tl!("Rotate items"));
        changed |= r.changed();
        lost_focus |= r.lost_focus();
        ui.end_row();
    });
    (changed, lost_focus)
}

/// Apply the window's edits, only when a widget reported a change this frame. A refused value is
/// reported in the window and the old one stays.
fn apply_live(app: &mut CadApp, edited: &ArrayLive, changed: bool) {
    if !changed {
        return;
    }
    match app.session.set_array_live(edited) {
        Ok(()) => {
            if let Some(s) = app.array_dlg.as_mut() {
                s.error = None;
                s.error_new = false;
                s.error_seen = false;
                s.hint = None;
            }
        }
        Err(e) => {
            if let Some(s) = app.array_dlg.as_mut() {
                s.error = Some(e.to_string());
                s.error_new = true;
                s.error_seen = false;
            }
        }
    }
}

/// Draw the window while `app.ui.dialog == Some("array")`.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let Some(st) = app.array_dlg.clone() else { return };
    let here = app.session.state().ok().map(|s| (s.uid, s.edit_space()));
    // A drawing change always closes the window, even while a command is suspended.
    if here != Some((st.uid, st.space)) {
        app.array_dlg = None;
        *open = false;
        return;
    }
    let Some(live) = app.session.array_live() else {
        // A transparent command suspended the array: keep the window and its state, draw nothing.
        let array_suspended = app.session.suspended.as_ref().is_some_and(|r| r.machine.array_live().is_some());
        if array_suspended {
            return;
        }
        app.array_dlg = None;
        *open = false;
        return;
    };
    let selecting = app.session.current_prompt().is_some_and(|p| p.accept.select);
    let title = match live.kind {
        ArrayLiveKind::Rectangular => crate::tl!("Rectangular Array"),
        ArrayLiveKind::Polar => crate::tl!("Polar Array"),
    };
    let mut edited = live.clone();
    let mut cols = live.cols.min(u64::from(u32::MAX)) as u32;
    let mut rows = live.rows.min(u64::from(u32::MAX)) as u32;
    let mut levels = live.levels.min(u64::from(u32::MAX)) as u32;
    let mut items = live.items.min(u64::from(u32::MAX)) as u32;
    let mut center = live.center.unwrap_or(Vec2::ZERO);
    let mut action = Action::None;
    let mut changed = false;
    let mut lost_focus = false;
    egui::Window::new(title)
        .id(egui::Id::unique("array_dialog"))
        .open(open)
        .collapsible(false)
        .resizable(true)
        .max_size(ctx.input(|i| i.viewport_rect()).size() - egui::vec2(16.0, 16.0))
        .show(ctx, |ui| {
            // The parameter fields scroll; the error, hint and buttons stay outside.
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                let fields = ui.add_enabled_ui(!selecting, |ui| {
                    let mut lost_focus = false;
                    let (changed, lf) = match live.kind {
                        ArrayLiveKind::Rectangular => rect_fields(ui, &mut cols, &mut edited),
                        ArrayLiveKind::Polar => polar_fields(ui, &mut center, &mut items, &mut edited, &mut action),
                    };
                    lost_focus |= lf;
                    let (common_changed, common_lf) = common_fields(ui, &mut rows, &mut levels, &mut edited);
                    lost_focus |= common_lf;
                    (changed | common_changed, lost_focus)
                });
                changed |= fields.inner.0;
                lost_focus |= fields.inner.1;
            });
            if let Some(e) = &st.error {
                ui.colored_label(Tokens::get().warn, e);
            }
            if let Some(h) = &st.hint {
                ui.label(crate::i18n::t(h));
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(!selecting, egui::Button::new(crate::tl!("OK"))).clicked() {
                    action = Action::Ok;
                }
                if ui.button(crate::tl!("Cancel")).clicked() {
                    action = Action::Cancel;
                }
                if selecting {
                    ui.label(crate::tl!("Select objects first"));
                }
            });
        });
    edited.cols = u64::from(cols);
    edited.rows = u64::from(rows);
    edited.levels = u64::from(levels);
    edited.items = u64::from(items);
    if live.kind == ArrayLiveKind::Polar && live.center.is_some() {
        edited.center = Some(center);
    }
    // Only the running command draws; a refused value is reported and the old one stays.
    apply_live(app, &edited, changed);
    match action {
        Action::Ok => {
            if ok(app) {
                *open = false;
            }
        }
        Action::Cancel => {
            app.session.cancel();
            app.array_dlg = None;
            *open = false;
        }
        Action::PickCenter => {
            app.session.array_pick_center();
        }
        // The window's X: Cancel.
        Action::None if !*open => {
            app.session.cancel();
            app.array_dlg = None;
        }
        Action::None => {}
    }
    // End-of-frame error lifecycle: a blur clears an error the user has already seen.
    if let Some(s) = app.array_dlg.as_mut() {
        s.end_frame(lost_focus);
    }
}

#[cfg(test)]
mod tests;
