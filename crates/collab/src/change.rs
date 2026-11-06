//! One change to a drawing, and where in the draw order it belongs.

use cadcraft_doc::{Entity, Handle, Space};
use serde::{Deserialize, Serialize};

/// Where an entity goes in its space's draw order.
///
/// The order is a position in the space's entity store, not a field, so it cannot be sent as a
/// number: a concurrent insert would shift every index. An anchor names another entity instead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Anchor {
    /// The bottom of the draw order.
    Bottom,
    /// The top of the draw order.
    Top,
    /// Directly above `handle` in the draw order.
    After { handle: Handle },
    /// Directly below `handle` in the draw order.
    Before { handle: Handle },
}

/// One change to a drawing, as sent from the authoritative server to a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum Change {
    /// A new entity in `space`; the server alone chooses `handle`.
    EntityCreate { space: Space, handle: Handle, order: Anchor, entity: Entity },
    /// An existing entity, replaced whole (geometry and common properties).
    EntityReplace { handle: Handle, entity: Entity },
    /// An existing entity, removed from its space.
    EntityDelete { handle: Handle },
    /// An existing entity moved in the draw order, with no other change.
    EntityOrder { handle: Handle, order: Anchor },
}

impl Change {
    /// The entity this change is about.
    pub fn handle(&self) -> Handle {
        match self {
            Change::EntityCreate { handle, .. }
            | Change::EntityReplace { handle, .. }
            | Change::EntityDelete { handle }
            | Change::EntityOrder { handle, .. } => *handle,
        }
    }
}
