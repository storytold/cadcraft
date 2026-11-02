use super::{Bounds2, Drawing, Entity, Handle, Input, PendingWindow, Result, Session, Space, Vec2, cmd, select};

impl Session {
    // ---------------- picking without a command ----------------

    /// Open a selection window with one corner at `corner`, for press-and-drag selection
    /// (AutoCAD's PICKDRAG = 2: a drag opens the window wherever it starts, even over an object;
    /// the next point, where the drag ends, closes it). Only while objects are being selected: no
    /// command running, or a command asking for objects. Returns whether a window was opened.
    pub fn begin_window(&mut self, corner: Vec2) -> bool {
        let during_command = self.running.is_some();
        if !self.settings.pickdrag || !corner.is_finite() || (during_command && !self.current_prompt().is_some_and(|p| p.accept.select)) {
            return false;
        }
        self.pending_window = Some(PendingWindow { corner, during_command });
        true
    }

    /// A click with no command running: pick/toggle objects or start a selection window.
    ///
    /// PICKADD on (the default): picks add to the selection and Shift removes. PICKADD off: each pick
    /// replaces the selection and Shift adds (Shift+pick on a selected object removes it).
    pub fn idle_click(&mut self, p: Vec2, shift: bool) -> Result<()> {
        let space = self.space();
        let pickadd = self.settings.pickadd;
        if let Some(pw) = self.pending_window.take() {
            let crossing = p.x < pw.corner.x;
            let hs = select::select_window(self.doc()?, &space, Bounds2::new(pw.corner, p), crossing);
            let hs = cmd::groups::expand(self, hs);
            let mut sel = if pickadd || shift { self.selection() } else { Vec::new() };
            if shift && pickadd {
                sel.retain(|h| !hs.contains(h));
            } else {
                sel.extend(hs);
            }
            self.set_selection(sel);
            return Ok(());
        }
        let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
        match select::pick(self.doc()?, &space, p, ap) {
            Some(h) => self.idle_pick(h, shift),
            None => self.pending_window = Some(PendingWindow { corner: p, during_command: false }),
        }
        Ok(())
    }

    /// An idle click that picked `h`, with [`Self::idle_click`]'s PICKADD and Shift rules.
    fn idle_pick(&mut self, h: Handle, shift: bool) {
        let pickadd = self.settings.pickadd;
        // With group selection the object's selectable groups come (or go) with it.
        let hs = cmd::groups::expand(self, vec![h]);
        let mut sel = if pickadd || shift { self.selection() } else { Vec::new() };
        let had = sel.contains(&h);
        if shift && (pickadd || had) {
            sel.retain(|x| !hs.contains(x));
        } else if !had {
            sel.extend(hs);
        }
        self.set_selection(sel);
    }

    /// Whether a click at `p` selects objects by picking: no command running, or a command at
    /// "Select objects" with no selection window or typed mode under way.
    pub fn click_picks(&self) -> bool {
        self.pending_window.is_none()
            && match self.running.as_ref() {
                None => self.lisp_prompt().is_none(),
                Some(r) => self.select_mode.is_none() && r.machine.prompt(self).accept.select,
            }
    }

    /// The objects under the pick box at `p` when selection cycling is on and more than one is
    /// there: what a click at `p` would choose between.
    fn cycling_hits(&self, p: Vec2) -> Vec<Handle> {
        if !self.settings.selection_cycling {
            return Vec::new();
        }
        let hs = self.pick_candidates(p);
        if hs.len() > 1 { hs } else { Vec::new() }
    }

    /// Every object under the pick box at `p`, nearest first, when a click there picks objects
    /// (empty otherwise): what Shift+Space steps the highlight through.
    pub fn pick_candidates(&self, p: Vec2) -> Vec<Handle> {
        if !self.click_picks() {
            return Vec::new();
        }
        let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
        self.doc().map(|d| select::pick_all(d, &self.space(), p, ap)).unwrap_or_default()
    }

    /// Whether selection cycling marks `p` with its badge (SELECTIONCYCLING 1 or 2): overlapping
    /// objects a click there would choose between.
    pub fn cycling_badge(&self, p: Vec2) -> bool {
        self.settings.selection_cycling_display >= 1 && !self.cycling_hits(p).is_empty()
    }

    /// The objects a click at `p` lists to choose from (SELECTIONCYCLING 2), nearest first; empty
    /// when the click should pick as usual.
    pub fn cycling_candidates(&self, p: Vec2) -> Vec<Handle> {
        if self.settings.selection_cycling_display < 2 {
            return Vec::new();
        }
        self.cycling_hits(p)
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
    /// object under `p` within `aperture`.
    pub fn pick_object(&self, space: &Space, p: Vec2, aperture: f64) -> Result<Option<Handle>> {
        let d = self.doc()?;
        if let Some((h, q)) = self.pick_hint
            && q == p
        {
            return Ok((d.space_of(h).as_ref() == Some(space)).then_some(h));
        }
        Ok(select::pick(d, space, p, aperture))
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
