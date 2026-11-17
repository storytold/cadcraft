//! A shared session's client connection, driven on its own thread.
//!
//! A frame never waits on a socket: the worker thread owns the [`cadcraft_collab_client::Session`],
//! sends queued operations and presence, and forwards what arrives as [`Update`] values. The UI
//! applies the change sets to its drawing and draws the presence it holds.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use cadcraft_collab::{Applied, Op, Presence, Reject, Role};
use cadcraft_collab_client::{Event, Session as Remote};

/// A 64-bit id for one window's operation ids.
///
/// The room answers a repeated `op_id` with the first answer and never applies it twice
/// (ADR 0003), and its window is keyed by the id alone: two participants — or the same one after
/// a reload — must not both send `local-0`. The id mixes the clock (milliseconds on the web), a
/// process-wide counter (two windows of one process always differ) and instance noise.
pub(crate) fn run_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nth = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    nanos.rotate_left(17) ^ nth.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ instance_noise().rotate_left(40)
}

/// What separates two pages of one process from each other. Native has an OS seed and a process
/// id; a browser page is one wasm instance per tab, where `Math.random` is the only source that
/// differs between two tabs opened in the same millisecond. `std::process::id` is not available
/// on wasm: calling it aborts the page.
#[cfg(target_arch = "wasm32")]
fn instance_noise() -> u64 {
    (js_sys::Math::random() * f64::from(u32::MAX)) as u64
}

#[cfg(not(target_arch = "wasm32"))]
fn instance_noise() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::hash::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.finish()
}

/// What the UI asks the worker to send.
pub enum Request {
    /// An operation.
    Op(Op),
    /// Where the participant is looking.
    Presence(Presence),
    /// Undo this participant's last operation.
    Undo(String),
    /// Write the file the cabinet stores, at this version.
    Commit(u64),
    /// Fetch the room's tail after `version`: a change set the window could not apply leaves it
    /// behind the room, and the answer puts it back in step.
    Resync(u64),
}

/// What the worker reports to the UI.
pub enum Update {
    /// The change set an operation produced.
    Applied(Applied),
    /// An operation was refused.
    Rejected(Reject),
    /// Another participant's view.
    Presence(Presence),
    /// The file a commit produced.
    Committed { version: u64, not_saved: std::collections::BTreeMap<String, usize>, bytes: Vec<u8> },
    /// The connection ended; the message says why.
    Lost(String),
}

/// A connection to a shared session.
pub struct Collab {
    requests: Sender<Request>,
    updates: Receiver<Update>,
    role: Role,
}

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
                Ok(Request::Commit(version)) => {
                    if session.commit(version).is_err() {
                        let _ = updates.send(Update::Lost("the connection closed".into()));
                        return;
                    }
                }
                Ok(Request::Resync(version)) => {
                    if session.resync(version).is_err() {
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
            Ok(Event::Rejected(rejected)) => {
                let _ = updates.send(Update::Rejected(rejected));
            }
            Ok(Event::Presence(presence)) => {
                let _ = updates.send(Update::Presence(presence));
            }
            Ok(Event::Committed { version, not_saved, bytes }) => {
                let _ = updates.send(Update::Committed { version, not_saved, bytes });
            }
            Ok(Event::Error { message, .. }) => {
                let _ = updates.send(Update::Lost(message));
                return;
            }
            // `Event::Idle` is the read timeout: the connection is fine, so round the loop again.
            Ok(_) => {}
            Err(error) => {
                let _ = updates.send(Update::Lost(error.to_string()));
                return;
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

    /// Whether this drawing is being edited together.
    pub fn is_shared(&self) -> bool {
        self.collab.is_some()
    }

    /// Apply what the shared session sent and report where this participant is looking.
    pub fn collab_tick(&mut self, ctx: &egui::Context) {
        let (updates, role) = match &self.collab {
            Some(collab) => (collab.poll(), collab.role()),
            None => return,
        };
        if !updates.is_empty() {
            ctx.request_repaint();
        }
        for update in updates {
            match update {
                Update::Applied(applied) => {
                    // The version is the room's answer for a drawing the window now holds: only a
                    // change that applied moves it. A refused change leaves the window on its own
                    // version and asks for the room's drawing instead of dropping the edit for
                    // good (CodeRabbit on #2937).
                    match self.session.apply_remote(&applied.changes) {
                        Ok(()) => self.collab_version = applied.version,
                        Err(error) => {
                            self.session.echo(format!("shared session: {error}"));
                            if let Some(collab) = &self.collab {
                                collab.send(Request::Resync(self.collab_version));
                            }
                        }
                    }
                }
                Update::Rejected(rejected) => self.session.echo(format!("shared session: {}", rejected.message)),
                Update::Presence(presence) => {
                    self.remote_presence.insert(presence.actor.clone(), presence);
                }
                Update::Committed { version, not_saved, bytes } => {
                    let path = self.session.state().ok().and_then(|state| state.path.clone());
                    let path = path.unwrap_or_else(|| format!("cadcraft-shared-{version}.dxf"));
                    let temporary = format!("{path}.tmp");
                    match std::fs::write(&temporary, &bytes).and_then(|()| std::fs::rename(&temporary, &path)) {
                        Ok(()) => {
                            self.session.echo(format!("shared session: wrote {path} (version {version})"));
                            if !not_saved.is_empty() {
                                self.session.echo(format!("the format did not keep: {not_saved:?}"));
                            }
                        }
                        Err(error) => self.session.echo(format!("shared session: could not write {path}: {error}")),
                    }
                }
                Update::Lost(reason) => {
                    self.session.echo(format!("shared session ended: {reason}"));
                    self.collab = None;
                    // The room is gone: a cursor it left on the canvas would stay there forever.
                    self.remote_presence.clear();
                }
            }
        }
        if !role.may_write() {
            return;
        }
        let now = web_time::Instant::now();
        if self.last_presence.is_none_or(|last| now.duration_since(last) >= std::time::Duration::from_millis(100)) {
            self.last_presence = Some(now);
            let presence = Presence {
                actor: String::new(),
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
            return Some(Err("redo is not available in a shared session yet".into()));
        }
        if matches!(id, "save" | "qsave" | "saveas") {
            let version = self.collab_version;
            collab.send(Request::Commit(version));
            return Some(Ok(serde_json::json!({ "queued": true })));
        }
        let (space, selection) = match self.session.state() {
            Ok(state) => (state.space.clone(), state.selection.clone()),
            Err(error) => return Some(Err(error.to_string())),
        };
        let kind = if is_structural(id) { cadcraft_collab::OpKind::Structural } else { cadcraft_collab::OpKind::Command };
        let op = Op {
            op_id: format!("{:016x}-{}", self.collab_run, self.collab_seq),
            actor: String::new(),
            base_version: self.collab_version,
            kind,
            command: id.to_string(),
            params: params.clone(),
            context: cadcraft_collab::OpContext { space, selection: Some(selection) },
        };
        self.collab_seq = self.collab_seq.saturating_add(1);
        collab.send(Request::Op(op));
        Some(Ok(serde_json::json!({ "queued": true })))
    }
}

/// Commands whose result depends on other objects: the server runs them in its order (ADR-0004).
pub fn is_structural(command: &str) -> bool {
    matches!(command, "laydel" | "laymrg" | "rename" | "layout.delete" | "layout.rename" | "layout.copy" | "block.delete" | "group.delete")
}

#[cfg(test)]
impl Collab {
    /// A session with no socket: requests land in a queue the test reads, and the test feeds the
    /// updates the worker would have sent.
    pub(crate) fn detached(role: Role) -> (Self, Receiver<Request>, Sender<Update>) {
        let (requests, request_rx) = channel();
        let (update_tx, updates) = channel();
        (Collab { requests, updates, role }, request_rx, update_tx)
    }
}

#[cfg(test)]
mod tests {
    use super::{Collab, Request, Update, is_structural, run_id};
    use cadcraft_collab::{Applied, Change, Presence, RecordId, RecordKind, Role};
    use cadcraft_doc::Space;

    /// Two windows, and two runs of one window, must not generate the same operation ids: the
    /// room's deduplication window is keyed by `op_id` (ADR 0003), so a shared counter alone would
    /// answer the second window with the first one's result (CodeRabbit on #2937).
    #[test]
    fn two_windows_do_not_share_operation_ids() {
        let one = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        let two = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        assert_ne!(one.collab_run, two.collab_run, "each window has its own run id");
        assert_ne!(run_id(), run_id(), "a later window in the same process differs too");
    }

    /// The room is gone: the cursors it left must not stay on the canvas (CodeRabbit on #2937).
    #[test]
    fn a_lost_session_leaves_no_stale_cursor() {
        let (collab, _requests, updates) = Collab::detached(Role::Editor);
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default()).with_collab(collab);
        app.remote_presence.insert(
            "u2".into(),
            Presence { actor: "u2".into(), space: Space::Model, cursor: Some([1.0, 2.0]), selection: Vec::new(), layer: "0".into(), view: None },
        );
        updates.send(Update::Lost("the room closed".into())).expect("the test is the worker");
        let ctx = egui::Context::default();
        app.collab_tick(&ctx);
        assert!(app.remote_presence.is_empty(), "a cursor of the ended session is not drawn");
        assert!(app.collab.is_none());
    }

    /// A change set the window cannot apply must not move its version past the edit: it asks the
    /// room for its drawing instead of dropping the edit for good (CodeRabbit on #2937).
    #[test]
    fn a_change_the_window_cannot_apply_asks_the_room_for_its_drawing() {
        let (collab, requests, updates) = Collab::detached(Role::Editor);
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default()).with_collab(collab);
        app.collab_version = 4;
        let refused = Applied {
            op_id: "r-1".into(),
            version: 5,
            actor: "u2".into(),
            changes: vec![Change::TableDelete { kind: RecordKind::Layer, id: RecordId(999), name: "missing".into() }],
        };
        updates.send(Update::Applied(refused)).expect("the test is the worker");
        let ctx = egui::Context::default();
        app.collab_tick(&ctx);
        assert_eq!(app.collab_version, 4, "the window still shows version 4");
        assert!(matches!(requests.try_recv(), Ok(Request::Resync(4))), "the room's drawing is asked for");
    }

    /// A view command is the window's own even in a shared session; a line the parser would turn
    /// into a prompt stays refused (CodeRabbit on #2937).
    #[test]
    fn a_view_command_reaches_the_window_in_a_shared_session() {
        let (collab, _requests, _updates) = Collab::detached(Role::Editor);
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default()).with_collab(collab);
        assert!(app.cmdline("find").is_none(), "`find` is not refused");
        assert!(app.ui.dialog.is_some(), "the window opened the Find dialog");
        assert!(app.cmdline("line").is_some(), "a command with prompts is still refused");
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
}
