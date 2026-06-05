//! WebSocket route support for path-based WebSocket endpoints
//! (the `ordinary-ws` feature).
//!
//! This module provides:
//! - `WsStream` — a high-level wrapper for sending/receiving text and JSON
//!   messages over a WebSocket connection.
//! - `WsQuery<T>` — query parameter extractor from the upgrade request.
//! - `WsParam<T>` — path parameter extractor from the upgrade request.
//!
//! Unlike the binary protocol WebSocket at `/_ws`, ordinary-ws routes
//! use standard text/JSON frames and support path-based routing with
//! parameter extraction.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use tokio::sync::mpsc;

use crate::error::Error;

// ─── WsMessage ────────────────────────────────────────────────────

/// A message received from or sent to a WebSocket client.
#[derive(Debug, Clone)]
pub enum WsMessage {
    /// A UTF-8 text frame.
    Text(String),
    /// A binary frame.
    Binary(Vec<u8>),
    /// A ping frame.
    Ping(Vec<u8>),
    /// A pong frame.
    Pong(Vec<u8>),
    /// The connection was closed.
    Close(Option<String>),
}

// ─── WsSender ─────────────────────────────────────────────────────

/// Sender for writing messages in ordinary-ws handlers.
///
/// Used as a handler parameter to send text/JSON/binary messages to
/// the WebSocket client. Analogous to [`Sender`](crate::handler::Sender)
/// in binary long-connection handlers.
#[derive(Clone)]
pub struct WsSender {
    tx: mpsc::Sender<WsMessage>,
}

impl WsSender {
    /// Creates a new `WsSender` wrapping the given channel.
    #[cfg(all(feature = "ws", feature = "binary"))]
    pub(crate) fn new(tx: mpsc::Sender<WsMessage>) -> Self {
        Self { tx }
    }

    /// Sends a text message to the client.
    pub async fn send_text(&self, text: impl Into<String>) -> Result<(), Error> {
        self.tx
            .send(WsMessage::Text(text.into()))
            .await
            .map_err(|_| Error::Ws {
                message: "ws channel closed".into(),
            })
    }

    /// Sends a JSON-serialized message as a text frame.
    pub async fn send_json<T: serde::Serialize>(&self, value: &T) -> Result<(), Error> {
        let text = serde_json::to_string(value).map_err(|e| Error::Serialize {
            message: format!("json error: {e}"),
        })?;
        self.send_text(text).await
    }

    /// Sends binary data to the client.
    pub async fn send_binary(&self, data: Vec<u8>) -> Result<(), Error> {
        self.tx
            .send(WsMessage::Binary(data))
            .await
            .map_err(|_| Error::Ws {
                message: "ws channel closed".into(),
            })
    }

    /// Sends a close frame and shuts down the connection.
    pub async fn close(&self, reason: Option<String>) -> Result<(), Error> {
        self.tx
            .send(WsMessage::Close(reason))
            .await
            .map_err(|_| Error::Ws {
                message: "ws channel closed".into(),
            })
    }
}

// ─── WsReceiver ───────────────────────────────────────────────────

/// Receiver for reading messages in ordinary-ws handlers.
///
/// Used as a handler parameter to receive text/binary messages from
/// the WebSocket client. Analogous to [`Receiver`](crate::handler::Receiver)
/// in binary long-connection handlers.
pub struct WsReceiver {
    rx: mpsc::Receiver<WsMessage>,
}

impl WsReceiver {
    /// Creates a new `WsReceiver` wrapping the given channel.
    #[cfg(all(feature = "ws", feature = "binary"))]
    pub(crate) fn new(rx: mpsc::Receiver<WsMessage>) -> Self {
        Self { rx }
    }

    /// Creates a placeholder receiver for internal use.
    pub fn placeholder() -> Self {
        let (_, rx) = mpsc::channel(1);
        Self { rx }
    }

    /// Receives the next message from the client.
    ///
    /// Returns `None` when the connection is closed.
    pub async fn recv(&mut self) -> Option<WsMessage> {
        self.rx.recv().await
    }

    /// Receives the next text message from the client, skipping
    /// non-text frames.
    ///
    /// Returns `None` when the connection is closed.
    pub async fn recv_text(&mut self) -> Option<String> {
        loop {
            match self.rx.recv().await? {
                WsMessage::Text(t) => return Some(t),
                WsMessage::Close(_) => return None,
                _ => continue,
            }
        }
    }

    /// Receives the next message and deserializes it as JSON.
    ///
    /// Returns `None` when the connection is closed.
    pub async fn recv_json<T: serde::de::DeserializeOwned>(&mut self) -> Option<Result<T, Error>> {
        let text = self.recv_text().await?;
        Some(serde_json::from_str(&text).map_err(|e| Error::Serialize {
            message: format!("json decode error: {e}"),
        }))
    }
}

// ─── WsQuery / WsParam — aliases for shared Query/Param ──────

/// Query parameter extractor for WebSocket upgrade requests.
/// Alias for [`crate::Query`].
pub type WsQuery<T> = crate::app::extractors::Query<T>;

/// Path parameter extractor for WebSocket upgrade requests.
/// Alias for [`crate::Param`].
pub type WsParam<T> = crate::app::extractors::Param<T>;

// ─── WsHandlerInvoker trait ──────────────────────────────────────

/// Trait for invoking ordinary-ws handler functions.
///
/// Generated by the `#[ws(...)]` macro. The invoker performs the
/// WebSocket upgrade, extracts query/path parameters, creates
/// `WsSender`/`WsReceiver`, and calls the user's handler function.
pub trait WsHandlerInvoker: Send + Sync {
    /// Performs the WebSocket upgrade and invokes the handler.
    fn call_ws(
        &'static self,
        query: &str,
        path_params: &HashMap<String, String>,
        headers: serde_json::Value,
        sender: WsSender,
        receiver: WsReceiver,
        state: std::sync::Arc<crate::state::StateMap>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'static>>;
}

// ─── WsRouteInfo ─────────────────────────────────────────────────

/// Stores information about a registered WebSocket route.
#[derive(Clone)]
pub struct WsRouteInfo {
    /// The path pattern (e.g. `"/chat/:room"`).
    pub path: &'static str,
    /// The handler name for logging/diagnostics.
    pub handler_name: &'static str,
    /// The compiled route pattern.
    pub(crate) pattern: crate::app::ordinary::RoutePattern,
    /// The ws handler invoker.
    pub invoker: &'static dyn WsHandlerInvoker,
    /// The service this route belongs to.
    pub service_name: String,
}
