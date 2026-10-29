//! The crash-recovery journal both backends share.
//!
//! Before a backend makes an interruptible write visible, it appends one JSON line naming the
//! staged bytes and the final name; after the commit it appends a matching `committed` line.
//! [`read`] + [`plan`] turn that log into work for [`StorageBackend::recover`][crate::StorageBackend::recover]:
//! entries with no matching commit are finished (the staged file is renamed into place) or
//! dropped (nothing staged survived). Phase 2 (version history) records its snapshots beside
//! these entries; Phase 3 (lease files) takes the same append-then-commit shape.

use serde::{Deserialize, Serialize};

/// One line of the journal: a write that started (`Pending`) or finished (`Committed`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Final backend-relative name the write targets.
    pub path: String,
    /// Staging name holding the bytes (backend-local: a sibling temp file, a WebDAV temp name).
    pub staged: String,
    /// Which half of the story this line tells.
    pub state: JournalState,
    /// Bytes the staged write holds; used to drop half-records that can't be completed.
    pub len: u64,
}

/// Which half of a journaled write a line records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JournalState {
    /// The staged bytes are complete; the commit may not have happened.
    Pending,
    /// The staged bytes were committed to `path`.
    Committed,
}

/// The journal file's name inside a backend root (local folder, WebDAV collection).
pub const JOURNAL_FILE: &str = ".cadcraft-journal.log";

/// Read every well-formed line of `bytes` (a journal file); corrupt lines are skipped, never
/// fatal — a torn final line is the normal residue of a crash.
pub fn read(bytes: &[u8]) -> Vec<JournalEntry> {
    let text = String::from_utf8_lossy(bytes);
    text.lines().filter_map(|line| serde_json::from_str::<JournalEntry>(line.trim()).ok()).collect()
}

/// What [`StorageBackend::recover`][crate::StorageBackend::recover] should do, derived from the
/// journal lines alone: pending writes with no later commit (`complete`), and paths whose every
/// line committed (`done`, for the byte counts in the report).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryPlan {
    /// Pending writes with no matching commit, oldest first.
    pub complete: Vec<JournalEntry>,
    /// Paths fully committed (informational: counted as completed in the report).
    pub done: Vec<String>,
}

impl RecoveryPlan {
    /// Derive the plan from journal lines in file order.
    pub fn plan(entries: &[JournalEntry]) -> RecoveryPlan {
        let mut pending: Vec<&JournalEntry> = Vec::new();
        let mut done: Vec<String> = Vec::new();
        for e in entries {
            match e.state {
                JournalState::Pending => pending.push(e),
                JournalState::Committed => {
                    if let Some(i) = pending.iter().position(|p| p.path == e.path && p.staged == e.staged) {
                        pending.remove(i);
                    }
                    if !done.contains(&e.path) {
                        done.push(e.path.clone());
                    }
                }
            }
        }
        RecoveryPlan { complete: pending.into_iter().cloned().collect(), done }
    }
}

/// One JSON line for the journal file (with trailing newline).
pub fn line(entry: &JournalEntry) -> String {
    let mut s = serde_json::to_string(entry).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, staged: &str, state: JournalState) -> JournalEntry {
        JournalEntry { path: path.into(), staged: staged.into(), state, len: 3 }
    }

    #[test]
    fn torn_tail_is_skipped() {
        let mut log = line(&entry("a.dxf", "a.tmp", JournalState::Pending));
        log.push_str("{\"path\": \"b.dxf");
        let got = read(log.as_bytes());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, "a.dxf");
    }

    #[test]
    fn plan_completes_only_uncommitted() {
        let entries =
            vec![entry("a", "a.1", JournalState::Pending), entry("a", "a.1", JournalState::Committed), entry("b", "b.1", JournalState::Pending)];
        let plan = RecoveryPlan::plan(&entries);
        assert_eq!(plan.complete.len(), 1);
        assert_eq!(plan.complete[0].path, "b");
        assert_eq!(plan.done, vec!["a".to_string()]);
    }
}
