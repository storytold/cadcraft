use std::sync::atomic::{AtomicU64, Ordering};

use super::{Arc, Drawing, Handle, Interactive, Serialize, Vec2};

/// An interactive command in progress.
pub struct Running {
    pub id: String,
    pub machine: Box<dyn Interactive>,
    pub before: Arc<Drawing>,
    pub selection_before: Vec<Handle>,
    /// Tells this run apart from any other run of the same command, which has the same `id`.
    pub serial: u64,
}

static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

impl Running {
    /// A run of command `id` with a serial number no other run has had.
    pub fn new(id: impl Into<String>, machine: Box<dyn Interactive>, before: Arc<Drawing>, selection_before: Vec<Handle>) -> Self {
        Running { id: id.into(), machine, before, selection_before, serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed) }
    }
}

/// A pending window/crossing selection started by a click on empty space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PendingWindow {
    pub corner: Vec2,
    pub during_command: bool,
}
