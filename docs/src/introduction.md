# AFast

[简体中文](https://github.com/ahriknow/afast/blob/develop/README_CN.md) | English

AFast is a high-performance Rust web backend framework. It eliminates manual route
definitions — annotate functions with `#[handler]` and the framework auto-registers
and dispatches requests. Data transport uses a compact binary protocol that is
smaller and faster than JSON. It supports one-click generation of TypeScript,
JavaScript, Kotlin, and Rust client code, with built-in interactive API documentation.

## Highlights

- **Zero Route Definitions** — `#[handler]` annotation, no manual routing table
- **Compact Binary Protocol** — Smaller and faster than JSON, designed for internal communication
- **Auto Code Generation** — TypeScript / JavaScript / Kotlin / Rust clients with full type definitions
- **Interactive API Docs** — Built-in Web docs with dark/light theme and online API testing
- **Multiple Transports** — WebSocket, HTTP/1.1, HTTP/2, and TCP, mix and match as needed
- **TLS / HTTPS** — Based on rustls with ALPN for HTTP/2 negotiation
- **Lifecycle Hooks** — `before_request`/`on_response`/`on_error`/`on_connect`/`on_disconnect`
- **Rate Limiting** — Named-policy rate limiting with pluggable storage backend

## Quick Example

```rust
use afast::{AFast, handler, service, State, Data, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

#[derive(Clone)]
struct AppState { db_url: String }

#[derive(AFastDeserialize, Tag)]
#[tag("Request body")]
struct HelloReq { name: String }

#[derive(AFastSerialize, Tag)]
#[tag("Response body")]
struct HelloResp { message: String }

#[handler(desc("Say hello"))]
async fn hello(
    state: State<AppState>,
    req: Data<HelloReq>,
) -> Result<HelloResp> {
    Ok(HelloResp { message: format!("Hello, {}!", req.name) })
}

#[tokio::main]
async fn main() {
    let svc = service!("api" => { h(hello) });
    AFast::new()
        .state(AppState { db_url: "localhost".into() })
        .service(svc)
        .http("0.0.0.0:5000")
        .run().await.unwrap();
}
```
