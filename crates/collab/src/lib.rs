//! The change set two CADCraft drawings differ by, and how to apply one.
//!
//! A collaboration server applies an operation to its authoritative [`Drawing`] and sends the
//! other participants the change set between the drawing before and after. A client applies it
//! to its own copy without recording an undo step (the server owns per-user undo, see ADR 0010).
//!
//! The model here is deliberately narrow: entities of one space (model space or one layout) and
//! their draw order. Table operations (layers, styles, blocks, layouts) are a separate change set
//! and arrive with their own types; keeping them apart lets a rename be one operation instead of
//! a delete plus a create (see ADR 0002).
//!
//! Identifiers are the drawing's own [`Handle`]s: they are unique per drawing, survive a
//! save/load round trip and are never reused within a session. The server allocates them and a
//! client never invents one, so two participants cannot collide.
//!
//! # Example
//!
//! ```
//! use cadcraft_collab::{apply, diff};
//! use cadcraft_doc::{Drawing, EntityKind, Handle, Point, Space};
//! use cadcraft_doc::geom::Vec3;
//!
//! let mut before = Drawing::new_metric();
//! let mut after = before.clone();
//! after.add(&Space::Model, Default::default(), EntityKind::Point(Point { p: Vec3::new(1.0, 2.0, 0.0), angle: 0.0 })).expect("a point");
//!
//! let changes = diff(&before, &after, &Space::Model).expect("a diff");
//! assert_eq!(changes.len(), 1);
//! apply(&mut before, &changes).expect("applied");
//! assert_eq!(before.space(&Space::Model), after.space(&Space::Model));
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod apply;
mod change;
mod context;
mod diff;
mod error;

#[cfg(test)]
mod tests;

pub use apply::apply;
pub use change::{Anchor, Change};
pub use context::OpContext;
pub use diff::diff;
pub use error::CollabError;
