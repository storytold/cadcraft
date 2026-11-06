//! The client side of a collaboration session: a replica of the authoritative drawing.
//!
//! The server applies every operation and sends the resulting change set (ADR 0001). A client
//! keeps its replica in step by applying those change sets in version order, and holds the
//! operations it sent until their answer arrives. It does not execute commands locally: the
//! authoritative result is the server's, and a replica that applied the same command itself could
//! diverge (a global solver, ranged formulas, derived geometry). Optimistic execution, with a
//! rollback to the confirmed version, is a later layer on top of this one.
//!
//! Transport is not part of this type: a WebSocket adapter, a test and an embedded server all call
//! [`Client::outbox`], [`Client::applied`] and [`Client::resync`].

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod client;

pub use client::{Client, ClientError};
