use afast::{Query, SseSender};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct SseQuery {
    pub room: Option<String>,
}

/// SSE endpoint that pushes periodic events.
///
/// Connect: `GET /sse?room=general`
/// Sends a "connected" event, then periodic "tick" events.
#[afast::sse(desc("Server-Sent Events stream"))]
pub async fn sse_stream(query: Query<SseQuery>, sender: SseSender) -> afast::Result<()> {
    let room = query.0.room.unwrap_or_else(|| "default".to_string());

    // Send initial connected event
    sender
        .send_event("connected", &serde_json::json!({"room": room}))
        .await?;

    // Send periodic tick events
    let mut count = 0u64;
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        count += 1;
        let sent = sender
            .send_event("tick", &serde_json::json!({"count": count, "room": room}))
            .await;
        if sent.is_err() {
            // Client disconnected
            break;
        }
    }

    Ok(())
}
