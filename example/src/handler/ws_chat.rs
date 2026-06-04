use afast::{WsParam, WsQuery, WsReceiver, WsSender};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ChatQuery {
    pub token: Option<String>,
}

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
    query: WsQuery<ChatQuery>,
    param: WsParam<ChatParam>,
    sender: WsSender,
    mut receiver: WsReceiver,
) -> afast::Result<()> {
    let room = &param.0.room;
    let token = query.0.token.as_deref().unwrap_or("<none>");
    println!("[ws-chat] client joined room: {}, token: {}", room, token);

    while let Some(msg) = receiver.recv().await {
        match msg {
            afast::WsMessage::Text(text) => {
                let reply = format!("[{}] {}", room, text);
                sender.send_text(&reply).await?;
            }
            afast::WsMessage::Close(_) => {
                println!("[ws-chat] client left room: {}", room);
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
