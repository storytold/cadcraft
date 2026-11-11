//! A shared session's client connection, driven on its own thread (or by the browser's frame).
//!
//! A frame never waits on a socket: on the desktop a worker thread owns the
//! [`cadcraft_collab_client::Session`], sends queued operations and presence, and forwards what
//! arrives as [`Update`] values. The browser has no threads, so [`Collab`] there owns a
//! [`cadcraft_collab_client::WebSession`] and drains its socket queue in `poll` once per frame.
//! The UI applies the change sets to its drawing and draws the presence it holds.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use cadcraft_collab::{Applied, CabinetCommit, Op, Presence, Reject, Role};
use cadcraft_collab_client::Event;
#[cfg(not(target_arch = "wasm32"))]
use cadcraft_collab_client::Session as Remote;

/// Ask the host page's cabinet to store a commit the room built without publishing it itself
/// (`cadcraft:commit`, with the file the room answered).
///
/// The page that hosts the web build owns the cabinet. The ordinary path is the room publishing
/// the file server-to-server (M87) and the app only reporting the version; this hook is the
/// fallback for a room without the cabinet's callback, and the desktop leaves it unset.
pub type HostCommitFn = Box<dyn Fn(Option<&[u8]>) -> Result<(), String>>;

/// The sync state the cabinet's indicator shows (`session-state.status`): the contract's words are
/// `synced`, `pending`, `offline` and `error`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    /// The socket is dialling or the replica is still arriving.
    Connecting,
    /// The replica is at the server's version and nothing is waiting.
    Synced,
    /// An operation is unanswered, or the replica is catching up after a reconnect.
    Pending,
    /// The socket closed; a reconnect is scheduled.
    Offline,
    /// The session failed in a way a reconnect will not fix.
    Error,
}

impl SyncStatus {
    /// The host contract's word for this state.
    pub fn word(self) -> &'static str {
        match self {
            SyncStatus::Connecting | SyncStatus::Pending => "pending",
            SyncStatus::Synced => "synced",
            SyncStatus::Offline | SyncStatus::Error => "offline",
        }
    }
}

/// One participant as the cabinet's list shows them, in the host bridge's shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionParticipant {
    /// Stable for the session: the server's client id (the token's `sub`).
    pub client_id: String,
    /// `owner`, `editor` or `viewer`, as the server named it.
    pub role: String,
    /// The colour the cursors use, `#rrggbb`.
    pub color: Option<String>,
}

/// What the UI asks the worker to send.
pub enum Request {
    /// An operation.
    Op(Op),
    /// Where the participant is looking.
    Presence(Presence),
    /// Undo this participant's last operation.
    Undo(String),
    /// Redo this participant's last undone operation.
    Redo(String),
    /// Write the file the cabinet stores, at this version.
    Commit(u64),
    /// Ask for the file of a commit the cabinet refused with `409` (TASK §6).
    SaveCopy,
}

/// The state the shared-session panel shows after a commit.
#[derive(Clone, Debug, PartialEq)]
pub struct CommitState {
    /// The drawing's version the file was written from.
    pub version: u64,
    /// The cabinet's answer when the room published the file itself (M87).
    pub cabinet: Option<CabinetCommit>,
    /// What the format could not keep.
    pub not_saved: std::collections::BTreeMap<String, usize>,
}

/// What the worker reports to the UI.
pub enum Update {
    /// The change set an operation produced.
    Applied(Applied),
    /// The room's drawing replaced the replica (the first join, or a snapshot after a reconnect):
    /// the window shows it, not the file it opened from (ADR 0001).
    Joined { drawing: Box<cadcraft_doc::Drawing>, version: u64 },
    /// An operation was refused.
    Rejected(Reject),
    /// Another participant's view.
    Presence(Presence),
    /// The file a commit produced. With `cabinet` the cabinet already stored it (M87): the
    /// drawing is not written anywhere else.
    Committed { version: u64, not_saved: std::collections::BTreeMap<String, usize>, bytes: Vec<u8>, cabinet: Option<CabinetCommit> },
    /// The cabinet refused the commit with `409`: the file changed outside the session.
    CommitConflict { version: u64, message: String },
    /// The file of a refused commit, to save as a copy (TASK §6).
    SaveCopy { version: u64, not_saved: std::collections::BTreeMap<String, usize>, bytes: Vec<u8> },
    /// The connection ended; the message says why.
    Lost(String),
}

/// A 64-bit id for one window's operation ids.
///
/// The room answers a repeated `op_id` with the first answer and never applies it twice
/// (ADR 0003), and its window is keyed by the id alone: two participants — or the same one after
/// a reload — must not both send `local-0`. The id mixes the clock (milliseconds on the web), a
/// process-wide counter (two windows of one process always differ) and the process id.
pub(crate) fn run_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nth = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    nanos.rotate_left(17) ^ nth.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(std::process::id()).rotate_left(48)
}

/// A connection to a shared session.
#[cfg(not(target_arch = "wasm32"))]
pub struct Collab {
    requests: Sender<Request>,
    updates: Receiver<Update>,
    role: Role,
}

#[cfg(not(target_arch = "wasm32"))]
impl Collab {
    /// Connect to `url` as `token` for the drawing `doc`.
    pub fn connect(url: &str, token: &str, doc: &str) -> Result<Self, String> {
        let (request_tx, request_rx) = channel();
        let (update_tx, update_rx) = channel();
        let (ready_tx, ready_rx) = channel();
        let (url, token, doc) = (url.to_string(), token.to_string(), doc.to_string());
        std::thread::spawn(move || worker(url, token, doc, request_rx, update_tx, ready_tx));
        let role = ready_rx.recv_timeout(Duration::from_secs(15)).map_err(|_| "the collaboration server did not answer".to_string())??;
        Ok(Collab { requests: request_tx, updates: update_rx, role })
    }

    /// What the participant may do.
    pub fn role(&self) -> Role {
        self.role
    }

    /// The desktop join blocks until the welcome, so a live connection is in sync; the worker
    /// reports a lost connection as [`Update::Lost`], which detaches the session.
    pub fn status(&self) -> SyncStatus {
        SyncStatus::Synced
    }

    /// The desktop `Session` does not expose its token's `sub`; the participant list still names
    /// everyone whose presence arrived.
    pub fn client_id(&self) -> Option<String> {
        None
    }

    /// Queue a message for the worker.
    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    /// Take the updates that arrived since the last frame; never blocks.
    pub fn poll(&self) -> Vec<Update> {
        let mut updates = Vec::new();
        loop {
            match self.updates.try_recv() {
                Ok(update) => updates.push(update),
                Err(TryRecvError::Empty) => return updates,
                Err(TryRecvError::Disconnected) => return updates,
            }
        }
    }
}

/// A connection with no socket, for tests: the app queues what it would send into the returned
/// receiver, and the test can feed updates back through the returned sender.
#[cfg(all(test, not(target_arch = "wasm32")))]
impl Collab {
    fn detached(role: Role) -> (Self, Receiver<Request>, Sender<Update>) {
        let (requests, request_rx) = channel();
        let (update_tx, updates) = channel();
        (Collab { requests, updates, role }, request_rx, update_tx)
    }
}

/// A connection to a shared session in the browser.
///
/// The browser cannot block and has no worker threads: the socket's callbacks queue what arrives,
/// and [`Collab::poll`] drains that queue once per frame. The session itself is the same
/// [`cadcraft_collab_client::WebSession`] the desktop's replica logic uses.
#[cfg(target_arch = "wasm32")]
pub struct Collab {
    session: std::cell::RefCell<cadcraft_collab_client::WebSession>,
    role: std::cell::Cell<Role>,
    /// The last transport error, reported once by the next `poll`.
    error: std::cell::RefCell<Option<String>>,
}

#[cfg(target_arch = "wasm32")]
impl Collab {
    /// Join the session the host page opened (`?host=parent` / `data-host="page"`).
    ///
    /// Returns as soon as the socket is dialled: the replica and its participants arrive in a
    /// later frame, and the host's role gates edits until the welcome names the server's.
    pub fn connect_host(url: &str, token: &str, doc: &str, role: Role) -> Result<Self, String> {
        Ok(Collab {
            session: std::cell::RefCell::new(cadcraft_collab_client::WebSession::connect(url, token, doc, role)?),
            role: std::cell::Cell::new(role),
            error: std::cell::RefCell::new(None),
        })
    }

    /// What the participant may do (the welcome's role once it arrives).
    pub fn role(&self) -> Role {
        self.role.get()
    }

    /// The connection's state for the cabinet's sync indicator.
    pub fn status(&self) -> SyncStatus {
        match self.session.borrow().status() {
            cadcraft_collab_client::Status::Connecting => SyncStatus::Connecting,
            cadcraft_collab_client::Status::InSync => SyncStatus::Synced,
            cadcraft_collab_client::Status::Pending => SyncStatus::Pending,
            cadcraft_collab_client::Status::Offline => SyncStatus::Offline,
        }
    }

    /// The participant's id (the token's `sub`), once the welcome named it.
    pub fn client_id(&self) -> Option<String> {
        self.session.borrow().client_id().map(str::to_string)
    }

    /// Send one request; the socket queue carries it now or after the next welcome.
    pub fn send(&self, request: Request) {
        let mut session = self.session.borrow_mut();
        let result = match request {
            Request::Op(op) => session.submit(op),
            Request::Presence(presence) => session.presence(presence),
            Request::Undo(op_id) => session.undo(&op_id),
            Request::Redo(op_id) => session.redo(&op_id),
            Request::Commit(version) => session.commit(version),
            Request::SaveCopy => session.save_copy("save-copy"),
        };
        if let Err(message) = result {
            self.error.replace(Some(message));
        }
    }

    /// Drain the socket queue; never blocks.
    pub fn poll(&self) -> Vec<Update> {
        let mut updates = Vec::new();
        if let Some(message) = self.error.borrow_mut().take() {
            updates.push(Update::Lost(message));
        }
        {
            let mut session = self.session.borrow_mut();
            for event in session.poll() {
                match event {
                    Event::Applied(applied) => updates.push(Update::Applied(applied)),
                    Event::Joined { drawing, version } => updates.push(Update::Joined { drawing, version }),
                    Event::Rejected(rejected) => updates.push(Update::Rejected(rejected)),
                    Event::Presence(presence) => updates.push(Update::Presence(presence)),
                    Event::Committed { version, not_saved, bytes, cabinet } => {
                        updates.push(Update::Committed { version, not_saved, bytes, cabinet });
                    }
                    Event::CommitConflict { version, message } => updates.push(Update::CommitConflict { version, message }),
                    Event::SaveCopy { version, not_saved, bytes } => updates.push(Update::SaveCopy { version, not_saved, bytes }),
                    Event::Error { message, .. } => updates.push(Update::Lost(message)),
                    Event::AssetStored { .. } | Event::Asset { .. } | Event::Pong | Event::Ignored => {}
                }
            }
            self.role.set(session.role());
        }
        updates
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn worker(url: String, token: String, doc: String, requests: Receiver<Request>, updates: Sender<Update>, ready: Sender<Result<Role, String>>) {
    // A short read timeout lets the worker drain the request queue between messages.
    let mut session = match Remote::connect(&url, &token, &doc, None, Duration::from_millis(250)) {
        Ok(session) => session,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let _ = ready.send(Ok(session.role()));
    // The welcome lists who is already here; the UI shows them before the first presence frame.
    for presence in session.participants() {
        let _ = updates.send(Update::Presence(presence.clone()));
    }
    loop {
        loop {
            match requests.try_recv() {
                Ok(Request::Op(op)) => {
                    if session.submit(op).is_err() {
                        let _ = updates.send(Update::Lost("the connection closed".into()));
                        return;
                    }
                }
                Ok(Request::Presence(presence)) => {
                    if session.presence(presence).is_err() {
                        let _ = updates.send(Update::Lost("the connection closed".into()));
                        return;
                    }
                }
                Ok(Request::Undo(op_id)) => {
                    if session.undo(&op_id).is_err() {
                        let _ = updates.send(Update::Lost("the connection closed".into()));
                        return;
                    }
                }
                Ok(Request::Redo(op_id)) => {
                    if session.redo(&op_id).is_err() {
                        let _ = updates.send(Update::Lost("the connection closed".into()));
                        return;
                    }
                }
                Ok(Request::Commit(version)) => {
                    if session.commit(version).is_err() {
                        let _ = updates.send(Update::Lost("the connection closed".into()));
                        return;
                    }
                }
                Ok(Request::SaveCopy) => {
                    if session.save_copy("save-copy").is_err() {
                        let _ = updates.send(Update::Lost("the connection closed".into()));
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        match session.next_event() {
            Ok(Event::Applied(applied)) => {
                let _ = updates.send(Update::Applied(applied));
            }
            Ok(Event::Joined { drawing, version }) => {
                let _ = updates.send(Update::Joined { drawing, version });
            }
            Ok(Event::Rejected(rejected)) => {
                let _ = updates.send(Update::Rejected(rejected));
            }
            Ok(Event::Presence(presence)) => {
                let _ = updates.send(Update::Presence(presence));
            }
            Ok(Event::Committed { version, not_saved, bytes, cabinet }) => {
                let _ = updates.send(Update::Committed { version, not_saved, bytes, cabinet });
            }
            Ok(Event::CommitConflict { version, message }) => {
                let _ = updates.send(Update::CommitConflict { version, message });
            }
            Ok(Event::SaveCopy { version, not_saved, bytes }) => {
                let _ = updates.send(Update::SaveCopy { version, not_saved, bytes });
            }
            Ok(Event::Error { message, .. }) => {
                let _ = updates.send(Update::Lost(message));
                return;
            }
            Ok(_) => {}
            Err(error) => {
                // A read timeout is the normal way round this loop; a closed socket is not.
                let text = error.to_string();
                if !text.contains("timed out") && !text.contains("WouldBlock") {
                    let _ = updates.send(Update::Lost(text));
                    return;
                }
            }
        }
    }
}

impl crate::CadApp {
    /// Attach a shared session; commands then go to the server instead of the local drawing.
    pub fn with_collab(mut self, collab: Collab) -> Self {
        self.collab = Some(collab);
        self
    }

    /// Attach a shared session that arrived after the app was built (the browser's
    /// `cadcraft:session`, which the host sends after the first frame).
    pub fn attach_collab(&mut self, collab: Collab) {
        // A session joins on its own frames: its drawing has not arrived yet, and the window may
        // still open the host's document until it does.
        self.collab_joined = false;
        self.collab = Some(collab);
    }

    /// Whether this drawing is being edited together.
    pub fn is_shared(&self) -> bool {
        self.collab.is_some()
    }

    /// The sync state the cabinet's indicator shows, while a session is attached.
    pub fn collab_status(&self) -> Option<SyncStatus> {
        self.collab.as_ref().map(Collab::status)
    }

    /// Who is in the session, in the host bridge's shape: this participant (once the welcome names
    /// their id) and everyone whose presence arrived. Each role is the server's; the colours match
    /// the cursors on the canvas.
    pub fn session_participants(&self) -> Vec<SessionParticipant> {
        let Some(collab) = &self.collab else { return Vec::new() };
        let mut people = Vec::with_capacity(self.remote_presence.len() + 1);
        if let Some(id) = collab.client_id() {
            people.push(SessionParticipant {
                color: Some(crate::canvas::presence_color_hex(&id)),
                client_id: id,
                role: role_tag(collab.role()).to_string(),
            });
        }
        for presence in self.remote_presence.values() {
            people.push(SessionParticipant {
                color: Some(crate::canvas::presence_color_hex(&presence.actor)),
                client_id: presence.actor.clone(),
                // The server fills the role; a presence from a server that predates roles cannot
                // claim one, and the list must not offer an edit the server would refuse.
                role: role_tag(presence.role.unwrap_or(Role::Viewer)).to_string(),
            });
        }
        people.sort_by(|a, b| a.client_id.cmp(&b.client_id));
        people.dedup_by(|a, b| a.client_id == b.client_id);
        people
    }

    /// Apply what the shared session sent and report where this participant is looking.
    pub fn collab_tick(&mut self, ctx: &egui::Context) {
        if self.collab.is_some() && self.show_participants {
            self.participants_window(ctx);
        }
        let updates = match &self.collab {
            Some(collab) => collab.poll(),
            None => return,
        };
        if !updates.is_empty() {
            ctx.request_repaint();
        }
        for update in updates {
            match update {
                Update::Joined { drawing, version } => {
                    self.collab_version = version;
                    self.collab_joined = true;
                    if let Err(error) = self.session.adopt_shared_drawing(*drawing) {
                        self.session.echo(format!("shared session: {error}"));
                    } else {
                        self.session.echo(format!("shared session: the room's drawing, version {version}"));
                    }
                }
                Update::Applied(applied) => {
                    self.collab_version = applied.version;
                    if let Err(error) = self.session.apply_remote(&applied.changes) {
                        self.session.echo(format!("shared session: {error}"));
                    }
                }
                Update::Rejected(rejected) => self.session.echo(format!("shared session: {}", rejected.message)),
                Update::Presence(presence) => {
                    self.remote_presence.insert(presence.actor.clone(), presence);
                }
                Update::Committed { version, not_saved, bytes, cabinet } => {
                    self.last_collab_commit = Some(CommitState { version, cabinet: cabinet.clone(), not_saved: not_saved.clone() });
                    self.collab_conflict = None;
                    match cabinet {
                        // The cabinet owns the file: writing it here too would create a second
                        // copy of the drawing, so a hosted session only reports where it went.
                        Some(cabinet) => self.session.echo(format!(
                            "shared session: the cabinet stored version {version} as file version {} (etag {})",
                            cabinet.version, cabinet.etag
                        )),
                        None => {
                            // A room without the cabinet's callback answers with the file: the
                            // hosting page stores it (`cadcraft:commit`) and confirms with
                            // `cadcraft:committed`; the desktop writes it beside its drawing.
                            let stored = self.host_commit.as_ref().map(|commit| commit(Some(&bytes)));
                            match stored {
                                Some(Ok(())) => self
                                    .session
                                    .echo(format!("shared session: asked the host to store version {version}; the cabinet answers with its version")),
                                Some(Err(message)) => self.session.echo(format!("shared session: the host refused version {version}: {message}")),
                                None => {
                                    let path = self.session.state().ok().and_then(|state| state.path.clone());
                                    let path = path.unwrap_or_else(|| format!("cadcraft-shared-{version}.dxf"));
                                    let temporary = format!("{path}.tmp");
                                    match std::fs::write(&temporary, &bytes).and_then(|()| std::fs::rename(&temporary, &path)) {
                                        Ok(()) => self.session.echo(format!("shared session: wrote {path} (version {version})")),
                                        Err(error) => self.session.echo(format!("shared session: could not write {path}: {error}")),
                                    }
                                }
                            }
                        }
                    }
                    if !not_saved.is_empty() {
                        self.session.echo(format!("the format did not keep: {not_saved:?}"));
                    }
                }
                Update::CommitConflict { version, message } => {
                    self.collab_conflict = Some(message.clone());
                    self.session.echo(format!("shared session: the cabinet refused version {version}: {message}"));
                }
                Update::SaveCopy { version, not_saved, bytes } => {
                    self.save_room_copy(version, &bytes, &not_saved);
                }
                Update::Lost(reason) => {
                    self.session.echo(format!("shared session ended: {reason}"));
                    self.remote_presence.clear();
                    self.show_participants = false;
                    self.collab = None;
                    self.collab_conflict = None;
                }
            }
        }
        // A viewer still reports where it looks (ADR-0006): only operations are refused, in
        // `collab_command`/`collab_cmdline` and by the server.
        let now = web_time::Instant::now();
        if self.last_presence.is_none_or(|last| now.duration_since(last) >= std::time::Duration::from_millis(100)) {
            self.last_presence = Some(now);
            let presence = Presence {
                actor: String::new(),
                // The server names the role from the token; a client must not claim one.
                role: None,
                space: self.session.state().map(|state| state.space.clone()).unwrap_or_default(),
                cursor: Some([self.session.cursor.x, self.session.cursor.y]),
                selection: self.session.selection(),
                layer: self.session.doc().map(|drawing| drawing.header.str("CLAYER", "0")).unwrap_or_else(|_| "0".into()),
                view: None,
            };
            if let Some(collab) = &self.collab {
                collab.send(Request::Presence(presence));
            }
        }
        // A shared session needs frames even when nobody touches the window: the replica's snapshot
        // arrives in bounded slices (one part or one batch per frame), presence is paced by frames
        // (ADR-0006) and a closed socket schedules its reconnect from a callback. Without this an
        // idle tab stalls the join at `pending` and never reconnects. A settled session ticks at
        // the presence pace; one that is still arriving repaints at once.
        if let Some(collab) = &self.collab {
            if collab.status() == SyncStatus::Synced {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            } else {
                // A bounded wait, not a repaint storm: an offline session must not spin the page.
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
    }

    /// Open the drawing the hosting page sent with the session (`cadcraft:open`, INTEGRATION.md
    /// §5.1): the cabinet's bytes are how the file reaches the browser, so the open runs here even
    /// though the window is in a shared session.
    ///
    /// The room is authoritative once its drawing has joined: a document that arrives after the
    /// join would fork the drawing this window shows and is refused, because the cabinet reloads
    /// the page to open another file.
    pub fn open_host_bytes(&mut self, name: &str, bytes: &[u8]) {
        if self.collab_joined {
            self.open_failed(format!(
                "Open: {name} was not opened: this window shows the shared session's drawing; the cabinet reloads the editor to open another file"
            ));
            return;
        }
        if let Some(e) = Self::open_size_error(name, bytes.len() as u64) {
            return self.open_failed(e);
        }
        let data = cadcraft_engine::cmd::file::base64_encode(bytes);
        let r = self.execute_here("open", &serde_json::json!({ "data": data, "name": name }));
        self.opened(r);
    }

    /// Send a command to the shared session instead of running it locally.
    ///
    /// `None` when this drawing is not shared. View and settings commands never reach here: the UI
    /// command catalog handles them first, so they stay local.
    pub fn collab_command(&mut self, id: &str, params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
        let collab = self.collab.as_ref()?;
        if !collab.role().may_write() {
            return Some(Err("this shared session is read-only".into()));
        }
        if matches!(id, "u" | "undo") {
            let op_id = format!("{:016x}-undo-{}", self.collab_run, self.collab_seq);
            self.collab_seq = self.collab_seq.saturating_add(1);
            collab.send(Request::Undo(op_id));
            return Some(Ok(serde_json::json!({ "queued": true })));
        }
        if id == "redo" || id == "mr" {
            let op_id = format!("{:016x}-redo-{}", self.collab_run, self.collab_seq);
            self.collab_seq = self.collab_seq.saturating_add(1);
            collab.send(Request::Redo(op_id));
            return Some(Ok(serde_json::json!({ "queued": true })));
        }
        if matches!(id, "save" | "qsave" | "saveas") {
            // The room builds the file and publishes it to the cabinet (M87): it is the only
            // participant that can write the authoritative drawing, and the cabinet answers with
            // its file version. Only a room that cannot publish it itself hands the bytes back,
            // and then the page's cabinet stores them (`cadcraft:commit`).
            collab.send(Request::Commit(self.collab_version));
            return Some(Ok(serde_json::json!({ "queued": true })));
        }
        let (space, selection) = match self.session.state() {
            Ok(state) => (state.space.clone(), state.selection.clone()),
            Err(error) => return Some(Err(error.to_string())),
        };
        // The author's layer travels with the operation: the server has no window to take it from,
        // and an object it creates must not land on the server drawing's own current layer.
        let clay = self.session.doc().ok().map(|drawing| drawing.header.str("CLAYER", "0"));
        let kind = if is_structural(id) { cadcraft_collab::OpKind::Structural } else { cadcraft_collab::OpKind::Command };
        let op = Op {
            op_id: format!("{:016x}-{}", self.collab_run, self.collab_seq),
            actor: String::new(),
            base_version: self.collab_version,
            kind,
            command: id.to_string(),
            params: params.clone(),
            context: cadcraft_collab::OpContext { space, selection: Some(selection), clay },
        };
        self.collab_seq = self.collab_seq.saturating_add(1);
        collab.send(Request::Op(op));
        Some(Ok(serde_json::json!({ "queued": true })))
    }

    /// Submit a complete command line to the shared session.
    ///
    /// A shared session has no interactive prompt, so the line must name everything it acts on:
    /// points by coordinates and objects by handle (see [`cadcraft_collab::cmdline`]). A line the
    /// parser refuses never becomes an operation — the drawing cannot change — and the refusal is
    /// echoed; it is also returned, as a later operation's rejection would be.
    pub fn collab_cmdline(&mut self, text: &str) -> Option<String> {
        let role = self.collab.as_ref().map(Collab::role)?;
        if !role.may_write() {
            let message = "this shared session is read-only".to_string();
            self.session.echo(message.clone());
            return Some(message);
        }
        let op = {
            let state = match self.session.state() {
                Ok(state) => state,
                Err(error) => {
                    let message = error.to_string();
                    self.session.echo(message.clone());
                    return Some(message);
                }
            };
            let op_id = format!("{:016x}-{}", self.collab_run, self.collab_seq);
            cadcraft_collab::cmdline::op_for(text, &state.doc, &state.space, &op_id, self.collab_version)
        };
        match op {
            Ok(op) => {
                if let Some(collab) = &self.collab {
                    collab.send(Request::Op(op));
                }
                self.collab_seq = self.collab_seq.saturating_add(1);
                None
            }
            Err(error) => {
                let message = error.to_string();
                self.session.echo(message.clone());
                Some(message)
            }
        }
    }

    /// Show who is in the shared session, with each participant's place in the drawing.
    fn participants_window(&mut self, ctx: &egui::Context) {
        let Some(collab) = &self.collab else {
            self.show_participants = false;
            return;
        };
        let role = collab.role();
        let mut open = self.show_participants;
        let mut leave = false;
        let mut save_copy = false;
        let conflict = self.collab_conflict.clone();
        let last_commit = self.last_collab_commit.clone();
        let people: Vec<(String, String)> = self
            .remote_presence
            .values()
            .map(|presence| {
                let space = match &presence.space {
                    cadcraft_doc::Space::Model => crate::i18n::t("model space").to_string(),
                    cadcraft_doc::Space::Paper(name) => name.clone(),
                };
                let layer = if presence.layer.is_empty() { crate::i18n::t("no active layer").to_string() } else { presence.layer.clone() };
                (presence.actor.clone(), format!("{space} · {layer}"))
            })
            .collect();
        egui::Window::new(crate::i18n::t("Shared session")).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(format!("{}: {}", crate::i18n::t("You"), role_name(role)));
            ui.separator();
            if people.is_empty() {
                ui.label(crate::i18n::t("No other participants"));
            }
            for (actor, place) in &people {
                ui.label(format!("{actor} — {place}"));
            }
            if let Some(commit) = &last_commit {
                ui.separator();
                match &commit.cabinet {
                    Some(cabinet) => {
                        ui.label(format!(
                            "{} {} · {} {}",
                            crate::i18n::t("Stored by the cabinet, drawing version"),
                            commit.version,
                            crate::i18n::t("file version"),
                            cabinet.version
                        ));
                        ui.label(format!("etag {}", cabinet.etag));
                    }
                    None => {
                        ui.label(format!("{} {}", crate::i18n::t("Committed drawing version"), commit.version));
                    }
                }
                if !commit.not_saved.is_empty() {
                    ui.colored_label(ui.visuals().warn_fg_color, format!("{} {:?}", crate::i18n::t("The format did not keep"), commit.not_saved));
                }
            }
            if let Some(conflict) = &conflict {
                ui.separator();
                ui.colored_label(ui.visuals().error_fg_color, conflict);
                if ui.button(crate::i18n::t("Save a copy...")).clicked() {
                    save_copy = true;
                }
            }
            ui.separator();
            if ui.button(crate::i18n::t("Leave session")).clicked() {
                leave = true;
            }
        });
        self.show_participants = open;
        if save_copy && let Some(collab) = &self.collab {
            collab.send(Request::SaveCopy);
            self.session.echo("shared session: preparing a copy of the room's file");
        }
        if leave {
            self.leave_shared();
        }
    }

    /// The name a copy of the room's file gets: the drawing's name with `(copy)` before its
    /// extension.
    fn room_copy_name(&self) -> String {
        let name = self
            .session
            .state()
            .ok()
            .and_then(|state| state.path.clone())
            .or_else(|| self.session.state().ok().map(|state| state.title.clone()))
            .unwrap_or_else(|| "cadcraft-shared.dxf".to_string());
        match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => format!("{stem} (copy).{ext}"),
            _ => format!("{name} (copy)"),
        }
    }

    /// Save the file of a commit the cabinet refused (`409`) as a copy (TASK §6).
    ///
    /// The cabinet is not written and the drawing is not marked saved: this is only the owner's
    /// work under a name they choose.
    fn save_room_copy(&mut self, version: u64, bytes: &[u8], not_saved: &std::collections::BTreeMap<String, usize>) {
        let name = self.room_copy_name();
        #[cfg(not(target_arch = "wasm32"))]
        match self.services.pick_save.as_ref().and_then(|pick| pick(&name)) {
            Some(path) => match std::fs::write(&path, bytes) {
                Ok(()) => self.session.echo(format!("shared session: saved a copy as {path} (version {version})")),
                Err(error) => self.session.echo(format!("shared session: could not save a copy as {path}: {error}")),
            },
            None => self.session.echo(format!("shared session: did not save a copy of version {version}")),
        }
        #[cfg(target_arch = "wasm32")]
        match cadcraft_engine::cmd::file::deliver() {
            Some(deliver) => match deliver(&name, "application/octet-stream", bytes) {
                Ok(()) => self.session.echo(format!("shared session: downloaded a copy as {name} (version {version})")),
                Err(error) => self.session.echo(format!("shared session: could not download a copy as {name}: {error}")),
            },
            None => self.session.echo(format!("shared session: this build cannot save a copy of version {version}")),
        }
        if !not_saved.is_empty() {
            self.session.echo(format!("the format did not keep: {not_saved:?}"));
        }
    }

    /// Disconnect from the shared session and go back to the local drawing.
    pub fn leave_shared(&mut self) {
        if self.collab.take().is_some() {
            self.remote_presence.clear();
            self.show_participants = false;
            self.collab_conflict = None;
            self.session.echo("shared session: left");
        }
    }

    /// The host's cabinet stored the shared session (`cadcraft:committed`).
    ///
    /// `version` is the cabinet's file version and `etag` names that stored content. The drawing
    /// is not written locally: the cabinet owns the file (INTEGRATION.md §5.2).
    pub fn host_commit_result(&mut self, version: u64, etag: String) {
        self.host_committed = Some((version, etag.clone()));
        self.last_collab_commit = Some(CommitState {
            version: self.collab_version,
            cabinet: Some(CabinetCommit { version, etag: etag.clone() }),
            not_saved: std::collections::BTreeMap::new(),
        });
        self.collab_conflict = None;
        self.session.echo(format!("shared session: the cabinet stored file version {version} (etag {etag})"));
    }
}

/// The participant's role in words.
fn role_name(role: Role) -> &'static str {
    match role {
        Role::Owner => crate::i18n::t("owner"),
        Role::Editor => crate::i18n::t("editor"),
        Role::Viewer => crate::i18n::t("viewer"),
    }
}

/// The server's word for the role, as the host contract names it.
fn role_tag(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Editor => "editor",
        Role::Viewer => "viewer",
    }
}

/// UI-only commands of a shared session: the participant list and leaving it.
pub fn route(app: &mut crate::CadApp, id: &str, _params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    match id {
        "ui.collab.participants" => {
            if app.collab.is_none() {
                return Some(Err("this drawing is not shared".into()));
            }
            app.show_participants = !app.show_participants;
            Some(Ok(serde_json::json!({ "participants": app.show_participants })))
        }
        "ui.collab.save_copy" => {
            if app.collab.is_none() {
                return Some(Err("this drawing is not shared".into()));
            }
            if app.collab_conflict.is_none() {
                return Some(Err("there is no refused commit to save".into()));
            }
            if let Some(collab) = &app.collab {
                collab.send(Request::SaveCopy);
            }
            Some(Ok(serde_json::json!({ "queued": true })))
        }
        "ui.collab.leave" => {
            if app.collab.is_none() {
                return Some(Err("this drawing is not shared".into()));
            }
            app.leave_shared();
            Some(Ok(serde_json::json!({ "left": true })))
        }
        _ => None,
    }
}

/// Commands whose result depends on other objects: the server runs them in its order (ADR-0004).
pub fn is_structural(command: &str) -> bool {
    matches!(command, "laydel" | "laymrg" | "rename" | "layout.delete" | "layout.rename" | "layout.copy" | "block.delete" | "group.delete")
}

#[cfg(test)]
mod tests {
    use super::{CabinetCommit, Collab, Request, Role, Update, is_structural};
    use serde_json::json;

    /// The copy of a refused commit is named after the drawing, not over its file: an extension
    /// stays and `(copy)` goes before it.
    #[test]
    fn a_room_copy_is_named_after_the_drawing() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        assert_eq!(app.room_copy_name(), "Drawing1 (copy)");
        app.session.state_mut().unwrap().path = Some("/tmp/plan.dxf".into());
        assert_eq!(app.room_copy_name(), "/tmp/plan (copy).dxf");
        app.session.state_mut().unwrap().path = None;
        app.session.state_mut().unwrap().title = "Drawing2".into();
        assert_eq!(app.room_copy_name(), "Drawing2 (copy)");
    }

    #[test]
    fn cascades_are_structural_and_plain_edits_are_not() {
        for command in ["laydel", "laymrg", "rename", "layout.delete", "layout.rename", "layout.copy"] {
            assert!(is_structural(command), "`{command}` changes other objects too");
        }
        for command in ["line", "move", "copy", "layer.set", "text.edit", "hatch"] {
            assert!(!is_structural(command), "`{command}` is an ordinary edit");
        }
    }

    /// Which drawing the window shows is not part of the drawing: a shared session switches
    /// locally instead of queueing `document.switch` as a room operation (CodeRabbit on #2938).
    /// An edit still travels as an operation, so the test can see the difference.
    #[test]
    fn a_shared_session_switches_drawings_without_an_operation() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        app.run("new", json!({})).expect("a second drawing");
        let (collab, requests, _updates) = Collab::detached(Role::Editor);
        app.attach_collab(collab);

        app.run("document.switch", json!({ "index": 0 })).expect("the switch runs in this window");
        assert_eq!(app.session.active, 0, "the tab really changed");
        assert!(requests.try_recv().is_err(), "a drawing switch is not a room operation");

        app.run("line", json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).expect("an edit is queued");
        match requests.try_recv().expect("the edit reached the room") {
            Request::Op(op) => assert_eq!(op.command, "line"),
            _ => panic!("an edit travels as an operation"),
        }
    }

    /// A viewer switches drawings too: the read-only rule covers edits, not which tab is shown.
    #[test]
    fn a_viewer_switches_drawings_locally() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        app.run("new", json!({})).expect("a second drawing");
        let (collab, requests, _updates) = Collab::detached(Role::Viewer);
        app.attach_collab(collab);

        app.run("document.switch", json!({ "index": 0 })).expect("the switch runs in this window");
        assert_eq!(app.session.active, 0, "the tab really changed");
        assert!(requests.try_recv().is_err(), "a drawing switch is not a room operation");
        assert!(app.run("line", json!({ "points": [[0.0, 0.0], [10.0, 0.0]] })).is_err(), "a viewer still cannot edit");
    }

    /// The host's document is the session's document (INTEGRATION.md §5.1): opening it runs in
    /// the window, never as a room operation the room refuses. A save asks the room, which
    /// publishes the file.
    #[test]
    fn a_host_document_and_a_save_are_not_ordinary_room_operations() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        let (collab, requests, _updates) = Collab::detached(Role::Owner);
        app.attach_collab(collab);

        // Junk cannot open, but it must be refused by the local open rather than queued as an
        // operation named `open` (the room refuses those).
        app.open_host_bytes("acceptance.dxf", b"not a drawing");
        assert!(requests.try_recv().is_err(), "opening the session's document is not a room operation");

        app.collab_version = 3;
        app.run("save", json!({})).expect("save is queued");
        match requests.try_recv().expect("the room writes the file") {
            Request::Commit(version) => assert_eq!(version, 3, "the version this window shows"),
            _ => panic!("a save asks the room to commit"),
        }
    }

    /// Once the room's drawing joined, a late host document is refused instead of forking the
    /// drawing the window shows: the cabinet reloads the editor to open another file.
    #[test]
    fn a_host_document_after_the_join_does_not_fork_the_room_drawing() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        let (collab, requests, _updates) = Collab::detached(Role::Editor);
        app.attach_collab(collab);
        app.collab_joined = true;
        let before = app.session.docs.len();

        app.open_host_bytes("other.dxf", b"0\nEOF\n");

        assert_eq!(app.session.docs.len(), before, "no second drawing was opened");
        assert!(requests.try_recv().is_err(), "nothing was queued either");
    }

    /// The room's drawing replaces the window's document on a join: a second participant sees the
    /// edits made before it arrived, and its operations build on the room's version.
    #[test]
    fn a_join_shows_the_room_drawing_at_the_rooms_version() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        let (collab, _requests, updates) = Collab::detached(Role::Editor);
        app.attach_collab(collab);
        assert_eq!(app.session.state().expect("a drawing").doc.entity_count(), 0);

        let mut room = cadcraft_doc::Drawing::new_metric();
        for x in [0.0, 10.0, 20.0] {
            room.add(
                &cadcraft_doc::Space::Model,
                cadcraft_doc::Common::default(),
                cadcraft_doc::EntityKind::Line(cadcraft_doc::Line {
                    a: cadcraft_doc::geom::Vec3::new(x, 0.0, 0.0),
                    b: cadcraft_doc::geom::Vec3::new(x, 5.0, 0.0),
                }),
            )
            .expect("a line");
        }
        updates.send(Update::Joined { drawing: Box::new(room), version: 12 }).expect("an update");

        app.collab_tick(&egui::Context::default());

        assert_eq!(app.session.state().expect("a drawing").doc.entity_count(), 3, "the room's three lines");
        assert_eq!(app.collab_version, 12, "operations build on the room's version");
        assert!(app.collab_joined);
    }

    /// Operation ids are the room's repeat key (ADR 0003): two windows must not both send
    /// `local-0`, or the room answers the second with the first one's change set and the edit is
    /// never applied.
    #[test]
    fn two_windows_do_not_share_operation_ids() {
        let one = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        let two = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        assert_ne!(one.collab_run, two.collab_run, "each window has its own run id");
        assert_ne!(super::run_id(), super::run_id(), "a later window in the same process differs too");
    }

    /// `cadcraft:committed` from a hosting page lands in the panel as the cabinet's file version,
    /// with no local write: the cabinet owns the file (INTEGRATION.md §5.2).
    #[test]
    fn a_host_commit_records_the_cabinets_version_and_etag() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        app.collab_version = 4;
        app.collab_conflict = Some("the cabinet refused version 4".into());
        app.host_commit_result(7, "\"e7\"".into());
        assert_eq!(app.host_committed, Some((7, "\"e7\"".to_string())));
        let commit = app.last_collab_commit.as_ref().expect("the panel shows the commit");
        assert_eq!(commit.version, 4, "the drawing version this app shows");
        assert_eq!(commit.cabinet, Some(CabinetCommit { version: 7, etag: "\"e7\"".into() }));
        assert!(commit.not_saved.is_empty());
        assert!(app.collab_conflict.is_none(), "a stored commit clears the refusal");
    }
}
