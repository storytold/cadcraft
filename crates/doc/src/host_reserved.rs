//! Handles a host session has given out that are not stored in the drawing.
//!
//! A record with no handle of its own is named by the session. Those handles stay off the
//! drawing: remembering them does not change saved-state or undo identity, and every allocator,
//! including one that runs on a copy, skips them.

use std::cell::RefCell;
use std::collections::HashSet;

struct Set {
    uid: Option<u64>,
    handles: HashSet<u64>,
    /// Handles written into `handles` on this thread. A repeated write of a prefix counts again,
    /// so a caller can show that each new name costs one write.
    writes: u64,
}

thread_local! {
    static HOST: RefCell<Set> = RefCell::new(Set { uid: None, handles: HashSet::new(), writes: 0 });
}

/// Whether `h` is reserved for the document installed on this thread.
pub fn host_reserves(h: u64) -> bool {
    HOST.with(|s| s.borrow().handles.contains(&h))
}

/// How many handles the installed set holds.
pub fn host_reserved_len() -> usize {
    HOST.with(|s| s.borrow().handles.len())
}

/// Whether `uid`'s set is installed and already holds `len` handles.
pub fn host_reserved_current(uid: u64, len: usize) -> bool {
    HOST.with(|s| {
        let g = s.borrow();
        g.uid == Some(uid) && g.handles.len() == len
    })
}

/// How many handles have been written into this thread's set.
pub fn host_reserved_writes() -> u64 {
    HOST.with(|s| s.borrow().writes)
}

/// Record `h` for the installed document. `false` when `uid` is not that document: nothing is
/// written, and the caller replaces the set.
pub fn note_host_reserved(uid: u64, h: u64) -> bool {
    HOST.with(|s| {
        let mut g = s.borrow_mut();
        if g.uid != Some(uid) {
            return false;
        }
        if g.handles.insert(h) {
            g.writes = g.writes.saturating_add(1);
        }
        true
    })
}

/// Install `uid`'s handles in place of whatever this thread held. Each new handle counts as one
/// write, including a prefix that was already reserved for another document.
pub fn replace_host_reserved(uid: u64, handles: impl IntoIterator<Item = u64>) {
    HOST.with(|s| {
        let mut g = s.borrow_mut();
        g.handles.clear();
        g.uid = Some(uid);
        for h in handles {
            if g.handles.insert(h) {
                g.writes = g.writes.saturating_add(1);
            }
        }
    });
}

/// Drop this thread's set. The next drawing installs its own.
pub fn clear_host_reserved() {
    HOST.with(|s| {
        let mut g = s.borrow_mut();
        g.uid = None;
        g.handles.clear();
    });
}
