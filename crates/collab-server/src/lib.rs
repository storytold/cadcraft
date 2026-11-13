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

mod assets;
pub mod auth;
mod cabinet;
mod net;
mod policy;
mod presence;
mod room;

pub use assets::{AssetError, AssetLimits, AssetStore, MAX_ASSET_BYTES, MAX_ASSET_FILES, MAX_ASSET_TOTAL, Stored};
pub use cabinet::{Cabinet, CabinetConfig, DEFAULT_ATTEMPTS, DEFAULT_TIMEOUT_MS, PublishError, Published};
pub use cadcraft_collab::{Applied, Op, OpKind, Outcome, Reject, Role};
pub use net::{Metrics, Server};
pub use policy::{MAX_CHANGES, MAX_OP_BYTES, MAX_PARTICIPANTS, changes_too_many, deny_reason, params_reason, size_reason};
pub use presence::{PRESENCE_TTL, PresenceBoard};
pub use room::{CommitFormat, Committed, Room, RoomError, Snapshot};

/// Install the raster reader the authoritative session uses for `imageattach`.
///
/// The operation carries the image bytes in `data` (ADR 0021), and the room decodes them with the
/// same reader the apps install. Without a reader the engine refuses the command, so the room
/// installs one when it is created; calling this again is harmless.
pub fn install_raster_reader() {
    cadcraft_engine::cmd::file::set_image_decoder(|bytes| cadcraft_io::raster::decode(bytes).map_err(|error| error.to_string()));
}
