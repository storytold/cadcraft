//! The transport-free part of a participant's session: the replica, the role and the events.
//!
//! [`Driver`] owns everything a session does after the socket handed it a message: it applies the
//! authoritative change sets to the replica and turns each message into an [`Event`]. The native
//! [`crate::Session`] drives it from a blocking socket; the browser [`crate::WebSession`] drives
//! the same type from the WebSocket's event queue once per frame. The two transports differ, the
//! protocol state machine does not.

use cadcraft_collab::message::Snapshot;
use cadcraft_collab::{Applied, CabinetCommit, ClientInfo, ClientMessage, Presence, Reject, Role, ServerMessage};
use cadcraft_doc::Drawing;

use crate::{Client, ClientError};

/// Why a session could not connect or continue.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("{0}")]
    Transport(String),
    #[error("the server refused the connection: {0}")]
    Refused(String),
    #[error("the welcome carries no drawing and this client has none")]
    MissingDrawing,
    #[error("the snapshot is not a drawing: {0}")]
    Snapshot(String),
    #[error("{0}")]
    Client(#[from] ClientError),
}

/// The state of one participant's connection, as a host's sync indicator shows it: the cabinet's
/// `session-state.status` words are `synced`, `pending`, `offline` and `error` (see
/// `craft-web-host`). A transport decides [`Status::Offline`] from its socket; the driver-side
/// state is [`Status::Connecting`], [`Status::InSync`] or [`Status::Pending`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Dialling, or waiting for the `welcome` that brings the replica.
    Connecting,
    /// The replica is at the server's version and nothing is waiting for an answer.
    InSync,
    /// The socket is open but an operation or a control message is unanswered.
    Pending,
    /// The socket closed; a reconnect is scheduled.
    Offline,
}

/// What arrived from the server.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// An operation was applied; the replica already shows it.
    Applied(Applied),
    /// The replica was replaced by the room's drawing: the first join, or a snapshot after a
    /// reconnect. The window must show it (the room is authoritative, ADR 0001), not the file it
    /// opened from.
    Joined { drawing: Box<Drawing>, version: u64 },
    /// An operation was refused.
    Rejected(Reject),
    /// Another participant's view.
    Presence(Presence),
    /// A commit answer. With `cabinet` the room published the file itself (M87): `bytes` is
    /// empty and the client must not write the drawing anywhere else.
    Committed { version: u64, not_saved: std::collections::BTreeMap<String, usize>, bytes: Vec<u8>, cabinet: Option<CabinetCommit> },
    /// The cabinet refused the commit with `409`: the file changed outside the session. The room
    /// keeps the file it built for [`Event::SaveCopy`]; the cabinet is not overwritten.
    CommitConflict { version: u64, message: String },
    /// The file of a refused commit, to save as a copy (TASK §6).
    SaveCopy { version: u64, not_saved: std::collections::BTreeMap<String, usize>, bytes: Vec<u8> },
    /// An attachment is in the room's store.
    AssetStored { sha256: String, bytes: u64, stored: bool },
    /// The bytes of an attachment the room stores.
    Asset { sha256: String, bytes: Vec<u8> },
    /// The heartbeat answer.
    Pong,
    /// The read timeout fired and nothing arrived: the connection is fine, the caller keeps
    /// draining its queue. Only the blocking transport reports it (a browser has no read).
    Idle,
    /// The server reports a problem outside an operation.
    Error { code: String, message: String, retryable: bool },
    /// A message this client does not act on (a delta it has already matched, say).
    Ignored,
}

/// The `hello` message both transports send first. The token travels in it and nowhere else.
pub(crate) fn hello(doc: &str, token: &str, last_version: Option<u64>, client: &str) -> ClientMessage {
    ClientMessage::Hello {
        app: "cadcraft".into(),
        doc: doc.into(),
        jwt: token.into(),
        client: ClientInfo { name: client.into(), ver: env!("CARGO_PKG_VERSION").into() },
        last_version,
    }
}

/// The drawing and version a `welcome` describes: its snapshot when it carries one, the replica
/// this client already has when it does not.
pub(crate) fn welcome_drawing(
    snapshot: &[Snapshot],
    replica: Option<Drawing>,
    last_version: Option<u64>,
    version: u64,
) -> Result<(Drawing, u64), SessionError> {
    if snapshot.is_empty() {
        let drawing = replica.ok_or(SessionError::MissingDrawing)?;
        // The welcome names the room's version; the replica is still behind it and the caller
        // resynchronises for the tail.
        return Ok((drawing, last_version.unwrap_or(version)));
    }
    let bytes = Snapshot::decode(snapshot).map_err(SessionError::Snapshot)?;
    let drawing = cadcraft_io::read(&bytes, "snapshot.dxf").map_err(|e| SessionError::Snapshot(e.to_string()))?;
    // The snapshot names the version it was taken at; the welcome's may be newer if operations
    // were applied while it was written, and the caller resynchronises for that tail.
    let version = snapshot.first().map(|part| part.version).unwrap_or(version);
    Ok((drawing, version))
}

/// The protocol state machine of one participant: replica, role, room and snapshot parts.
pub(crate) struct Driver {
    client: Client,
    role: Role,
    session_id: String,
    /// Who was already in the room when this participant joined, as `welcome` reported them.
    participants: Vec<Presence>,
    /// Snapshot parts that arrived after the welcome, joined when the last one is in.
    parts: Vec<Snapshot>,
}

impl Driver {
    /// A driver for a `welcome`'s drawing.
    pub(crate) fn new(client: Client, role: Role, session_id: String, participants: Vec<Presence>) -> Self {
        Driver { client, role, session_id, participants, parts: Vec::new() }
    }

    /// The replica.
    pub(crate) fn drawing(&self) -> &Drawing {
        self.client.drawing()
    }

    /// The version the replica shows.
    pub(crate) fn version(&self) -> u64 {
        self.client.version()
    }

    /// The participant's role.
    pub(crate) fn role(&self) -> Role {
        self.role
    }

    /// The room's id.
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The participants the welcome named (the others' latest presence).
    pub(crate) fn participants(&self) -> &[Presence] {
        &self.participants
    }

    /// The replica, for the operations a reconnect resends.
    pub(crate) fn client_mut(&mut self) -> &mut Client {
        &mut self.client
    }

    /// The replica, read-only.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    /// Apply one server message.
    ///
    /// `None` means the message was a snapshot part and more parts follow: nothing to report yet.
    pub(crate) fn on_message(&mut self, message: ServerMessage) -> Result<Option<Event>, SessionError> {
        Ok(Some(match message {
            ServerMessage::Applied { applied } => {
                let before = self.client.version();
                self.client.applied(&applied)?;
                // The room answers a resent operation again when a participant reconnects
                // (ADR 0003): the stored change set may name a version this replica already has,
                // and applying it to the window would replace newer objects with old ones and move
                // `collab_version` back. Only a change set that advanced the replica is a change
                // the window must see (CodeRabbit on #2938).
                if self.client.version() > before { Event::Applied(applied) } else { Event::Ignored }
            }
            ServerMessage::Presence { presence } => Event::Presence(presence),
            ServerMessage::Rejected { rejected } => {
                // The room will never apply it, and a resend after a reconnect would only be
                // refused again: it leaves the outbox, so the sync indicator settles (ADR 0003).
                self.client.discard(&rejected.op_id);
                Event::Rejected(rejected)
            }
            ServerMessage::CommitResult { version, not_saved, bytes_base64, cabinet } => {
                let bytes = cadcraft_collab::message::decode_bytes(&bytes_base64).map_err(SessionError::Snapshot)?;
                Event::Committed { version, not_saved, bytes, cabinet }
            }
            ServerMessage::Committed { version, not_saved, cabinet } => {
                Event::Committed { version, not_saved, bytes: Vec::new(), cabinet: Some(cabinet) }
            }
            ServerMessage::CommitConflict { version, message } => Event::CommitConflict { version, message },
            ServerMessage::SaveCopyResult { version, not_saved, bytes_base64 } => {
                let bytes = cadcraft_collab::message::decode_bytes(&bytes_base64).map_err(SessionError::Snapshot)?;
                Event::SaveCopy { version, not_saved, bytes }
            }
            ServerMessage::AssetStored { sha256, bytes, stored } => Event::AssetStored { sha256, bytes, stored },
            ServerMessage::Asset { sha256, bytes_base64 } => {
                let bytes = cadcraft_collab::message::decode_bytes(&bytes_base64).map_err(SessionError::Snapshot)?;
                Event::Asset { sha256, bytes }
            }
            ServerMessage::Pong => Event::Pong,
            ServerMessage::Error { code, message, retryable } => Event::Error { code, message, retryable },
            ServerMessage::Delta { applied, .. } => {
                // The tail a reconnect asks for: the entries the replica is missing are applied
                // here, and the window must see them too — an `Event::Ignored` would leave the
                // drawing and `collab_version` behind the replica, and the next change set would
                // land on a base without the tail (CodeRabbit on #2938). Entries at or below the
                // replica's version are the ones it already has, and are skipped by `applied`.
                let mut merged: Option<cadcraft_collab::Applied> = None;
                for entry in applied {
                    let before = self.client.version();
                    self.client.applied(&entry)?;
                    if self.client.version() == before {
                        continue;
                    }
                    match &mut merged {
                        Some(last) => {
                            last.op_id.clone_from(&entry.op_id);
                            last.version = entry.version;
                            last.actor.clone_from(&entry.actor);
                            last.changes.extend(entry.changes);
                        }
                        None => merged = Some(entry),
                    }
                }
                merged.map_or(Event::Ignored, Event::Applied)
            }
            ServerMessage::Snapshot { snapshot } => {
                self.parts.push(snapshot);
                if self.parts.last().is_some_and(|part| part.more) {
                    return Ok(None);
                }
                let parts = std::mem::take(&mut self.parts);
                let bytes = Snapshot::decode(&parts).map_err(SessionError::Snapshot)?;
                let drawing = cadcraft_io::read(&bytes, "snapshot.dxf").map_err(|e| SessionError::Snapshot(e.to_string()))?;
                let version = parts.first().map_or_else(|| self.version(), |part| part.version);
                let event = Event::Joined { drawing: Box::new(drawing.clone()), version };
                self.client.resync(drawing, version);
                event
            }
            ServerMessage::Welcome { session_id, role, participants, .. } => {
                self.session_id = session_id;
                self.role = role;
                self.participants = participants;
                Event::Ignored
            }
        }))
    }

    /// The version to ask a `resync` from after a reconnect, when the server's journal has to
    /// carry the replica forward. `None` when the welcome's snapshot already replaced it, or when
    /// the replica is at the room's version.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn resync_from(&self, room_version: u64) -> Option<u64> {
        (self.client.version() < room_version).then(|| self.client.version())
    }

    /// Take a `welcome` on an existing session (a reconnect): the role, the room and the
    /// participants are the server's, the replica is replaced only when the welcome carries a
    /// snapshot. Returns the version to `resync` from when the journal has to carry the replica
    /// forward.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn reconnect(
        &mut self,
        session_id: String,
        role: Role,
        participants: Vec<Presence>,
        room_version: u64,
        snapshot: &[Snapshot],
    ) -> Result<Option<u64>, SessionError> {
        self.session_id = session_id;
        self.role = role;
        self.participants = participants;
        if !snapshot.is_empty() {
            let bytes = Snapshot::decode(snapshot).map_err(SessionError::Snapshot)?;
            let drawing = cadcraft_io::read(&bytes, "snapshot.dxf").map_err(|e| SessionError::Snapshot(e.to_string()))?;
            let version = snapshot.first().map(|part| part.version).unwrap_or(room_version);
            self.client.resync(drawing, version);
        }
        Ok(self.resync_from(room_version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_collab::{Op, OpContext, OpKind, Reject, diff};
    use cadcraft_doc::geom::Vec3;
    use cadcraft_doc::{Common, EntityKind, Point, Space};
    use serde_json::json;

    fn a_point(x: f64) -> Drawing {
        let mut drawing = Drawing::new_metric();
        drawing.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(x, 0.0, 0.0), angle: 0.0 })).unwrap();
        drawing
    }

    fn a_driver(drawing: Drawing, version: u64) -> Driver {
        Driver::new(Client::new(drawing, version), Role::Editor, "room".into(), Vec::new())
    }

    /// A reconnect keeps the replica and asks for the journal tail: the welcome carries no
    /// snapshot, and `resync_from` names the version the replica shows.
    #[test]
    fn a_welcome_without_a_snapshot_keeps_the_replica_and_asks_for_the_tail() {
        let replica = a_point(1.0);
        let (drawing, version) = welcome_drawing(&[], Some(replica.clone()), Some(3), 5).unwrap();
        assert_eq!(version, 3, "the replica stays at the version it shows");
        let driver = a_driver(drawing, version);
        assert_eq!(driver.resync_from(5), Some(3));
        assert_eq!(driver.resync_from(3), None, "a replica at the room's version needs no tail");
    }

    /// A welcome with no snapshot and no replica cannot build one: that is the first connect of a
    /// client that lost its drawing, not a reconnect.
    #[test]
    fn a_welcome_without_a_snapshot_and_without_a_replica_is_missing_the_drawing() {
        assert!(matches!(welcome_drawing(&[], None, None, 5), Err(SessionError::MissingDrawing)));
    }

    /// The journal tail after a reconnect is applied through the same driver the first connect
    /// uses, and it reaches the window: a page that dropped the entries showed a drawing and a
    /// `collab_version` behind the room (CodeRabbit on #2938).
    #[test]
    fn a_journal_tail_brings_the_replica_to_the_room_version() {
        let before = a_point(1.0);
        let after = a_point(2.0);
        let mut driver = a_driver(before.clone(), 3);
        let applied = Applied { op_id: "a4".into(), version: 4, actor: "u1".into(), changes: diff(&before, &after, &Space::Model).unwrap() };

        let event = driver.on_message(ServerMessage::Delta { version: 4, more: false, applied: vec![applied] }).unwrap();

        assert!(matches!(event, Some(Event::Applied(ref change)) if change.version == 4), "the window applies the tail");
        assert_eq!(driver.version(), 4);
        assert_eq!(driver.drawing().space(&Space::Model), after.space(&Space::Model));
    }

    /// A commit answer is the same event on both transports; the room's own `committed` names the
    /// cabinet that stored the file.
    #[test]
    fn a_committed_message_carries_the_cabinets_version_and_etag() {
        let mut driver = a_driver(Drawing::new_metric(), 1);
        let cabinet = CabinetCommit { version: 7, etag: "\"e7\"".into() };
        let event = driver
            .on_message(ServerMessage::Committed { version: 1, not_saved: std::collections::BTreeMap::new(), cabinet: cabinet.clone() })
            .unwrap();
        assert_eq!(
            event,
            Some(Event::Committed { version: 1, not_saved: std::collections::BTreeMap::new(), bytes: Vec::new(), cabinet: Some(cabinet) })
        );
    }

    /// The operations a client sent before the socket dropped stay queued and are resent by the
    /// same driver, so a reconnect loses no edit (ADR 0003).
    #[test]
    fn the_outbox_survives_a_reconnect() {
        let mut driver = a_driver(Drawing::new_metric(), 3);
        let op = Op {
            op_id: "o1".into(),
            actor: "u1".into(),
            base_version: 3,
            kind: OpKind::Command,
            command: "line".into(),
            params: json!({ "points": [[0.0, 0.0], [1.0, 0.0]] }),
            context: OpContext::in_space(Space::Model),
        };
        driver.client_mut().submit(op);
        assert_eq!(driver.client().outbox().len(), 1);
    }

    /// A refused operation never comes back: keeping it would leave the sync indicator pending
    /// forever and resend it after every reconnect, for another refusal (CodeRabbit on #2938).
    #[test]
    fn a_rejected_operation_leaves_the_outbox() {
        let mut driver = a_driver(Drawing::new_metric(), 3);
        let op = Op {
            op_id: "o1".into(),
            actor: "u1".into(),
            base_version: 3,
            kind: OpKind::Command,
            command: "line".into(),
            params: json!({ "points": [[0.0, 0.0], [1.0, 0.0]] }),
            context: OpContext::in_space(Space::Model),
        };
        driver.client_mut().submit(op);
        let rejected = Reject { op_id: "o1".into(), code: "stale".into(), message: "another operation came first".into(), retryable: true };

        let event = driver.on_message(ServerMessage::Rejected { rejected }).unwrap();

        assert!(matches!(event, Some(Event::Rejected(_))), "the participant is still told why");
        assert!(driver.client().outbox().is_empty(), "a refused operation is not queued again");
    }

    /// The room answers a resent operation again after a reconnect: the stored change set names a
    /// version this replica already has. The window must not apply it over newer edits or move its
    /// `collab_version` back (CodeRabbit on #2938).
    #[test]
    fn a_stored_answer_is_not_a_change_the_window_sees() {
        let replica = a_point(1.0);
        let mut driver = a_driver(replica.clone(), 3);

        let stored = Applied { op_id: "o1".into(), version: 2, actor: "u1".into(), changes: Vec::new() };
        assert!(matches!(driver.on_message(ServerMessage::Applied { applied: stored }).unwrap(), Some(Event::Ignored)));
        assert_eq!(driver.client().version(), 3, "the replica stays at the version it shows");

        // An answer the replica has not reached is still a change the window applies.
        let mut with_another = replica.clone();
        with_another.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(2.0, 0.0, 0.0), angle: 0.0 })).unwrap();
        let changes = diff(&replica, &with_another, &Space::Model).unwrap();
        let applied = Applied { op_id: "o2".into(), version: 4, actor: "u2".into(), changes };
        assert!(matches!(driver.on_message(ServerMessage::Applied { applied }).unwrap(), Some(Event::Applied(_))));
        assert_eq!(driver.client().version(), 4);
    }

    /// The journal tail a reconnect asks for reaches the window: every entry the replica was
    /// missing becomes one change set, so the drawing the participant sees and the version the next
    /// operation is based on follow the room (CodeRabbit on #2938).
    #[test]
    fn a_resync_tail_reaches_the_window() {
        let first = a_point(1.0);
        let mut second = first.clone();
        second.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(2.0, 0.0, 0.0), angle: 0.0 })).unwrap();
        let mut third = second.clone();
        third.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(3.0, 0.0, 0.0), angle: 0.0 })).unwrap();
        let mut driver = a_driver(first.clone(), 3);

        // The room repeats the version the replica has, then carries it to 5.
        let delta = ServerMessage::Delta {
            version: 5,
            more: false,
            applied: vec![
                Applied { op_id: "o1".into(), version: 3, actor: "u1".into(), changes: Vec::new() },
                Applied { op_id: "o2".into(), version: 4, actor: "u2".into(), changes: diff(&first, &second, &Space::Model).unwrap() },
                Applied { op_id: "o3".into(), version: 5, actor: "u3".into(), changes: diff(&second, &third, &Space::Model).unwrap() },
            ],
        };
        let Some(Event::Applied(merged)) = driver.on_message(delta).unwrap() else {
            panic!("the tail is a change the window applies");
        };
        assert_eq!(merged.version, 5, "the window follows the room's version");
        assert_eq!(merged.changes.len(), 2, "both missing creations are in the one change set");
        assert_eq!(driver.client().version(), 5);
        assert_eq!(driver.client().drawing().space(&Space::Model), third.space(&Space::Model));
    }
}
