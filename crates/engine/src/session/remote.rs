//! Applying changes from the collaboration server, and running an operation in the context the
//! client named.
//!
//! A collaboration server executes every operation itself and sends the other participants the
//! resulting change set (ADR 0001). Applying that change set is not a command: it has no undo
//! step of its own — the service owns per-user history (ADR 0010) — and it never allocates a
//! handle, because the server names the entity. The drawing's revision grows so the UI redraws.

use cadcraft_collab::{Change, OpContext};

use super::{Arc, Drawing, EngineError, Result, Session, Space, Value};

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
                // `apply` stops at the first refusal with the earlier changes already in the
                // drawing: put the drawing back, so the window is either the room's version or
                // the one it had, never half of each (CodeRabbit on #2936).
                st.doc = previous;
                return Err(EngineError::Other(format!("remote change refused: {error}")));
            }
        }
        // A layout the room renamed or deleted must not leave the window editing a space the
        // drawing no longer has: follow the room's rename, then its spelling, or fall back to
        // model space.
        if let Space::Paper(name) = st.space.clone() {
            let moved = changes.iter().rev().find_map(|change| match change {
                Change::LayoutRename { from, to, .. } if from.eq_ignore_ascii_case(&name) => Some(to.clone()),
                _ => None,
            });
            let name = moved.unwrap_or(name);
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

    /// Replace this window's drawing with the room's (TASK §6 п.5).
    ///
    /// A participant joins a room whose drawing is the server's: the window's own document (an
    /// empty untitled drawing, or the host's file) holds none of the room's objects, so every
    /// change set would be refused. The drawing replaces the active one whole and counts as saved
    /// — the room owns the file and the history (ADR 0009, ADR 0010). A window that had no
    /// drawing opens the room's one as a new document; any other keeps its view and its tab.
    pub fn adopt_shared_drawing(&mut self, drawing: Drawing) {
        if self.docs.is_empty() {
            self.open_drawing(drawing, "Shared session", None);
            return;
        }
        self.end_running();
        {
            let Some(st) = self.docs.get_mut(self.active) else { return };
            st.undo.clear();
            st.redo.clear();
            st.undo_ctl = Default::default();
            st.selection.clear();
            st.previous_selection.clear();
            st.space = Space::Model;
            st.mspace = None;
            st.saved = Arc::new(drawing.clone());
            st.doc = Arc::new(drawing);
            st.revision = st.revision.saturating_add(1);
        }
        self.bump_doc_epoch();
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

    /// `apply` stops at the first refusal with the earlier changes already in the drawing: the
    /// window must not keep the part it applied, or it holds a drawing the room never answered
    /// for (CodeRabbit on #2936).
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

    /// A layout the room renamed is a space this window can keep editing in only under the room's
    /// spelling; a deleted one leaves the window in model space instead of a space that is gone.
    #[test]
    fn a_remote_layout_change_never_leaves_the_window_on_a_missing_layout() {
        let mut s = Session::new();
        s.doc_mut().unwrap().layouts.push(Layout::new("Plan", 1));
        s.state_mut().unwrap().space = Space::Paper("Plan".into());

        let rename = vec![Change::LayoutRename { id: cadcraft_collab::RecordId(0), from: "Plan".into(), to: "Sheet".into() }];
        s.apply_remote(&rename).unwrap();
        assert_eq!(s.state().unwrap().space, Space::Paper("Sheet".into()), "the room's name for the layout");

        let delete = vec![Change::LayoutDelete { id: cadcraft_collab::RecordId(0), name: "Sheet".into() }];
        s.apply_remote(&delete).unwrap();
        assert_eq!(s.state().unwrap().space, Space::Model, "a deleted layout leaves model space");
    }

    /// A join replaces the window's own drawing with the room's: the window's empty document holds
    /// none of the room's objects, so every change set would be refused (CodeRabbit on #2936).
    #[test]
    fn a_join_replaces_the_window_drawing_with_the_rooms() {
        let mut s = Session::new();
        line(&mut s);
        let mut room = cadcraft_doc::Drawing::new_metric();
        let point = room.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(4.0, 4.0, 0.0), angle: 0.0 })).unwrap();
        let joined = room.clone();

        s.adopt_shared_drawing(room);

        let doc = s.doc().unwrap();
        assert_eq!(doc.entity_count(), 1, "the room's drawing, not this window's");
        assert!(doc.entity(point).is_some());
        assert_eq!(doc, &joined, "the window holds the room's drawing whole");
        assert!(s.undo().unwrap().is_none(), "the room owns the history (ADR 0010)");
        assert!(s.state().unwrap().selection.is_empty());
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
