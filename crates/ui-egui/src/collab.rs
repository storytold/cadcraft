//! A shared session's client connection, driven on its own thread.
//!
//! A frame never waits on a socket: the worker thread owns the [`cadcraft_collab_client::Session`],
//! sends queued operations and presence, and forwards what arrives as [`Update`] values. The UI
//! applies the change sets to its drawing and draws the presence it holds.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use cadcraft_collab::{Applied, Op, Presence, Reject, Role};
use cadcraft_collab_client::{Event, Session as Remote};

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

    /// Whether this drawing is being edited together.
    pub fn is_shared(&self) -> bool {
        self.collab.is_some()
    }

    /// Apply what the shared session sent and report where this participant is looking.
    pub fn collab_tick(&mut self, ctx: &egui::Context) {
        if self.collab.is_some() && self.show_participants {
            self.participants_window(ctx);
        }
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
                    self.collab_version = applied.version;
                    if let Err(error) = self.session.apply_remote(&applied.changes) {
                        self.session.echo(format!("shared session: {error}"));
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
                    self.remote_presence.clear();
                    self.show_participants = false;
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
            let op_id = format!("local-undo-{}", self.collab_seq);
            self.collab_seq = self.collab_seq.saturating_add(1);
            collab.send(Request::Undo(op_id));
            return Some(Ok(serde_json::json!({ "queued": true })));
        }
        if id == "redo" || id == "mr" {
            let op_id = format!("local-redo-{}", self.collab_seq);
            self.collab_seq = self.collab_seq.saturating_add(1);
            collab.send(Request::Redo(op_id));
            return Some(Ok(serde_json::json!({ "queued": true })));
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
            op_id: format!("local-{}", self.collab_seq),
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

    /// Show who is in the shared session, with each participant's place in the drawing.
    fn participants_window(&mut self, ctx: &egui::Context) {
        let Some(collab) = &self.collab else {
            self.show_participants = false;
            return;
        };
        let role = collab.role();
        let mut open = self.show_participants;
        let mut leave = false;
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
            ui.separator();
            if ui.button(crate::i18n::t("Leave session")).clicked() {
                leave = true;
            }
        });
        self.show_participants = open;
        if leave {
            self.leave_shared();
        }
    }

    /// Disconnect from the shared session and go back to the local drawing.
    pub fn leave_shared(&mut self) {
        if self.collab.take().is_some() {
            self.remote_presence.clear();
            self.show_participants = false;
            self.session.echo("shared session: left");
        }
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
    use super::is_structural;

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
