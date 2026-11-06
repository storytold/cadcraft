//! The authoritative side of a collaboration session: one drawing, one order, one change set per
//! operation.
//!
//! The room owns a [`Session`] (headless, with no local undo history of its own: ADR 0010), applies
//! every operation to it, and answers with the change set between the drawing before and after
//! (ADR 0001). It is transport-agnostic on purpose: a WebSocket adapter, the CLI and tests all call
//! [`Room::apply`] and send [`Outcome::Applied`] unchanged.
//!
//! Ordering, idempotency and the stale-structural rule follow ADR 0003 and ADR 0004. A snapshot and
//! a journal are next; this module starts with the part every later piece depends on.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod room;

pub use room::{Applied, CommitFormat, Committed, Op, OpKind, Outcome, Reject, Room, RoomError, Snapshot};
