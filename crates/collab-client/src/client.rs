//! One participant's replica and the operations waiting for an answer.

use cadcraft_collab::{Applied, Change, CollabError, Op, apply};
use cadcraft_doc::Drawing;

/// Why a client could not apply an answer.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("a change set arrived for version {got}, but this replica is at {expected}: resynchronise")]
    Gap { expected: u64, got: u64 },
    #[error("the change set does not fit this replica: {0}")]
    Change(#[from] CollabError),
}

/// A participant's replica of the authoritative drawing.
pub struct Client {
    drawing: Drawing,
    version: u64,
    pending: Vec<Op>,
}

impl Client {
    /// A replica of `drawing` at `version`, with nothing waiting.
    pub fn new(drawing: Drawing, version: u64) -> Self {
        Client { drawing, version, pending: Vec::new() }
    }

    /// The replica; equal to the server's drawing once every answer arrived.
    pub fn drawing(&self) -> &Drawing {
        &self.drawing
    }

    /// The version this replica shows.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Operations sent and not answered yet, in the order they were sent.
    pub fn outbox(&self) -> &[Op] {
        &self.pending
    }

    /// Queue an operation to send. It changes nothing locally until the server's answer arrives.
    pub fn submit(&mut self, op: Op) {
        self.pending.push(op);
    }

    /// Apply an answer from the server.
    ///
    /// An answer for a version already shown is ignored (a repeat after a reconnect); an answer
    /// that skips a version means the replica missed an operation, and the caller must
    /// [`resync`](Client::resync) instead of guessing. The answer for one of this client's
    /// operations also empties it from the outbox.
    pub fn applied(&mut self, applied: &Applied) -> Result<(), ClientError> {
        if applied.version <= self.version {
            return Ok(());
        }
        if applied.version != self.version.saturating_add(1) {
            return Err(ClientError::Gap { expected: self.version, got: applied.version });
        }
        apply(&mut self.drawing, &applied.changes)?;
        self.version = applied.version;
        self.pending.retain(|op| op.op_id != applied.op_id);
        Ok(())
    }

    /// Replace the replica with a snapshot from the server.
    ///
    /// The operations still waiting stay queued: sending them again is harmless, because the
    /// server answers a repeated `opId` from its window instead of applying it twice (ADR 0003).
    pub fn resync(&mut self, drawing: Drawing, version: u64) {
        self.drawing = drawing;
        self.version = version;
    }

    /// Apply a delta the server broadcast for another participant's operation.
    pub fn broadcast(&mut self, version: u64, changes: &[Change]) -> Result<(), ClientError> {
        let applied = Applied { op_id: String::new(), version, actor: String::new(), changes: changes.to_vec() };
        self.applied(&applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_collab::{OpContext, OpKind, diff};
    use cadcraft_doc::geom::Vec3;
    use cadcraft_doc::{Common, EntityKind, Point, Space};
    use serde_json::json;

    fn a_point(x: f64) -> Drawing {
        let mut drawing = Drawing::new_metric();
        drawing.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(x, 0.0, 0.0), angle: 0.0 })).unwrap();
        drawing
    }

    fn answer(before: &Drawing, after: &Drawing, op_id: &str, version: u64) -> Applied {
        Applied { op_id: op_id.into(), version, actor: "u1".into(), changes: diff(before, after, &Space::Model).unwrap() }
    }

    fn an_op(op_id: &str) -> Op {
        Op {
            op_id: op_id.into(),
            actor: "u1".into(),
            base_version: 0,
            kind: OpKind::Command,
            command: "line".into(),
            params: json!({ "points": [[0.0, 0.0], [1.0, 0.0]] }),
            context: OpContext::in_space(Space::Model),
        }
    }

    #[test]
    fn an_answer_brings_the_replica_to_the_server_state() {
        let before = Drawing::new_metric();
        let after = a_point(1.0);
        let mut client = Client::new(before.clone(), 0);

        client.applied(&answer(&before, &after, "a1", 1)).unwrap();

        assert_eq!(client.version(), 1);
        assert_eq!(client.drawing().space(&Space::Model), after.space(&Space::Model));
    }

    #[test]
    fn a_gap_asks_for_a_resync_instead_of_guessing() {
        let before = Drawing::new_metric();
        let after = a_point(1.0);
        let mut client = Client::new(before.clone(), 0);

        assert!(client.applied(&answer(&before, &after, "a1", 2)).is_err());
        assert_eq!(client.version(), 0);

        client.resync(after.clone(), 2);
        assert_eq!(client.version(), 2);
        assert_eq!(client.drawing().space(&Space::Model), after.space(&Space::Model));
    }

    #[test]
    fn a_repeated_answer_is_ignored() {
        let before = Drawing::new_metric();
        let after = a_point(1.0);
        let answer = answer(&before, &after, "a1", 1);
        let mut client = Client::new(before.clone(), 0);
        client.applied(&answer).unwrap();
        client.applied(&answer).unwrap();
        assert_eq!(client.version(), 1);
        assert_eq!(client.drawing().space(&Space::Model), after.space(&Space::Model));
    }

    #[test]
    fn an_answer_empties_only_its_own_operation() {
        let before = Drawing::new_metric();
        let after = a_point(1.0);
        let mut client = Client::new(before.clone(), 0);
        client.submit(an_op("a1"));
        client.submit(an_op("a2"));
        assert_eq!(client.outbox().len(), 2);

        client.applied(&answer(&before, &after, "a1", 1)).unwrap();

        assert_eq!(client.outbox().len(), 1);
        assert_eq!(client.outbox()[0].op_id, "a2");
    }

    #[test]
    fn a_broadcast_is_an_answer_too() {
        let before = Drawing::new_metric();
        let after = a_point(1.0);
        let changes = diff(&before, &after, &Space::Model).unwrap();
        let mut client = Client::new(before, 0);
        client.broadcast(1, &changes).unwrap();
        assert_eq!(client.version(), 1);
        assert_eq!(client.drawing().space(&Space::Model), after.space(&Space::Model));
    }
}
