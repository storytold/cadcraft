//! Where a command runs, as the server states it.

use cadcraft_doc::{Handle, Space};
use serde::{Deserialize, Serialize};

/// The explicit context of an operation.
///
/// A command reads its target space from the session and, in some commands, its selection from the
/// window. A server has no window, so the client sends these values with the operation instead of
/// letting the server guess: the space (model space or one layout) and, when the command needs
/// one, the selection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpContext {
    /// The space the command acts on. Model space unless the client names a layout.
    pub space: Space,
    /// The selection the command sees, when it needs one; `None` keeps the server's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Vec<Handle>>,
}

impl OpContext {
    /// A context for `space` with no selection.
    pub fn in_space(space: Space) -> Self {
        OpContext { space, selection: None }
    }
}
