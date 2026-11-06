//! Why a change set could not be applied.

use cadcraft_doc::{Handle, Space};

/// A change set that does not fit the drawing it is applied to.
///
/// Every variant is a protocol error, not a user error: the authoritative server and the client
/// disagree about state, and the client must resynchronise from a snapshot instead of guessing.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CollabError {
    #[error("no such space {0:?}")]
    NoSpace(Space),
    #[error("no such entity {}", .0.hex())]
    NoEntity(Handle),
    #[error("entity {} already exists", .0.hex())]
    Duplicate(Handle),
    #[error("anchor {} is missing", .0.hex())]
    NoAnchor(Handle),
    #[error("the change names entity {} but carries {}", .change.hex(), .entity.hex())]
    HandleMismatch { change: Handle, entity: Handle },
    #[error("no such layer `{0}`")]
    NoLayer(String),
}
