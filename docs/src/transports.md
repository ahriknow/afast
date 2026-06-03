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
