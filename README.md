# AFast

[简体中文](./README_CN.md) | English | [📖 Full Documentation](https://ahriknow.github.io/afast)

AFast is a high-performance Rust web backend framework. It eliminates manual route
definitions — annotate functions with `#[handler]` and the framework auto-registers
and dispatches requests. Data transport uses a compact binary protocol that is
smaller and faster than JSON. It supports one-click generation of TypeScript,
JavaScript, Kotlin, and Rust client code, with built-in interactive API documentation.

## Features

- **Zero Route Definitions** — `#[handler]` annotation, no manual routing table
- **Compact Binary Protocol** — Smaller and faster than JSON, designed for internal communication
- **Auto Code Generation** — TypeScript / JavaScript / Kotlin / Rust clients with full type definitions
- **Interactive API Docs** — Built-in Web docs with dark/light theme and online API testing
- **Multiple Transports** — WebSocket, HTTP/1.1, HTTP/2, and TCP, mix and match as needed
- **TLS / HTTPS** — Based on rustls with ALPN for HTTP/2 negotiation
- **RESTful Endpoints** — Standard HTTP methods with Query/Param/Body/Header extractors
- **Long Connections** — Bidirectional persistent communication via Receiver/Sender over WS/TCP
- **Lifecycle Hooks** — `before_request`/`on_response`/`on_error`/`on_connect`/`on_disconnect`, global and per-service
- **Rate Limiting** — Named-policy rate limiting with pluggable storage backend
- **Client-Side Caching** — `cache(seconds)` attribute with class-level static cache

## Quick Start

```toml
[dependencies]
afast = { version = "0.1.11", features = ["http", "ws", "ts"] }
tokio = { version = "1", features = ["full"] }
```

```rust
use afast::{AFast, handler, service, State, Data, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

#[derive(Clone)]
struct AppState { db_url: String }

#[derive(AFastDeserialize, Tag)]
#[tag("Request")]
struct HelloReq { name: String }

#[derive(AFastSerialize, Tag)]
#[tag("Response")]
struct HelloResp { message: String }

#[handler(desc("Say hello"))]
async fn hello(state: State<AppState>, req: Data<HelloReq>) -> Result<HelloResp> {
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

```bash
cargo run --features "http,ws,ts"
```

## Documentation

📖 **[Full Documentation](https://ahriknow.github.io/afast)** — Complete guide with examples

| Topic | Description |
|-------|-------------|
| [Quick Start](https://ahriknow.github.io/afast/quick-start.html) | Getting started guide |
| [Core Concepts](https://ahriknow.github.io/afast/core-concepts.html) | Handlers, Services, Extractors |
| [Features](https://ahriknow.github.io/afast/features.html) | All available features |
| [Lifecycle Hooks](https://ahriknow.github.io/afast/hooks.html) | Request/connection hooks |
| [Rate Limiting](https://ahriknow.github.io/afast/rate-limiting.html) | Rate limit configuration |
| [Transport Layer](https://ahriknow.github.io/afast/transports.html) | HTTP, WebSocket, TCP |
| [Code Generation](https://ahriknow.github.io/afast/code-generation.html) | TS/JS/KT/RS clients |
| [Binary Protocol](https://ahriknow.github.io/afast/binary-protocol.html) | Wire format specification |

## Project Structure

```
afast/           — Main framework crate
afast-macros/    — Proc macros (#[handler], register!, #[derive(Tag)])
example/         — Example project with full usage
docs/            — Documentation source (mdbook)
```

## License

MIT
