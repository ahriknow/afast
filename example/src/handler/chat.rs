//! Chat handler — bidirectional long-connection streaming.
//!
//! This module demonstrates:
//! - Long-connection handlers using `Receiver` and `Sender`
//! - The handler runs in a loop, reading messages from the client
//!   and echoing them back
//! - The connection stays open until the client disconnects
//! - Works over WebSocket and TCP transports (not HTTP)
//!
//! ## How it works
//!
//! 1. Client connects and sends `ChatJoin` data (name)
//! 2. Handler enters a loop reading messages via `receiver.recv()`
//! 3. Each message is echoed back via `sender.send()`
//! 4. When the client disconnects, `recv()` returns `None` and the loop ends

use afast::{AFastDeserialize, Tag, handler};

use crate::state::AppState;

// ─── Request Types ────────────────────────────────────────────────

#[derive(AFastDeserialize, Tag)]
#[tag("Initial chat join data")]
pub struct ChatJoin {
    #[tag("Display name for the chat session")]
    pub name: String,
}

// ─── Handlers ─────────────────────────────────────────────────────

#[handler(desc("Chat echo — returns a Socket for bidirectional streaming"))]
pub async fn chat_echo(
    afast::State(_state): afast::State<AppState>,
    afast::Data(join): afast::Data<ChatJoin>,
    mut receiver: afast::Receiver,
    sender: afast::Sender,
) {
    println!("afast: chat_echo connection opened for '{}'", join.name);

    while let Some(data) = receiver.recv().await {
        if sender.send(data).await.is_err() {
            println!("afast: chat_echo send error for '{}', closing", join.name);
            break;
        }
    }

    println!("afast: chat_echo connection closed for '{}'", join.name);
}
