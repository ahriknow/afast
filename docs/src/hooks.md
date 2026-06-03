# Lifecycle Hooks

Enable the `hook` feature to intercept request lifecycle events for observability, tracing, logging, or custom middleware.

## Example

```rust
use afast::hook::{Hook, RequestContext, RequestGuard};

struct TimingHook;

impl Hook for TimingHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        println!("→ {} ({})", ctx.handler_name, ctx.transport);
        Some(Box::new(std::time::Instant::now()))
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
```

## Global and Service Hooks

```rust
let app = AFast::new()
    .hook(TimingHook)                    // Global: all handlers
    .service(
        service!("api" => { h(handler) })
            .hook(ApiSpecificHook)       // Service: only this service's handlers
    );
```

- **Global hooks** run for every handler.
- **Service hooks** run only for handlers in that service.
- Both always execute — they never replace each other.
- Execution order: global first, then service (onion model 🧅).
  - `before_request`: Hook1 → Hook2 → handler
  - `on_response`/`on_error`: Guard2 → Guard1 (reversed)

## Long Connection Hooks

For WebSocket/TCP long-connection handlers, the full lifecycle is:

```
on_connect → before_request → handler → on_response/on_error → on_disconnect
```

## RequestContext Fields

| Field | Type | Description |
|-------|------|-------------|
| `handler_name` | `&'static str` | Handler function name |
| `handler_desc` | `&'static str` | Description from `#[handler(desc(...))]` |
| `transport` | `&'static str` | `"tcp"`, `"ws"`, or `"http"` |
| `handler_id` | `usize` | Handler offset in dispatch table |
| `state` | `Arc<StateMap>` | Shared application state |
