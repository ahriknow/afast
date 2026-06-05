//! WebSocket chat handler — path-based WebSocket with text/JSON frames.
//!
//! This module demonstrates:
//! - `#[afast::ws]` macro for WebSocket endpoints
//! - `WsSender` / `WsReceiver` for sending/receiving messages
//! - `WsMessage` enum for handling different frame types (Text, Binary, Close, etc.)
//! - `Query<T>` extractor for URL query parameters from the upgrade request
//! - `Param<T>` extractor for path parameters (`:room`)
//!
//! ## How to connect
//!
//! ```javascript
//! const ws = new WebSocket('ws://localhost:5001/chat/lobby?token=abc');
//! ws.onopen = () => ws.send('Hello!');
//! ws.onmessage = (e) => console.log(e.data); // "[lobby] Hello!"
//! ```
//!
//! ## Difference from binary chat
//!
//! This handler uses standard WebSocket text/JSON frames (ordinary-ws),
//! while `chat_echo` in `chat.rs` uses the afast binary protocol.
//! Ordinary-ws is easier to test with browser DevTools and standard
//! WebSocket clients.

use afast::{Ctx, Param, Query, WsReceiver, WsSender};
use serde::Deserialize;

/// Query parameters from the WebSocket upgrade URL.
#[derive(Deserialize)]
pub struct ChatQuery {
    pub token: Option<String>,
}

/// Path parameters from the route pattern `/chat/:room`.
#[derive(Deserialize)]
pub struct ChatParam {
    pub room: String,
}

/// WebSocket chat echo handler.
///
/// Connect: `ws://localhost:5001/chat/general?token=abc`
/// Sends back each received message prefixed with the room name.
#[afast::ws(desc("WebSocket chat echo"))]
pub async fn chat_ws(
    ctx: Ctx<crate::RequestInfo>,
    query: Query<ChatQuery>,
    param: Param<ChatParam>,
    sender: WsSender,
    mut receiver: WsReceiver,
) -> afast::Result<()> {
    let room = &param.0.room;
    let token = query.0.token.as_deref().unwrap_or("<none>");
    eprintln!(
        "[ws-chat] client joined room: {}, token: {}, request_id: {}",
        room, token, ctx.0.request_id,
    );

    while let Some(msg) = receiver.recv().await {
        match msg {
            afast::WsMessage::Text(text) => {
                let reply = format!("[{}] {}", room, text);
                sender.send_text(&reply).await?;
            }
            afast::WsMessage::Close(_) => {
                eprintln!("[ws-chat] client left room: {}", room);
                break;
            }
            _ => {}
        }
    }

    eprintln!(
        "[ws-chat] room '{}' session ended after {:?} (request_id: {})",
        room,
        ctx.0.started_at.elapsed(),
        ctx.0.request_id,
    );

    Ok(())
}
