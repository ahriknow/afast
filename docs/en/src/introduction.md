# AFast

[简体中文](/zh/) | English

AFast is a high-performance Rust web backend framework. Annotate functions with
`#[handler]` — the framework auto-registers routes, dispatches requests via a
compact binary protocol, and generates TypeScript / JavaScript / Kotlin / Rust
client code with one click.

## Highlights

- **Zero Route Definitions** — `#[handler]` annotation, no manual routing table
- **Compact Binary Protocol** — smaller and faster than JSON, designed for internal communication
- **Zero-Copy State** — `State<T>` holds `&'static T`, no per-request clone, no `T: Clone` required
- **Auto Code Generation** — TypeScript / JavaScript / Kotlin / Rust clients with full type definitions
- **Interactive API Docs** — built-in Web docs with dark/light theme, online testing, WS/SSE debugging
- **Multiple Transports** — WebSocket, HTTP/1.1, HTTP/2, TCP, mix and match
- **TLS / HTTPS** — rustls with ALPN for HTTP/2 negotiation
- **RESTful Endpoints** — `#[get]`/`#[post]`/`#[put]`/`#[delete]` with JSON
- **WebSocket & SSE** — `#[ws]` and `#[sse]` route macros
- **Long Connections** — bidirectional streaming via `Receiver`/`Sender`
- **Lifecycle Hooks** — `before_request`/`on_response`/`on_error`/`on_connect`/`on_disconnect`
- **Request Context** — `Ctx<T>` per-request context, hooks write, handlers read
- **Rate Limiting** — named policies with pluggable storage backend

## Quick Example

```rust
use afast::{AFast, handler, service, State, Data, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

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
