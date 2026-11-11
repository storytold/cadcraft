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
    /// Show the drawing the room replaces the replica with (the first join, or a snapshot after a
    /// reconnect): the room is authoritative (ADR 0001), so the window shows the room's drawing
    /// rather than the file it opened from.
    ///
    /// The drawing keeps its name and path — the window still edits the cabinet's file — but not
    /// its undo history: the join is not a local step, and a shared session keeps its steps on the
    /// server (ADR 0010). The drawing counts as saved, because a room drawing is what a commit
    /// writes; local changes after the join make it dirty again.
    pub fn adopt_shared_drawing(&mut self, drawing: Drawing) -> Result<()> {
        let st = self.state_mut()?;
        if st.block_edit.is_some() {
            return Err(EngineError::Other("a shared drawing cannot be adopted while the Block Editor is open".into()));
        }
        // A layout the room does not have would leave the window on a space that is not there.
        if let Space::Paper(name) = &st.space
            && !drawing.layouts.iter().any(|layout| &layout.name == name)
        {
            st.space = Space::Model;
        }
        let doc = Arc::new(drawing);
        st.doc = doc.clone();
        st.saved = doc;
        st.undo.clear();
        st.redo.clear();
        st.selection.clear();
        st.previous_selection.clear();
        st.revision = st.revision.saturating_add(1);
        Ok(())
    }

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
    /// The author's layer is resolved the same way: the operation names it, and objects created
    /// while the command runs go there. An unknown layer is refused before anything runs, so the
    /// drawing does not change; the layer is not a session setting, so the server drawing's own
    /// `CLAYER` is left alone (ADR-0019).
    pub fn execute_intent(&mut self, id: &str, params: &Value, context: &OpContext) -> Result<Value> {
        let clay = match &context.clay {
            // The drawing's spelling of the name, so every replica holds the same bytes.
            Some(layer) => match self.doc()?.layer(layer).map(|found| found.name.clone()) {
                Some(name) => Some(name),
                None => {
                    let shown: String = layer.chars().take(64).collect();
                    return Err(EngineError::Other(format!("there is no layer `{shown}` in this drawing")));
                }
            },
            None => None,
        };
        {
            let st = self.state_mut()?;
            st.space = context.space.clone();
            if let Some(selection) = &context.selection {
                st.selection = selection.clone();
            }
            st.clay = clay;
        }
        let result = self.execute(id, params);
        // The layer belongs to the operation, not to the session: the next command must not
        // inherit it, and a drawing a commit writes keeps its own `CLAYER`.
        if let Ok(st) = self.state_mut() {
            st.clay = None;
        }
        result
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
    use cadcraft_doc::{Common, EntityKind, Layout, Line, Point, Space};
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

    /// A join shows the room's drawing, not the file the window opened: the room is
    /// authoritative (ADR 0001). The drawing keeps its name, loses this window's undo history
    /// (ADR 0010) and counts as saved until the next local edit.
    #[test]
    fn a_joined_session_adopts_the_room_drawing() {
        let mut s = Session::new();
        line(&mut s);
        s.state_mut().unwrap().title = "acceptance.dxf".into();
        s.state_mut().unwrap().path = Some("/srv/acceptance.dxf".into());
        assert!(s.state().unwrap().is_dirty(), "the local line is an unsaved change");

        let mut room = Drawing::new_metric();
        for x in [0.0, 30.0, 60.0] {
            room.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x, 10.0, 0.0) }))
                .unwrap();
        }
        s.adopt_shared_drawing(room).unwrap();

        let st = s.state().unwrap();
        assert_eq!(st.doc.entity_count(), 3, "the room's three lines, not the local one");
        assert_eq!(st.title, "acceptance.dxf", "the window still edits the cabinet's file");
        assert_eq!(st.path.as_deref(), Some("/srv/acceptance.dxf"));
        assert!(!st.is_dirty(), "a room drawing is what a commit writes");
        assert!(st.undo.is_empty() && st.redo.is_empty(), "the room owns the steps");
    }

    /// A join from a paper space the room does not have leaves the window in model space instead
    /// of on a layout that is gone.
    #[test]
    fn a_join_leaves_a_missing_layout_for_model_space() {
        let mut s = Session::new();
        s.state_mut().unwrap().space = Space::Paper("Sheet1".into());
        s.adopt_shared_drawing(Drawing::new_metric()).unwrap();
        assert_eq!(s.state().unwrap().space, Space::Model);
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
    fn an_intent_puts_new_objects_on_the_layer_it_names() {
        let mut s = Session::new();
        s.doc_mut().unwrap().layers.push(cadcraft_doc::Layer::new("WALLS"));
        let context = OpContext::in_space(Space::Model).on_layer("walls");

        s.execute_intent("line", &json!({"points": [[0.0, 0.0], [1.0, 0.0]]}), &context).unwrap();

        let d = s.doc().unwrap();
        let entity = d.model.iter().next().expect("the line");
        assert_eq!(entity.common.layer, "WALLS", "the drawing's spelling of the layer it named");
        assert_eq!(d.header.str("CLAYER", "?"), "0", "the server drawing's own current layer is left alone");
        assert_eq!(s.state().unwrap().clay, None, "the layer belongs to the operation, not the session");
    }

    #[test]
    fn an_unknown_layer_is_refused_before_the_drawing_changes() {
        let mut s = Session::new();
        let context = OpContext::in_space(Space::Model).on_layer("GHOST");
        let before = s.doc().unwrap().clone();
        let revision = s.state().unwrap().revision;

        let error = s.execute_intent("line", &json!({"points": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]}), &context).unwrap_err();

        assert!(error.to_string().contains("there is no layer `GHOST` in this drawing"), "{error}");
        assert_eq!(s.doc().unwrap(), &before, "no object was created, not even the first segment");
        assert_eq!(s.doc().unwrap().entity_count(), 0);
        assert_eq!(s.state().unwrap().revision, revision);
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
