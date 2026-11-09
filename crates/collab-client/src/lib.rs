//! The client side of a collaboration session: a replica of the authoritative drawing.
//!
//! The server applies every operation and sends the resulting change set (ADR 0001). A client
//! keeps its replica in step by applying those change sets in version order, and holds the
//! operations it sent until their answer arrives. It does not execute commands locally: the
//! authoritative result is the server's, and a replica that applied the same command itself could
//! diverge (a global solver, ranged formulas, derived geometry). Optimistic execution, with a
//! rollback to the confirmed version, is a later layer on top of this one.
//!
//! Transport is not part of the protocol: the blocking desktop transport ([`Session`], behind the
//! `ws` feature) and the browser transport ([`WebSession`], which has no threads and no blocking
//! read) drive the same transport-free state machine. A test and an embedded server can call
//! [`Client::outbox`], [`Client::applied`] and [`Client::resync`] directly.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod client;
mod driver;
#[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
mod session;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
mod ws;

pub use client::{Client, ClientError};
pub use driver::{Event, SessionError};
#[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
pub use session::Session;
#[cfg(target_arch = "wasm32")]
pub use web::WebSession;
#[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
pub use ws::Connection;
