# Transport Layer

AFast supports multiple transport layers that can run simultaneously on different ports.

## HTTP

HTTP server endpoints:

| Method | Path | Description |
|--------|------|-------------|
| POST | `/_api` | Binary handler dispatch |
| GET | `/_ws` | WebSocket upgrade (merged mode) |
| GET | `/code/{service}/{lang}` | On-demand code gen (requires `code`) |
| GET | `/doc` | API docs index (requires `doc`) |
| GET | `/doc/{service}` | Service-specific docs (requires `doc`) |
| * | Ordinary routes | RESTful endpoints (requires `ordinary-http`) |
| GET | `/path/:param` | WebSocket upgrade for ordinary-ws routes (requires `ordinary-ws`) |
| GET | `/path` | SSE stream for ordinary-sse routes (requires `ordinary-sse`) |

**HTTP Response Format:**
- Success: `[0u8][0i64][data: bytes]`
- Error: `[1u8][code: i64][message: bytes]`

## WebSocket

WS frame format:

```
Request:  [req_id: SeqId][handler_id: u32][len: Len][payload]
Push:     [0: SeqId][conn_id: u32][len: Len][payload]
Heartbeat:[0xFFFFFFFF: SeqId][len: Len][conn_id1: u32]...
```

`SeqId` type is controlled by the `seq64` feature (`i32` or `i64`), `Len` type by the `len64` feature (`u32` or `u64`).

## TCP

TCP uses 4-byte big-endian length-prefix framing with complete binary payloads per frame. Suitable for embedded devices or raw TCP scenarios.

## HTTP + WS Port Merging

When `ws_addr` and `http_addr` are set to the **same address**, AFast merges WebSocket into the HTTP server via HTTP Upgrade:

```rust
// Same port for both HTTP and WebSocket
let app = AFast::new()
    .service(svc)
    .ws("0.0.0.0:5000")
    .http("0.0.0.0:5000");  // Same address, auto-merged
```

## Ordinary HTTP (REST)

With `ordinary-http`, define RESTful routes using `get`/`post`/`put`/`patch`/`delete` inside the `service!` macro:

```rust
use afast::{get, Query, Param, Body, Header, Json, HttpResult};

#[derive(Deserialize)]
struct UserQuery { page: i64, size: i64 }

#[derive(Serialize)]
struct UserResponse { id: i64, name: String }

#[get(":id")]
async fn get_user(
    state: State<AppState>,
    param: Param<UserPath>,
    query: Query<UserQuery>,
) -> HttpResult<Json<UserResponse>> {
    Ok(Json(UserResponse { id: param.id, name: format!("User {}", param.id) }))
}
```

### Response Types

| Type | HTTP Status | Content-Type |
|------|-------------|-------------|
| `Json<T>` | 200 | `application/json` |
| `Text` | 200 | `text/plain` |
| `Html` | 200 | `text/html` |
| `File` | 200 | Custom + `Content-Disposition: attachment` |
| `Status(code)` | Custom | — |
| `Redirect(url)` | 302 | `Location` header |

## Ordinary WebSocket

With `ordinary-ws`, define WebSocket routes using `ws` inside the `service!` macro. These routes use text/JSON frames instead of the binary protocol:

```rust
use afast::{ws, WsSender, WsReceiver, WsParam};

#[derive(Deserialize)]
struct ChatParam { room: String }

#[ws(desc("Chat room"))]
async fn chat_ws(
    param: WsParam<ChatParam>,
    sender: WsSender,
    mut receiver: WsReceiver,
) -> afast::Result<()> {
    while let Some(msg) = receiver.recv().await {
        if let afast::WsMessage::Text(text) = msg {
            sender.send_text(format!("[{}] {}", param.0.room, text)).await?;
        }
    }
    Ok(())
}

let svc = service!("chat" => {
    ws("/chat/:room", chat_ws),
});
```

Connect: `ws://host:port/chat/general?token=abc`

TS/JS clients auto-generate platform-aware WebSocket connection methods, compatible with browsers, UniApp, and WeChat Mini Programs.

## Server-Sent Events (SSE)

Requires the `ordinary-sse` feature. Register SSE routes with `sse()`:

```rust
use afast::{SseSender, Query};

#[derive(Deserialize)]
struct SseQuery { room: Option<String> }

#[afast::sse(desc("SSE event stream"))]
async fn notifications(
    query: Query<SseQuery>,
    sender: SseSender,
) -> afast::Result<()> {
    let room = query.0.room.unwrap_or_default();

    // Send named event
    sender.send_event("connected", &serde_json::json!({"room": room})).await?;

    // Send data event (auto-serialized as JSON)
    let mut count = 0u64;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        count += 1;
        if sender.send_event("tick", &serde_json::json!({"count": count})).await.is_err() {
            break;  // Client disconnected
        }
    }
    Ok(())
}

let svc = service!("events" => {
    sse("/notifications", notifications),
});
```

Connect: `GET http://host:port/notifications?room=general`

Response headers:
- `Content-Type: text/event-stream; charset=utf-8`
- `Cache-Control: no-cache`
- `Connection: keep-alive`
- `Transfer-Encoding: chunked`

Wire format:
```
event: connected
data: {"room":"general"}

event: tick
data: {"count":1}
```

### SseSender Methods

| Method | Description |
|--------|-------------|
| `send<T: Serialize>(data)` | Send a `data:` event with JSON-serialized value |
| `send_event<T: Serialize>(event, data)` | Send a named event with JSON-serialized value |

### SseEvent Fields

| Field | Type | Wire Format |
|-------|------|-------------|
| `event` | `Option<&str>` | `event: name\n` |
| `data` | `String` | `data: ...\n` |
| `id` | `Option<&str>` | `id: ...\n` |
| `retry` | `Option<u64>` | `retry: ...\n` |

### Client Codegen

- **TS/JS**: Generates `EventSource`-based methods
- **KT (OkHttp)**: Uses `okhttp3.sse.EventSource`
- **KT (non-OkHttp)**: Uses `java.net.http.HttpClient` with `BodyHandlers.ofLines()`

```typescript
// TS/JS generated client
const es = client.apis.sse_stream({ room: "general" });
es.addEventListener("connected", (e) => console.log("Connected:", e.data));
es.addEventListener("tick", (e) => console.log("Tick:", JSON.parse(e.data)));
es.close();
```

## Long Connections

Handlers using `Receiver`/`Sender` are auto-detected as long-connection mode:

```rust
#[handler(desc("Chat"))]
async fn chat(
    state: State<AppState>,
    auth: Custom<Auth>,
    mut receiver: Receiver,
    sender: Sender,
) -> Result<()> {
    sender.send(b"Welcome!".to_vec()).await?;
    while let Some(msg) = receiver.recv().await {
        sender.send(msg).await?;  // echo
    }
    Ok(())
}
```

Generated clients return a `Socket` object for long-connection handlers, with `send()`/`close()` and `onMessage` callback.
