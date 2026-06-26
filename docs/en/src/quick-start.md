# Quick Start

## Add Dependency

```toml
[dependencies]
afast = { version = "0.1.22", features = ["http", "ordinary-http", "ts"] }
tokio = { version = "1", features = ["full"] }
```

## Define State and Handlers

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

## Register Routes and Run

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

## Run

```bash
cargo run
```

- HTTP API: `POST http://localhost:5000/_api` (binary protocol)
- API Docs: `http://localhost:5000/doc`
- Generated TS client: `./client/api.ts`

## Multiple Transports

```rust
AFast::new()
    .state(app_state)
    .service(svc)
    .ws("0.0.0.0:3001")     // Binary WebSocket
    .tcp("0.0.0.0:4001")    // Binary TCP
    .http("0.0.0.0:5001")   // HTTP + ordinary routes
    .run().await.unwrap();
```

Or merge WS into HTTP (same port):

```rust
AFast::new()
    .state(app_state)
    .service(svc)
    .ws("0.0.0.0:5001")
    .http("0.0.0.0:5001")   // Same port → auto-merged
    .run().await.unwrap();
```
