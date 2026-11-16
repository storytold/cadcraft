//! Applying changes from the collaboration server, and running an operation in the context the
//! client named.
//!
//! A collaboration server executes every operation itself and sends the other participants the
//! resulting change set (ADR 0001). Applying that change set is not a command: it has no undo
//! step of its own — the service owns per-user history (ADR 0010) — and it never allocates a
//! handle, because the server names the entity. The drawing's revision grows so the UI redraws.

use cadcraft_collab::{Change, OpContext};

use super::{Arc, EngineError, Result, Session, Space, Value};

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
            let previous = st.doc.clone();
            let doc = Arc::make_mut(&mut st.doc);
            if let Err(error) = cadcraft_collab::apply(doc, changes) {
                // `apply` stops at the first refusal and has already changed part of the
                // drawing: put the drawing back, so the window is either the room's version or
                // the one it had, never half of each.
                st.doc = previous;
                return Err(EngineError::Other(format!("remote change refused: {error}")));
            }
        }
        // A layout the room renamed or deleted must not leave the window on a space the drawing
        // no longer has: follow the room's spelling, or fall back to model space.
        if let Space::Paper(name) = st.space.clone() {
            st.space = match st.doc.layouts.iter().find(|layout| layout.name.eq_ignore_ascii_case(&name)) {
                Some(layout) => Space::Paper(layout.name.clone()),
                None => Space::Model,
            };
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
        self.state_mut()?.space = context.space.clone();
        // The selection comes from a client: `set_selection` drops a handle it names twice, so a
        // command cannot act on the same object twice.
        if let Some(selection) = &context.selection {
            self.set_selection(selection.clone());
        }
        self.execute(id, params)
    }

    /// Whether an undoable command records an undo step. A long-lived server session turns this
    /// off: its history belongs to the collaboration service (ADR 0010).
    ///
    /// Turning recording off drops the history each open drawing already has: a later undo would
    /// otherwise restore a snapshot taken before an edit made while recording was off, and discard
    /// that edit with it.
    pub fn set_record_undo(&mut self, record: bool) {
        self.record_undo = record;
        if !record {
            for st in &mut self.docs {
                st.undo.clear();
                st.redo.clear();
                st.undo_ctl.group = None;
                st.undo_ctl.marks.clear();
            }
        }
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

    /// `apply` stops at the first refusal: the window must not keep the part it already applied,
    /// or it holds a drawing the room never answered for.
    #[test]
    fn a_refused_remote_change_leaves_the_drawing_and_its_revision_alone() {
        let mut s = Session::new();
        line(&mut s);
        let before = s.doc().unwrap().clone();
        let present = before.model.iter().next().unwrap().handle;
        let revision = s.state().unwrap().revision;
        // The first change succeeds, the second names an object the drawing does not have.
        let changes = vec![Change::EntityDelete { handle: present }, Change::EntityDelete { handle: cadcraft_doc::Handle(9_999) }];

        let error = s.apply_remote(&changes).unwrap_err();

        assert!(error.to_string().contains("remote change refused"), "{error}");
        assert_eq!(s.doc().unwrap(), &before, "the first change must not stay applied");
        assert_eq!(s.state().unwrap().revision, revision, "the window has nothing new to draw");
    }

    /// A layout the room renamed or deleted is not a space this window can keep editing in: the
    /// session follows the room's spelling, or leaves paper space.
    #[test]
    fn a_remote_layout_change_never_leaves_the_window_on_a_missing_layout() {
        let mut s = Session::new();
        s.doc_mut().unwrap().layouts.push(Layout::new("Plan", 1));
        s.state_mut().unwrap().space = Space::Paper("Plan".into());

        let rename = vec![Change::LayoutRename { id: cadcraft_collab::RecordId(0), from: "Plan".into(), to: "PLAN".into() }];
        s.apply_remote(&rename).unwrap();
        assert_eq!(s.state().unwrap().space, Space::Paper("PLAN".into()), "the drawing's spelling of the layout");

        let delete = vec![Change::LayoutDelete { id: cadcraft_collab::RecordId(0), name: "PLAN".into() }];
        s.apply_remote(&delete).unwrap();
        assert_eq!(s.state().unwrap().space, Space::Model, "a deleted layout leaves model space");
    }

    /// The selection of an operation comes from a client: a handle it names twice is one object,
    /// so a command cannot move or copy the same object twice.
    #[test]
    fn an_intent_deduplicates_the_selection_it_is_given() {
        let mut s = Session::new();
        line(&mut s);
        let entity = s.doc().unwrap().model.iter().next().unwrap().handle;
        let mut context = OpContext::in_space(Space::Model);
        context.selection = Some(vec![entity, entity, entity]);

        s.execute_intent("line", &json!({"points": [[0.0, 5.0], [1.0, 5.0]]}), &context).unwrap();

        assert_eq!(s.selection(), vec![entity]);
    }

    /// A routine's single undo step belongs to the same rule as a command's: a session that
    /// records no history does not grow one, and still redraws.
    #[test]
    fn a_routine_records_no_undo_step_when_recording_is_off() {
        let mut s = Session::new();
        s.set_record_undo(false);
        let revision = s.state().unwrap().revision;

        s.cmdline("(command \"_.LINE\" '(0.0 0.0) '(10.0 0.0) \"\")").unwrap();

        assert_eq!(s.doc().unwrap().entity_count(), 1, "the routine drew its line");
        assert!(s.state().unwrap().revision > revision, "the window redraws");
        assert!(s.undo().unwrap().is_none(), "no local step for a session whose history is the room's");
    }

    /// Turning recording off drops the history already recorded: a later undo must not restore a
    /// snapshot taken before an edit made with recording off.
    #[test]
    fn turning_recording_off_drops_the_history_it_had() {
        let mut s = Session::new();
        line(&mut s);
        assert_eq!(s.doc().unwrap().entity_count(), 1);
        assert!(!s.state().unwrap().undo.is_empty());

        s.set_record_undo(false);

        assert!(s.state().unwrap().undo.is_empty(), "the recorded step goes with recording");
        assert!(s.undo().unwrap().is_none(), "nothing to undo");
        assert_eq!(s.doc().unwrap().entity_count(), 1, "the drawing itself is untouched");
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
