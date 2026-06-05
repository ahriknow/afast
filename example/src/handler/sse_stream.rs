//! SSE (Server-Sent Events) handler — pushes periodic events to the client.
//!
//! This module demonstrates:
//! - `#[afast::sse]` macro for SSE endpoints
//! - `SseSender` for pushing events to the client
//! - `SseEvent` for constructing events with custom types
//! - `Query<T>` extractor for URL query parameters
//!
//! ## How to connect
//!
//! ```javascript
//! const es = new EventSource('http://localhost:5001/sse?room=lobby');
//! es.addEventListener('connected', (e) => console.log('Connected:', e.data));
//! es.addEventListener('tick', (e) => console.log('Tick:', e.data));
//! ```
//!
//! ## SSE wire format
//!
//! Each event is sent as:
//! ```text
//! event: tick
//! data: {"count":1,"room":"lobby"}
//!
//! ```

use afast::{Ctx, Query, SseSender};
use serde::Deserialize;

/// Query parameters for the SSE endpoint.
#[derive(Deserialize)]
pub struct SseQuery {
    pub room: Option<String>,
}

/// SSE endpoint that pushes periodic events.
///
/// Connect: `GET /sse?room=general`
/// Sends a "connected" event, then periodic "tick" events.
#[afast::sse(desc("Server-Sent Events stream"))]
pub async fn sse_stream(
    ctx: Ctx<crate::RequestInfo>,
    query: Query<SseQuery>,
    sender: SseSender,
) -> afast::Result<()> {
    let room = query.0.room.unwrap_or_else(|| "default".to_string());

    eprintln!(
        "[sse] client connected to room: {}, request_id: {}",
        room, ctx.0.request_id,
    );

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

    eprintln!(
        "[sse] client disconnected from room: {} after {:?} (request_id: {})",
        room,
        ctx.0.started_at.elapsed(),
        ctx.0.request_id,
    );

    Ok(())
}
