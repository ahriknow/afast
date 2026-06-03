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
