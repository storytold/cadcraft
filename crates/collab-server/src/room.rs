//! One room: the authoritative drawing, the order of operations and the answers they get.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

use cadcraft_collab::{Applied, Op, OpKind, Outcome, Reject, Role, TableRegistry, TableRenames, diff_all};
use cadcraft_doc::Drawing;
use cadcraft_engine::Session;
use cadcraft_io::{SaveOptions, read, write_with};

use crate::policy::{changes_too_many, deny_reason, size_reason};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How many applied operations the room answers from memory before a repeat is answered as unknown.
const IDEMPOTENCY_WINDOW: usize = 1_000;
/// How many operations the journal keeps (compaction and snapshots come later).
const JOURNAL_LIMIT: usize = 10_000;

/// A room's state at one version, as a drawing file.
///
/// The snapshot is DXF (ADR-0005): the writer keeps the data CADCraft does not model (unknown
/// entities, other applications' objects and xdata) through its raw pass, and entity handles
/// survive the round trip, so a journal tail still names the same objects after a restore.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// The drawing's version at the moment it was written.
    pub version: u64,
    /// The DXF bytes.
    pub bytes: Vec<u8>,
}

/// The file format a commit writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommitFormat {
    /// DXF, the native path (ASCII, the drawing's version).
    Dxf,
    /// DWG through the bridge, which runs in a child process and needs a native build.
    Dwg,
}

impl CommitFormat {
    fn path(self) -> &'static str {
        match self {
            CommitFormat::Dxf => "commit.dxf",
            CommitFormat::Dwg => "commit.dwg",
        }
    }
}

/// The file a commit produced, with what the format could not keep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Committed {
    /// The drawing's version the file was written from.
    pub version: u64,
    /// The file bytes.
    pub bytes: Vec<u8>,
    /// What the format leaves out, by object type: the cabinet warns, keeps the previous version
    /// and writes this to the audit (ADR-0009).
    pub not_saved: BTreeMap<String, usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum RoomError {
    #[error("the room has no drawing")]
    NoDrawing,
    #[error("the drawing could not be saved or read back: {0}")]
    Io(String),
}

/// The authoritative drawing and the order of operations applied to it.
pub struct Room {
    session: Session,
    registry: TableRegistry,
    version: u64,
    applied: VecDeque<Applied>,
    journal: VecDeque<Applied>,
    journal_path: Option<PathBuf>,
}

impl Room {
    /// A room whose authoritative drawing is `drawing`.
    pub fn new(drawing: Drawing) -> Result<Self, RoomError> {
        let mut session = Session::empty();
        session.docs.push(cadcraft_engine::DocState::new(drawing, "Drawing1", None));
        session.active = 0;
        // The service owns history (ADR 0010): a per-operation snapshot would only grow memory.
        session.set_record_undo(false);
        let mut registry = TableRegistry::new();
        registry.reconcile(session.doc().map_err(|_| RoomError::NoDrawing)?);
        Ok(Room { session, registry, version: 0, applied: VecDeque::new(), journal: VecDeque::new(), journal_path: None })
    }

    /// The authoritative drawing, for a snapshot or a test replica.
    pub fn drawing(&self) -> Result<&Drawing, RoomError> {
        self.session.doc().map_err(|_| RoomError::NoDrawing)
    }

    /// The version of the drawing; the whole order is this counter.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Apply one operation and answer with its change set.
    ///
    /// A repeat of an `op_id` inside the window returns the first answer: the operation is never
    /// applied twice (ADR 0003). A structural operation with a stale `base_version` is rejected and
    /// the client waits for the room's order instead of retrying (ADR 0004).
    pub fn apply(&mut self, op: &Op) -> Result<Outcome, RoomError> {
        self.apply_as(Role::Editor, op)
    }

    /// Apply one operation for a participant with `role`; a viewer is refused before anything runs.
    pub fn apply_as(&mut self, role: Role, op: &Op) -> Result<Outcome, RoomError> {
        if !role.may_write() {
            return Ok(Outcome::Rejected(Reject {
                op_id: op.op_id.clone(),
                code: "forbidden".into(),
                message: "this participant may view the drawing, not change it".into(),
                retryable: false,
            }));
        }
        if let Some(reason) = deny_reason(&op.command) {
            return Ok(Outcome::Rejected(Reject { op_id: op.op_id.clone(), code: "forbidden".into(), message: reason.into(), retryable: false }));
        }
        if let Some(reason) = size_reason(op) {
            return Ok(Outcome::Rejected(Reject { op_id: op.op_id.clone(), code: "too-large".into(), message: reason.into(), retryable: false }));
        }
        if let Some(applied) = self.applied.iter().find(|a| a.op_id == op.op_id) {
            return Ok(Outcome::Applied(applied.clone()));
        }
        if op.kind == OpKind::Structural && op.base_version != self.version {
            return Ok(Outcome::Rejected(Reject {
                op_id: op.op_id.clone(),
                code: "stale".into(),
                message: format!("the drawing is at version {}; this structural operation waited for {}", self.version, op.base_version),
                retryable: false,
            }));
        }
        let before = self.drawing()?.clone();
        if let Err(error) = self.session.execute_intent(&op.command, &op.params, &op.context) {
            return Ok(Outcome::Rejected(Reject { op_id: op.op_id.clone(), code: "invalid".into(), message: error.to_string(), retryable: false }));
        }
        let after = self.drawing()?.clone();
        let renames = renames_of(&op.command, &op.params, &before, &after);
        let changes = match diff_all(&before, &after, &mut self.registry, &renames) {
            Ok(changes) => changes,
            Err(error) => {
                // The room and the command disagree about the drawing: give the drawing back and
                // tell the client to resynchronise.
                if let Ok(doc) = self.session.doc_mut() {
                    *doc = before;
                }
                return Ok(Outcome::Rejected(Reject {
                    op_id: op.op_id.clone(),
                    code: "invalid".into(),
                    message: error.to_string(),
                    retryable: false,
                }));
            }
        };
        if changes_too_many(changes.len()) {
            if let Ok(doc) = self.session.doc_mut() {
                *doc = before;
            }
            return Ok(Outcome::Rejected(Reject {
                op_id: op.op_id.clone(),
                code: "too-large".into(),
                message: format!("the operation changes {} objects; the limit is {}", changes.len(), crate::policy::MAX_CHANGES),
                retryable: false,
            }));
        }
        self.version = self.version.saturating_add(1);
        let applied = Applied { op_id: op.op_id.clone(), version: self.version, actor: op.actor.clone(), changes };
        self.remember(applied.clone());
        Ok(Outcome::Applied(applied))
    }

    /// Write the authoritative drawing as a DXF snapshot at the current version.
    pub fn snapshot(&self) -> Result<Snapshot, RoomError> {
        let bytes = write_with(self.drawing()?, "snapshot.dxf", &SaveOptions::default()).map_err(|e| RoomError::Io(e.to_string()))?;
        Ok(Snapshot { version: self.version, bytes })
    }

    /// A room restored from a snapshot; the journal tail is applied on top of it.
    pub fn restore(snapshot: &Snapshot) -> Result<Self, RoomError> {
        let drawing = read(&snapshot.bytes, "snapshot.dxf").map_err(|e| RoomError::Io(e.to_string()))?;
        let mut room = Room::new(drawing)?;
        room.version = snapshot.version;
        Ok(room)
    }

    /// Persist every applied operation to `path`, one JSON line each.
    ///
    /// The journal is what a restart replays on top of the last snapshot (ADR-0005). A write that
    /// fails is reported to the caller that set the path, not to a participant; the room keeps
    /// working, and the next compaction writes a fresh snapshot.
    pub fn with_journal(mut self, path: impl Into<PathBuf>) -> Result<Self, RoomError> {
        let path = path.into();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| RoomError::Io(e.to_string()))?;
        }
        std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| RoomError::Io(e.to_string()))?;
        self.journal_path = Some(path);
        Ok(self)
    }

    /// Restore a snapshot and replay the journal tail written by [`Room::with_journal`].
    pub fn restore_with_journal(snapshot: &Snapshot, journal: &Path) -> Result<Self, RoomError> {
        let mut room = Room::restore(snapshot)?;
        let text = std::fs::read_to_string(journal).unwrap_or_default();
        let mut entries: Vec<Applied> =
            text.lines().filter_map(|line| serde_json::from_str::<Applied>(line).ok()).filter(|a| a.version > snapshot.version).collect();
        entries.sort_by_key(|a| a.version);
        for applied in entries {
            if applied.version != room.version.saturating_add(1) {
                return Err(RoomError::Io(format!(
                    "the journal goes from version {} to {}, with {} missing",
                    room.version,
                    applied.version,
                    applied.version - room.version - 1
                )));
            }
            let doc = room.session.doc_mut().map_err(|_| RoomError::NoDrawing)?;
            cadcraft_collab::apply(doc, &applied.changes).map_err(|e| RoomError::Io(e.to_string()))?;
            room.version = applied.version;
            room.remember_memory_only(applied);
        }
        Ok(room)
    }

    /// Write a snapshot to `snapshot` (bytes and a `.version` file beside it) and empty the
    /// journal: everything it held is in the snapshot now.
    pub fn compact(&mut self, snapshot: &Path) -> Result<(), RoomError> {
        let saved = self.snapshot()?;
        let temporary = snapshot.with_extension("dxf.tmp");
        std::fs::write(&temporary, &saved.bytes).map_err(|e| RoomError::Io(e.to_string()))?;
        std::fs::rename(&temporary, snapshot).map_err(|e| RoomError::Io(e.to_string()))?;
        std::fs::write(snapshot.with_extension("version"), saved.version.to_string()).map_err(|e| RoomError::Io(e.to_string()))?;
        self.journal.clear();
        if let Some(path) = &self.journal_path {
            std::fs::write(path, b"").map_err(|e| RoomError::Io(e.to_string()))?;
        }
        Ok(())
    }

    /// Apply a journal tail (operations after the snapshot) in order, ignoring the answers.
    pub fn replay(&mut self, ops: &[Op]) -> Result<(), RoomError> {
        for op in ops {
            self.apply(op)?;
        }
        Ok(())
    }

    /// Write the drawing as the file the cabinet stores.
    ///
    /// The loss is counted **before** anything is written (`loss_on_save`) and returned with the
    /// bytes: DXF keeps what CADCraft does not model through its raw pass, DWG through the bridge
    /// drops custom-class entities and some foreign data (ADR-0009). The caller must keep the
    /// previous version and show the list.
    pub fn commit(&self, format: CommitFormat) -> Result<Committed, RoomError> {
        let path = format.path();
        let not_saved = cadcraft_engine::cmd::file::loss_on_save(&self.session, path);
        let bytes = write_with(self.drawing()?, path, &SaveOptions::default()).map_err(|e| RoomError::Io(e.to_string()))?;
        Ok(Committed { version: self.version, bytes, not_saved })
    }

    /// The journal of applied operations, oldest first.
    pub fn journal(&self) -> impl Iterator<Item = &Applied> {
        self.journal.iter()
    }

    fn remember(&mut self, applied: Applied) {
        if let Some(path) = &self.journal_path
            && let Ok(line) = serde_json::to_string(&applied)
            && let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path)
        {
            use std::io::Write as _;
            let _ = writeln!(file, "{line}");
        }
        self.remember_memory_only(applied);
    }

    /// Add an applied operation to the windows without writing it again (a replayed line is
    /// already in the file).
    fn remember_memory_only(&mut self, applied: Applied) {
        self.applied.push_back(applied.clone());
        while self.applied.len() > IDEMPOTENCY_WINDOW {
            self.applied.pop_front();
        }
        self.journal.push_back(applied);
        while self.journal.len() > JOURNAL_LIMIT {
            self.journal.pop_front();
        }
    }
}

/// The renames an operation performed, so a rename is one change instead of a delete and a create.
fn renames_of(command: &str, params: &Value, before: &Drawing, after: &Drawing) -> TableRenames {
    let mut renames = TableRenames::default();
    match command {
        "rename" => {
            let table = params.get("table").and_then(Value::as_str).unwrap_or("layer");
            if table.eq_ignore_ascii_case("layer")
                && let (Some(from), Some(to)) = (params.get("from").and_then(Value::as_str), params.get("to").and_then(Value::as_str))
            {
                renames.layers.push((from.into(), to.into()));
            }
        }
        "layout.rename" => {
            let to = params.get("to").and_then(Value::as_str);
            let from = params.get("from").and_then(Value::as_str);
            let to = match to {
                Some(to) => to,
                None => return renames,
            };
            match from {
                Some(from) => renames.layouts.push((from.into(), to.into())),
                None => {
                    // The command renamed the current layout; the layout that disappeared is it.
                    let gone: Vec<String> =
                        before.layouts.iter().map(|l| l.name.clone()).filter(|name| !after.layouts.iter().any(|l| l.name == *name)).collect();
                    if let [old] = gone.as_slice() {
                        renames.layouts.push((old.clone(), to.into()));
                    }
                }
            }
        }
        _ => {}
    }
    renames
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_collab::apply;
    use cadcraft_collab::{Change, OpContext};
    use cadcraft_doc::geom::Vec3;
    use cadcraft_doc::{Common, EntityKind, Layer, Line, Space};
    use serde_json::json;

    fn line_op(op_id: &str, x: f64) -> Op {
        Op {
            op_id: op_id.into(),
            actor: "u1".into(),
            base_version: 0,
            kind: OpKind::Command,
            command: "line".into(),
            params: json!({ "points": [[x, 0.0], [x + 1.0, 0.0]] }),
            context: OpContext::in_space(Space::Model),
        }
    }

    fn a_drawing_with_a_layer() -> Drawing {
        let mut drawing = Drawing::new_metric();
        drawing.layers.push(Layer::new("WALLS"));
        drawing
            .add(
                &Space::Model,
                Common { layer: "WALLS".into(), ..Default::default() },
                EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(1.0, 0.0, 0.0) }),
            )
            .unwrap();
        drawing
    }

    #[test]
    fn two_replicas_converge() {
        let mut room = Room::new(Drawing::new_metric()).unwrap();
        let mut replica = room.drawing().unwrap().clone();
        for op in [line_op("a", 0.0), line_op("b", 5.0)] {
            let Outcome::Applied(applied) = room.apply(&op).unwrap() else { panic!("an ordinary line must apply") };
            apply(&mut replica, &applied.changes).unwrap();
        }
        assert_eq!(room.version(), 2);
        assert_eq!(replica.space(&Space::Model), room.drawing().unwrap().space(&Space::Model));
    }

    #[test]
    fn an_operation_is_applied_once() {
        let mut room = Room::new(Drawing::new_metric()).unwrap();
        let op = line_op("once", 0.0);
        let Outcome::Applied(first) = room.apply(&op).unwrap() else { panic!() };
        let Outcome::Applied(again) = room.apply(&op).unwrap() else { panic!() };
        assert_eq!(first, again);
        assert_eq!(room.version(), 1);
        assert_eq!(room.drawing().unwrap().entity_count(), 1);
    }

    #[test]
    fn a_stale_structural_operation_is_rejected() {
        let mut room = Room::new(Drawing::new_metric()).unwrap();
        let Outcome::Applied(_) = room.apply(&line_op("first", 0.0)).unwrap() else { panic!() };
        let mut op = line_op("late", 1.0);
        op.kind = OpKind::Structural;
        op.base_version = 0;
        match room.apply(&op).unwrap() {
            Outcome::Rejected(reject) => {
                assert_eq!(reject.code, "stale");
                assert!(!reject.retryable);
            }
            Outcome::Applied(_) => panic!("a structural operation built on an old version must not apply"),
        }
        assert_eq!(room.version(), 1);
    }

    #[test]
    fn an_invalid_command_keeps_the_drawing_and_the_version() {
        let mut room = Room::new(Drawing::new_metric()).unwrap();
        let mut op = line_op("bad", 0.0);
        op.command = "no-such-command".into();
        match room.apply(&op).unwrap() {
            Outcome::Rejected(reject) => assert_eq!(reject.code, "invalid"),
            Outcome::Applied(_) => panic!("an unknown command cannot apply"),
        }
        assert_eq!(room.version(), 0);
        assert_eq!(room.drawing().unwrap().entity_count(), 0);
    }

    #[test]
    fn a_snapshot_restores_the_drawing_and_the_version() {
        let mut room = Room::new(a_drawing_with_a_layer()).unwrap();
        let Outcome::Applied(_) = room.apply(&line_op("a", 0.0)).unwrap() else { panic!() };
        let snapshot = room.snapshot().unwrap();
        assert_eq!(snapshot.version, 1);

        let restored = Room::restore(&snapshot).unwrap();
        assert_eq!(restored.version(), room.version());
        assert_eq!(restored.drawing().unwrap().space(&Space::Model), room.drawing().unwrap().space(&Space::Model));
        let names = |room: &Room| room.drawing().unwrap().layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&restored), names(&room));
    }

    #[test]
    fn a_journal_tail_replays_on_top_of_a_snapshot() {
        let mut room = Room::new(Drawing::new_metric()).unwrap();
        let Outcome::Applied(_) = room.apply(&line_op("a", 0.0)).unwrap() else { panic!() };
        let snapshot = room.snapshot().unwrap();
        let after = [line_op("b", 3.0), line_op("c", 6.0)];
        for op in &after {
            let Outcome::Applied(_) = room.apply(op).unwrap() else { panic!() };
        }

        let mut restored = Room::restore(&snapshot).unwrap();
        restored.replay(&after).unwrap();

        assert_eq!(restored.version(), room.version());
        assert_eq!(restored.drawing().unwrap().space(&Space::Model), room.drawing().unwrap().space(&Space::Model));
    }

    #[test]
    fn a_layer_rename_is_one_change() {
        let mut room = Room::new(a_drawing_with_a_layer()).unwrap();
        let mut replica = room.drawing().unwrap().clone();
        let op = Op {
            op_id: "rename-1".into(),
            actor: "u1".into(),
            base_version: 0,
            kind: OpKind::Structural,
            command: "rename".into(),
            params: json!({ "table": "layer", "from": "WALLS", "to": "WALLS-2" }),
            context: OpContext::in_space(Space::Model),
        };
        let Outcome::Applied(applied) = room.apply(&op).unwrap() else { panic!() };
        assert_eq!(applied.changes.len(), 1, "a rename is one change: {:?}", applied.changes);
        assert!(matches!(applied.changes[0], Change::TableRename { .. }));
        apply(&mut replica, &applied.changes).unwrap();
        assert_eq!(
            replica.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
            room.drawing().unwrap().layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>()
        );
        assert_eq!(replica.space(&Space::Model), room.drawing().unwrap().space(&Space::Model));
    }
}

#[cfg(test)]
mod commit_tests {
    use super::*;
    use cadcraft_collab::OpContext;
    use cadcraft_doc::Space;

    #[test]
    fn a_commit_writes_dxf_that_reopens_to_the_same_drawing() {
        let mut room = Room::new(cadcraft_doc::Drawing::new_metric()).unwrap();
        let op = Op {
            op_id: "a".into(),
            actor: "u".into(),
            base_version: 0,
            kind: OpKind::Command,
            command: "line".into(),
            params: serde_json::json!({ "points": [[0.0, 0.0], [5.0, 5.0]] }),
            context: OpContext::in_space(Space::Model),
        };
        let Outcome::Applied(_) = room.apply(&op).unwrap() else { panic!() };

        let committed = room.commit(CommitFormat::Dxf).unwrap();
        assert_eq!(committed.version, 1);
        assert!(committed.not_saved.is_empty(), "a synthetic DXF keeps everything");

        let reopened = cadcraft_io::read(&committed.bytes, "commit.dxf").unwrap();
        assert_eq!(reopened.space(&Space::Model), room.drawing().unwrap().space(&Space::Model));
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use cadcraft_collab::{OpContext, OpKind};
    use cadcraft_doc::Space;
    use serde_json::json;

    fn an_op(command: &str) -> Op {
        Op {
            op_id: "op".into(),
            actor: "u1".into(),
            base_version: 0,
            kind: OpKind::Command,
            command: command.into(),
            params: json!({ "points": [[0.0, 0.0], [1.0, 0.0]] }),
            context: OpContext::in_space(Space::Model),
        }
    }

    #[test]
    fn a_viewer_cannot_change_the_drawing() {
        let mut room = Room::new(Drawing::new_metric()).unwrap();
        match room.apply_as(Role::Viewer, &an_op("line")).unwrap() {
            Outcome::Rejected(reject) => assert_eq!(reject.code, "forbidden"),
            Outcome::Applied(_) => panic!("a viewer must not write"),
        }
        assert_eq!(room.version(), 0);
        assert_eq!(room.drawing().unwrap().entity_count(), 0);
    }

    #[test]
    fn a_denied_command_changes_nothing() {
        let mut room = Room::new(Drawing::new_metric()).unwrap();
        match room.apply(&an_op("open")).unwrap() {
            Outcome::Rejected(reject) => assert_eq!(reject.code, "forbidden"),
            Outcome::Applied(_) => panic!("`open` must not run in a room"),
        }
        assert_eq!(room.version(), 0);
        assert_eq!(room.drawing().unwrap().entity_count(), 0);
    }
}

#[cfg(test)]
mod journal_tests {
    use super::*;
    use cadcraft_collab::OpContext;
    use cadcraft_doc::Space;
    use serde_json::json;

    fn line_op(op_id: &str, x: f64) -> Op {
        Op {
            op_id: op_id.into(),
            actor: "u1".into(),
            base_version: 0,
            kind: OpKind::Command,
            command: "line".into(),
            params: json!({ "points": [[x, 0.0], [x + 1.0, 0.0]] }),
            context: OpContext::in_space(Space::Model),
        }
    }

    fn workspace() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cadcraft-journal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.subsec_nanos()).unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn a_journal_replays_on_top_of_a_snapshot() {
        let dir = workspace();
        let journal = dir.join("journal.jsonl");
        let state = dir.join("state.dxf");
        let mut room = Room::new(Drawing::new_metric()).unwrap().with_journal(&journal).unwrap();
        room.apply(&line_op("a", 0.0)).unwrap();
        room.apply(&line_op("b", 3.0)).unwrap();
        room.compact(&state).unwrap();
        room.apply(&line_op("c", 6.0)).unwrap();

        let bytes = std::fs::read(&state).unwrap();
        let version = std::fs::read_to_string(state.with_extension("version")).unwrap().trim().parse::<u64>().unwrap();
        let restored = Room::restore_with_journal(&Snapshot { version, bytes }, &journal).unwrap();

        assert_eq!(restored.version(), 3);
        assert_eq!(restored.drawing().unwrap().space(&Space::Model), room.drawing().unwrap().space(&Space::Model));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_journal_with_a_gap_is_refused() {
        let dir = workspace();
        let full = dir.join("full.jsonl");
        let tail = dir.join("tail.jsonl");
        let mut room = Room::new(Drawing::new_metric()).unwrap().with_journal(&full).unwrap();
        room.apply(&line_op("a", 0.0)).unwrap();
        room.apply(&line_op("b", 3.0)).unwrap();
        let text = std::fs::read_to_string(&full).unwrap();
        let last = text.lines().last().unwrap().to_string();
        std::fs::write(&tail, format!("{last}\n")).unwrap();

        let snapshot = Room::new(Drawing::new_metric()).unwrap().snapshot().unwrap();
        assert!(Room::restore_with_journal(&snapshot, &tail).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
