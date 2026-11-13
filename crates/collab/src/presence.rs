//! Where a participant is looking, as other participants see it.
//!
//! Presence is ephemeral (ADR-0006): it is never journaled, never enters a snapshot, and a lost
//! message is replaced by the next frame. It carries no drawing content: a space, a cursor in world
//! coordinates, the selection's handles and the active layer.

use cadcraft_doc::{Handle, Space};
use serde::{Deserialize, Serialize};

use crate::Role;

/// A participant's view, as sent to the others.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    /// Who is looking (the JWT `sub`).
    pub actor: String,
    /// What the participant may do, as the server knows it. A client cannot claim a role: the
    /// server fills this in from the token before it stores or forwards the presence, so the
    /// cabinet's participant list can name each role instead of guessing one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// Model space or one layout.
    pub space: Space,
    /// The cursor in world coordinates, when the pointer is over the drawing area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<[f64; 2]>,
    /// The entities the participant selected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selection: Vec<Handle>,
    /// The current layer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub layer: String,
    /// The view: centre and height in drawing units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<View>,
}

/// A viewport's centre and height, in drawing units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The centre of the view.
    pub center: [f64; 2],
    /// How tall the view is.
    pub height: f64,
}
