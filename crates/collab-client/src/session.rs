//! A participant's whole side of a session on the desktop: the socket, the replica and what arrives.
//!
//! [`Session`] is the piece the desktop app drives from a worker thread: it connects, decodes the
//! welcome's snapshot, sends operations and turns each server message into an [`Event`], applying
//! the change sets to its replica on the way. Presence and commit go through the same connection.
//! The browser has no blocking socket; [`crate::WebSession`] drives the same [`Driver`] from the
//! WebSocket's event queue instead.

use std::time::Duration;

use cadcraft_collab::{ClientMessage, Op, Presence, Role};
use cadcraft_doc::Drawing;

use crate::driver::{Driver, Event, SessionError, hello, welcome_drawing};
use crate::{Client, Connection};

/// A connected participant.
pub struct Session {
    connection: Connection,
    driver: Driver,
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
        let hello = hello(doc, token, last_version, "cadcraft");
        let (connection, first) = Connection::connect(url, &hello, timeout).map_err(SessionError::Transport)?;
        match first {
            cadcraft_collab::ServerMessage::Welcome { session_id, role, version, snapshot, participants, .. } => {
                let (drawing, shown) = welcome_drawing(&snapshot, replica, last_version, version)?;
                Ok(Session { connection, driver: Driver::new(Client::new(drawing, shown), role, session_id, participants) })
            }
            cadcraft_collab::ServerMessage::Error { message, .. } => Err(SessionError::Refused(message)),
            other => Err(SessionError::Refused(format!("expected a welcome, got {other:?}"))),
        }
    }

    /// The replica.
    pub fn drawing(&self) -> &Drawing {
        self.driver.drawing()
    }

    /// The version the replica shows.
    pub fn version(&self) -> u64 {
        self.driver.version()
    }

    /// The participant's role.
    pub fn role(&self) -> Role {
        self.driver.role()
    }

    /// The room's id.
    pub fn session_id(&self) -> &str {
        self.driver.session_id()
    }

    /// The participants the welcome named (the others' latest presence).
    pub fn participants(&self) -> &[Presence] {
        self.driver.participants()
    }

    /// Send an operation; the replica changes when its answer arrives.
    pub fn submit(&mut self, op: Op) -> Result<(), SessionError> {
        self.driver.client_mut().submit(op.clone());
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

    /// Ask for the file of a commit the cabinet refused with `409`, to save as a copy.
    pub fn save_copy(&mut self, op_id: &str) -> Result<(), SessionError> {
        self.connection.send(&ClientMessage::SaveCopy { op_id: op_id.into() }).map_err(SessionError::Transport)
    }

    /// Ask for the journal tail after `last_version`, or a snapshot when it is gone.
    pub fn resync(&mut self, last_version: u64) -> Result<(), SessionError> {
        self.connection.send(&ClientMessage::Resync { last_version }).map_err(SessionError::Transport)
    }

    /// Store an attachment's bytes under their content address (ADR 0021).
    ///
    /// The address must be [`cadcraft_collab::asset_id`] of `bytes`; the room checks it again and
    /// never reads a file by a name this client chose.
    pub fn put_asset(&mut self, sha256: &str, bytes: &[u8]) -> Result<(), SessionError> {
        let bytes_base64 = cadcraft_collab::message::encode_bytes(bytes);
        self.connection.send(&ClientMessage::AssetPut { sha256: sha256.into(), bytes_base64 }).map_err(SessionError::Transport)
    }

    /// Ask the room for an attachment's bytes by their content address.
    pub fn get_asset(&mut self, sha256: &str) -> Result<(), SessionError> {
        self.connection.send(&ClientMessage::AssetGet { sha256: sha256.into() }).map_err(SessionError::Transport)
    }

    /// Wait for one message and apply it to the replica.
    pub fn next_event(&mut self) -> Result<Event, SessionError> {
        loop {
            let message = self.connection.receive().map_err(SessionError::Transport)?;
            if let Some(event) = self.driver.on_message(message)? {
                return Ok(event);
            }
        }
    }

    /// Close the socket.
    pub fn close(&mut self) {
        self.connection.close();
    }
}
