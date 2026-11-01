use super::{
    Arc, Bounds2, Drag, Drawing, EngineError, Entity, Handle, Input, PendingWindow, Prompt, Result, Running, Session, Snapshot, Step, Value, Vec2,
    assoc, cmd, find_command, guard, pointmod, select, snap, sysvars,
};

impl Session {
    // ---------------- interactive commands ----------------

    /// Start a command by name or alias as if typed at the command line.
    pub fn start(&mut self, name: &str) -> Result<()> {
        self.last_polar = None;
        self.pick_hint = None;
        let name = name.trim();
        let (transparent, name) = match name.strip_prefix('\'') {
            Some(n) => (true, n),
            None => (false, name),
        };
        let lower = name.trim_start_matches(['_', '.', '-']).to_ascii_lowercase();
        // A command-line form with an id of its own (-ATTEDIT) is not the command it prefixes.
        let dashed = cmd::resolve_alias(&name.trim_start_matches(['_', '.']).to_ascii_lowercase());
        let id = if find_command(&dashed).is_some() { dashed } else { cmd::resolve_alias(&lower) };
        let Some(spec) = find_command(&id) else {
            // A command defined in LISP: (defun c:NAME …).
            if !transparent && let Some(f) = self.lisp_command(&lower) {
                if self.running.is_some() {
                    self.cancel();
                }
                self.last_command = Some(lower.clone());
                self.echo(format!("Command: {}", lower.to_ascii_uppercase()));
                return self.lisp_run_command(&lower, f);
            }
            // A bare system variable name (ORTHOMODE, GRIDMODE…) acts as SETVAR on that variable.
            let var = lower.to_ascii_uppercase();
            if !name.starts_with(['_', '.', '-']) && sysvars::get(self, &var).is_some() {
                self.start(if transparent { "'setvar" } else { "setvar" })?;
                return self.input(Input::Text(var));
            }
            return Err(EngineError::UnknownCommand(name.to_string()));
        };
        if transparent && spec.transparent && self.running.is_some() {
            // Transparent commands (zoom, pan…) run without cancelling the active one.
            if spec.interactive.is_none() {
                self.echo(format!(">>{}", spec.label));
                return self.execute(spec.id, &Value::Null).map(|_| ());
            }
            if self.suspended.is_some() {
                self.echo("A transparent command is already in progress.");
                return Ok(());
            }
        }
        let nested = transparent && spec.transparent && self.running.is_some() && spec.interactive.is_some();
        // Ctrl+Z / U / UNDO while a command's prompt offers its own Undo keyword (LINE, PLINE…)
        // takes back the last segment and keeps the command running, as the keyword does.
        if !nested && matches!(spec.id, "u" | "undo") && self.current_prompt().is_some_and(|p| p.keywords.iter().any(|k| k == "Undo")) {
            self.echo(format!("Command: {}", spec.id.to_ascii_uppercase()));
            return self.input(Input::Keyword("Undo".into()));
        }
        if !nested && self.running.is_some() {
            self.cancel();
        }
        (spec.enabled)(self).map_err(|m| EngineError::Disabled(spec.id.into(), m))?;
        if let Some(why) = cmd::blocks::bedit::refused_in_editor(self, spec.id) {
            return Err(EngineError::Disabled(spec.id.into(), why));
        }
        if nested {
            // The outer command stays the one Enter repeats.
            self.echo(format!(">>{}", spec.id.to_ascii_uppercase()));
        } else {
            self.last_command = Some(spec.id.to_string());
            self.echo(format!("Command: {}", spec.id.to_ascii_uppercase()));
        }
        match spec.interactive {
            Some(factory) => {
                let machine = guard::Guarded::create(spec.id, factory, self)?;
                let st = self.state()?;
                let before = st.doc.clone();
                let selection_before = st.selection.clone();
                if nested {
                    self.suspended = self.running.take();
                }
                self.running = Some(Running { id: spec.id.to_string(), machine, before, selection_before });
                self.pending_window = None;
                // Some commands complete immediately (e.g. ERASE with a pickfirst selection).
                self.feed(None)?;
                Ok(())
            }
            None => {
                let r = self.execute(spec.id, &Value::Null)?;
                if let Some(s) = r.as_str() {
                    self.echo(s.to_string());
                } else if !r.is_null()
                    && let Some(msg) = r.get("message").and_then(Value::as_str)
                {
                    for l in msg.lines() {
                        self.echo(l.to_string());
                    }
                }
                Ok(())
            }
        }
    }

    /// Feed an object-snap pick, retaining its viewport origin for measurements. Point modifiers
    /// that combine or constrain coordinates produce ordinary drawing-space points.
    pub fn input_snapped(&mut self, hit: snap::SnapHit) -> Result<()> {
        let source = hit.viewport.filter(|_| self.point_mods.iter().all(|f| matches!(f, pointmod::Frame::Snap { mode } if *mode != 0)));
        let previous = std::mem::replace(&mut self.picked_viewport, source);
        let result = self.input(hit.input());
        self.picked_viewport = previous;
        result
    }

    /// Give an input to the running command. With no command running, `Enter` repeats the last
    /// command (as in AutoCAD) and points do nothing. A LISP routine waiting in a `get…`
    /// function takes the input instead; one paused in `command` carries on after it.
    pub fn input(&mut self, input: Input) -> Result<()> {
        if self.lisp_input(input.clone()) {
            return Ok(());
        }
        let r = self.input_to_command(input);
        self.lisp_after_command_input();
        r
    }

    fn input_to_command(&mut self, input: Input) -> Result<()> {
        if self.running.is_none() {
            if input == Input::Enter
                && let Some(last) = self.last_command.clone()
            {
                return self.start(&last);
            }
            return Ok(());
        }
        if !self.point_mods.is_empty() {
            return self.modifier_input(input);
        }
        // A deferred snap is only meaningful where the prompt resolves it; elsewhere it is the
        // point it was picked at.
        let input = match input {
            Input::Deferred(d) if !self.current_prompt().is_some_and(|p| p.deferred) => Input::Point(d.at),
            other => other,
        };
        // Generic selection handling during "Select objects:" prompts.
        let input = self.preprocess_selection(input)?;
        match input {
            Some(i) => {
                // SIngle: the first selection that finds objects also ends the prompt.
                let single = self.select_single && matches!(&i, Input::Pick(hs) if !hs.is_empty());
                self.feed(Some(i))?;
                if single {
                    self.select_single = false;
                    if self.current_prompt().is_some_and(|p| p.accept.select) {
                        self.feed(Some(Input::Enter))?;
                    }
                }
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn preprocess_selection(&mut self, input: Input) -> Result<Option<Input>> {
        // The command's own prompt: a typed selection mode only applies at its "Select objects".
        let Some(prompt) = self.running.as_ref().map(|r| r.machine.prompt(self)) else { return Ok(Some(input)) };
        if !prompt.accept.select {
            return Ok(Some(input));
        }
        if self.select_mode.is_some() {
            return self.select_mode_input(input);
        }
        // `_L`, `_ALL`, `_W`: the English name of a selection keyword, as scripts write it.
        let input = match input {
            Input::Text(t) => Input::Text(t.trim().strip_prefix('_').map(str::to_string).unwrap_or(t)),
            other => other,
        };
        if let Input::Text(t) | Input::Keyword(t) = &input
            && let Some(m) = select::SelectMode::from_keyword(t)
        {
            self.pending_window = None;
            self.select_mode = Some(m);
            return Ok(None);
        }
        if let Input::Text(t) | Input::Keyword(t) = &input
            && !prompt.keywords.iter().any(|k| k.eq_ignore_ascii_case(t.trim()))
        {
            match t.trim().to_ascii_lowercase().as_str() {
                "si" | "single" => {
                    self.select_single = true;
                    return Ok(None);
                }
                // AUto (a pick, or a window where nothing is picked) is how points always select;
                // Multiple picks the same way until Enter.
                "au" | "auto" | "m" | "multiple" => return Ok(None),
                _ => {}
            }
        }
        let space = self.space();
        match input {
            Input::Point(p) => {
                let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
                if let Some(pw) = self.pending_window.take() {
                    let crossing = p.x < pw.corner.x;
                    let hs = select::select_window(self.doc()?, &space, Bounds2::new(pw.corner, p), crossing);
                    return Ok(Some(Input::Pick(cmd::groups::expand(self, hs))));
                }
                match select::pick(self.doc()?, &space, p, ap) {
                    Some(h) => Ok(Some(Input::Pick(cmd::groups::expand(self, vec![h])))),
                    None => {
                        self.pending_window = Some(PendingWindow { corner: p, during_command: true });
                        Ok(None)
                    }
                }
            }
            Input::Text(ref t) | Input::Keyword(ref t) => {
                let tl = t.trim().to_ascii_lowercase();
                let d = self.doc()?;
                let store = d.space(&space);
                let picked = match tl.as_str() {
                    "all" => store.map(|s| s.iter().filter(|e| d.is_visible(e)).map(|e| e.handle).collect()),
                    "l" | "last" => store.and_then(|s| s.last()).map(|e| vec![e.handle]),
                    "p" | "previous" => Some(self.state()?.previous_selection.clone()),
                    _ => None,
                };
                match picked {
                    Some(hs) => Ok(Some(Input::Pick(cmd::groups::expand(self, hs)))),
                    None => Ok(Some(input)),
                }
            }
            Input::Cancel => {
                self.pending_window = None;
                Ok(Some(Input::Cancel))
            }
            other => Ok(Some(other)),
        }
    }

    /// Input while a typed selection mode (W, C, F, WP, CP) collects its points: the selected
    /// objects go to the command as a pick once the mode is complete.
    fn select_mode_input(&mut self, input: Input) -> Result<Option<Input>> {
        let Some(mut mode) = self.select_mode.take() else { return Ok(Some(input)) };
        if mode.kind == select::SelectKind::Group {
            return match input {
                Input::Text(name) | Input::Keyword(name) => match select::select_group(self.doc()?, &self.space(), &name) {
                    Some(hs) => Ok(Some(Input::Pick(hs))),
                    None => {
                        self.echo(format!("No group named \"{}\".", name.trim()));
                        Ok(None)
                    }
                },
                // Enter or Esc leaves the group name prompt.
                Input::Cancel => Ok(Some(input)),
                Input::Enter => Ok(None),
                _ => {
                    self.echo("Invalid group name.");
                    self.select_mode = Some(mode);
                    Ok(None)
                }
            };
        }
        match input {
            Input::Point(p) => {
                if mode.push(p) {
                    return Ok(mode.select(self.doc()?, &self.space()).map(Input::Pick));
                }
            }
            Input::Keyword(k) if k == "Undo" => {
                mode.pts.pop();
            }
            // Enter ends a fence or polygon; with too few points the mode is dropped.
            Input::Enter => return Ok(mode.select(self.doc()?, &self.space()).map(Input::Pick)),
            Input::Cancel | Input::Pick(_) => return Ok(Some(input)),
            _ => self.echo("Invalid point."),
        }
        self.select_mode = Some(mode);
        Ok(None)
    }

    pub(crate) fn feed(&mut self, input: Option<Input>) -> Result<()> {
        let base = if matches!(input, Some(Input::Point(_))) { self.current_prompt().and_then(|p| p.base) } else { None };
        let Some(mut run) = self.running.take() else { return Ok(()) };
        let step = match input {
            Some(Input::Cancel) => Ok(Step::Cancel),
            Some(i) => {
                if let Input::Point(p) = &i {
                    self.last_point = *p;
                    if let Some(b) = base.filter(|b| b.is_finite() && p.is_finite() && b.dist(*p) > 1e-12) {
                        self.last_segment = Some((b, *p));
                    }
                    // A temporary tracking point (TT) serves one point.
                    self.tracking.release_temporary();
                }
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run.machine.input(self, i)));
                match r {
                    Ok(r) => r,
                    Err(_) => Err(EngineError::Internal(run.id.clone(), "panic".into())),
                }
            }
            // A command starts with no modifiers pending.
            None => {
                self.point_mods.clear();
                run.machine.begin(self)
            }
        };
        // Redraw what the command has added so far (LINE adds a segment per point), not only when it ends.
        if let Ok(st) = self.state_mut()
            && !Arc::ptr_eq(&run.before, &st.doc)
        {
            st.revision += 1;
        }
        match step {
            Ok(Step::Continue) => {
                self.running = Some(run);
                Ok(())
            }
            Ok(Step::Done) => {
                self.finish(run, false);
                self.resume_suspended();
                Ok(())
            }
            Ok(Step::Cancel) => {
                self.echo("*Cancel*");
                self.finish(run, true);
                self.resume_suspended();
                Ok(())
            }
            Err(EngineError::Internal(id, m)) => {
                // Restore and end the command.
                if let Ok(st) = self.state_mut() {
                    st.doc = run.before.clone();
                }
                self.echo(format!("Internal error in {id}: {m}"));
                self.resume_suspended();
                Err(EngineError::Internal(id, m))
            }
            Err(e) => {
                // Invalid input: report and keep prompting (AutoCAD re-prompts).
                self.echo(e.to_string());
                self.running = Some(run);
                Ok(())
            }
        }
    }

    /// End a command: every way out of one comes through here. The passes after a command
    /// (constraints, associativity, groups) can panic like the command itself; a panic restores
    /// the drawing as it was before the command, as a panic in its `input` does.
    fn finish(&mut self, run: Running, cancelled: bool) {
        let (id, before, selection) = (run.id.clone(), run.before.clone(), run.selection_before.clone());
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.finish_guarded(run, cancelled))) {
            if let Ok(st) = self.state_mut() {
                st.doc = before;
                st.selection = selection;
            }
            self.echo(format!("Internal error in {id}: {}", crate::guard::message(&*p)));
        }
    }

    fn finish_guarded(&mut self, run: Running, cancelled: bool) {
        // A typed selection mode or SIngle never outlives its command.
        self.select_mode = None;
        self.select_single = false;
        if find_command(&run.id).is_some_and(|c| c.undoable)
            && let Err(e) = self.refuse_non_finite(None, &run.before, &run.selection_before, &run.id)
        {
            self.echo(e.to_string());
        }
        // A command outside undo that changed the drawing (UNDO, which swapped it for an earlier
        // one) records no undo step, as in `execute`.
        if find_command(&run.id).is_some_and(|c| !c.undoable) && self.state().is_ok_and(|st| !Arc::ptr_eq(&run.before, &st.doc)) {
            self.pending_window = None;
            return;
        }
        self.pending_window = None;
        self.select_mode = None;
        let label = find_command(&run.id).map(|c| c.label).unwrap_or("Command");
        let _ = cancelled;
        if let Some(m) = cmd::blocks::refedit::after_command(self, &run.before, &run.id) {
            self.echo(m);
        }
        self.after_command(Some(&run.before), None, true);
        // A transparent command's changes belong to the command it ran inside, and a command a
        // LISP routine ran belongs to the routine's single undo step.
        if self.suspended.is_some() || self.lisp_groups_undo() {
            if let Ok(st) = self.state_mut()
                && !Arc::ptr_eq(&run.before, &st.doc)
            {
                st.revision += 1;
            }
            return;
        }
        if let Ok(st) = self.state_mut()
            && !Arc::ptr_eq(&run.before, &st.doc)
        {
            st.push_undo(Snapshot { label: label.to_string(), doc: run.before, selection: run.selection_before });
            st.revision += 1;
        }
    }

    /// The post-command passes, in order, run after every command — by [`Session::execute`] and
    /// when an interactive command finishes — so their edits land in the command's own undo step:
    /// group membership is pruned, constraints are inferred, purged and re-solved, and
    /// associative dimensions and hatches follow their sources. A new pass is added here, once.
    ///
    /// `before` is the drawing the command started from; `uid` (when known) is the document it
    /// started in, so associativity is not applied across a document switch; `ok` is false for a
    /// failed or non-undoable programmatic command, which skips the constraint pass.
    pub(super) fn after_command(&mut self, before: Option<&Arc<Drawing>>, uid: Option<u64>, ok: bool) {
        #[cfg(test)]
        crate::guard::tests::panic_if_armed();
        cmd::groups::after_command(self, before);
        cmd::constraints::after_command(self, before, ok);
        assoc::after_command(self, before, uid);
    }

    fn resume_suspended(&mut self) {
        if let Some(outer) = self.suspended.take() {
            self.echo(format!("Resuming {} command.", outer.id.to_ascii_uppercase()));
            self.running = Some(outer);
        }
    }

    /// Cancel the running command (Esc). With no command, clears the selection. Inside a
    /// transparent command, cancels it and the command it interrupted.
    pub fn cancel(&mut self) {
        self.point_mods.clear();
        // Esc stops a LISP routine waiting for input, and the command it paused in.
        if self.lisp_waiting() {
            if self.running.is_some() {
                let _ = self.feed(Some(Input::Cancel));
            }
            self.echo("*Cancel*");
            self.lisp_abort("Function cancelled");
            return;
        }
        if self.running.is_some() {
            if self.suspended.is_some() {
                if let Some(nested) = self.running.take() {
                    self.finish(nested, true);
                }
                self.running = self.suspended.take();
            }
            let _ = self.feed(Some(Input::Cancel));
        } else {
            self.pending_window = None;
            self.set_selection(Vec::new());
        }
    }

    /// The multiline text the running command is collecting, for the in-place editor
    /// ([`Interactive::editing_mtext`]).
    pub fn editing_mtext(&self) -> Option<cadcraft_doc::MText> {
        self.running.as_ref().and_then(|r| r.machine.editing_mtext())
    }

    pub fn current_prompt(&self) -> Option<Prompt> {
        if let Some(p) = self.lisp_prompt() {
            return Some(p);
        }
        self.running.as_ref().map(|r| {
            let p = r.machine.prompt(self);
            match &self.select_mode {
                Some(m) if p.accept.select => m.prompt(),
                _ => p,
            }
        })
    }

    /// Whether the cursor picks objects right now, so the UI shows the pick box (PICKBOX) at the
    /// crosshair: at prompts that select objects, and with no command running when PICKFIRST is on
    /// (noun-verb selection).
    pub fn picking_objects(&self) -> bool {
        match self.current_prompt() {
            Some(p) => p.picks_objects(),
            None => self.settings.pickfirst,
        }
    }

    /// Whether the object under the cursor is highlighted (SELECTIONPREVIEW): the cursor picks
    /// objects ([`Self::picking_objects`]) and the bit for a command asking for them (2) or for
    /// no command (1) is on.
    pub fn previews_selection(&self) -> bool {
        let bit = if self.current_prompt().is_some() { 2 } else { 1 };
        self.picking_objects() && self.settings.selectionpreview & bit != 0
    }

    /// Whether selected objects show grips while `selected` objects are selected (GRIPS,
    /// GRIPOBJLIMIT).
    pub fn shows_grips(&self, selected: usize) -> bool {
        let limit = self.settings.gripobjlimit;
        self.settings.grips > 0 && (limit == 0 || selected <= limit as usize)
    }

    /// The text shown on the command line: the active prompt, or "Command:".
    pub fn prompt_text(&self) -> String {
        if let Some(p) = self.lisp_prompt() {
            return p.display();
        }
        match &self.running {
            Some(r) if self.suspended.is_some() => format!(">>{} {}", r.id.to_ascii_uppercase(), r.machine.prompt(self).display()),
            Some(r) => format!("{} {}", r.id.to_ascii_uppercase(), r.machine.prompt(self).display()),
            None => "Command:".into(),
        }
    }

    /// Preview entities for the cursor: objects drawn with their own properties
    /// ([`Interactive::preview_objects`]), then the rubber band with the current properties, in
    /// the grip blue over such objects (so it shows over their fills, whatever their colour).
    pub fn preview(&self, cursor: Vec2) -> Vec<Entity> {
        let mut common = self.current_common();
        let (kinds, mut out) = match (self.lisp_preview(cursor), &self.running) {
            (Some(k), _) => (k, Vec::new()),
            (None, Some(r)) => (r.machine.preview(self, cursor), r.machine.preview_objects(self, cursor)),
            (None, None) => (Vec::new(), Vec::new()),
        };
        if !out.is_empty() {
            common.color = cadcraft_doc::color::Color::True(cadcraft_doc::color::Rgb(0x2f, 0x6b, 0xff));
        }
        out.extend(kinds.into_iter().map(|k| Entity { handle: Handle(0), common: common.clone(), kind: k }));
        out
    }

    /// The active command's preview as a transform of existing objects (see
    /// [`Interactive::drag`]), with its extra rubber-band geometry as entities.
    pub fn drag(&self, cursor: Vec2) -> Option<(Drag, Vec<Entity>)> {
        let r = self.running.as_ref()?;
        let d = r.machine.drag(self, cursor)?;
        let common = self.current_common();
        let extra = d.extra.iter().map(|k| Entity { handle: Handle(0), common: common.clone(), kind: k.clone() }).collect();
        Some((d, extra))
    }
}
