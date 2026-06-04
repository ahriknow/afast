# Lifecycle Hooks

Enable the `hook` feature to intercept request lifecycle events for observability, tracing, logging, or custom middleware.

## Quick Example

```rust
use afast::hook::{Hook, RequestContext, RequestGuard, ConnectionGuard};

struct LoggingHook;

impl Hook for LoggingHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        println!("→ {} ({})", ctx.handler_name, ctx.transport);
        Some(Box::new(std::time::Instant::now()))
    }

    fn on_connect(&self, ctx: &RequestContext) -> Option<Box<dyn ConnectionGuard>> {
        println!("↕ connect: {} ({})", ctx.handler_name, ctx.transport);
        Some(Box::new(()))
    }
}

impl RequestGuard for std::time::Instant {
    fn on_response(&mut self, ctx: &RequestContext, _resp: &[u8]) {
        println!("← {} OK ({:?})", ctx.handler_name, self.elapsed());
    }
    fn on_error(&mut self, ctx: &RequestContext, err: &afast::Error) {
        println!("✗ {} error: {}", ctx.handler_name, err);
    }
}

impl ConnectionGuard for () {
    fn on_disconnect(&mut self, ctx: &RequestContext) {
        println!("✕ disconnect: {} ({})", ctx.handler_name, ctx.transport);
    }
}
```

## Hook Traits

### `Hook` — Entry Point

```rust
pub trait Hook: Send + Sync + 'static {
    /// Called before each request. Return a `RequestGuard` to observe the response.
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> { None }

    /// Called when a long connection (WS/TCP) is established.
    fn on_connect(&self, ctx: &RequestContext) -> Option<Box<dyn ConnectionGuard>> { None }
}
```

### `RequestGuard` — Per-Request Observer

```rust
pub trait RequestGuard: Send + 'static {
    /// Called when the handler returns Ok.
    fn on_response(&mut self, ctx: &RequestContext, response: &[u8]) {}

    /// Called when the handler returns Err.
    fn on_error(&mut self, ctx: &RequestContext, error: &afast::Error) {}
}
```

### `ConnectionGuard` — Long Connection Observer

```rust
pub trait ConnectionGuard: Send + 'static {
    /// Called when the connection is closed.
    fn on_disconnect(&mut self, ctx: &RequestContext) {}
}
```

## Global and Service Hooks

```rust
let app = AFast::new()
    .hook(LoggingHook)                   // Global: all handlers
    .service(
        service!("api" => { h(handler) })
            .hook(ApiSpecificHook)       // Service: only this service's handlers
    );
```

- **Global hooks** run for every handler across all services.
- **Service hooks** run only for handlers in that service.
- Both always execute — they never replace each other.
- Execution order: global first, then service (onion model 🧅).

## Hook Lifecycle by Transport

### Binary Protocol (HTTP `POST /_api`, WS `/_ws`, TCP)

```
before_request → handler → on_response / on_error
```

For long-connection binary handlers (`call_stream`):

```
on_connect → before_request → handler → on_response / on_error → on_disconnect
```

### Ordinary HTTP (`ordinary-http`)

```
before_request → handler → on_response / on_error
```

`on_connect` / `on_disconnect` are **not** called for ordinary HTTP (stateless request/response).

### Ordinary WebSocket (`ordinary-ws`)

```
before_request → on_connect → handler → on_response / on_error → on_disconnect
```

- `before_request`: fires when the HTTP upgrade request arrives.
- `on_connect`: fires after the WebSocket handshake completes.
- `on_response` / `on_error`: fires after the handler function returns.
- `on_disconnect`: fires after the handler returns and forwarding tasks are cleaned up.

### Ordinary SSE (`ordinary-sse`)

```
before_request → handler (spawned) → on_response
```

- `before_request`: fires before the SSE response is sent.
- `on_response`: fires immediately after spawning the handler (the HTTP 200 response is being sent).
- `on_error` / `on_disconnect` are **not** called (the handler runs in a spawned task; errors are logged to stderr).

## Hook Key — Route Matching

Hooks are matched by **`"service_name:route_path"`**, not by handler function name. This avoids conflicts when the same function name appears in different groups within the same service:

```rust
service!("admin" => {
    group("users" => {
        get("info", get_info),    // key: "admin:/users/info"
    }),
    group("posts" => {
        get("info", get_info),    // key: "admin:/posts/info" — no conflict!
    }),
})
```

For merged services (same service name registered multiple times), hook entries are automatically deduplicated.

## RequestContext Fields

| Field | Type | Description |
|-------|------|-------------|
| `handler_name` | `&'static str` | Handler function name |
| `handler_desc` | `&'static str` | Description from `#[handler(desc(...))]` |
| `transport` | `&'static str` | `"tcp"`, `"ws"`, `"http"`, or `"sse"` |
| `handler_id` | `usize` | Handler offset in binary dispatch table (0 for ordinary routes) |
| `state` | `Arc<StateMap>` | Shared application state |

## Supported Extractors

All extractors work across all transports:

| Extractor | HTTP | WS | SSE | TCP |
|-----------|:---:|:---:|:---:|:---:|
| `State<T>` | ✅ | ✅ | ✅ | ✅ |
| `Query<T>` | ✅ | ✅ | ✅ | — |
| `Param<T>` | ✅ | ✅ | ✅ | — |
| `Header<T>` | ✅ | ✅ | ✅ | — |
| `Body<T>` | ✅ | — | — | — |
| `Custom<T>` | — | — | — | ✅ |
| `Data` | — | — | — | ✅ |
| `WsSender` | — | ✅ | — | — |
| `WsReceiver` | — | ✅ | — | — |
| `SseSender` | — | — | ✅ | — |
| `Sender` | — | — | — | ✅ |
| `Receiver` | — | — | — | ✅ |

## Server Example Output

```
[hook] → chat_ws (ws)                ← before_request
[check-svc] ▶ chat_ws                ← service hook
[hook] ↕ connect: chat_ws (ws)       ← on_connect
[ws-chat] client joined room: test   ← handler runs
[ws-chat] client left room: test     ← handler returns
[hook] ← chat_ws OK (1.28ms)         ← on_response
[check-svc] ◀ chat_ws done           ← service hook done
[hook] ✕ disconnect: chat_ws (ws)    ← on_disconnect
```
