# Request Context (`Ctx`)

The `Ctx<T>` extractor provides per-request (or per-connection) typed data that flows through the entire handler lifecycle. Unlike `State<T>` which is application-global, `Ctx<T>` is scoped to a single request.

## Core Concepts

| | `State<T>` | `Ctx<T>` |
|---|---|---|
| Scope | Application-global | Per-request / per-connection |
| Storage | `StateMap` (set once at startup) | `RequestCtx` (created per request) |
| Lifetime | Entire application | Request start → fully finished |
| Use case | Database pool, config | Request ID, auth info, timing |
| Set by | `AFast::state()` | Hooks (`before_request`, `on_connect`) |

## How It Works

```
Request arrives
    │
    ▼
RequestCtx::new()          ← framework creates empty context
    │
    ▼
Hook: before_request()     ← hook inserts values: ctx.ctx.insert(RequestId(...))
    │
    ▼
Handler executes           ← framework extracts: Ctx<RequestId> from context
    │
    ▼
Hook: on_response()        ← hook reads values: ctx.ctx.get::<RequestId>()
    │
    ▼
RequestCtx dropped         ← all values freed
```

For long-connection handlers (WS/TCP), the `RequestCtx` lives for the entire connection:

```
Connection established
    │
    ▼
RequestCtx::new()          ← created once
    │
    ▼
Hook: on_connect()         ← hook inserts connection-scoped data
    │
    ▼
Message 1: handler executes  ← reads Ctx<T>
Message 2: handler executes  ← same Ctx<T> (shared across messages)
    ...
    │
    ▼
Hook: on_disconnect()      ← hook reads final state
    │
    ▼
RequestCtx dropped
```

## Quick Example

### 1. Define your context data

```rust
#[derive(Clone)]
pub struct RequestInfo {
    pub request_id: String,
    pub started_at: std::time::Instant,
}
```

The type must be `Clone + Send + Sync + 'static` (for extraction via `Ctx<T>`).

### 2. Create a hook that inserts data

```rust
use afast::hook::{Hook, RequestContext, RequestGuard};

struct CtxHook;

impl Hook for CtxHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        ctx.ctx.insert(RequestInfo {
            request_id: format!("req-{:08x}", /* ... */),
            started_at: std::time::Instant::now(),
        });
        None  // No guard needed if only writing context
    }
}
```

### 3. Use in handlers

```rust
use afast::{Ctx, handler};

#[handler(desc("My handler"))]
async fn my_handler(ctx: Ctx<RequestInfo>) -> afast::Result<MyResp> {
    let elapsed = ctx.0.started_at.elapsed();
    println!("request {} took {:?}", ctx.0.request_id, elapsed);
    // ...
}
```

`ctx.0` gives you the inner `RequestInfo` value.

## Supported Handler Types

`Ctx<T>` works in **all** handler types:

| Handler Type | Macro | Example |
|---|---|---|
| Binary protocol | `#[handler]` | `async fn h(ctx: Ctx<Info>) -> Result<T>` |
| HTTP ordinary | `#[get]` / `#[post]` / etc. | `async fn h(ctx: Ctx<Info>) -> HttpResult<Json<T>>` |
| WebSocket | `#[ws]` | `async fn h(ctx: Ctx<Info>, sender: WsSender) -> Result<()>` |
| SSE | `#[sse]` | `async fn h(ctx: Ctx<Info>, sender: SseSender) -> Result<()>` |
| Long-connection | `#[handler]` + `Receiver`/`Sender` | `async fn h(ctx: Ctx<Info>, rx: Receiver, tx: Sender)` |

`Ctx<T>` does **not** participate in the binary/ordinary mutual exclusion check, so it can be combined with any other extractor.

## Parameter Position

`Ctx<T>` can be placed at any position in the parameter list. By convention, put it first:

```rust
#[handler(desc("..."))]
async fn my_handler(
    ctx: Ctx<RequestInfo>,          // ← context first
    state: State<AppState>,          // ← then state
    data: Data<MyReq>,               // ← then data
) -> afast::Result<MyResp> {
    // ...
}
```

## Reading and Writing in Hooks

Hooks interact with the context via `RequestContext::ctx`:

```rust
impl Hook for MyHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        // Write
        ctx.ctx.insert(MyData { value: 42 });

        // Read (useful if another hook wrote earlier)
        // ctx.ctx.get::<OtherData>()

        Some(Box::new(MyGuard))
    }
}

impl RequestGuard for MyGuard {
    fn on_response(&mut self, ctx: &RequestContext, _resp: &[u8]) {
        // Read what the handler or earlier hooks wrote
        if let Some(data) = ctx.ctx.get::<MyData>() {
            println!("value was {}", data.value);
        }
    }
}
```

## API Reference

### `RequestCtx` — Container

```rust
// Create empty context
let ctx = RequestCtx::new();

// Insert a value (keyed by type)
ctx.insert(my_value);

// Retrieve a cloned value
let val: Option<MyType> = ctx.get::<MyType>();

// Clone (cheap — shares Arc)
let ctx2 = ctx.clone();
```

### `Ctx<T>` — Extractor

```rust
pub struct Ctx<T>(pub T);

// Access the inner value
let inner: T = my_ctx.0;
```

## Performance

- `RequestCtx::new()` allocates a single `Arc<RwLock<HashMap>>` — very cheap.
- `insert()` and `get()` acquire a `RwLock` — uncontended in the typical case (sequential write then read), so overhead is negligible.
- `RequestCtx::clone()` is an `Arc` clone — O(1).
- For large context values, wrap in `Arc<T>` to make `get()` clone cheap.
- Handlers that don't use `Ctx<T>` pay zero extraction cost — the empty context is created but never read.
