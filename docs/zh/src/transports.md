# 传输层

AFast 支持多种传输层，可以在不同端口上同时运行。

## HTTP

HTTP 服务端点：

| 方法 | 路径 | 描述 |
|------|------|------|
| POST | `/_api` | 二进制 handler 分发 |
| GET | `/_ws` | WebSocket 升级（合并模式） |
| GET | `/code/{service}/{lang}` | 按需代码生成（需要 `code`） |
| GET | `/doc` | API 文档索引（需要 `doc`） |
| GET | `/doc/{service}` | 服务特定文档（需要 `doc`） |
| * | ordinary 路由 | RESTful 端点（需要 `ordinary-http`） |
| GET | `/path/:param` | ordinary-ws 路由的 WebSocket 升级（需要 `ordinary-ws`） |
| GET | `/path` | ordinary-sse 路由的 SSE 流（需要 `ordinary-sse`） |

**HTTP 响应格式：**
- 成功: `[0u8][0i64][data: bytes]`
- 错误: `[1u8][code: i64][message: bytes]`

## WebSocket

WS 帧格式：

```
Request:  [req_id: SeqId][handler_id: u32][len: Len][payload]
Push:     [0: SeqId][conn_id: u32][len: Len][payload]
Heartbeat:[0xFFFFFFFF: SeqId][len: Len][conn_id1: u32]...
```

`SeqId` 类型由 `seq64` Feature 控制（`i32` 或 `i64`），`Len` 类型由 `len64` Feature 控制（`u32` 或 `u64`）。

## TCP

TCP 使用 4 字节大端序长度前缀帧，每帧包含完整的二进制负载。适用于嵌入式设备或原生 TCP 场景。

## HTTP + WS 端口合并

当 `ws_addr` 和 `http_addr` 设置为**相同地址**时，AFast 通过 HTTP Upgrade 将 WebSocket 合并到 HTTP 服务器中：

```rust
// 相同端口同时用于 HTTP 和 WebSocket
let app = AFast::new()
    .service(svc)
    .ws("0.0.0.0:5000")
    .http("0.0.0.0:5000");  // 相同地址，自动合并
```

## TLS / HTTPS

AFast 支持基于 rustls 的 TLS/HTTPS，ALPN 协商 HTTP/2。

### 基本用法

```rust
let app = AFast::new()
    .service(svc)
    .https("0.0.0.0:5443", "./cert.pem", "./key.pem", None);
```

### 优雅降级

如果证书文件不存在，服务器自动降级为普通 HTTP：

```text
afast: TLS cert files not found, starting without encryption: [::]:5443
```

### 热重载证书

通过 `broadcast` channel 在运行时重载证书，无需重启：

```rust
let (reload_tx, reload_rx) = tokio::sync::broadcast::channel(1);

let app = AFast::new()
    .https("0.0.0.0:5443", "./cert.pem", "./key.pem", Some(reload_rx));

// 使用原始路径重载
reload_tx.send(None).unwrap();

// 使用新路径重载
reload_tx.send(Some(TlsReloadMessage {
    cert_path: "/new/cert.pem".into(),
    key_path: "/new/key.pem".into(),
})).unwrap();
```

## Ordinary HTTP (REST)

使用 `ordinary-http`，在 `service!` 宏内使用 `get`/`post`/`put`/`patch`/`delete` 定义 RESTful 路由：

```rust
use afast::{get, Query, Param, Body, Header, Json, HttpResult};

#[derive(Deserialize)]
struct UserQuery { page: i64, size: i64 }

#[derive(Serialize)]
struct UserResponse { id: i64, name: String }

#[get(desc("获取用户信息"))]
async fn get_user(
    state: State<AppState>,
    param: Param<UserPath>,
    query: Query<UserQuery>,
) -> HttpResult<Json<UserResponse>> {
    Ok(Json(UserResponse { id: param.id, name: format!("User {}", param.id) }))
}
```

### 响应类型

| 类型 | HTTP 状态码 | Content-Type |
|------|-------------|-------------|
| `Json<T>` | 200 | `application/json` |
| `Text` | 200 | `text/plain` |
| `Html` | 200 | `text/html` |
| `File` | 200 | 自定义 + `Content-Disposition: attachment` |
| `Status(code)` | 自定义 | — |
| `Redirect::temporary(url)` / `Redirect::permanent(url)` | 302 / 301 | `Location` 头 |

### CORS

通过 `AFast::cors()` 启用 CORS（跨源资源共享），需要 `http` feature。所有 HTTP 端点——包括二进制 `/_api`、普通 HTTP 路由以及代码/文档端点——都会自动包含 CORS 头：

```rust
use afast::{AFast, CorsConfig};

// 开发环境：允许所有来源
AFast::new()
    .cors(CorsConfig::permissive())
    .http("0.0.0.0:5000")
    .run().await;

// 生产环境：指定来源并启用凭证
AFast::new()
    .cors(
        CorsConfig::new(vec!["https://example.com", "https://app.example.com"])
            .allow_credentials(true)
            .max_age(7200)
    )
    .http("0.0.0.0:5000")
    .run().await;
```

服务器会自动：
- 响应 `OPTIONS` 预检请求，返回 `204 No Content`
- 在每个 HTTP 响应中注入 `Access-Control-Allow-Origin`
- 为预检请求设置 `Access-Control-Allow-Methods`、`Access-Control-Allow-Headers` 和 `Access-Control-Max-Age`

### 安全头

每个 HTTP 响应默认包含以下安全头：

| 头部 | 值 |
|------|------|
| `x-content-type-options` | `nosniff` |
| `x-frame-options` | `DENY` |
| `content-security-policy` | `default-src 'self'` |

可通过 `AFast::security_headers()` 覆盖：

```rust
AFast::new()
    .security_headers(vec![
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "SAMEORIGIN"),
        ("content-security-policy", "default-src 'self'; script-src 'self' 'unsafe-inline'"),
    ])
    .http("0.0.0.0:5000")
    .run().await;
```

## Ordinary WebSocket

使用 `ordinary-ws`，在 `service!` 宏内使用 `ws` 定义 WebSocket 路由。这些路由使用文本/JSON 帧而非二进制协议：

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

连接: `ws://host:port/chat/general?token=abc`

TS/JS 客户端会自动生成平台感知的 WebSocket 连接方法，兼容浏览器、UniApp 和微信小程序。

## Server-Sent Events (SSE)

需要 `ordinary-sse` Feature。使用 `sse()` 注册 SSE 路由：

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

    // 发送命名事件
    sender.send_event("connected", &serde_json::json!({"room": room})).await?;

    // 发送数据事件（自动序列化为 JSON）
    let mut count = 0u64;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        count += 1;
        if sender.send_event("tick", &serde_json::json!({"count": count})).await.is_err() {
            break;  // 客户端断开连接
        }
    }
    Ok(())
}

let svc = service!("events" => {
    sse("/notifications", notifications),
});
```

连接: `GET http://host:port/notifications?room=general`

响应头：
- `Content-Type: text/event-stream; charset=utf-8`
- `Cache-Control: no-cache`
- `Connection: keep-alive`
- `Transfer-Encoding: chunked`

线路格式：
```
event: connected
data: {"room":"general"}

event: tick
data: {"count":1}
```

### SseSender 方法

| 方法 | 描述 |
|------|------|
| `send<T: Serialize>(data)` | 发送 `data:` 事件，值为 JSON 序列化 |
| `send_event<T: Serialize>(event, data)` | 发送命名事件，值为 JSON 序列化 |

### SseEvent 字段

| 字段 | 类型 | 线路格式 |
|------|------|----------|
| `event` | `Option<&str>` | `event: name\n` |
| `data` | `String` | `data: ...\n` |
| `id` | `Option<&str>` | `id: ...\n` |
| `retry` | `Option<u64>` | `retry: ...\n` |

### 客户端代码生成

- **TS/JS**: 生成基于 `EventSource` 的方法
- **KT (OkHttp)**: 使用 `okhttp3.sse.EventSource`
- **KT (非 OkHttp)**: 使用 `java.net.http.HttpClient` 配合 `BodyHandlers.ofLines()`

```typescript
// TS/JS 生成的客户端
const es = client.apis.sse_stream({ room: "general" });
es.addEventListener("connected", (e) => console.log("Connected:", e.data));
es.addEventListener("tick", (e) => console.log("Tick:", JSON.parse(e.data)));
es.close();
```

## 长连接

使用 `Receiver`/`Sender` 的 handler 会被自动检测为长连接模式：

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
        sender.send(msg).await?;  // 回显
    }
    Ok(())
}
```

生成的客户端为长连接 handler 返回 `Socket` 对象，提供 `send()`/`close()` 方法和 `onMessage` 回调。
