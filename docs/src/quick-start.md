# Quick Start

## Add Dependency

```toml
[dependencies]
afast = { version = "0.1.10", features = ["http", "ws", "ts"] }
tokio = { version = "1", features = ["full"] }
```

## Define a Handler

```rust
use afast::{AFast, handler, service, State, Data, Custom, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

#[derive(Clone)]
struct AppState { db_url: String }

#[derive(Clone)]
struct CacheState { redis_url: String }

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
    cache: State<CacheState>,
    auth: Custom<AuthCustom>,
    req: Data<HelloReq>,
) -> Result<HelloResp> {
    println!("DB: {}, Cache: {}", state.db_url, cache.redis_url);
    Ok(HelloResp { message: format!("Hello, {}!", req.name) })
}

#[tokio::main]
async fn main() {
    let svc = service!("api", "Example API" => {
        h(hello),
    });

    let app = AFast::new()
        .state(AppState { db_url: "localhost".into() })
        .state(CacheState { redis_url: "redis://localhost".into() })
        .service(svc)
        .ws("0.0.0.0:3000")
        .http("0.0.0.0:5000");

    app.run().await.unwrap();
}
```

## Run

```bash
cargo run --features "http,ws,ts"
```

- WebSocket API: `ws://localhost:3000`
- HTTP API: `POST http://localhost:5000/_api`
- Generated TS client code in `./code/api.ts`
