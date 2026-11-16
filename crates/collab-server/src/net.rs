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
use cadcraft_collab::{ClientMessage, Outcome, Reject, ServerMessage};
use tungstenite::{Message, WebSocket, accept};

use crate::auth::{Authenticator, Claims};
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
             # TYPE cadcraft_collab_version gauge\ncadcraft_collab_version {}\n\
             # TYPE cadcraft_collab_journal_length gauge\ncadcraft_collab_journal_length {}\n\
             # TYPE cadcraft_collab_participants gauge\ncadcraft_collab_participants {}\n\
             # TYPE cadcraft_collab_connections_open gauge\ncadcraft_collab_connections_open {}\n",
            self.connections.load(Ordering::Relaxed),
            self.applied.load(Ordering::Relaxed),
            self.rejected.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
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
        }
    }

    /// Name the drawing this room serves (`--file-id`): a token whose `doc` is another drawing is
    /// refused at the first message, so the per-drawing access the claim carries is enforced here
    /// and not only by the client.
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
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
                let already = lock(&self.room).applied(&op_id).cloned();
                let outcome = match already {
                    Some(applied) => Ok(Outcome::Applied(applied)),
                    None => lock(&self.room).undo(&claims.sub, &op_id),
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
                let already = lock(&self.room).applied(&op_id).cloned();
                let outcome = match already {
                    Some(applied) => Ok(Outcome::Applied(applied)),
                    None => lock(&self.room).redo(&claims.sub, &op_id),
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
            ClientMessage::Presence { presence } => {
                // The token says who this is; a client cannot label its cursor as someone else.
                let mut presence = presence;
                presence.actor = claims.sub.clone();
                lock(&self.presence).update(presence.clone(), Instant::now());
                self.broadcast(Some(&claims.sub), ServerMessage::Presence { presence });
            }
            ClientMessage::Resync { last_version } => self.resync(last_version, tx),
            ClientMessage::Commit { expected_version } => {
                let committed = {
                    let room = lock(&self.room);
                    if expected_version == room.version() { room.commit(CommitFormat::Dxf).ok() } else { None }
                };
                let message = match committed {
                    Some(committed) => ServerMessage::CommitResult {
                        version: committed.version,
                        not_saved: committed.not_saved,
                        bytes_base64: encode_bytes(&committed.bytes),
                    },
                    None => ServerMessage::Error {
                        code: "stale".into(),
                        message: "the drawing moved; resynchronise before committing".into(),
                        retryable: true,
                    },
                };
                let _ = tx.send(message);
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
            let reachable = tail.is_empty() || tail.first().is_some_and(|a| a.version == last_version.saturating_add(1));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::StaticAuthenticator;
    use cadcraft_collab::Role;
    use cadcraft_doc::Drawing;

    fn server(doc: Option<&str>) -> Server {
        let auth = Arc::new(StaticAuthenticator::new(Vec::new()));
        let server = Server::new(Room::new(Drawing::new_metric()).unwrap(), auth, "session", "cadcraft");
        match doc {
            Some(doc) => server.with_doc(doc),
            None => server,
        }
    }

    fn claims(doc: &str) -> Claims {
        Claims { sub: "u1".into(), doc: doc.into(), role: Role::Editor }
    }

    /// The token's `doc` is the cabinet's file id: a room that serves one drawing must not accept
    /// a token for another, or an editor of one drawing edits every drawing that shares the
    /// secret (CodeRabbit on #2936).
    #[test]
    fn a_token_is_bound_to_the_drawing_the_room_serves() {
        let bound = server(Some("file-1"));
        assert!(bound.accepts_doc(&claims("file-1"), "file-1"));
        assert!(!bound.accepts_doc(&claims("file-2"), "file-2"), "another drawing's token");
        assert!(!bound.accepts_doc(&claims("file-1"), "file-2"), "the client asked for another drawing");
        assert!(!bound.accepts_doc(&claims(""), "file-1"), "a dev token in a room bound to a drawing");

        let unbound = server(None);
        assert!(unbound.accepts_doc(&claims("file-1"), "file-1"), "no served id: the claim only has to match the request");
        assert!(!unbound.accepts_doc(&claims("file-1"), "file-2"));
        assert!(unbound.accepts_doc(&claims(""), "anything"), "a dev token names no drawing");
    }

    /// The idle shutdown looks at sockets, not presence: a viewer never sends presence and would
    /// otherwise have the room closed under it (CodeRabbit on #2936).
    #[test]
    fn open_connections_are_counted_even_without_presence() {
        let server = server(None);
        assert_eq!(server.connections_open(), 0);
        let (tx, _rx) = channel();
        lock(&server.peers).push(("u1".into(), tx));
        assert_eq!(server.connections_open(), 1);
        assert_eq!(server.participants(), 0, "a connection that sent no presence is still a connection");
    }
}
