//! Who is looking where, kept for a short while and never journaled.
//!
//! Presence is ephemeral (ADR-0006): a board entry is replaced by the next frame, expires after
//! [`PRESENCE_TTL`] and is dropped when the connection closes. It holds no drawing content.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use cadcraft_collab::Presence;

use crate::policy::MAX_PARTICIPANTS;

/// How long a participant's presence is kept without an update.
pub const PRESENCE_TTL: Duration = Duration::from_secs(30);

/// The latest presence of each participant.
#[derive(Clone, Debug, Default)]
pub struct PresenceBoard {
    entries: BTreeMap<String, (Presence, Instant)>,
}

impl PresenceBoard {
    /// An empty board.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a participant's view, replacing the previous one.
    ///
    /// A new participant beyond [`MAX_PARTICIPANTS`] is refused: presence must not grow without a
    /// bound, and the room's limit is enforced here as well as at the connection.
    pub fn update(&mut self, presence: Presence, now: Instant) -> bool {
        if !self.entries.contains_key(&presence.actor) && self.entries.len() >= MAX_PARTICIPANTS {
            return false;
        }
        self.entries.insert(presence.actor.clone(), (presence, now));
        true
    }

    /// Forget a participant (a `leave` or a closed connection).
    pub fn remove(&mut self, actor: &str) {
        self.entries.remove(actor);
    }

    /// Drop participants whose presence is older than [`PRESENCE_TTL`]; returns who left.
    pub fn prune(&mut self, now: Instant) -> Vec<String> {
        let stale: Vec<String> =
            self.entries.iter().filter(|(_, (_, at))| now.duration_since(*at) > PRESENCE_TTL).map(|(actor, _)| actor.clone()).collect();
        for actor in &stale {
            self.entries.remove(actor);
        }
        stale
    }

    /// Every participant's latest presence, oldest update first.
    pub fn participants(&self) -> impl Iterator<Item = &Presence> {
        self.entries.values().map(|(presence, _)| presence)
    }

    /// How many participants the board holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no participant is present.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_collab::Presence;
    use cadcraft_doc::Space;

    fn presence(actor: &str) -> Presence {
        Presence { actor: actor.into(), space: Space::Model, cursor: Some([1.0, 2.0]), selection: vec![], layer: "0".into(), view: None }
    }

    #[test]
    fn stale_presence_expires() {
        let now = Instant::now();
        let mut board = PresenceBoard::new();
        assert!(board.update(presence("u1"), now));
        assert_eq!(board.len(), 1);
        assert!(board.prune(now + PRESENCE_TTL).is_empty(), "an entry exactly at the ttl is still fresh");
        assert_eq!(board.prune(now + PRESENCE_TTL + Duration::from_millis(1)), vec!["u1".to_string()]);
        assert!(board.is_empty());
    }

    #[test]
    fn an_update_replaces_the_previous_view() {
        let now = Instant::now();
        let mut board = PresenceBoard::new();
        board.update(presence("u1"), now);
        let mut moved = presence("u1");
        moved.cursor = Some([9.0, 9.0]);
        board.update(moved, now + Duration::from_secs(1));
        assert_eq!(board.len(), 1);
        assert_eq!(board.participants().next().and_then(|p| p.cursor), Some([9.0, 9.0]));
    }

    #[test]
    fn the_board_is_bounded() {
        let now = Instant::now();
        let mut board = PresenceBoard::new();
        for i in 0..MAX_PARTICIPANTS {
            assert!(board.update(presence(&format!("u{i}")), now));
        }
        assert!(!board.update(presence("one-too-many"), now));
        assert_eq!(board.len(), MAX_PARTICIPANTS);
    }

    #[test]
    fn a_leave_removes_the_participant() {
        let now = Instant::now();
        let mut board = PresenceBoard::new();
        board.update(presence("u1"), now);
        board.remove("u1");
        assert!(board.is_empty());
    }
}
