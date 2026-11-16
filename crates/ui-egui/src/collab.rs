//! A shared session's client connection, driven on its own thread.
//!
//! A frame never waits on a socket: the worker thread owns the [`cadcraft_collab_client::Session`],
//! sends queued operations and presence, and forwards what arrives as [`Update`] values. The UI
//! applies the change sets to its drawing and draws the presence it holds.

use cadcraft_collab::{Applied, Op, Presence, Reject, Role};
#[cfg(not(target_arch = "wasm32"))]
use cadcraft_collab_client::{Event, Session as Remote};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

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
    /// Fetch the room's drawing and the operations after `version`: a change the window could not
    /// apply leaves it behind the room, and the answer puts it back in step.
    Resync(u64),
}

/// What the worker reports to the UI.
pub enum Update {
    /// The room's drawing and version, at the moment the participant joined.
    Joined { drawing: Box<cadcraft_doc::Drawing>, version: u64 },
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
#[cfg(not(target_arch = "wasm32"))]
pub struct Collab {
    requests: Sender<Request>,
    updates: Receiver<Update>,
    role: Role,
}

/// A shared session in the browser build: the host page carries the transport (TASK §6 п.5), and
/// until it does, a window cannot open one — but the window still compiles and runs offline.
#[cfg(target_arch = "wasm32")]
pub struct Collab {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
impl Collab {
    /// A connection with no socket: a test drives the worker's two ends itself.
    pub(crate) fn detached(role: Role) -> (Self, std::sync::mpsc::Receiver<Request>, std::sync::mpsc::Sender<Update>) {
        let (request_tx, request_rx) = channel();
        let (update_tx, update_rx) = channel();
        (Collab { requests: request_tx, updates: update_rx, role }, request_rx, update_tx)
    }
}

#[cfg(target_arch = "wasm32")]
impl Collab {
    /// A browser window joins a shared session through its host page, which this build does not
    /// carry yet.
    pub fn connect(_url: &str, _token: &str, _doc: &str) -> Result<Self, String> {
        Err("a shared session needs the desktop app: this build has no collaboration transport yet".into())
    }

    /// What the participant may do.
    pub fn role(&self) -> Role {
        self.role
    }

    /// Nothing to send: no session is open.
    pub fn send(&self, _request: Request) {}

    /// Nothing arrives: no session is open.
    pub fn poll(&self) -> Vec<Update> {
        Vec::new()
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
    // The room's drawing is the session's: the window replaces its own document with it before
    // the first change set arrives, or every change would name objects it does not have.
    let _ = updates.send(Update::Joined { drawing: Box::new(session.drawing().clone()), version: session.version() });
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
            // `Idle` is the read timeout: the socket is fine, so go round again and drain the
            // queue. Every other outcome is either forwarded above or ignored.
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
                Update::Joined { drawing, version } => {
                    // The room's drawing replaces this window's: the participant joins the room's
                    // objects, not the empty file the window started with (TASK §6 п.5).
                    self.session.adopt_shared_drawing(*drawing);
                    self.collab_version = version;
                }
                Update::Applied(applied) => {
                    // The version is the room's answer for a drawing the window now holds: only a
                    // change that applied moves it. A change the drawing refused leaves the window
                    // on its own version and asks the room for the drawing it is missing, instead
                    // of dropping the edit for good (CodeRabbit on #2936).
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

/// The id an operation or an undo carries, unique across participants and across restarts.
///
/// The room deduplicates by id (ADR 0003) and keys its undo history by it: two participants — or
/// the same one after a reload — must not both send `local-0`. Every window starts its counter at
/// zero, so the id mixes the clock, a process-wide counter (two windows of one process always
/// differ) and per-instance noise (CodeRabbit on #2936).
pub fn run_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nth = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    nanos.rotate_left(17) ^ nth.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ instance_noise().rotate_left(40)
}

/// What separates two instances of one program from each other: an OS seed and the process id.
fn instance_noise() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::hash::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.finish()
}

/// A command line that carries all of its parameters: `line {"points": [[0, 0], [10, 0]]}`.
///
/// Such a line needs no prompt, so a shared session can run it as one operation; a line without
/// its objects (`line`) is the interactive prompt a shared session refuses (CodeRabbit on #2936).
pub fn explicit_command(text: &str) -> Option<(String, serde_json::Value)> {
    let line = text.trim_start();
    let first = line.split_whitespace().next()?;
    let rest = line.get(first.len()..)?.trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let params = serde_json::from_str(rest).ok()?;
    Some((first.to_ascii_lowercase(), params))
}

/// The commands that write the drawing's file: in a shared session they are the room's commit,
/// never a local save (CodeRabbit on #2936).
pub fn is_shared_save(id: &str) -> bool {
    matches!(id.to_ascii_lowercase().as_str(), "save" | "qsave" | "saveas" | "ui.saveas")
}

#[cfg(test)]
mod tests {
    use super::{Collab, Request, Role, Update, explicit_command, is_shared_save, is_structural, run_id};

    fn shared_app(role: Role) -> (crate::CadApp, std::sync::mpsc::Receiver<Request>, std::sync::mpsc::Sender<Update>) {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        app.session.new_drawing(false);
        let (collab, requests, updates) = Collab::detached(role);
        (app.with_collab(collab), requests, updates)
    }

    /// The room's drawing is not the window's: a join replaces it and takes the room's version, or
    /// the first change set names objects the window does not have (CodeRabbit on #2936).
    #[test]
    fn a_join_shows_the_room_drawing_at_the_rooms_version() {
        let (mut app, _requests, updates) = shared_app(Role::Editor);
        let mut room = cadcraft_doc::Drawing::new_metric();
        let handle = room
            .add(
                &cadcraft_doc::Space::Model,
                Default::default(),
                cadcraft_doc::EntityKind::Point(cadcraft_doc::Point { p: cadcraft_doc::geom::Vec3::ZERO, angle: 0.0 }),
            )
            .expect("a point");
        assert_eq!(app.session.doc().expect("a drawing").entity_count(), 0);

        updates.send(Update::Joined { drawing: Box::new(room), version: 7 }).expect("an update");
        app.collab_tick(&egui::Context::default());

        assert!(app.session.doc().expect("the room's drawing").entity(handle).is_some(), "the room's objects arrived");
        assert_eq!(app.collab_version, 7, "the window is at the room's version");
    }

    /// A change the window cannot apply does not move its version past the edit: it asks the room
    /// for the drawing it is missing instead of diverging silently (CodeRabbit on #2936).
    #[test]
    fn a_change_the_window_cannot_apply_asks_the_room_for_its_tail() {
        let (mut app, requests, updates) = shared_app(Role::Editor);
        app.collab_version = 4;
        let applied = cadcraft_collab::Applied {
            op_id: "op-1".into(),
            version: 5,
            actor: "u2".into(),
            changes: vec![cadcraft_collab::Change::EntityDelete { handle: cadcraft_doc::Handle(9_999) }],
        };

        updates.send(Update::Applied(applied)).expect("an update");
        app.collab_tick(&egui::Context::default());

        assert_eq!(app.collab_version, 4, "the refused change is not a version the window holds");
        assert!(matches!(requests.try_recv(), Ok(Request::Resync(4))), "the window asks for what it lost");
        assert!(app.is_shared(), "a change the drawing refused is not a lost connection");
    }

    /// A menu item or a shortcut for a save reaches the room, never the local engine: the room is
    /// the only participant that may write the authoritative drawing (CodeRabbit on #2936).
    #[test]
    fn a_shared_save_reaches_the_room() {
        let (mut app, requests, _updates) = shared_app(Role::Editor);
        app.collab_version = 3;

        app.run("qsave", serde_json::Value::Null).expect("a queued save");
        assert!(matches!(requests.try_recv(), Ok(Request::Commit(3))), "the save is the room's commit");

        app.start("qsave");
        assert!(matches!(requests.try_recv(), Ok(Request::Commit(3))), "a menu save is the room's commit too");

        app.start("line");
        assert!(requests.try_recv().is_err(), "a command with prompts is still refused");
    }

    /// A command line that names all of its parameters is a complete operation, not an interactive
    /// prompt the room cannot answer (CodeRabbit on #2936).
    #[test]
    fn a_parameterised_command_line_reaches_the_room() {
        let (mut app, requests, _updates) = shared_app(Role::Editor);

        assert!(app.cmdline("line {\"points\": [[0.0, 0.0], [10.0, 0.0]]}").is_none(), "the operation is queued, not refused");
        match requests.try_recv().expect("the edit reached the room") {
            Request::Op(op) => {
                assert_eq!(op.command, "line");
                assert_eq!(op.params, serde_json::json!({ "points": [[0.0, 0.0], [10.0, 0.0]] }), "the parameters travel as given");
            }
            _ => panic!("an explicit command is one operation"),
        }
        assert!(app.cmdline("line").is_some(), "a line with nothing filled in is still the refused prompt");
    }

    /// A shared session edits the room's drawing: opening another file would put the room's
    /// changes and a commit's bytes into a file that is not the room's (CodeRabbit on #2936).
    #[test]
    fn a_shared_session_refuses_to_open_another_file() {
        let (mut app, _requests, _updates) = shared_app(Role::Editor);
        let before = app.session.doc().expect("a drawing").clone();

        app.open_path(std::path::Path::new("/tmp/other.dxf"));

        assert_eq!(app.session.doc().expect("a drawing"), &before, "the open never ran");
        assert!(app.session.log.iter().any(|line| line.contains("leave the session")), "the refusal is shown");
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

    /// Every window starts its counter at zero: an id made of the counter alone would make the
    /// room answer a second participant's edit with the first one's result (CodeRabbit on #2936).
    #[test]
    fn two_windows_do_not_share_operation_ids() {
        let ids: Vec<u64> = (0..4).map(|_| run_id()).collect();
        assert!(ids.iter().all(|id| *id != 0), "{ids:?}");
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "the ids must differ: {ids:?}");
    }

    /// A line that names its own parameters is a complete operation; one that does not is the
    /// interactive prompt a shared session refuses (CodeRabbit on #2936).
    #[test]
    fn a_command_line_with_parameters_is_one_operation() {
        let (name, params) = explicit_command("LINE {\"points\": [[0.0, 0.0], [10.0, 0.0]]}").expect("a complete line");
        assert_eq!(name, "line");
        assert_eq!(params, serde_json::json!({ "points": [[0.0, 0.0], [10.0, 0.0]] }));
        assert!(explicit_command("line").is_none(), "no parameters: the interactive prompt");
        assert!(explicit_command("line 0,0 10,0").is_none(), "coordinates are typed into prompts, not JSON");
        assert!(explicit_command("line {not json}").is_none());
    }

    /// The file actions of a shared session are the room's commit: a menu or a shortcut must not
    /// reach the local engine's save (CodeRabbit on #2936).
    #[test]
    fn the_shared_file_actions_are_the_rooms() {
        for id in ["save", "qsave", "saveas", "ui.saveas", "QSAVE"] {
            assert!(is_shared_save(id), "`{id}` writes the file");
        }
        for id in ["ui.zoom.extents", "line", "export"] {
            assert!(!is_shared_save(id), "`{id}` is not the drawing's file");
        }
    }
}
