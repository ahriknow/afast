//! Transport layer implementations for the afast framework.
//!
//! This module contains the WebSocket, HTTP, and TCP transport backends.
//! Each is gated behind its respective Cargo feature (`ws`, `http`, `tcp`).

#[cfg(feature = "http")]
mod http;
#[cfg(feature = "tcp")]
mod tcp;
#[cfg(feature = "ws")]
mod ws;

#[cfg(feature = "http")]
pub use http::{HttpConfig, serve};
#[cfg(feature = "tcp")]
pub use tcp::handle_connection as handle_tcp_connection;
#[cfg(feature = "ws")]
pub use ws::handle_connection;
#[cfg(all(feature = "ws", feature = "http"))]
pub use ws::handle_websocket;
