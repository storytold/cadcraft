use std::collections::HashSet;

use super::{Bounds2, Drawing, Entity, Handle, Input, PendingWindow, Result, Session, Space, Vec2, cmd, select};

/// Whether a live selection preview adds new objects or removes already-selected ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewAction {
    #[default]
    Add,
    Remove,
}

/// Result of a live cursor/area selection preview query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionPreview {
    pub handles: Vec<Handle>,
    pub action: PreviewAction,
    /// True when candidate count exceeded `SELECTIONPREVIEWLIMIT` and preview highlighting was suppressed.
    pub limit_exceeded: bool,
}

/// Active selection rubberband geometry in camera-plane UV coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum SelectionOverlayUv {
    /// Two-corner screen-aligned box (`crossing`: false = Window, true = Crossing).
    Box { a_uv: Vec2, b_uv: Vec2, crossing: bool },
    /// Closed polygon `WP` (`crossing = false`) or `CP` (`crossing = true`).
    Polygon { pts_uv: Vec<Vec2>, crossing: bool },
    /// Open fence polyline `F`.
    Fence { pts_uv: Vec<Vec2> },
}

impl Session {
    // ---------------- picking & projection ----------------

    /// Store the canvas camera-plane UV coordinate and projection for the next point/pick dispatch.
    pub fn set_pending_pick(&mut self, uv: Vec2, proj: select::PickProjection) {
        self.pending_cursor_uv = uv.is_finite().then_some((uv, proj));
    }

    pub(crate) fn take_pick_uv(&self, world_p: Vec2) -> Vec2 {
        self.pending_cursor_uv.map(|(uv, _)| uv).unwrap_or_else(|| self.pick_projection().project(world_p))
    }

    pub(crate) fn take_pick_anchor(&self, world_p: Vec2) -> cadcraft_geom::Vec3 {
        self.pick_projection().unproject_target_plane(self.take_pick_uv(world_p))
    }

    /// Active camera projection for picking and selection in the current space.
    pub fn pick_projection(&self) -> select::PickProjection {
        if let Some((_, proj)) = self.pending_cursor_uv {
            return proj;
        }
        let Ok(st) = self.state() else { return select::PickProjection::top() };
        if self.space() != Space::Model {
            return select::PickProjection::from_view(cadcraft_geom::Camera3D::TOP, st.paper_view().center, 0.0, 0.0);
        }
        let v = st.view();
        select::PickProjection::from_view(st.camera, v.center, v.target_z, v.twist)
    }

    pub(crate) fn mode_overlay_uv(
        &self,
        mode: &select::SelectMode,
        cursor_uv: Vec2,
        proj: &select::PickProjection,
    ) -> Option<SelectionOverlayUv> {
        let mut pts_uv = mode.uv_points(proj);
        let first = *pts_uv.first()?;
        match mode.kind {
            select::SelectKind::Window => Some(SelectionOverlayUv::Box { a_uv: first, b_uv: cursor_uv, crossing: false }),
            select::SelectKind::Crossing => Some(SelectionOverlayUv::Box { a_uv: first, b_uv: cursor_uv, crossing: true }),
            select::SelectKind::Box => Some(SelectionOverlayUv::Box {
                a_uv: first,
                b_uv: cursor_uv,
                crossing: self.window_is_crossing_uv(first, cursor_uv),
            }),
            select::SelectKind::WPolygon | select::SelectKind::CPolygon => {
                pts_uv.push(cursor_uv);
                Some(SelectionOverlayUv::Polygon { pts_uv, crossing: mode.kind == select::SelectKind::CPolygon })
            }
            select::SelectKind::Fence => {
                pts_uv.push(cursor_uv);
                Some(SelectionOverlayUv::Fence { pts_uv })
            }
            select::SelectKind::Group => None,
        }
    }

    /// Active selection rubberband in camera UV space for the canvas overlay.
    pub fn selection_overlay_uv(&self, cursor_uv: Vec2, proj: &select::PickProjection) -> Option<SelectionOverlayUv> {
        if let Some(pw) = self.pending_window {
            let a_uv = proj.project_3d_point(pw.corner_3d);
            return Some(SelectionOverlayUv::Box {
                a_uv,
                b_uv: cursor_uv,
                crossing: self.window_is_crossing_uv(a_uv, cursor_uv),
            });
        }
        if let Some(mode) = self.select_mode.as_ref() {
            return self.mode_overlay_uv(mode, cursor_uv, proj);
        }
        self.running.as_ref().and_then(|r| r.machine.custom_selection_overlay_uv(self, cursor_uv, proj))
    }

    /// Live selection preview candidates in camera-plane UV space for rollover, `PendingWindow`,
    /// `SelectMode` (`W`, `C`, `BOX`, `WP`, `CP`, `F`) and custom selection commands (e.g. `STRETCH`).
    pub fn preview_selection_uv(
        &self,
        cursor_uv: Vec2,
        aperture: f64,
        shift: bool,
        proj: &select::PickProjection,
    ) -> SelectionPreview {
        if !self.previews_selection() {
            return SelectionPreview::default();
        }
        let Ok(doc) = self.doc() else {
            return SelectionPreview::default();
        };
        let space = self.space();
        let frozen = self.viewport_frozen();
        let filter = self.settings.previewfilter;
        let cmd_removing = self.running.as_ref().is_some_and(|r| r.machine.is_removing_selection())
            || self.current_prompt().is_some_and(|p| p.message.starts_with("Remove"));

        if let Some(overlay) = self.selection_overlay_uv(cursor_uv, proj) {
            let idle_removing = self.running.is_none() && shift && self.settings.pickadd;
            let action = if cmd_removing || idle_removing { PreviewAction::Remove } else { PreviewAction::Add };
            let raw = match overlay {
                SelectionOverlayUv::Box { a_uv, b_uv, crossing } => {
                    if (b_uv.x - a_uv.x).abs() <= 1e-9 && (b_uv.y - a_uv.y).abs() <= 1e-9 {
                        return SelectionPreview { handles: Vec::new(), action, limit_exceeded: false };
                    }
                    select::select_window_projected_in(doc, &space, frozen, Bounds2::new(a_uv, b_uv), crossing, proj)
                }
                SelectionOverlayUv::Polygon { pts_uv, crossing } => {
                    if pts_uv.len() < 3 {
                        return SelectionPreview { handles: Vec::new(), action, limit_exceeded: false };
                    }
                    select::select_polygon_projected_in(doc, &space, frozen, &pts_uv, crossing, proj)
                }
                SelectionOverlayUv::Fence { pts_uv } => {
                    if pts_uv.len() < 2 {
                        return SelectionPreview { handles: Vec::new(), action, limit_exceeded: false };
                    }
                    select::select_fence_projected_in(doc, &space, frozen, &pts_uv, proj)
                }
            };
            let cur_sel: HashSet<Handle> = self.selection().into_iter().collect();
            let mut seen = HashSet::new();
            let mut handles: Vec<Handle> = cmd::groups::expand(self, raw)
                .into_iter()
                .filter(|h| seen.insert(*h))
                .filter(|h| doc.entity(*h).is_none_or(|e| !select::preview_filtered(doc, e, filter)))
                .filter(|h| match action {
                    PreviewAction::Remove => cur_sel.contains(h),
                    PreviewAction::Add => {
                        if self.settings.pickadd || self.running.is_some() {
                            !cur_sel.contains(h)
                        } else {
                            true
                        }
                    }
                })
                .collect();
            let limit = self.settings.selectionpreviewlimit as usize;
            if limit > 0 && handles.len() > limit {
                handles.clear();
                return SelectionPreview { handles, action, limit_exceeded: true };
            }
            return SelectionPreview { handles, action, limit_exceeded: false };
        }

        if self.select_mode.is_some() || self.pending_window.is_some() {
            let action = if cmd_removing { PreviewAction::Remove } else { PreviewAction::Add };
            return SelectionPreview { handles: Vec::new(), action, limit_exceeded: false };
        }

        let Some(h) = select::pick_projected_in(doc, &space, frozen, cursor_uv, aperture, proj)
            .filter(|h| doc.entity(*h).is_none_or(|e| !select::preview_filtered(doc, e, filter)))
        else {
            return SelectionPreview::default();
        };
        let cur_sel = self.selection();
        let in_sel = cur_sel.contains(&h);
        let action = if self.running.is_some() {
            if cmd_removing { PreviewAction::Remove } else { PreviewAction::Add }
        } else if shift && (self.settings.pickadd || in_sel) {
            PreviewAction::Remove
        } else {
            PreviewAction::Add
        };
        if action == PreviewAction::Remove && !in_sel {
            return SelectionPreview { handles: Vec::new(), action, limit_exceeded: false };
        }
        SelectionPreview { handles: vec![h], action, limit_exceeded: false }
    }

    // ---------------- picking without a command ----------------

    /// Open a selection window with one corner at `corner` and camera UV `uv` under `proj`.
    pub fn begin_window_at(&mut self, corner: Vec2, uv: Vec2, proj: &select::PickProjection) -> bool {
        let during_command = self.running.is_some();
        if !self.settings.pickdrag || !corner.is_finite() || (during_command && !self.current_prompt().is_some_and(|p| p.accept.select)) {
            return false;
        }
        let corner_3d = proj.unproject_target_plane(uv);
        self.pending_window = Some(PendingWindow { corner, corner_3d, during_command });
        true
    }

    /// Open a selection window with one corner at `corner`, for press-and-drag selection
    /// (AutoCAD's PICKDRAG = 2: a drag opens the window wherever it starts, even over an object;
    /// the next point, where the drag ends, closes it). Only while objects are being selected: no
    /// command running, or a command asking for objects. Returns whether a window was opened.
    pub fn begin_window(&mut self, corner: Vec2) -> bool {
        let proj = self.pick_projection();
        self.begin_window_at(corner, proj.project(corner), &proj)
    }

    /// Whether a selection window in camera UV space from `corner_uv` to `uv` is a crossing window
    /// (dragged leftwards on screen).
    pub fn window_is_crossing_uv(&self, corner_uv: Vec2, uv: Vec2) -> bool {
        (uv.x - corner_uv.x) < 0.0
    }

    /// Whether a selection window from `corner` to `p` is a crossing window: it was dragged
    /// leftwards on screen (in a twisted/3D view, not along the drawing's X axis).
    pub fn window_is_crossing(&self, corner: Vec2, p: Vec2) -> bool {
        let proj = self.pick_projection();
        self.window_is_crossing_uv(proj.project(corner), proj.project(p))
    }

    /// A click with no command running using explicit camera UV coordinates `uv`: pick/toggle
    /// objects or start/finish a selection window.
    pub fn idle_click_at(&mut self, p: Vec2, uv: Vec2, proj: &select::PickProjection, shift: bool) -> Result<()> {
        let space = self.space();
        let pickadd = self.settings.pickadd;
        if let Some(pw) = self.pending_window.take() {
            let a_uv = proj.project_3d_point(pw.corner_3d);
            let crossing = self.window_is_crossing_uv(a_uv, uv);
            let hs = select::select_window_projected_in(self.doc()?, &space, self.viewport_frozen(), Bounds2::new(a_uv, uv), crossing, proj);
            let hs = cmd::groups::expand(self, hs);
            let mut sel = if pickadd || shift { self.stored_selection() } else { Vec::new() };
            if shift && pickadd {
                sel.retain(|h| !hs.contains(h));
            } else {
                sel.extend(hs);
            }
            self.set_selection(sel);
            return Ok(());
        }
        let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
        match select::pick_projected_in(self.doc()?, &space, self.viewport_frozen(), uv, ap, proj) {
            Some(h) => self.idle_pick(h, shift),
            None => {
                self.pending_window = Some(PendingWindow {
                    corner: p,
                    corner_3d: proj.unproject_target_plane(uv),
                    during_command: false,
                });
            }
        }
        Ok(())
    }

    /// A click with no command running: pick/toggle objects or start a selection window.
    ///
    /// PICKADD on (the default): picks add to the selection and Shift removes. PICKADD off: each pick
    /// replaces the selection and Shift adds (Shift+pick on a selected object removes it).
    pub fn idle_click(&mut self, p: Vec2, shift: bool) -> Result<()> {
        let proj = self.pick_projection();
        self.idle_click_at(p, proj.project(p), &proj, shift)
    }

    /// An idle click that picked `h`, with [`Self::idle_click`]'s PICKADD and Shift rules.
    fn idle_pick(&mut self, h: Handle, shift: bool) {
        let pickadd = self.settings.pickadd;
        // With group selection the object's selectable groups come (or go) with it.
        let hs = cmd::groups::expand(self, vec![h]);
        let mut sel = if pickadd || shift { self.stored_selection() } else { Vec::new() };
        let had = sel.contains(&h);
        if shift && (pickadd || had) {
            sel.retain(|x| !hs.contains(x));
        } else if !had {
            sel.extend(hs);
        }
        self.set_selection(sel);
    }

    /// Whether a click selects objects by picking: no command running, or a command at
    /// "Select objects" with no selection window or typed mode under way.
    pub fn click_picks(&self) -> bool {
        self.pending_window.is_none()
            && match self.running.as_ref() {
                None => self.lisp_prompt().is_none(),
                Some(r) => self.select_mode.is_none() && r.machine.prompt(self).accept.select,
            }
    }

    fn cycling_hits_uv(&self, uv: Vec2, proj: &select::PickProjection) -> Vec<Handle> {
        if !self.settings.selection_cycling {
            return Vec::new();
        }
        let hs = self.pick_candidates_at(uv, proj);
        if hs.len() > 1 { hs } else { Vec::new() }
    }

    /// Every object under the pick box at camera UV coordinate `uv`, topmost first, when a click
    /// there picks objects (empty otherwise): what Shift+Space steps the highlight through.
    pub fn pick_candidates_at(&self, uv: Vec2, proj: &select::PickProjection) -> Vec<Handle> {
        if !self.click_picks() {
            return Vec::new();
        }
        let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
        self.doc().map(|d| select::pick_all_projected_in(d, &self.space(), self.viewport_frozen(), uv, ap, proj)).unwrap_or_default()
    }

    /// Every object under the pick box at `p`, nearest first, when a click there picks objects
    /// (empty otherwise): what Shift+Space steps the highlight through.
    pub fn pick_candidates(&self, p: Vec2) -> Vec<Handle> {
        let proj = self.pick_projection();
        self.pick_candidates_at(proj.project(p), &proj)
    }

    /// Whether selection cycling marks camera UV coordinate `uv` with its badge (SELECTIONCYCLING 1 or 2).
    pub fn cycling_badge_at(&self, uv: Vec2, proj: &select::PickProjection) -> bool {
        self.settings.selection_cycling_display >= 1 && !self.cycling_hits_uv(uv, proj).is_empty()
    }

    /// Whether selection cycling marks `p` with its badge (SELECTIONCYCLING 1 or 2): overlapping
    /// objects a click there would choose between.
    pub fn cycling_badge(&self, p: Vec2) -> bool {
        let proj = self.pick_projection();
        self.cycling_badge_at(proj.project(p), &proj)
    }

    /// The objects under camera UV `uv` when selection cycling chooses between them (SELECTIONCYCLING 2).
    pub fn cycling_candidates_at(&self, uv: Vec2, proj: &select::PickProjection) -> Vec<Handle> {
        if self.settings.selection_cycling_display < 2 {
            return Vec::new();
        }
        self.cycling_hits_uv(uv, proj)
    }

    /// The objects a click at `p` lists to choose from (SELECTIONCYCLING 2), nearest first; empty
    /// when the click should pick as usual.
    pub fn cycling_candidates(&self, p: Vec2) -> Vec<Handle> {
        let proj = self.pick_projection();
        self.cycling_candidates_at(proj.project(p), &proj)
    }

    /// Pick `h` from the selection cycling list, as a click on it alone would have: into the
    /// command at "Select objects", else into the selection (Shift as for [`Self::idle_click`]).
    pub fn pick_cycled(&mut self, h: Handle, shift: bool) -> Result<()> {
        // The list may be older than an undo or an erase.
        if self.doc()?.entity(h).is_none() {
            return Ok(());
        }
        if self.running.is_some() {
            let hs = cmd::groups::expand(self, vec![h]);
            return self.input(Input::Pick(hs));
        }
        self.idle_pick(h, shift);
        Ok(())
    }

    /// Remember the selection used by a command as "Previous".
    pub fn remember_selection(&mut self, sel: &[Handle]) {
        if let Ok(st) = self.state_mut() {
            st.previous_selection = sel.to_vec();
        }
    }

    // ---------------- one object by name ----------------

    /// Give the running command an object by name, as AutoLISP's `command` does with an entity
    /// name or an `entsel` list (name and point). A prompt picking one object with a point
    /// (BREAK's "Select object", MEASURE's "Select object to measure"…) takes the point, and a
    /// pick there finds that object ([`Session::pick_object`]) wherever it lies. A bare name
    /// stands at the object's DXF group 10 point (a line's start, a circle's centre, a
    /// polyline's first vertex), the point AutoCAD picks it at. The name holds for that input
    /// only: a later pick at the same point finds what lies there. Other prompts take the object
    /// as a pick.
    pub fn input_object(&mut self, h: Handle, at: Option<Vec2>) -> Result<()> {
        let one = self.current_prompt().is_some_and(|p| p.accept.point && !p.accept.select && p.picks_objects());
        let p = if one { at.or_else(|| self.doc().ok().and_then(|d| group_10(d, d.entity(h)?))) } else { None };
        let Some(p) = p.filter(|p| p.is_finite()) else { return self.input(Input::Pick(vec![h])) };
        self.pick_hint = Some((h, p));
        let r = self.input(Input::Point(p));
        self.pick_hint = None;
        r
    }

    /// The object a prompt picking one object finds at `p` in `space`: the one named for that
    /// point ([`Session::input_object`]), which is none when it lies in another space, else the
    /// object under `p` (or `pending_cursor_uv`) within `aperture`. Inside an MSPACE viewport a pick in
    /// model space finds what that viewport draws: not the objects on the layers frozen in it.
    pub fn pick_object(&self, space: &Space, p: Vec2, aperture: f64) -> Result<Option<Handle>> {
        let d = self.doc()?;
        if let Some((h, q)) = self.pick_hint
            && q == p
        {
            return Ok((d.space_of(h).as_ref() == Some(space)).then_some(h));
        }
        let proj = self.pick_projection();
        let uv = self.take_pick_uv(p);
        // The viewport shows model space only: its frozen layers hide nothing on the sheet.
        let frozen = if *space == Space::Model { self.viewport_frozen() } else { &[] };
        Ok(select::pick_projected_in(d, space, frozen, uv, aperture, &proj))
    }
}

/// An object's DXF group 10 point: the first point of its entity list, or its first grip where
/// entity lists are not built in.
pub(crate) fn group_10(d: &Drawing, e: &Entity) -> Option<Vec2> {
    let Some(codec) = cmd::file::entity_codec() else { return e.kind.grips().first().copied() };
    let (_, tags) = (codec.groups)(d, e);
    let i = tags.iter().position(|t| t.code == 10)?;
    let y = tags.get(i + 1).filter(|t| t.code == 20)?;
    Some(Vec2::new(tags[i].value.as_f64(), y.value.as_f64()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod preview_tests {
    use cadcraft_geom::{Vec2, Vec3};
    use serde_json::json;

    use crate::{Input, PreviewAction, Session};

    #[test]
    fn preview_polygon_fence_and_stretch_in_2d_and_front_view() {
        let mut s = Session::new();
        s.viewport_px = (1000.0, 800.0);
        s.execute("line", &json!({ "points": [[0, -100], [10, -100]] })).unwrap();
        s.execute("line", &json!({ "points": [[20, 200], [30, 200]] })).unwrap();
        let hs = s.doc().unwrap().model.handles();
        let (h1, h2) = (hs[0], hs[1]);
        s.execute("view.set", &json!({ "preset": "front", "center": [15.0, 0.0], "height": 40.0 })).unwrap();
        let proj = s.pick_projection();

        // ERASE -> WP in Front view: first two polygon points don't close an area yet; third vertex + cursor highlights h2.
        s.start("erase").unwrap();
        s.input(Input::Keyword("WP".into())).unwrap();
        assert!(s.previews_selection());
        let uv1 = Vec2::new(4.0, 5.0);
        s.set_pending_pick(uv1, proj);
        s.input(Input::Point(proj.unproject_target_plane(uv1).xy())).unwrap();
        let p_partial = s.preview_selection_uv(Vec2::new(16.0, 5.0), 0.5, false, &proj);
        assert!(p_partial.handles.is_empty(), "polygon needs at least 3 vertices including cursor");

        let uv2 = Vec2::new(16.0, 5.0);
        s.set_pending_pick(uv2, proj);
        s.input(Input::Point(proj.unproject_target_plane(uv2).xy())).unwrap();
        let uv3 = Vec2::new(16.0, -5.0);
        s.set_pending_pick(uv3, proj);
        s.input(Input::Point(proj.unproject_target_plane(uv3).xy())).unwrap();
        let p_poly = s.preview_selection_uv(Vec2::new(4.0, -5.0), 0.5, false, &proj);
        assert_eq!(p_poly.handles, vec![h2]);
        assert_eq!(p_poly.action, PreviewAction::Add);
        s.cancel();

        // ERASE -> Fence highlights h1 from first placed vertex + cursor.
        s.start("erase").unwrap();
        s.input(Input::Keyword("F".into())).unwrap();
        let f0 = Vec2::new(-10.0, 5.0);
        s.set_pending_pick(f0, proj);
        s.input(Input::Point(proj.unproject_target_plane(f0).xy())).unwrap();
        let p_fence = s.preview_selection_uv(Vec2::new(-10.0, -5.0), 0.5, false, &proj);
        assert_eq!(p_fence.handles, vec![h1]);
        s.cancel();

        // STRETCH exposes custom 3D UV overlay & live preview across corner drag and Remove mode.
        s.start("stretch").unwrap();
        let c0 = Vec2::new(-18.0, 5.0);
        s.set_pending_pick(c0, proj);
        s.input(Input::Point(proj.unproject_target_plane(c0).xy())).unwrap();
        assert!(s.selection_overlay_uv(Vec2::new(-2.0, -5.0), &proj).is_some());
        let p_stretch = s.preview_selection_uv(Vec2::new(-2.0, -5.0), 0.5, false, &proj);
        assert_eq!(p_stretch.handles, vec![h1]);
        assert_eq!(p_stretch.action, PreviewAction::Add);
        // Commit window so h1 is selected, then switch to Remove and hover over h1 with a window.
        let c1 = Vec2::new(-2.0, -5.0);
        s.set_pending_pick(c1, proj);
        s.input(Input::Point(proj.unproject_target_plane(c1).xy())).unwrap();
        s.input(Input::Keyword("Remove".into())).unwrap();
        let r0 = Vec2::new(-18.0, 5.0);
        s.set_pending_pick(r0, proj);
        s.input(Input::Point(proj.unproject_target_plane(r0).xy())).unwrap();
        let p_rm = s.preview_selection_uv(Vec2::new(-2.0, -5.0), 0.5, false, &proj);
        assert_eq!(p_rm.handles, vec![h1]);
        assert_eq!(p_rm.action, PreviewAction::Remove);
        // Unselected h2 is ignored in Remove mode (window covering only h2).
        let c_rm_end = Vec2::new(-2.0, -5.0);
        s.set_pending_pick(c_rm_end, proj);
        s.input(Input::Point(proj.unproject_target_plane(c_rm_end).xy())).unwrap();
        let r1 = Vec2::new(2.0, 5.0);
        s.set_pending_pick(r1, proj);
        s.input(Input::Point(proj.unproject_target_plane(r1).xy())).unwrap();
        let p_rm_miss = s.preview_selection_uv(Vec2::new(18.0, -5.0), 0.5, false, &proj);
        assert!(p_rm_miss.handles.is_empty());
        let _ = Vec3::ZERO;
    }

    #[test]
    fn preview_respects_add_remove_filter_and_limit() {
        let mut s = Session::new();
        s.viewport_px = (1000.0, 800.0);
        s.execute("layer.new", &json!({ "name": "L" })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        s.execute("line", &json!({ "points": [[0, 5], [10, 5]] })).unwrap();
        s.execute("line", &json!({ "points": [[0, 10], [10, 10]] })).unwrap();
        let hs = s.doc().unwrap().model.handles();
        let (h0, h1, h_locked) = (hs[0], hs[1], hs[2]);
        s.execute("properties.set", &json!({ "handles": [h_locked.hex()], "layer": "L" })).unwrap();
        s.execute("layer.set", &json!({ "name": "L", "locked": true })).unwrap();
        let proj = s.pick_projection();
        let uv_a = proj.project(Vec2::new(-2.0, 15.0));
        let uv_b = proj.project(Vec2::new(12.0, -2.0));

        // Pre-select h0, open an idle window covering all lines:
        s.set_selection(vec![h0]);
        assert!(s.begin_window_at(Vec2::new(-2.0, 15.0), uv_a, &proj));
        // Add excludes already-selected h0 and locked _h_locked (PREVIEWFILTER bit 1).
        let add = s.preview_selection_uv(uv_b, 0.5, false, &proj);
        assert_eq!(add.action, PreviewAction::Add);
        assert_eq!(add.handles, vec![h1]);
        // Shift idle switches to Remove and targets only selected h0 inside the window.
        let rm = s.preview_selection_uv(uv_b, 0.5, true, &proj);
        assert_eq!(rm.action, PreviewAction::Remove);
        assert_eq!(rm.handles, vec![h0]);

        // SELECTIONPREVIEWLIMIT caps massive previews without erroring.
        s.settings.pickadd = false;
        s.settings.selectionpreviewlimit = 1;
        let capped = s.preview_selection_uv(uv_b, 0.5, false, &proj);
        assert!(capped.handles.is_empty());
        assert!(capped.limit_exceeded);

        // SELECTIONPREVIEW = 0 disables idle preview.
        s.settings.selectionpreview = 0;
        let off = s.preview_selection_uv(uv_b, 0.5, false, &proj);
        assert!(off.handles.is_empty());
    }
}
