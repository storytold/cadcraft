//! One room: the authoritative drawing, the order of operations and the answers they get.

use std::collections::VecDeque;

use cadcraft_collab::{Change, OpContext, TableRegistry, TableRenames, diff_all};
use cadcraft_doc::Drawing;
use cadcraft_engine::Session;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How many applied operations the room answers from memory before a repeat is answered as unknown.
const IDEMPOTENCY_WINDOW: usize = 1_000;
/// How many operations the journal keeps (compaction and snapshots come later).
const JOURNAL_LIMIT: usize = 10_000;

/// What an operation is, for the ordering rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpKind {
    /// A command from the allowlist, with explicit addresses.
    Command,
    /// A normalized operation that has no command.
    Primitive,
    /// An operation whose result depends on other objects (a cascade): exclusive, in order.
    Structural,
}

/// An operation as a client sends it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Op {
    /// The client's identifier; a repeat is answered from the window, never applied twice.
    pub op_id: String,
    /// Who sent it (the JWT `sub`).
    pub actor: String,
    /// The version the client had when it built the operation.
    pub base_version: u64,
    /// Command, primitive or structural.
    pub kind: OpKind,
    /// The command id (`"line"`, `"rename"`, `"layout.rename"`, …).
    pub command: String,
    /// The command's JSON parameters.
    pub params: Value,
    /// The space and selection the command acts on.
    pub context: OpContext,
}

/// The authoritative result of one operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// The operation it answers.
    pub op_id: String,
    /// The drawing's version after it.
    pub version: u64,
    /// Who applied it.
    pub actor: String,
    /// What the other participants must apply to reach this version.
    pub changes: Vec<Change>,
}

/// Why an operation was not applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reject {
    /// The operation it answers.
    pub op_id: String,
    /// A stable code: `stale`, `invalid`, `forbidden`.
    pub code: String,
    /// What the client should say.
    pub message: String,
    /// Whether sending it again (after resynchronising) can work.
    pub retryable: bool,
}

/// What happened to an operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Applied; the change set is the authoritative result.
    Applied(Applied),
    /// Not applied; the drawing is unchanged.
    Rejected(Reject),
}

/// Why the room could not answer at all (as opposed to rejecting an operation).
#[derive(Debug, thiserror::Error)]
pub enum RoomError {
    #[error("the room has no drawing")]
    NoDrawing,
}

/// The authoritative drawing and the order of operations applied to it.
pub struct Room {
    session: Session,
    registry: TableRegistry,
    version: u64,
    applied: VecDeque<Applied>,
    journal: VecDeque<Applied>,
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
        Ok(Room { session, registry, version: 0, applied: VecDeque::new(), journal: VecDeque::new() })
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
        self.version = self.version.saturating_add(1);
        let applied = Applied { op_id: op.op_id.clone(), version: self.version, actor: op.actor.clone(), changes };
        self.remember(applied.clone());
        Ok(Outcome::Applied(applied))
    }

    /// The journal of applied operations, oldest first.
    pub fn journal(&self) -> impl Iterator<Item = &Applied> {
        self.journal.iter()
    }

    fn remember(&mut self, applied: Applied) {
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
