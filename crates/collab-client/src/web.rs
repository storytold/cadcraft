//! The browser transport: the page's `WebSocket` feeds a queue, the frame drains it.
//!
//! There are no threads on wasm32 and the browser has no blocking read. The socket's callbacks
//! push [`Incoming`] values into a queue and [`WebSession::poll`] drains it once per frame; every
//! text frame then goes through the same [`crate::driver::Driver`] the desktop
//! [`crate::Session`] uses, so a replica reaches the same state on both transports.
//!
//! A close schedules a reconnect with the same backoff as the desktop worker (250 ms doubling to
//! 10 s). The `hello` of that reconnect names the version this replica shows; the room answers
//! with the journal tail, or with a snapshot when the journal cannot reach it. Operations the
//! server never answered stay in the replica's outbox and are sent again with their ids, which the
//! room answers from its window instead of applying twice (ADR 0003).
//!
//! The token lives in this object and nowhere else: it goes in the first `hello` frame, never in
//! the socket URL, a log line, a DOM attribute or a URL.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use cadcraft_collab::{ClientMessage, Op, Presence, Role, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;
use web_sys::{CloseEvent, Event as WebEvent, MessageEvent, WebSocket};

use crate::Client;
use crate::driver::{Driver, Event, Status, hello};
use crate::join::Join as SnapshotJoin;

/// What the frame loop hears from the socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Incoming {
    /// The socket is open: send `hello` (again, after a reconnect).
    Opened,
    /// One text frame from the server.
    Text(String),
    /// The socket closed; a reconnect is scheduled.
    Closed,
}

/// The socket's own state, between messages: what a sync indicator needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketState {
    /// A socket exists but its `open` event has not fired yet.
    Connecting,
    /// The socket is open and can send.
    Open,
    /// The socket closed; [`schedule`] will dial again.
    Closed,
}

/// The socket and the messages it produced since the last frame.
pub struct Socket {
    inner: Rc<RefCell<Inner>>,
}

impl fmt::Debug for Socket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Socket").field("socket", &"<WebSocket>").finish()
    }
}

struct Inner {
    url: String,
    socket: Option<WebSocket>,
    incoming: VecDeque<Incoming>,
    attempt: u32,
    live: bool,
    closers: Closers,
}

#[derive(Default)]
#[allow(dead_code, reason = "the closures are kept alive by being stored; they are never called from Rust")]
struct Closers {
    open: Option<Closure<dyn FnMut(WebEvent)>>,
    message: Option<Closure<dyn FnMut(MessageEvent)>>,
    close: Option<Closure<dyn FnMut(CloseEvent)>>,
}

impl Socket {
    /// Open `url`; the token is not part of it.
    fn open(url: &str) -> Result<Socket, String> {
        let inner = Rc::new(RefCell::new(Inner {
            url: url.to_string(),
            socket: None,
            incoming: VecDeque::new(),
            attempt: 0,
            live: true,
            closers: Closers::default(),
        }));
        dial(&inner)?;
        Ok(Socket { inner })
    }

    /// Drain what arrived since the previous frame.
    fn poll(&self) -> Vec<Incoming> {
        self.inner.borrow_mut().incoming.drain(..).collect()
    }

    /// The socket's state, for the sync indicator.
    fn state(&self) -> SocketState {
        let inner = self.inner.borrow();
        match &inner.socket {
            Some(socket) if socket.ready_state() == WebSocket::OPEN => SocketState::Open,
            Some(_) => SocketState::Connecting,
            None => SocketState::Closed,
        }
    }

    /// Send one text frame; an error means the socket is not open.
    fn send(&self, text: &str) -> Result<(), String> {
        let inner = self.inner.borrow();
        let Some(socket) = &inner.socket else { return Err("the collaboration socket is not open".into()) };
        if socket.ready_state() != WebSocket::OPEN {
            return Err("the collaboration socket is not open".into());
        }
        socket.send_with_str(text).map_err(|error| format!("cannot send a collaboration frame: {error:?}"))
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.live = false;
        if let Some(socket) = inner.socket.take() {
            let _ = socket.close();
        }
    }
}

/// Make one socket and wire its callbacks.
fn dial(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
    let url = inner.borrow().url.clone();
    // Drop the previous socket's callbacks first: they point at closures this call replaces.
    if let Some(old) = inner.borrow_mut().socket.take() {
        old.set_onopen(None);
        old.set_onmessage(None);
        old.set_onclose(None);
        let _ = old.close();
    }
    let socket = WebSocket::new(&url).map_err(|error| format!("cannot open a collaboration socket: {error:?}"))?;
    socket.set_binary_type(web_sys::BinaryType::Arraybuffer);
    let weak = Rc::downgrade(inner);

    let on_open = Closure::<dyn FnMut(WebEvent)>::new(move |_event: WebEvent| {
        if let Some(inner) = weak.upgrade() {
            let mut inner = inner.borrow_mut();
            inner.attempt = 0;
            inner.incoming.push_back(Incoming::Opened);
        }
    });
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));

    let weak = Rc::downgrade(inner);
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Some(inner) = weak.upgrade() else { return };
        if let Some(text) = event.data().as_string() {
            inner.borrow_mut().incoming.push_back(Incoming::Text(text));
        }
    });
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let weak = Rc::downgrade(inner);
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_event: CloseEvent| {
        let Some(inner) = weak.upgrade() else { return };
        {
            let mut inner = inner.borrow_mut();
            if !inner.live {
                return;
            }
            inner.socket = None;
            inner.incoming.push_back(Incoming::Closed);
        }
        schedule(&inner);
    });
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

    let mut inner = inner.borrow_mut();
    inner.socket = Some(socket);
    inner.closers = Closers { open: Some(on_open), message: Some(on_message), close: Some(on_close) };
    Ok(())
}

/// Reconnect after a backoff: 250 ms, doubling, at most 10 s.
fn schedule(inner: &Rc<RefCell<Inner>>) {
    let delay = {
        let mut inner = inner.borrow_mut();
        if !inner.live {
            return;
        }
        let delay = 250u32.saturating_mul(1u32 << inner.attempt.min(6));
        inner.attempt = inner.attempt.saturating_add(1);
        delay.min(10_000)
    };
    let weak = Rc::downgrade(inner);
    let timer = Closure::<dyn FnMut()>::once(move || {
        if let Some(inner) = weak.upgrade()
            && inner.borrow().live
            && let Err(_error) = dial(&inner)
        {
            schedule(&inner);
        }
    });
    if let Some(window) = web_sys::window()
        && window.set_timeout_with_callback_and_timeout_and_arguments_0(timer.as_ref().unchecked_ref(), delay as i32).is_ok()
    {
        // A closure that fires once; keeping it alive for the page would leak one small
        // allocation per reconnect, dropping it would free code that is still running.
        timer.forget();
    }
}

/// A participant's session in the browser: the socket, the replica and the events per frame.
///
/// [`WebSession::connect`] returns as soon as the socket is dialled; the replica arrives with the
/// welcome's snapshot (or is kept on a reconnect). The application drains [`poll`](Self::poll)
/// once per frame and shows what came.
pub struct WebSession {
    socket: Socket,
    doc: String,
    token: String,
    /// The participant's id, as the `welcome` names it (the token's `sub`).
    actor_id: Option<String>,
    /// The role the host named; the welcome's role replaces it.
    role: Role,
    driver: Option<Driver>,
    /// A snapshot being brought in: decoded part by part and read in bounded batches, so a large
    /// drawing does not freeze the frame that received it.
    join: Option<Joining>,
    /// Operations and control messages submitted before the first welcome.
    waiting_ops: Vec<Op>,
    waiting: VecDeque<ClientMessage>,
    events: VecDeque<Event>,
}

/// How much a browser gives one frame to bringing in a snapshot: reading more than this at once
/// makes the page stutter, and a large drawing is shown as it arrives instead (ADR-0006's pace
/// idea, applied to the join).
const JOIN_FRAME: Duration = Duration::from_millis(8);
/// The most entity records one [`cadcraft_io::Incremental::step`] reads: at 200 000 entities a
/// record costs tens of microseconds, so this stays near a frame's share.
const JOIN_BATCH: usize = 256;

/// A snapshot on its way in: its parts are base64-decoded one poll each, then the drawing is read
/// in bounded steps. Nothing here runs in the `welcome` frame.
struct Joining {
    join: SnapshotJoin,
    role: Role,
    session_id: String,
    participants: Vec<Presence>,
    room_version: u64,
}

impl fmt::Debug for WebSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSession")
            .field("doc", &self.doc)
            .field("token", &"<redacted>")
            .field("role", &self.role)
            .field("version", &self.driver.as_ref().map(Driver::version))
            .finish()
    }
}

impl WebSession {
    /// Dial `url` for the drawing `doc`. `role` is the host's role until the welcome names the
    /// server's; the token goes in the first frame, never in the URL.
    pub fn connect(url: &str, token: &str, doc: &str, role: Role) -> Result<WebSession, String> {
        Ok(WebSession {
            socket: Socket::open(url)?,
            doc: doc.to_string(),
            token: token.to_string(),
            actor_id: None,
            role,
            driver: None,
            join: None,
            waiting_ops: Vec::new(),
            waiting: VecDeque::new(),
            events: VecDeque::new(),
        })
    }

    /// The participant's role: the welcome's, or the host's until the welcome arrives.
    pub fn role(&self) -> Role {
        self.driver.as_ref().map(Driver::role).unwrap_or(self.role)
    }

    /// The participant's id (the token's `sub`), once the welcome named it. The host's participant
    /// list uses it for this participant's own entry.
    pub fn client_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    /// The connection's state for the cabinet's sync indicator: a closed socket is offline, a
    /// replica that is still arriving or an unanswered operation is pending, otherwise in sync.
    pub fn status(&self) -> Status {
        match self.socket.state() {
            SocketState::Closed => Status::Offline,
            SocketState::Connecting => Status::Connecting,
            SocketState::Open => {
                if self.join.is_some() {
                    return Status::Pending;
                }
                match &self.driver {
                    None => Status::Connecting,
                    Some(driver) if driver.client().outbox().is_empty() => Status::InSync,
                    Some(_) => Status::Pending,
                }
            }
        }
    }

    /// The join's progress, once its parts are decoded: entity records read and the file's total.
    /// `None` when no snapshot is being read.
    pub fn join_progress(&self) -> Option<(usize, usize)> {
        self.join.as_ref().and_then(|joining| joining.join.progress())
    }

    /// The room's id, once the welcome named it.
    pub fn session_id(&self) -> Option<&str> {
        self.driver.as_ref().map(Driver::session_id)
    }

    /// The version the replica shows; `0` before the welcome.
    pub fn version(&self) -> u64 {
        self.driver.as_ref().map(Driver::version).unwrap_or(0)
    }

    /// The replica, once the welcome brought it.
    pub fn drawing(&self) -> Option<&cadcraft_doc::Drawing> {
        self.driver.as_ref().map(Driver::drawing)
    }

    /// The participants the welcome named.
    pub fn participants(&self) -> &[Presence] {
        self.driver.as_ref().map(Driver::participants).unwrap_or(&[])
    }

    /// Send an operation; the replica changes when its answer arrives. Before the welcome the
    /// operation waits for the replica.
    pub fn submit(&mut self, op: Op) -> Result<(), String> {
        if self.join.is_some() {
            // The snapshot being read replaces the replica; submit after it is in.
            self.waiting_ops.push(op);
            return Ok(());
        }
        match &mut self.driver {
            Some(driver) => {
                driver.client_mut().submit(op.clone());
                // A closed socket loses this frame, but the operation is already in the replica's
                // outbox: the next welcome resends it with its id (ADR 0003), so it is not queued
                // twice.
                let text =
                    serde_json::to_string(&ClientMessage::Op { op }).map_err(|error| format!("cannot encode a collaboration message: {error}"))?;
                let _ = self.socket.send(&text);
                Ok(())
            }
            None => {
                self.waiting_ops.push(op);
                Ok(())
            }
        }
    }

    /// Ask to undo this participant's last operation.
    pub fn undo(&mut self, op_id: &str) -> Result<(), String> {
        self.send(ClientMessage::Undo { op_id: op_id.into() })
    }

    /// Ask to redo this participant's last undone operation.
    pub fn redo(&mut self, op_id: &str) -> Result<(), String> {
        self.send(ClientMessage::Redo { op_id: op_id.into() })
    }

    /// Tell the others where this participant is looking.
    pub fn presence(&mut self, presence: Presence) -> Result<(), String> {
        self.send(ClientMessage::Presence { presence })
    }

    /// Ask for the file to be written by the room.
    pub fn commit(&mut self, expected_version: u64) -> Result<(), String> {
        self.send(ClientMessage::Commit { expected_version })
    }

    /// Ask for the file of a commit the cabinet refused with `409`.
    pub fn save_copy(&mut self, op_id: &str) -> Result<(), String> {
        self.send(ClientMessage::SaveCopy { op_id: op_id.into() })
    }

    /// Ask for the journal tail after `last_version`, or a snapshot when it is gone.
    pub fn resync(&mut self, last_version: u64) -> Result<(), String> {
        self.send(ClientMessage::Resync { last_version })
    }

    /// Store an attachment's bytes under their content address (ADR 0021).
    pub fn put_asset(&mut self, sha256: &str, bytes: &[u8]) -> Result<(), String> {
        let bytes_base64 = cadcraft_collab::message::encode_bytes(bytes);
        self.send(ClientMessage::AssetPut { sha256: sha256.into(), bytes_base64 })
    }

    /// Ask the room for an attachment's bytes by their content address.
    pub fn get_asset(&mut self, sha256: &str) -> Result<(), String> {
        self.send(ClientMessage::AssetGet { sha256: sha256.into() })
    }

    /// Drain the socket and return what arrived since the previous frame.
    pub fn poll(&mut self) -> Vec<Event> {
        for incoming in self.socket.poll() {
            match incoming {
                Incoming::Opened => {
                    let last_version = self.driver.as_ref().map(Driver::version);
                    let hello = hello(&self.doc, &self.token, last_version, "cadcraft-web");
                    match serde_json::to_string(&hello) {
                        Ok(text) => {
                            if let Err(error) = self.socket.send(&text) {
                                self.events.push_back(Event::Error { code: "transport".into(), message: error, retryable: true });
                            }
                        }
                        Err(error) => self.events.push_back(Event::Error {
                            code: "transport".into(),
                            message: format!("cannot encode the hello: {error}"),
                            retryable: false,
                        }),
                    }
                }
                Incoming::Text(text) => self.on_text(&text),
                Incoming::Closed => {}
            }
        }
        self.advance_join();
        self.events.drain(..).collect()
    }

    /// Bring the snapshot in without blocking a frame: one base64 part, the drawing's group scan,
    /// or a bounded batch of entity records per call, until the frame's share is spent.
    fn advance_join(&mut self) {
        let Some(mut joining) = self.join.take() else { return };
        match joining.join.advance(JOIN_FRAME, JOIN_BATCH) {
            Ok(true) => {
                self.join = Some(joining);
                self.complete_join();
            }
            Ok(false) => self.join = Some(joining),
            Err(error) => {
                self.events.push_back(Event::Error { code: "session".into(), message: error.to_string(), retryable: false });
            }
        }
    }

    /// The snapshot is read: replace the replica, send what waited for it and catch up with the
    /// journal tail the welcome named (operations may have been applied while it was read).
    fn complete_join(&mut self) {
        let Some(joining) = self.join.take() else { return };
        let shown = joining.join.version();
        let Some(drawing) = joining.join.drawing() else { return };
        let mut driver = Driver::new(Client::new(drawing, shown), joining.role, joining.session_id, joining.participants);
        for op in std::mem::take(&mut self.waiting_ops) {
            driver.client_mut().submit(op);
        }
        let last_version = driver.resync_from(joining.room_version);
        self.driver = Some(driver);
        self.flush();
        if let Some(last_version) = last_version {
            let _ = self.send(ClientMessage::Resync { last_version });
        }
    }

    fn send(&mut self, message: ClientMessage) -> Result<(), String> {
        if self.driver.is_none() || self.join.is_some() {
            // Presence is replaced by the next frame; keep the durable messages for the join.
            if !matches!(message, ClientMessage::Presence { .. }) {
                self.queue(message);
            }
            return Ok(());
        }
        let text = serde_json::to_string(&message).map_err(|error| format!("cannot encode a collaboration message: {error}"))?;
        match self.socket.send(&text) {
            Ok(()) => Ok(()),
            // The socket is between reconnects. Presence updates the next frame replaces are
            // dropped; an undo, a commit or an attachment waits for the next welcome.
            Err(_) => {
                if !matches!(message, ClientMessage::Presence { .. }) {
                    self.queue(message);
                }
                Ok(())
            }
        }
    }

    /// Hold a message for the next welcome. A page that stays offline must not grow the queue
    /// without bound: the oldest request is dropped once it is full.
    fn queue(&mut self, message: ClientMessage) {
        self.waiting.push_back(message);
        if self.waiting.len() > 64 {
            self.waiting.pop_front();
        }
    }

    fn on_text(&mut self, text: &str) {
        let Ok(message) = serde_json::from_str::<ServerMessage>(text) else { return };
        if let ServerMessage::Welcome { session_id, actor_id, role, version, snapshot, participants, .. } = message {
            self.on_welcome(session_id, actor_id, role, version, snapshot, participants);
            return;
        }
        // While a snapshot is being read, the replica it replaces is stale: the tail comes from
        // the journal after the join, so these messages are not applied twice.
        if self.join.is_some() {
            return;
        }
        let Some(driver) = &mut self.driver else { return };
        match driver.on_message(message) {
            Ok(Some(event)) => self.events.push_back(event),
            Ok(None) => {}
            Err(error) => {
                // A gap means this replica missed a version: ask for the journal tail or a fresh
                // snapshot instead of guessing (ADR 0001).
                let last_version = driver.version();
                self.events.push_back(Event::Error { code: "session".into(), message: error.to_string(), retryable: true });
                let _ = self.send(ClientMessage::Resync { last_version });
            }
        }
    }

    fn on_welcome(
        &mut self,
        session_id: String,
        actor_id: String,
        role: Role,
        room_version: u64,
        snapshot: Vec<cadcraft_collab::message::Snapshot>,
        participants: Vec<Presence>,
    ) {
        self.actor_id = Some(actor_id);
        // The welcome names who is already here; show them before the first presence frame, as
        // the desktop worker does.
        for presence in &participants {
            self.events.push_back(Event::Presence(presence.clone()));
        }
        if let Some(driver) = &mut self.driver
            && snapshot.is_empty()
        {
            match driver.reconnect(session_id, role, participants, room_version, &snapshot) {
                Ok(last_version) => {
                    self.flush();
                    if let Some(last_version) = last_version {
                        let _ = self.send(ClientMessage::Resync { last_version });
                    }
                }
                Err(error) => self.events.push_back(Event::Error { code: "session".into(), message: error.to_string(), retryable: false }),
            }
            return;
        }
        if snapshot.is_empty() {
            self.events.push_back(Event::Error {
                code: "session".into(),
                message: crate::driver::SessionError::MissingDrawing.to_string(),
                retryable: false,
            });
            return;
        }
        // A snapshot, first join or reconnect: read it across frames instead of freezing this one.
        // The old replica waits until the new one is in, and a tail is fetched after it.
        self.join = Some(Joining { join: SnapshotJoin::new(snapshot), role, session_id, participants, room_version });
    }

    /// Send the replica's unconfirmed operations and the queued control messages, in order.
    fn flush(&mut self) {
        let Some(driver) = &self.driver else { return };
        let outbox = driver.client().outbox().to_vec();
        for op in outbox {
            let _ = self.send(ClientMessage::Op { op });
        }
        let waiting: Vec<ClientMessage> = self.waiting.drain(..).collect();
        for message in waiting {
            let _ = self.send(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The token is a credential: a debug print must not put it in a log.
    #[test]
    fn a_web_session_debug_redacts_the_token() {
        // A socket cannot be opened outside a browser; the redaction is checked on the struct's
        // Debug through a session that never dialled: the same formatting path is used.
        let session = WebSession {
            socket: Socket {
                inner: Rc::new(RefCell::new(Inner {
                    url: "ws://example".into(),
                    socket: None,
                    incoming: VecDeque::new(),
                    attempt: 0,
                    live: false,
                    closers: Closers::default(),
                })),
            },
            doc: "plan.dxf".into(),
            token: "jwt-secret".into(),
            actor_id: None,
            role: Role::Editor,
            driver: None,
            join: None,
            waiting_ops: Vec::new(),
            waiting: VecDeque::new(),
            events: VecDeque::new(),
        };
        let debug = format!("{session:?}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(!debug.contains("jwt-secret"), "{debug}");
    }
}
