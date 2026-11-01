//! CADCraft MCP server.
//!
//! JSON-RPC 2.0 over stdio (newline-delimited), implementing the MCP lifecycle, tools and
//! resources. Tools map onto the app's control-channel methods, served either by a headless
//! [`Headless`] session in-process or by a running app through its loopback control port
//! ([`Remote`]). Agents can type at the command line exactly like a user (`command_line`), call
//! any command with JSON (`execute`), inspect the drawing and render it to a PNG.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod args;
mod backend;
pub mod control_token;
mod server;
mod tools;

pub use backend::{Backend, Headless, Remote};
pub use server::{PROTOCOL_VERSION, Server};
pub use tools::{call_tool, tool_definitions};

#[cfg(test)]
mod tests;
