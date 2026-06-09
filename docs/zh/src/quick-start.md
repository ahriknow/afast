# 快速开始

## 添加依赖

```toml
[dependencies]
afast = { version = "0.1.16", features = ["http", "ordinary-http", "ts"] }
tokio = { version = "1", features = ["full"] }
```

## 定义 State 和 Handler

```rust
use afast::{AFast, handler, service, State, Data, Custom, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};
use std::sync::{Arc, Mutex};

// State — no Clone required, holds &'static T internally
struct AppState {
    db_url: String,
    counter: Arc<Mutex<u64>>,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Auth info")]
struct AuthCustom { token: i64, platform: String }

#[derive(AFastDeserialize, Tag)]
#[tag("Request body")]
struct HelloReq { name: String }

#[derive(AFastSerialize, Tag)]
#[tag("Response body")]
struct HelloResp { message: String }

#[handler(desc("Say hello"), name("hello"))]
async fn hello(
    state: State<AppState>,
    auth: Custom<AuthCustom>,
    req: Data<HelloReq>,
) -> Result<HelloResp> {
    let mut count = state.counter.lock().unwrap();
    *count += 1;
    Ok(HelloResp { message: format!("Hello, {}! (count: {})", req.name, count) })
}
```

## 注册路由并运行

```rust
#[tokio::main]
async fn main() {
    let svc = service!("api", "Example API" => {
        h(hello),
    });

    AFast::new()
        .state(AppState {
            db_url: "localhost".into(),
            counter: Arc::new(Mutex::new(0)),
        })
        .service(svc)
        .http("0.0.0.0:5000")
        .run().await.unwrap();
}
```

## 运行

```bash
cargo run
```

- HTTP API: `POST http://localhost:5000/_api` (二进制协议)
- API 文档: `http://localhost:5000/doc`
- 生成的 TS 客户端: `./client/api.ts`

## 多种传输层

```rust
AFast::new()
    .state(app_state)
    .service(svc)
    .ws("0.0.0.0:3001")     // 二进制 WebSocket
    .tcp("0.0.0.0:4001")    // 二进制 TCP
    .http("0.0.0.0:5001")   // HTTP + ordinary 路由
    .run().await.unwrap();
```

或者将 WS 合并到 HTTP（同一端口）：

```rust
AFast::new()
    .state(app_state)
    .service(svc)
    .ws("0.0.0.0:5001")
    .http("0.0.0.0:5001")   // 相同端口 → 自动合并
    .run().await.unwrap();
```
