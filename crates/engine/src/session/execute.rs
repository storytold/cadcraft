use super::{Arc, EngineError, Result, Session, Snapshot, Value, cmd, find_command};

impl Session {
    // ---------------- programmatic execution ----------------

    /// Run a command with JSON parameters (no dialogs). Records undo when the drawing changes.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        (spec.enabled)(self).map_err(|m| EngineError::Disabled(spec.id.into(), m))?;
        if let Some(why) = cmd::blocks::bedit::refused_in_editor(self, spec.id) {
            return Err(EngineError::Disabled(spec.id.into(), why));
        }
        let before = self.state().ok().map(|s| (s.doc.clone(), s.selection.clone(), s.uid));
        let editing = self.state().is_ok_and(|s| s.block_edit.is_some());
        let run = spec.run;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self, params)));
        let result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(ToString::to_string)
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panic".into());
                // Restore the drawing as it was.
                if let Some((doc, sel, uid)) = &before
                    && let Some(st) = self.docs.iter_mut().find(|d| d.uid == *uid)
                {
                    st.doc = doc.clone();
                    st.selection = sel.clone();
                }
                return Err(EngineError::Internal(id.into(), msg));
            }
        };
        let violation = before.as_ref().and_then(|b| cmd::blocks::refedit::after_command(self, &b.0, spec.id));
        let result = match (violation, result) {
            (Some(m), Ok(_)) => Err(EngineError::Other(m)),
            (_, r) => r,
        };
        // Opening or closing the Block Editor swaps the whole drawing: nothing in it changed.
        if self.state().is_ok_and(|s| s.block_edit.is_some()) == editing {
            self.after_command(before.as_ref().map(|b| &b.0), before.as_ref().map(|b| b.2), spec.undoable && result.is_ok());
        }
        let result = match (result, &before) {
            (Ok(v), Some((doc, sel, uid))) if spec.undoable => self.refuse_non_finite(Some(*uid), doc, sel, spec.id).map(|()| v),
            (r, _) => r,
        };
        let grouped = self.lisp_groups_undo();
        if spec.undoable
            && let Some((doc, sel, uid)) = before
            && let Some(st) = self.docs.iter_mut().find(|d| d.uid == uid)
            && !Arc::ptr_eq(&doc, &st.doc)
        {
            // Inside a LISP routine the routine's single undo step records it.
            if !grouped {
                st.push_undo(Snapshot { label: spec.label.to_string(), doc, selection: sel });
            }
            st.revision += 1;
        }
        result
    }

    /// Bump the revision without an undo step (view-only or settings changes that redraw).
    pub fn touch(&mut self) {
        if let Ok(st) = self.state_mut() {
            st.revision += 1;
        }
    }

    // ---------------- undo ----------------

    pub fn undo(&mut self) -> Result<Option<String>> {
        let st = self.state_mut()?;
        let Some(snap) = st.undo.pop() else { return Ok(None) };
        let cur = Snapshot { label: snap.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() };
        st.doc = snap.doc;
        st.follow_layout(&cur.doc);
        st.selection = Vec::new();
        st.redo.push(cur);
        st.revision += 1;
        Ok(Some(snap.label))
    }

    pub fn redo(&mut self) -> Result<Option<String>> {
        let st = self.state_mut()?;
        let Some(snap) = st.redo.pop() else { return Ok(None) };
        let cur = Snapshot { label: snap.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() };
        st.doc = snap.doc;
        st.follow_layout(&cur.doc);
        st.selection = Vec::new();
        st.undo.push(cur);
        st.revision += 1;
        Ok(Some(snap.label))
    }
}
