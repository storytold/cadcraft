//! The host's collaboration messages, in a form the shell and its tests share.
//!
//! The page hosting CADCraft (`?host=parent` or `data-host="page"`) sends `cadcraft:session`
//! with the room, the socket URL, the token and the role, and answers `cadcraft:commit` with
//! `cadcraft:committed`. This module holds that exchange without a browser, so the state machine
//! is tested natively and the wasm shell only carries the values across.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::fmt;

use cadcraft_collab::Role;
use cadcraft_ui_egui::collab::SessionParticipant;

/// The session the host opened. A credential-bearing value: its `Debug` prints the token as
/// `<redacted>`, and it is never put in a URL, a log or storage.
#[derive(Clone, PartialEq, Eq)]
pub struct HostSession {
    session_id: String,
    ws_url: String,
    token: String,
    role: Role,
}

impl HostSession {
    /// Build a session from the host's fields. `role` is the host's word for it; an unknown one
    /// leaves the app single-user instead of guessing a permission.
    pub fn new(session_id: impl Into<String>, ws_url: impl Into<String>, token: impl Into<String>, role: &str) -> Option<HostSession> {
        Some(HostSession { session_id: session_id.into(), ws_url: ws_url.into(), token: token.into(), role: role_from_host(role)? })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The sync server's WebSocket URL. The token is not part of it.
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for HostSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostSession")
            .field("session_id", &self.session_id)
            .field("ws_url", &self.ws_url)
            .field("token", &"<redacted>")
            .field("role", &self.role)
            .finish()
    }
}

/// The host's word for the role (`owner`, `editor`, `viewer`), or `None` for one CADCraft does
/// not know.
pub fn role_from_host(role: &str) -> Option<Role> {
    match role {
        "owner" => Some(Role::Owner),
        "editor" => Some(Role::Editor),
        "viewer" => Some(Role::Viewer),
        _ => None,
    }
}

/// What the host bridge delivered to the app.
#[derive(Clone, Debug)]
pub enum HostEvent {
    /// `cadcraft:session` (host → app): open a collaboration session.
    Session(HostSession),
    /// `cadcraft:committed` (host → app): the cabinet stored the commit.
    Committed { version: u64, etag: String },
}

/// What the shell does after one host event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostAction {
    /// Dial the session; the host opened it, so the app owns no network call before this point.
    Connect(HostSession),
    /// The cabinet stored the commit; show its version and etag.
    Committed { version: u64, etag: String },
}

/// The host-side collaboration state the shell keeps between frames.
#[derive(Default, Debug)]
pub struct HostState {
    /// The session the host opened, if any. Without it the app stays single-user and makes no
    /// network call.
    pub session: Option<HostSession>,
    /// The last commit the cabinet confirmed.
    pub committed: Option<(u64, String)>,
    /// Whether the socket has joined the room at least once.
    pub connected: bool,
}

impl HostState {
    /// Apply one host message.
    pub fn on_event(&mut self, event: HostEvent) -> HostAction {
        match event {
            HostEvent::Session(session) => {
                self.connected = false;
                self.session = Some(session.clone());
                HostAction::Connect(session)
            }
            HostEvent::Committed { version, etag } => {
                self.committed = Some((version, etag.clone()));
                HostAction::Committed { version, etag }
            }
        }
    }
}

/// Decides which host messages the cabinet needs: `session-state` when the status or the
/// participant list changes, `presence` at the sync server's pace (10 Hz) and only when the list
/// changed. Kept without a browser so the pacing is tested natively.
#[derive(Debug, Default)]
pub struct HostPublisher {
    /// The last `(status, participants)` a `session-state` carried.
    last_state: Option<(String, Vec<SessionParticipant>)>,
    /// The participant list a `presence` last carried, and when.
    last_presence: Option<Vec<SessionParticipant>>,
    last_presence_ms: Option<f64>,
}

impl HostPublisher {
    /// The `session-state` to post, if the status or the participants changed since the last one.
    pub fn session_state(&mut self, status: &str, participants: &[SessionParticipant]) -> Option<(String, Vec<SessionParticipant>)> {
        let signature = (status.to_string(), participants.to_vec());
        if self.last_state.as_ref() == Some(&signature) {
            return None;
        }
        self.last_state = Some(signature.clone());
        Some(signature)
    }

    /// The participant list to post as `presence`, if it changed since the last post and at least
    /// `every_ms` have passed (the host contract asks for a few times a second at most).
    pub fn presence(&mut self, now_ms: f64, participants: &[SessionParticipant], every_ms: f64) -> Option<Vec<SessionParticipant>> {
        if self.last_presence.as_deref() == Some(participants) {
            return None;
        }
        if let Some(last) = self.last_presence_ms
            && now_ms - last < every_ms
        {
            return None;
        }
        self.last_presence_ms = Some(now_ms);
        self.last_presence = Some(participants.to_vec());
        Some(participants.to_vec())
    }

    /// Forget what was sent: the next session starts with a fresh state.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether a `session-state` has been sent in this session.
    pub fn started(&self) -> bool {
        self.last_state.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A `cadcraft:session` message starts a connection, keeps the role the host named and never
    /// prints the token.
    #[test]
    fn a_session_message_starts_a_connection_and_keeps_the_token_out_of_debug() {
        let mut state = HostState::default();
        assert!(state.session.is_none());
        let session = HostSession::new("s1", "wss://collab.example/ws", "jwt-secret", "editor").expect("a known role");
        assert_eq!(state.on_event(HostEvent::Session(session.clone())), HostAction::Connect(session));
        assert!(state.session.is_some());
        let debug = format!("{:?}", state.session.as_ref().expect("the session is stored"));
        assert!(debug.contains("s1"), "{debug}");
        assert!(!debug.contains("jwt-secret"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }

    /// `cadcraft:committed` is recorded for the panel: the cabinet's file version and etag.
    #[test]
    fn a_commit_message_is_recorded_for_the_ui() {
        let mut state = HostState::default();
        let action = state.on_event(HostEvent::Committed { version: 7, etag: "\"e7\"".into() });
        assert_eq!(action, HostAction::Committed { version: 7, etag: "\"e7\"".into() });
        assert_eq!(state.committed, Some((7, "\"e7\"".to_string())));
    }

    /// An unknown role leaves the app single-user: a typo must not grant edit rights.
    #[test]
    fn an_unknown_role_is_refused_instead_of_guessed() {
        assert!(HostSession::new("s1", "wss://collab.example/ws", "jwt", "commenter").is_none());
        assert!(HostSession::new("s1", "wss://collab.example/ws", "jwt", "").is_none());
        assert_eq!(role_from_host("owner"), Some(Role::Owner));
        assert_eq!(role_from_host("viewer"), Some(Role::Viewer));
    }

    /// Without a session message the app stays single-user and commits nothing to a host.
    #[test]
    fn without_a_session_the_app_stays_single_user() {
        let state = HostState::default();
        assert!(state.session.is_none());
        assert!(state.committed.is_none());
    }

    fn people(ids: &[(&str, &str)]) -> Vec<SessionParticipant> {
        ids.iter().map(|(id, role)| SessionParticipant { client_id: (*id).to_string(), role: (*role).to_string(), color: None }).collect()
    }

    /// The cabinet hears the sync status and the participant list when they change, but not the
    /// same state twice: an idle session must not repaint the host every frame.
    #[test]
    fn session_state_is_sent_on_a_change_only() {
        let mut publisher = HostPublisher::default();
        let here = people(&[("a", "editor"), ("v", "viewer")]);
        assert_eq!(publisher.session_state("synced", &here), Some(("synced".to_string(), here.clone())));
        assert_eq!(publisher.session_state("synced", &here), None, "the same state is not repeated");
        assert_eq!(publisher.session_state("pending", &here), Some(("pending".to_string(), here.clone())));
        let fewer = people(&[("a", "editor")]);
        assert_eq!(publisher.session_state("synced", &fewer), Some(("synced".to_string(), fewer)));
        assert!(publisher.started());
    }

    /// Presence follows the list at the server's pace: a join or a role change is posted once, and
    /// a second change inside the window waits for the next one.
    #[test]
    fn presence_follows_the_list_at_the_presence_pace() {
        let mut publisher = HostPublisher::default();
        let one = people(&[("a", "editor")]);
        assert_eq!(publisher.presence(1_000.0, &one, 100.0), Some(one.clone()));
        assert_eq!(publisher.presence(1_050.0, &one, 100.0), None, "unchanged");
        let two = people(&[("a", "editor"), ("v", "viewer")]);
        assert_eq!(publisher.presence(1_060.0, &two, 100.0), None, "inside the window");
        // The change is still pending after the window: the next frame posts it.
        assert_eq!(publisher.presence(1_120.0, &two, 100.0), Some(two.clone()));
        assert_eq!(publisher.presence(1_180.0, &two, 100.0), None, "unchanged again");
    }

    /// Leaving the session resets the publisher, so the next one starts with its own state.
    #[test]
    fn leaving_resets_the_publisher() {
        let mut publisher = HostPublisher::default();
        let one = people(&[("a", "editor")]);
        assert!(publisher.session_state("synced", &one).is_some());
        publisher.reset();
        assert!(!publisher.started());
        assert_eq!(publisher.session_state("synced", &one), Some(("synced".to_string(), one)));
    }
}
