//! Transport layer implementations for the afast framework.
//!
//! This module contains the WebSocket, HTTP, and TCP transport backends.
//! Each is gated behind its respective Cargo feature (`ws`, `http`, `tcp`).

#[cfg(all(feature = "http", feature = "binary"))]
mod http;
#[cfg(feature = "tcp")]
mod tcp;
#[cfg(all(feature = "ws", feature = "binary"))]
mod ws;

#[cfg(all(feature = "http", feature = "binary"))]
pub use http::{HttpConfig, serve};
#[cfg(feature = "tcp")]
pub use tcp::handle_connection as handle_tcp_connection;
#[cfg(all(feature = "ws", feature = "binary"))]
pub use ws::handle_connection;
#[cfg(all(feature = "ws", feature = "http", feature = "binary"))]
pub use ws::handle_websocket;
