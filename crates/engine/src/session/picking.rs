use super::{Bounds2, Handle, Input, PendingWindow, Result, Session, Vec2, cmd, select};

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
}
