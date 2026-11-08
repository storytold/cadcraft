//! One change to a drawing, and where in the draw order it belongs.

use cadcraft_doc::{Entity, Handle, Space};
use serde::{Deserialize, Serialize};

use serde_json::Value;

use crate::table::{RecordId, RecordKind};

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
    /// A named record (a layer) created; entities may name it after this change.
    TableCreate {
        kind: RecordKind,
        id: RecordId,
        name: String,
        /// The record's properties, so recreating it (an undo of a deletion) keeps them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        properties: Option<Value>,
    },
    /// A named record renamed; every reference follows, on the receiving side too.
    TableRename { kind: RecordKind, id: RecordId, from: String, to: String },
    /// A named record deleted; its objects go with it.
    TableDelete { kind: RecordKind, id: RecordId, name: String },
    /// A layout (paper space) created.
    LayoutCreate {
        id: RecordId,
        name: String,
        tab_order: u32,
        /// The page setup and view, so recreating it keeps them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        properties: Option<Value>,
    },
    /// A layout renamed; the active-layout name follows.
    LayoutRename { id: RecordId, from: String, to: String },
    /// A layout deleted; its entities go with it.
    LayoutDelete { id: RecordId, name: String },
}
