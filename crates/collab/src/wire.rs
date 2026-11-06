//! The messages an operation and its answer carry.
//!
//! These are the common envelope's payloads (`.craft/protocol/ARCHITECTURE.md` §2): a client sends
//! an [`Op`], the server answers [`Outcome::Applied`] with the change set between the drawing
//! before and after, or [`Outcome::Rejected`] and leaves the drawing as it was.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Change, OpContext};

/// What an operation is, for the ordering rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpKind {
    /// A command from the allowlist, with explicit addresses.
    Command,
    /// A normalized operation that has no command.
    Primitive,
    /// An operation whose result depends on other objects (a cascade): exclusive, in order.
    Structural,
}

/// An operation as a client sends it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Op {
    /// The client's identifier; a repeat is answered from the server's window, never applied twice.
    pub op_id: String,
    /// Who sent it (the JWT `sub`).
    pub actor: String,
    /// The version the client had when it built the operation.
    pub base_version: u64,
    /// Command, primitive or structural.
    pub kind: OpKind,
    /// The command id (`"line"`, `"rename"`, `"layout.rename"`, …).
    pub command: String,
    /// The command's JSON parameters.
    pub params: Value,
    /// The space and selection the command acts on.
    pub context: OpContext,
}

/// The authoritative result of one operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// The operation it answers.
    pub op_id: String,
    /// The drawing's version after it.
    pub version: u64,
    /// Who applied it.
    pub actor: String,
    /// What the other participants must apply to reach this version.
    pub changes: Vec<Change>,
}

/// Why an operation was not applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reject {
    /// The operation it answers.
    pub op_id: String,
    /// A stable code: `stale`, `invalid`, `forbidden`.
    pub code: String,
    /// What the client should say.
    pub message: String,
    /// Whether sending it again (after resynchronising) can work.
    pub retryable: bool,
}

/// What happened to an operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Applied; the change set is the authoritative result.
    Applied(Applied),
    /// Not applied; the drawing is unchanged.
    Rejected(Reject),
}
