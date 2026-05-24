use afast::{AFastDeserialize, AFastSerialize, Tag, handler};

use crate::state::AppState;

// ─── Request Types ────────────────────────────────────────────────

#[derive(AFastDeserialize, Tag)]
#[tag("Initial chat join data")]
pub struct ChatJoin {
    #[tag("Display name for the chat session")]
    pub name: String,
}

// ─── Response Types ───────────────────────────────────────────────

#[derive(AFastSerialize, Tag)]
#[tag("Chat message from server")]
pub struct ChatMessage {
    #[tag("Sender name")]
    pub sender: String,
    #[tag("Message content")]
    pub text: String,
    #[tag("Unix timestamp in seconds")]
    pub ts: i64,
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
        // Echo back: try UTF-8 text, fall back to binary
        let response = if let Ok(text) = String::from_utf8(data.clone()) {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            let msg = ChatMessage {
                sender: join.name.clone(),
                text,
                ts,
            };
            afast::AFastSerialize::to_bytes(&msg)
        } else {
            // Binary data: echo as-is
            data
        };

        if sender.send(response).await.is_err() {
            println!("afast: chat_echo send error for '{}', closing", join.name);
            break;
        }
    }

    println!("afast: chat_echo connection closed for '{}'", join.name);
}
