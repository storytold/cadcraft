//! The WebSocket transport: a room, its participants and the messages between them.
//!
//! One thread accepts connections; each connection has one thread that reads with a short timeout
//! and drains a send queue, so a broadcast and an incoming operation interleave on the same socket.
//! A slow writer never blocks the room: its messages wait in the queue.
//!
//! TLS is terminated by the deployment's proxy: this listener speaks plain WebSocket on loopback or
//! a private network. The token is verified in the first message and never appears in a URL.

use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use cadcraft_collab::message::{Snapshot, encode_bytes};
use cadcraft_collab::{CabinetCommit, ClientMessage, Outcome, Reject, ServerMessage};
use tungstenite::{Message, WebSocket, accept};

use crate::assets::{AssetError, AssetStore};
use crate::auth::{Authenticator, Claims};
use crate::cabinet::PublishError;
use crate::{CommitFormat, PresenceBoard, Room};

/// How long a socket waits for a message before the loop drains its send queue.
const READ_POLL: Duration = Duration::from_millis(50);
/// How long a new connection has to send its `hello`.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

type Peers = Arc<Mutex<Vec<(String, Sender<ServerMessage>)>>>;

/// What a room has done, for a health check and a metrics scrape.
#[derive(Debug, Default)]
pub struct Metrics {
    /// Connections accepted.
    pub connections: AtomicU64,
    /// Operations applied.
    pub applied: AtomicU64,
    /// Operations rejected.
    pub rejected: AtomicU64,
    /// Messages that could not be handled.
    pub errors: AtomicU64,
    /// Commits the cabinet stored (M87).
    pub commits: AtomicU64,
    /// Commits the cabinet refused with `409` (the file changed elsewhere).
    pub conflicts: AtomicU64,
}

impl Metrics {
    /// The counters and the room's state, as Prometheus text.
    ///
    /// `participants` is the presence board's size (only a client that sends `Presence` has an
    /// entry there); `connections_open` is the number of open sockets, which is what the idle
    /// shutdown must look at.
    pub fn render(&self, version: u64, journal: usize, participants: usize, connections_open: usize) -> String {
        format!(
            "# TYPE cadcraft_collab_connections_total counter\ncadcraft_collab_connections_total {}\n\
             # TYPE cadcraft_collab_operations_total counter\ncadcraft_collab_operations_total{{result=\"applied\"}} {}\n\
             cadcraft_collab_operations_total{{result=\"rejected\"}} {}\n\
             # TYPE cadcraft_collab_errors_total counter\ncadcraft_collab_errors_total {}\n\
             # TYPE cadcraft_collab_commits_total counter\ncadcraft_collab_commits_total {}\n\
             # TYPE cadcraft_collab_commit_conflicts_total counter\ncadcraft_collab_commit_conflicts_total {}\n\
             # TYPE cadcraft_collab_version gauge\ncadcraft_collab_version {}\n\
             # TYPE cadcraft_collab_journal_length gauge\ncadcraft_collab_journal_length {}\n\
             # TYPE cadcraft_collab_participants gauge\ncadcraft_collab_participants {}\n\
             # TYPE cadcraft_collab_connections_open gauge\ncadcraft_collab_connections_open {}\n",
            self.connections.load(Ordering::Relaxed),
            self.applied.load(Ordering::Relaxed),
            self.rejected.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
            self.commits.load(Ordering::Relaxed),
            self.conflicts.load(Ordering::Relaxed),
            version,
            journal,
            participants,
            connections_open,
        )
    }
}

/// A room served over WebSocket.
pub struct Server {
    room: Arc<Mutex<Room>>,
    presence: Arc<Mutex<PresenceBoard>>,
    peers: Peers,
    auth: Arc<dyn Authenticator>,
    app: String,
    session: String,
    /// The drawing this room serves, when the deployment names one (`--file-id`): a token's `doc`
    /// must be this id, so a token for another drawing cannot join.
    doc: Option<String>,
    metrics: Arc<Metrics>,
    /// Attachments of this room; `None` when the deployment did not give `--assets` (ADR 0021).
    assets: Option<Arc<AssetStore>>,
    /// One commit at a time: each publish must see the etag the previous one stored.
    commit: Mutex<()>,
}

impl Server {
    /// A server for `room`, authenticating connections with `auth`.
    pub fn new(room: Room, auth: Arc<dyn Authenticator>, session_id: impl Into<String>, app: impl Into<String>) -> Self {
        Server {
            room: Arc::new(Mutex::new(room)),
            presence: Arc::new(Mutex::new(PresenceBoard::new())),
            peers: Arc::new(Mutex::new(Vec::new())),
            auth,
            app: app.into(),
            session: session_id.into(),
            doc: None,
            metrics: Arc::new(Metrics::default()),
            assets: None,
            commit: Mutex::new(()),
        }
    }

    /// Name the drawing this room serves (`--file-id`): a token whose `doc` is another drawing is
    /// refused at the first message, so the per-drawing ACL the claim carries is enforced here and
    /// not only by the client.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// Serve attachments from `assets` as well; the room must be given the same store
    /// ([`Room::with_assets`]) so an operation and the store agree on the room (ADR 0021).
    pub fn with_assets(mut self, assets: Arc<AssetStore>) -> Self {
        self.assets = Some(assets);
        self
    }

    /// The counters of this room.
    pub fn metrics(&self) -> Arc<Metrics> {
        Arc::clone(&self.metrics)
    }

    /// Bind `addr` and serve in background threads; returns the bound address.
    pub fn bind(self: &Arc<Self>, addr: impl ToSocketAddrs) -> io::Result<(SocketAddr, thread::JoinHandle<()>)> {
        let listener = TcpListener::bind(addr)?;
        let local = listener.local_addr()?;
        let server = Arc::clone(self);
        let handle = thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let server = Arc::clone(&server);
                thread::spawn(move || {
                    server.metrics.connections.fetch_add(1, Ordering::Relaxed);
                    let _ = server.connection(stream);
                });
            }
        });
        Ok((local, handle))
    }

    /// The authoritative room, for a test or a snapshot job.
    pub fn room(&self) -> MutexGuard<'_, Room> {
        lock(&self.room)
    }

    /// The drawing's version.
    pub fn version(&self) -> u64 {
        lock(&self.room).version()
    }

    /// How many operations the journal keeps.
    pub fn journal_len(&self) -> usize {
        lock(&self.room).journal().count()
    }

    /// How many participants are present.
    pub fn participants(&self) -> usize {
        lock(&self.presence).len()
    }

    /// How many sockets are open, including clients that have not sent presence yet.
    pub fn connections_open(&self) -> usize {
        lock(&self.peers).len()
    }

    /// Whether `claims` may enter the room this server serves, with the `hello`'s `doc` as
    /// `requested`.
    ///
    /// The token's `doc` is the cabinet's file id: it must name the drawing the client asked for,
    /// and the drawing this room serves when the deployment named one. An empty `doc` is the
    /// `--dev-token` case, which only a deployment that names no drawing accepts.
    fn accepts_doc(&self, claims: &Claims, requested: &str) -> bool {
        if claims.doc.is_empty() {
            return self.doc.is_none();
        }
        claims.doc == requested && self.doc.as_deref().is_none_or(|served| served == claims.doc.as_str())
    }

    fn connection(&self, stream: TcpStream) -> io::Result<()> {
        stream.set_read_timeout(Some(HELLO_TIMEOUT))?;
        let mut socket = accept(stream).map_err(io::Error::other)?;
        let (app, doc, jwt, last_version) = match read_client(&mut socket)? {
            Some(ClientMessage::Hello { app, doc, jwt, last_version, .. }) => (app, doc, jwt, last_version),
            _ => return Ok(()),
        };
        if app != self.app {
            send(&mut socket, &ServerMessage::Error { code: "unsupported".into(), message: "another application's room".into(), retryable: false })?;
            return Ok(());
        }
        let claims = match self.auth.verify(&jwt) {
            Ok(claims) => claims,
            Err(reason) => {
                send(&mut socket, &ServerMessage::Error { code: "auth".into(), message: reason, retryable: false })?;
                return Ok(());
            }
        };
        if !self.accepts_doc(&claims, &doc) {
            send(&mut socket, &ServerMessage::Error { code: "auth".into(), message: "the token is for another drawing".into(), retryable: false })?;
            return Ok(());
        }
        let (tx, rx) = channel();
        lock(&self.peers).push((claims.sub.clone(), tx.clone()));

        let participants = lock(&self.presence).participants().cloned().collect();
        let snapshot = self.snapshot_for(last_version);
        let version = lock(&self.room).version();
        send(
            &mut socket,
            &ServerMessage::Welcome {
                session_id: self.session.clone(),
                actor_id: claims.sub.clone(),
                role: claims.role,
                version,
                participants,
                snapshot,
            },
        )?;

        socket.get_mut().set_read_timeout(Some(READ_POLL))?;
        let result = self.pump(&mut socket, &claims, &tx, &rx);
        self.leave(&claims.sub);
        result
    }

    /// Read, answer and flush until the socket closes.
    fn pump(&self, socket: &mut WebSocket<TcpStream>, claims: &Claims, tx: &Sender<ServerMessage>, rx: &Receiver<ServerMessage>) -> io::Result<()> {
        loop {
            loop {
                match rx.try_recv() {
                    Ok(message) => send(socket, &message)?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let Ok(message) = serde_json::from_str::<ClientMessage>(text.as_str()) else { continue };
                    if !self.handle(message, claims, tx) {
                        return Ok(());
                    }
                }
                Ok(Message::Close(_)) => return Ok(()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(error)) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
                Err(_) => return Ok(()),
            }
        }
    }

    /// Answer one message; `false` closes the connection.
    fn handle(&self, message: ClientMessage, claims: &Claims, tx: &Sender<ServerMessage>) -> bool {
        match message {
            ClientMessage::Op { mut op } => {
                // The token says who sent it; a client cannot file an edit under another name.
                op.actor = claims.sub.clone();
                match lock(&self.room).apply_as(claims.role, &op) {
                    Ok(Outcome::Applied(applied)) => {
                        self.metrics.applied.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Applied { applied: applied.clone() });
                        self.broadcast(Some(&claims.sub), ServerMessage::Applied { applied });
                    }
                    Ok(Outcome::Rejected(rejected)) => {
                        self.metrics.rejected.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Rejected { rejected });
                    }
                    Err(error) => {
                        self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Error { code: "internal".into(), message: error.to_string(), retryable: true });
                    }
                }
            }
            ClientMessage::Undo { op_id } => {
                if !claims.role.may_write() {
                    let _ = tx.send(ServerMessage::Rejected {
                        rejected: Reject { op_id, code: "forbidden".into(), message: "a viewer cannot undo".into(), retryable: false },
                    });
                    return true;
                }
                // The lock is held until the answer is broadcast, as the `Op` arm does: an
                // operation from another participant must not broadcast its version in between.
                let mut room = lock(&self.room);
                let already = room.applied(&op_id).cloned();
                let outcome = match already {
                    Some(applied) => Ok(Outcome::Applied(applied)),
                    None => room.undo(&claims.sub, &op_id),
                };
                match outcome {
                    Ok(Outcome::Applied(applied)) => {
                        self.metrics.applied.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Applied { applied: applied.clone() });
                        self.broadcast(Some(&claims.sub), ServerMessage::Applied { applied });
                    }
                    Ok(Outcome::Rejected(rejected)) => {
                        self.metrics.rejected.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Rejected { rejected });
                    }
                    Err(error) => {
                        self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Error { code: "internal".into(), message: error.to_string(), retryable: true });
                    }
                }
            }
            ClientMessage::Redo { op_id } => {
                if !claims.role.may_write() {
                    let _ = tx.send(ServerMessage::Rejected {
                        rejected: Reject { op_id, code: "forbidden".into(), message: "a viewer cannot redo".into(), retryable: false },
                    });
                    return true;
                }
                // Held through the broadcast for the same reason as `Undo`.
                let mut room = lock(&self.room);
                let already = room.applied(&op_id).cloned();
                let outcome = match already {
                    Some(applied) => Ok(Outcome::Applied(applied)),
                    None => room.redo(&claims.sub, &op_id),
                };
                match outcome {
                    Ok(Outcome::Applied(applied)) => {
                        self.metrics.applied.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Applied { applied: applied.clone() });
                        self.broadcast(Some(&claims.sub), ServerMessage::Applied { applied });
                    }
                    Ok(Outcome::Rejected(rejected)) => {
                        self.metrics.rejected.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Rejected { rejected });
                    }
                    Err(error) => {
                        self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(ServerMessage::Error { code: "internal".into(), message: error.to_string(), retryable: true });
                    }
                }
            }
            ClientMessage::Presence { mut presence } => {
                // The token says who this is; a client cannot label its cursor as someone else,
                // nor claim another role: the server fills both from the claims.
                presence.actor = claims.sub.clone();
                presence.role = Some(claims.role);
                // A board that is full refuses a new participant: do not broadcast a cursor the
                // board (and so the `welcome` of the next connection) does not hold.
                if lock(&self.presence).update(presence.clone(), Instant::now()) {
                    self.broadcast(Some(&claims.sub), ServerMessage::Presence { presence });
                }
            }
            ClientMessage::Resync { last_version } => self.resync(last_version, tx),
            ClientMessage::Commit { expected_version } => {
                if !claims.role.may_write() {
                    let _ = tx.send(ServerMessage::Rejected {
                        rejected: Reject {
                            op_id: "commit".into(),
                            code: "forbidden".into(),
                            message: "a viewer cannot commit".into(),
                            retryable: false,
                        },
                    });
                    return true;
                }
                // One commit at a time: the etag read and the accept/refuse that follows it must
                // not interleave, or two commits of the same version see the same etag and the
                // second one records a conflict the cabinet does not have. The room lock is not
                // held across the network call; this one is.
                let _commit = lock(&self.commit);
                let (committed, cabinet, expected) = {
                    let room = lock(&self.room);
                    if expected_version != room.version() {
                        let _ = tx.send(ServerMessage::Error {
                            code: "stale".into(),
                            message: "the drawing moved; resynchronise before committing".into(),
                            retryable: true,
                        });
                        return true;
                    }
                    match room.commit(CommitFormat::Dxf) {
                        Ok(committed) => (committed, room.cabinet(), room.cabinet_etag().map(str::to_string)),
                        Err(error) => {
                            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                            let _ = tx.send(ServerMessage::Error { code: "commit".into(), message: error.to_string(), retryable: true });
                            return true;
                        }
                    }
                };
                match cabinet {
                    // The cabinet owns the file: the room posts it and every participant is told
                    // the stored version; nobody writes the file locally (TASK §6).
                    Some(cabinet) => match cabinet.publish(&self.session, &committed.bytes, committed.version, expected.as_deref()) {
                        Ok(published) => {
                            self.metrics.commits.fetch_add(1, Ordering::Relaxed);
                            {
                                let mut room = lock(&self.room);
                                room.accept_cabinet(&published);
                                room.remember_commit(&committed);
                            }
                            let message = ServerMessage::Committed {
                                version: committed.version,
                                not_saved: committed.not_saved.clone(),
                                cabinet: CabinetCommit { version: published.version, etag: published.etag },
                            };
                            let _ = tx.send(message.clone());
                            self.broadcast(Some(&claims.sub), message);
                        }
                        Err(PublishError::Conflict(message)) => {
                            self.metrics.conflicts.fetch_add(1, Ordering::Relaxed);
                            lock(&self.room).remember_conflict(&committed, message.clone());
                            let _ = tx.send(ServerMessage::CommitConflict { version: committed.version, message });
                        }
                        Err(PublishError::Unauthorized) => {
                            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                            let _ = tx.send(ServerMessage::Error {
                                code: "cabinet-unauthorized".into(),
                                message: "the cabinet refused the service token".into(),
                                retryable: false,
                            });
                        }
                        Err(PublishError::TooLarge) => {
                            let _ = tx.send(ServerMessage::Error {
                                code: "cabinet-too-large".into(),
                                message: "the cabinet refused the committed file: too large".into(),
                                retryable: false,
                            });
                        }
                        Err(PublishError::Refused(message)) => {
                            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                            let _ = tx.send(ServerMessage::Error { code: "cabinet-refused".into(), message, retryable: false });
                        }
                        Err(PublishError::Transport(message)) => {
                            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                            let _ = tx.send(ServerMessage::Error { code: "cabinet-unreachable".into(), message, retryable: true });
                        }
                    },
                    // No cabinet: the participant writes the file itself, as before.
                    None => {
                        lock(&self.room).remember_commit(&committed);
                        let _ = tx.send(ServerMessage::CommitResult {
                            version: committed.version,
                            not_saved: committed.not_saved,
                            bytes_base64: encode_bytes(&committed.bytes),
                            cabinet: None,
                        });
                    }
                }
            }
            ClientMessage::SaveCopy { op_id } => {
                if !claims.role.may_write() {
                    let _ = tx.send(ServerMessage::Rejected {
                        rejected: Reject { op_id, code: "forbidden".into(), message: "a viewer cannot save a copy".into(), retryable: false },
                    });
                    return true;
                }
                match lock(&self.room).take_refused_copy() {
                    Some(copy) => {
                        let _ = tx.send(ServerMessage::SaveCopyResult {
                            version: copy.version,
                            not_saved: copy.not_saved,
                            bytes_base64: encode_bytes(&copy.bytes),
                        });
                    }
                    None => {
                        let _ = tx.send(ServerMessage::Error {
                            code: "no-copy".into(),
                            message: "there is no refused commit to save".into(),
                            retryable: false,
                        });
                    }
                }
            }
            ClientMessage::AssetPut { sha256, bytes_base64 } => {
                if !claims.role.may_write() {
                    let _ = tx.send(ServerMessage::Error {
                        code: "forbidden".into(),
                        message: "a viewer may read attachments, not store them".into(),
                        retryable: false,
                    });
                    return true;
                }
                let Some(store) = &self.assets else {
                    let _ = tx.send(ServerMessage::Error {
                        code: "unsupported".into(),
                        message: "this room has no attachment store".into(),
                        retryable: false,
                    });
                    return true;
                };
                let Some(bytes) = cadcraft_collab::message::decode_bytes(&bytes_base64).ok() else {
                    let _ = tx.send(ServerMessage::Error {
                        code: "invalid".into(),
                        message: "the attachment bytes are not valid base64".into(),
                        retryable: false,
                    });
                    return true;
                };
                match store.put(&sha256, &bytes) {
                    Ok(stored) => {
                        let _ = tx.send(ServerMessage::AssetStored { sha256: stored.sha256, bytes: stored.bytes, stored: stored.stored });
                    }
                    Err(error) => {
                        self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(asset_error(&error));
                    }
                }
            }
            ClientMessage::AssetGet { sha256 } => {
                let Some(store) = &self.assets else {
                    let _ = tx.send(ServerMessage::Error {
                        code: "unsupported".into(),
                        message: "this room has no attachment store".into(),
                        retryable: false,
                    });
                    return true;
                };
                match store.get(&sha256) {
                    Ok(bytes) => {
                        let _ = tx.send(ServerMessage::Asset { sha256, bytes_base64: cadcraft_collab::message::encode_bytes(&bytes) });
                    }
                    Err(error) => {
                        self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(asset_error(&error));
                    }
                }
            }
            ClientMessage::Ping => {
                let _ = tx.send(ServerMessage::Pong);
            }
            ClientMessage::Hello { .. } => {
                let _ = tx.send(ServerMessage::Error { code: "invalid".into(), message: "already connected".into(), retryable: false });
                return false;
            }
        }
        true
    }

    fn resync(&self, last_version: u64, tx: &Sender<ServerMessage>) {
        let (delta, snapshot) = {
            let room = lock(&self.room);
            let tail: Vec<_> = room.journal().filter(|a| a.version > last_version).cloned().collect();
            // An empty tail only proves the client is up to date when it already holds the
            // room's version: after a compaction (or a restart from a snapshot) the journal is
            // empty for a client that is behind, and a delta with no changes would move it past
            // the edits it never saw (CodeRabbit on #2937).
            let reachable = last_version >= room.version() || tail.first().is_some_and(|a| a.version == last_version.saturating_add(1));
            if reachable {
                (Some(ServerMessage::Delta { version: room.version(), more: false, applied: tail }), None)
            } else {
                (None, room.snapshot().ok())
            }
        };
        if let Some(delta) = delta {
            let _ = tx.send(delta);
        }
        if let Some(snapshot) = snapshot {
            for part in Snapshot::encode(snapshot.version, &snapshot.bytes) {
                let _ = tx.send(ServerMessage::Snapshot { snapshot: part });
            }
        }
    }

    /// The drawing in parts for a client that resumes from `last_version`, when the journal cannot
    /// reach it (including a first connection).
    fn snapshot_for(&self, last_version: Option<u64>) -> Vec<Snapshot> {
        let room = lock(&self.room);
        let covered = match last_version {
            Some(version) if version == room.version() => true,
            Some(version) => room.journal().any(|a| a.version == version.saturating_add(1)),
            None => false,
        };
        if covered {
            return Vec::new();
        }
        room.snapshot().map(|snapshot| Snapshot::encode(snapshot.version, &snapshot.bytes)).unwrap_or_default()
    }

    fn broadcast(&self, except: Option<&str>, message: ServerMessage) {
        for (actor, tx) in lock(&self.peers).iter() {
            if Some(actor.as_str()) == except {
                continue;
            }
            let _ = tx.send(message.clone());
        }
    }

    fn leave(&self, actor: &str) {
        lock(&self.presence).remove(actor);
        lock(&self.peers).retain(|(name, _)| name != actor);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn send(socket: &mut WebSocket<TcpStream>, message: &ServerMessage) -> io::Result<()> {
    let text = serde_json::to_string(message).map_err(io::Error::other)?;
    socket.send(Message::Text(text.into())).map_err(io::Error::other)
}

fn read_client(socket: &mut WebSocket<TcpStream>) -> io::Result<Option<ClientMessage>> {
    match socket.read() {
        Ok(Message::Text(text)) => Ok(serde_json::from_str(text.as_str()).ok()),
        _ => Ok(None),
    }
}

/// The client-facing answer to a store error; no path or contents are echoed.
fn asset_error(error: &AssetError) -> ServerMessage {
    let (code, retryable) = match error {
        AssetError::TooLarge { .. } | AssetError::TooManyFiles { .. } | AssetError::Quota { .. } => ("limit", false),
        AssetError::NotFound => ("not-found", false),
        AssetError::Io(_) => ("internal", true),
        AssetError::BadId | AssetError::Mismatch | AssetError::NotAnImage | AssetError::BadRoom { .. } => ("invalid", false),
    };
    ServerMessage::Error { code: code.into(), message: error.to_string(), retryable }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::StaticAuthenticator;
    use cadcraft_collab::{Op, OpContext, OpKind, Presence};
    use cadcraft_doc::{Drawing, Space};
    use serde_json::json;

    fn a_server() -> Server {
        Server::new(Room::new(Drawing::new_metric()).unwrap(), Arc::new(StaticAuthenticator::default()), "s1", "cadcraft")
    }

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

    fn presence(actor: &str) -> Presence {
        Presence { actor: actor.into(), role: None, space: Space::Model, cursor: Some([1.0, 2.0]), selection: vec![], layer: "0".into(), view: None }
    }

    /// The claims of `doc`: an editor of that drawing.
    fn claims(doc: &str) -> Claims {
        Claims { sub: "u1".into(), doc: doc.into(), role: crate::Role::Editor }
    }

    #[test]
    fn a_token_naming_another_drawing_cannot_join() {
        let server = a_server().with_doc("file-1");
        let other = claims("file-2");
        assert!(!server.accepts_doc(&other, "file-2"), "a token for another drawing is refused, whatever it asks for");
        let right = claims("file-1");
        assert!(server.accepts_doc(&right, "file-1"));
        assert!(!server.accepts_doc(&right, "file-2"), "the `hello` must name the token's drawing too");
        let dev = Claims { doc: String::new(), ..right };
        assert!(!server.accepts_doc(&dev, "file-1"), "an unnamed token only fits a deployment that names no drawing");
        assert!(a_server().accepts_doc(&dev, "anything"), "the `--dev-token` case stays explicit");
    }

    #[test]
    fn open_connections_are_counted_even_without_presence() {
        let server = a_server();
        assert_eq!(server.connections_open(), 0);
        let (tx, _rx) = channel();
        lock(&server.peers).push(("viewer".into(), tx));
        assert_eq!(server.connections_open(), 1, "a viewer that has not sent presence is still a connection");
        assert_eq!(server.participants(), 0, "but not a participant of the board");
    }

    /// A client behind a compacted journal must be sent the drawing: the tail is empty, and an
    /// empty delta with the room's version would move the client past the edits it never saw
    /// (CodeRabbit on #2937).
    #[test]
    fn a_client_behind_a_compacted_journal_gets_the_drawing() {
        let mut room = Room::new(Drawing::new_metric()).unwrap();
        room.apply(&line_op("op-1", 0.0)).unwrap();
        assert_eq!(room.version(), 1);
        let state = std::env::temp_dir().join(format!("cadcraft-resync-{}.dxf", std::process::id()));
        room.compact(&state).unwrap();
        assert_eq!(room.journal().count(), 0, "a compaction empties the journal");
        let server = Server::new(room, Arc::new(StaticAuthenticator::default()), "s1", "cadcraft");

        let (tx, answers) = channel();
        server.resync(0, &tx);
        assert!(
            matches!(answers.try_recv(), Ok(ServerMessage::Snapshot { .. })),
            "a client at 0 with the room at 1 cannot be caught up from an empty journal"
        );
        // A client that already holds the room's version needs nothing.
        let (tx, answers) = channel();
        server.resync(1, &tx);
        assert!(
            matches!(answers.try_recv(), Ok(ServerMessage::Delta { version: 1, applied, .. }) if applied.is_empty()),
            "a client at the room's version gets an empty delta"
        );

        let _ = std::fs::remove_file(&state);
        let _ = std::fs::remove_file(state.with_extension("version"));
    }

    #[test]
    fn a_presence_the_board_refused_is_not_broadcast() {
        let server = a_server();
        let now = Instant::now();
        for i in 0..crate::MAX_PARTICIPANTS {
            assert!(lock(&server.presence).update(presence(&format!("p{i}")), now));
        }
        let (listener, heard) = channel();
        lock(&server.peers).push(("listener".into(), listener));
        let (own, _own_rx) = channel();

        server.handle(ClientMessage::Presence { presence: presence("late") }, &claims("d1"), &own);

        assert!(heard.try_recv().is_err(), "the board refused the entry, so no peer is told about it");
        assert_eq!(server.participants(), crate::MAX_PARTICIPANTS);
    }

    #[test]
    fn an_undo_and_an_operation_never_arrive_out_of_order() {
        let server = Arc::new(a_server());
        let (observer, heard) = channel();
        lock(&server.peers).push(("observer".into(), observer));
        let claims = claims("d1");
        let workers: Vec<_> = (0u8..2)
            .map(|worker| {
                let server = Arc::clone(&server);
                let claims = claims.clone();
                thread::spawn(move || {
                    let (own, _own_rx) = channel();
                    for n in 0..200u32 {
                        let message = if worker == 0 {
                            ClientMessage::Op { op: line_op(&format!("op-{worker}-{n}"), f64::from(n)) }
                        } else {
                            ClientMessage::Undo { op_id: format!("undo-{worker}-{n}") }
                        };
                        server.handle(message, &claims, &own);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("a worker thread");
        }

        let mut version = 0;
        for message in heard.try_iter() {
            if let ServerMessage::Applied { applied } = message {
                assert_eq!(applied.version, version + 1, "version {} arrived after version {version}", applied.version);
                version = applied.version;
            }
        }
        assert!(version >= 200, "the workers did not apply their operations: {version}");
    }
}
