//! A participant's whole side of a session: the socket, the replica and what arrives.
//!
//! [`Session`] is the piece an application drives per frame: it connects, decodes the welcome's
//! snapshot, sends operations and turns each server message into an [`Event`], applying the change
//! sets to its replica on the way. Presence and commit go through the same connection.

use std::time::Duration;

use cadcraft_collab::message::Snapshot;
use cadcraft_collab::{Applied, ClientInfo, ClientMessage, Op, Presence, Reject, Role, ServerMessage};
use cadcraft_doc::Drawing;

use crate::{Client, ClientError, Connection};

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

/// What arrived from the server.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// An operation was applied; the replica already shows it.
    Applied(Applied),
    /// An operation was refused.
    Rejected(Reject),
    /// Another participant's view.
    Presence(Presence),
    /// A commit answer.
    Committed { version: u64, not_saved: std::collections::BTreeMap<String, usize>, bytes: Vec<u8> },
    /// The heartbeat answer.
    Pong,
    /// The server reports a problem outside an operation.
    Error { code: String, message: String, retryable: bool },
    /// A message this client does not act on (a delta it has already matched, say).
    Ignored,
}

/// A connected participant.
pub struct Session {
    connection: Connection,
    client: Client,
    role: Role,
    session_id: String,
    /// Who was already in the room when this participant joined, as `welcome` reported them.
    participants: Vec<Presence>,
    /// Snapshot parts that arrived after the welcome, joined when the last one is in.
    parts: Vec<Snapshot>,
}

impl Session {
    /// Connect, send `hello` and build the replica from the welcome's snapshot.
    pub fn connect(url: &str, token: &str, doc: &str, last_version: Option<u64>, timeout: Duration) -> Result<Self, SessionError> {
        Self::connect_inner(url, token, doc, last_version, None, timeout)
    }

    /// Reconnect with the replica this client already has.
    ///
    /// The server sends a snapshot when the journal cannot reach `version`, and nothing when it
    /// can; in the second case the replica is kept and the caller [`resync`](Session::resync)s for
    /// the tail.
    pub fn connect_from(url: &str, token: &str, doc: &str, replica: Drawing, version: u64, timeout: Duration) -> Result<Self, SessionError> {
        Self::connect_inner(url, token, doc, Some(version), Some(replica), timeout)
    }

    fn connect_inner(
        url: &str,
        token: &str,
        doc: &str,
        last_version: Option<u64>,
        replica: Option<Drawing>,
        timeout: Duration,
    ) -> Result<Self, SessionError> {
        let hello = ClientMessage::Hello {
            app: "cadcraft".into(),
            doc: doc.into(),
            jwt: token.into(),
            client: ClientInfo { name: "cadcraft".into(), ver: env!("CARGO_PKG_VERSION").into() },
            last_version,
        };
        let (connection, first) = Connection::connect(url, &hello, timeout).map_err(SessionError::Transport)?;
        match first {
            ServerMessage::Welcome { session_id, role, version, snapshot, participants, .. } => {
                let (drawing, shown) = if snapshot.is_empty() {
                    let drawing = replica.ok_or(SessionError::MissingDrawing)?;
                    // The welcome names the room's version; the replica is still behind it and the
                    // caller resynchronises for the tail.
                    (drawing, last_version.unwrap_or(version))
                } else {
                    let bytes = Snapshot::decode(&snapshot).map_err(SessionError::Snapshot)?;
                    let drawing = cadcraft_io::read(&bytes, "snapshot.dxf").map_err(|e| SessionError::Snapshot(e.to_string()))?;
                    (drawing, version)
                };
                Ok(Session { connection, client: Client::new(drawing, shown), role, session_id, participants, parts: Vec::new() })
            }
            ServerMessage::Error { message, .. } => Err(SessionError::Refused(message)),
            other => Err(SessionError::Refused(format!("expected a welcome, got {other:?}"))),
        }
    }

    /// The replica.
    pub fn drawing(&self) -> &Drawing {
        self.client.drawing()
    }

    /// The version the replica shows.
    pub fn version(&self) -> u64 {
        self.client.version()
    }

    /// The participant's role.
    pub fn role(&self) -> Role {
        self.role
    }

    /// The room's id.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The participants the welcome named (the others' latest presence).
    pub fn participants(&self) -> &[Presence] {
        &self.participants
    }

    /// Send an operation; the replica changes when its answer arrives.
    pub fn submit(&mut self, op: Op) -> Result<(), SessionError> {
        self.client.submit(op.clone());
        self.connection.send(&ClientMessage::Op { op }).map_err(SessionError::Transport)
    }

    /// Ask to undo this participant's last operation.
    pub fn undo(&mut self, op_id: &str) -> Result<(), SessionError> {
        self.connection.send(&ClientMessage::Undo { op_id: op_id.into() }).map_err(SessionError::Transport)
    }

    /// Ask to redo this participant's last undone operation.
    pub fn redo(&mut self, op_id: &str) -> Result<(), SessionError> {
        self.connection.send(&ClientMessage::Redo { op_id: op_id.into() }).map_err(SessionError::Transport)
    }

    /// Tell the others where this participant is looking.
    pub fn presence(&mut self, presence: Presence) -> Result<(), SessionError> {
        self.connection.send(&ClientMessage::Presence { presence }).map_err(SessionError::Transport)
    }

    /// Ask for the file to be written.
    pub fn commit(&mut self, expected_version: u64) -> Result<(), SessionError> {
        self.connection.send(&ClientMessage::Commit { expected_version }).map_err(SessionError::Transport)
    }

    /// Ask for the journal tail after `last_version`, or a snapshot when it is gone.
    pub fn resync(&mut self, last_version: u64) -> Result<(), SessionError> {
        self.connection.send(&ClientMessage::Resync { last_version }).map_err(SessionError::Transport)
    }

    /// Wait for one message and apply it to the replica.
    pub fn next_event(&mut self) -> Result<Event, SessionError> {
        loop {
            let message = self.connection.receive().map_err(SessionError::Transport)?;
            match message {
                ServerMessage::Applied { applied } => {
                    self.client.applied(&applied)?;
                    return Ok(Event::Applied(applied));
                }
                ServerMessage::Presence { presence } => return Ok(Event::Presence(presence)),
                ServerMessage::Rejected { rejected } => return Ok(Event::Rejected(rejected)),
                ServerMessage::CommitResult { version, not_saved, bytes_base64 } => {
                    let bytes = cadcraft_collab::message::decode_bytes(&bytes_base64).map_err(SessionError::Snapshot)?;
                    return Ok(Event::Committed { version, not_saved, bytes });
                }
                ServerMessage::Pong => return Ok(Event::Pong),
                ServerMessage::Error { code, message, retryable } => return Ok(Event::Error { code, message, retryable }),
                ServerMessage::Delta { applied, .. } => {
                    for entry in applied {
                        self.client.applied(&entry)?;
                    }
                    return Ok(Event::Ignored);
                }
                ServerMessage::Snapshot { snapshot } => {
                    self.parts.push(snapshot);
                    if self.parts.last().is_some_and(|part| part.more) {
                        continue;
                    }
                    let parts = std::mem::take(&mut self.parts);
                    let bytes = Snapshot::decode(&parts).map_err(SessionError::Snapshot)?;
                    let drawing = cadcraft_io::read(&bytes, "snapshot.dxf").map_err(|e| SessionError::Snapshot(e.to_string()))?;
                    let version = parts.first().map(|part| part.version).unwrap_or(self.client.version());
                    self.client.resync(drawing, version);
                    return Ok(Event::Ignored);
                }
                ServerMessage::Welcome { .. } => return Ok(Event::Ignored),
            }
        }
    }

    /// Close the socket.
    pub fn close(&mut self) {
        self.connection.close();
    }
}
