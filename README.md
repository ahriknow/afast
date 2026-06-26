# AFast

[简体中文](./README_CN.md) | English | [📖 Documentation](https://afast.ahriknow.help)

A high-performance Rust web framework. Annotate functions with `#[handler]` — the framework auto-registers routes, dispatches requests via a compact binary protocol, and generates TypeScript / JavaScript / Kotlin / Rust client code with one click.

## Features

- **Zero Route Definitions** — `#[handler]` annotation, no manual routing table
- **Compact Binary Protocol** — smaller and faster than JSON
- **Auto Code Generation** — TypeScript / JavaScript / Kotlin / Rust clients
- **Interactive API Docs** — built-in Web docs with online testing
- **Multiple Transports** — WebSocket, HTTP/1.1, HTTP/2, TCP
- **TLS / HTTPS** — rustls with ALPN for HTTP/2, hot-reload via channel
- **File Upload** — `Multipart` (raw) and `MultipartForm<T>` (typed) extractors with `#[derive(FromFormData)]`
- **RESTful Endpoints** — `#[get]`/`#[post]`/`#[put]`/`#[delete]` with JSON
- **WebSocket & SSE** — `#[ws]` and `#[sse]` route macros
- **Long Connections** — bidirectional streaming via `Receiver`/`Sender`
- **Zero-Copy State** — `State<T>` holds `&'static T`, no per-request clone
- **Lifecycle Hooks** — `before_request`/`on_response`/`on_error`/`on_connect`/`on_disconnect`
- **Request Context** — `Ctx<T>` per-request context, hooks write, handlers read
- **Rate Limiting** — named policies with pluggable storage backend
- **Client-Side Caching** — `cache(seconds)` attribute

## Quick Start

```toml
[dependencies]
afast = { version = "0.1.22", features = ["http", "ordinary-http"] }
tokio = { version = "1", features = ["full"] }
```

```rust
use afast::{AFast, Ctx, handler, service, State, Data, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

struct AppState { greeting: String }

#[derive(Clone)]
struct RequestId(pub String);

#[derive(AFastDeserialize, Tag)]
#[tag("Hello request")]
struct HelloReq { name: String }

#[derive(AFastSerialize, Tag)]
#[tag("Hello response")]
struct HelloResp { message: String }

#[handler(desc("Say hello"))]
async fn hello(
    ctx: Ctx<RequestId>,
    state: State<AppState>,
    req: Data<HelloReq>,
) -> Result<HelloResp> {
    println!("request_id={}", ctx.0 .0);
    Ok(HelloResp { message: format!("{}, {}!", state.greeting, req.name) })
}

#[tokio::main]
async fn main() {
    let svc = service!("api" => { h(hello) });
    AFast::new()
        .state(AppState { greeting: "Hello".into() })
        .service(svc)
        .http("0.0.0.0:5000")
        .run().await.unwrap();
}
```

## Documentation

📖 **[afast.ahriknow.help](https://afast.ahriknow.help)** — Quick Start, Core Concepts, Features, Hooks, Rate Limiting, Code Generation, Binary Protocol, and more.

## Project Structure

```
afast/           — Main framework crate
afast-macros/    — Proc macros (#[handler], service!, #[derive(Tag)])
example/         — Example project with full usage
docs/            — Documentation source (mdbook)
```

## License

MIT
