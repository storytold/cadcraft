//! Applying changes from the collaboration server, and running an operation in the context the
//! client named.
//!
//! A collaboration server executes every operation itself and sends the other participants the
//! resulting change set (ADR 0001). Applying that change set is not a command: it has no undo
//! step of its own — the service owns per-user history (ADR 0010) — and it never allocates a
//! handle, because the server names the entity. The drawing's revision grows so the UI redraws.

use cadcraft_collab::{Change, OpContext};

use super::{Arc, EngineError, Result, Session, Value};

impl Session {
    /// Apply a change set from the authoritative server to the active drawing.
    ///
    /// No undo step is recorded: a remote change is not the local session's to undo. The revision
    /// grows, and a selection entry that names a removed entity is dropped.
    pub fn apply_remote(&mut self, changes: &[Change]) -> Result<()> {
        let st = self.state_mut()?;
        if st.block_edit.is_some() {
            return Err(EngineError::Other("a remote change cannot be applied while the Block Editor is open".into()));
        }
        {
            let doc = Arc::make_mut(&mut st.doc);
            cadcraft_collab::apply(doc, changes).map_err(|e| EngineError::Other(format!("remote change refused: {e}")))?;
        }
        let doc = &st.doc;
        st.selection.retain(|h| doc.entity(*h).is_some());
        st.revision = st.revision.saturating_add(1);
        Ok(())
    }

    /// Run a command with the space and selection the client named, instead of the session's.
    ///
    /// A server has no window to take a selection from; the operation carries its own context.
    pub fn execute_intent(&mut self, id: &str, params: &Value, context: &OpContext) -> Result<Value> {
        {
            let st = self.state_mut()?;
            st.space = context.space.clone();
            if let Some(selection) = &context.selection {
                st.selection = selection.clone();
            }
        }
        self.execute(id, params)
    }

    /// Whether an undoable command records an undo step. A long-lived server session turns this
    /// off: its history belongs to the collaboration service (ADR 0010).
    pub fn set_record_undo(&mut self, record: bool) {
        self.record_undo = record;
    }

    /// Whether undoable commands record undo steps in this session.
    pub fn records_undo(&self) -> bool {
        self.record_undo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_collab::diff;
    use cadcraft_doc::{Common, EntityKind, Layout, Point, Space};
    use cadcraft_geom::Vec3;
    use serde_json::json;

    fn line(s: &mut Session) {
        s.execute("line", &json!({"points": [[0.0, 0.0], [10.0, 0.0]]})).unwrap();
    }

    #[test]
    fn a_remote_change_needs_no_undo_step() {
        let mut s = Session::new();
        // A collaboration server records no local history of its own (ADR 0010).
        s.set_record_undo(false);
        line(&mut s);
        let before = s.doc().unwrap().clone();
        let mut after = before.clone();
        after.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(1.0, 1.0, 0.0), angle: 0.0 })).unwrap();
        let changes = diff(&before, &after, &Space::Model).unwrap();
        assert_eq!(changes.len(), 1);
        let revision = s.state().unwrap().revision;

        s.apply_remote(&changes).unwrap();

        assert_eq!(s.doc().unwrap().entity_count(), 2);
        assert_eq!(s.state().unwrap().revision, revision + 1);
        assert!(s.undo().unwrap().is_none(), "a remote change is not this session's to undo");
    }

    #[test]
    fn a_remote_removal_drops_it_from_the_selection() {
        let mut s = Session::new();
        line(&mut s);
        let before = s.doc().unwrap().clone();
        let removed = before.model.iter().next().unwrap().handle;
        s.set_selection(vec![removed]);
        let changes = vec![Change::EntityDelete { handle: removed }];

        s.apply_remote(&changes).unwrap();

        assert_eq!(s.doc().unwrap().entity_count(), 0);
        assert!(s.selection().is_empty());
    }

    #[test]
    fn an_intent_runs_in_the_space_it_names() {
        let mut s = Session::new();
        s.doc_mut().unwrap().layouts.push(Layout::new("Sheet1", 1));
        let context = OpContext::in_space(Space::Paper("Sheet1".into()));

        s.execute_intent("line", &json!({"points": [[0.0, 0.0], [1.0, 0.0]]}), &context).unwrap();

        let d = s.doc().unwrap();
        assert_eq!(d.model.len(), 0);
        let sheet = d.layouts.iter().find(|l| l.name == "Sheet1").unwrap();
        assert_eq!(sheet.entities.len(), 1);
    }

    #[test]
    fn a_server_session_does_not_stack_undo_steps() {
        let mut s = Session::new();
        s.set_record_undo(false);
        line(&mut s);
        assert_eq!(s.doc().unwrap().entity_count(), 1);
        assert!(s.undo().unwrap().is_none());
        assert!(!s.records_undo());
    }

    #[test]
    fn a_local_session_still_records_undo_steps() {
        let mut s = Session::new();
        line(&mut s);
        assert!(s.undo().unwrap().is_some());
        assert_eq!(s.doc().unwrap().entity_count(), 0);
    }
}
