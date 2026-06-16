# AFast Development Guide for AI

This document is specifically designed for AI assistants to understand how to develop with the AFast framework effectively.

## Core Rules

1. **Always use `#[handler]` macro** — Never manually register routes
2. **Use `Result<T>` for error handling** — All handlers must return `Result<T>`
3. **Use `State<T>` for shared state** — Zero-copy, no clone needed
4. **Use `Data<T>` for request body** — Auto deserialization from binary payload
5. **Use `Custom<T>` for auth in binary protocol** — Client-provided authentication
6. **Use `Header<T>` for auth in HTTP routes** — HTTP request header extraction
7. **Use `Ctx<T>` for request context** — Set by hooks, read by handlers
8. **All types must derive `Tag`** — Request/response types need `#[derive(Tag)]` and `#[tag("desc")]`
9. **Parameters must use destructuring syntax** — `afast::X(name): afast::X<Type>`

## Handler Signature Pattern

**Important: Parameters MUST use destructuring syntax `afast::X(name): afast::X<Type>`**

### Binary Protocol Handler (Basic)

```rust
use afast::{AFastDeserialize, AFastSerialize, Tag, handler};
use crate::state::AppState;

#[derive(AFastDeserialize, Tag)]
#[tag("Request body description")]
pub struct MyRequest {
    #[tag("Field description")]
    pub name: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("Response body description")]
pub struct MyResponse {
    #[tag("Field description")]
    pub message: String,
}

#[handler(desc("Describe what this handler does"))]
pub async fn my_handler(
    afast::State(state): afast::State<AppState>,
    afast::Data(req): afast::Data<MyRequest>,
) -> afast::Result<MyResponse> {
    let db = state.db.lock().await;
    Ok(MyResponse { message: format!("Hello, {}!", req.name) })
}
```

### Binary Handler with Authentication (Custom)

```rust
use afast::{AFastDeserialize, AFastSerialize, Tag, handler};

#[derive(AFastDeserialize, Tag)]
#[tag("Authentication token")]
pub struct AuthCustom {
    #[tag("Bearer token")]
    pub token: String,
}

#[handler(desc("Protected endpoint"))]
pub async fn protected(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<MyRequest>,
) -> afast::Result<MyResponse> {
    // auth.token is available
    Ok(MyResponse { message: "Authorized".into() })
}
```

### Binary Handler with Request Context (Ctx)

```rust
use afast::{Tag, handler};

#[derive(Clone, Debug)]
pub struct RequestInfo {
    pub request_id: String,
}

#[handler(desc("Endpoint with context"))]
pub async fn with_context(
    afast::Ctx(ctx): afast::Ctx<RequestInfo>,
    afast::State(state): afast::State<AppState>,
) -> afast::Result<MyResponse> {
    println!("Request ID: {}", ctx.request_id);
    Ok(MyResponse { message: "Done".into() })
}
```

### HTTP REST Handler (Ordinary HTTP)

HTTP handlers use `Header<T>` for auth and return `HttpResult<Json<T>>`:

```rust
use afast::{get, post, put, delete, Tag};
use serde::{Deserialize, Serialize};

// HTTP auth uses Header, not Custom
#[derive(Debug, Deserialize, Tag)]
#[tag("HTTP auth header")]
pub struct AuthHeader {
    #[tag("Authorization header")]
    pub authorization: String,
}

impl AuthHeader {
    pub fn token(&self) -> &str {
        self.authorization.strip_prefix("Bearer ").unwrap_or(&self.authorization)
    }
}

#[derive(Debug, Deserialize, Tag)]
#[tag("Query parameters")]
pub struct ListUsersQuery {
    #[tag("Page number")]
    pub page: Option<i64>,
    #[tag("Items per page")]
    pub size: Option<i64>,
}

#[derive(Debug, Serialize, Tag)]
#[tag("User HTTP response")]
pub struct UserHttp {
    #[tag("User ID")]
    pub id: i64,
    #[tag("Username")]
    pub username: String,
}

#[derive(Debug, Serialize, Tag)]
#[tag("User list response")]
pub struct ListUsersHttpResponse {
    #[tag("Total count")]
    pub total: i64,
    #[tag("User list")]
    pub items: Vec<UserHttp>,
}

// Note: #[get] contains desc, NOT a path! Path is defined in service! macro
#[get(desc("List users via HTTP"))]
pub async fn list_users_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Query(query): afast::Query<ListUsersQuery>,
) -> afast::HttpResult<afast::Json<ListUsersHttpResponse>> {
    let db = state.db.lock().await;
    let _user_id = db.get_user_id_by_token(auth.token()).await
        .ok_or_else(|| afast::Error::custom(401, "invalid token"))?;
    let items = db.read(0, 100).await;
    Ok(afast::Json(ListUsersHttpResponse { total: items.len() as i64, items: vec![] }))
}
```

## Service Registration

Paths are defined in the `service!` macro, NOT in handler attributes:

```rust
use afast::{AFast, service};

// Binary handlers are wrapped in h()
// HTTP handler paths are defined in service!
let admin_svc = service!("admin", "Admin Service" => {
    group("user" => {
        // Binary protocol handlers
        h(create_user),
        h(list_users),
        // HTTP REST handlers — paths defined here
        get("", list_users_http),
        post("", create_user_http),
        group(":user_id" => {
            get("", get_user_http),
            put("", update_user_http),
            delete("", delete_user_http),
        })
    })
});

// Catch-all route
let check_svc = service!("check", "Check Service" => {
    h(health),
    get("*", catch_all_get),
});

#[tokio::main]
async fn main() {
    AFast::new()
        .state(AppState::new())
        .service(admin_svc)
        .service(check_svc)
        .http("0.0.0.0:5000")
        .run()
        .await
        .unwrap();
}
```

## AppState Pattern

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
}

impl AppState {
    pub fn new() -> Self {
        Self { db: Arc::new(Mutex::new(Database::new())) }
    }
}
```

## Type Derivation Rules

**Binary protocol types** — for `Data<T>` and return values in `#[handler]`:

```rust
#[derive(AFastDeserialize, Tag)]
#[tag("Request description")]
pub struct MyRequest {
    #[tag("Field description")]
    pub field: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("Response description")]
pub struct MyResponse {
    #[tag("Field description")]
    pub field: String,
}
```

**HTTP types** — for `Body<T>`, `Query<T>` and return values in `#[get]`/`#[post]` (requires serde):

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Tag)]
#[tag("HTTP request body")]
pub struct MyBody {
    #[tag("Field description")]
    pub field: String,
}

#[derive(Debug, Serialize, Tag)]
#[tag("HTTP response")]
pub struct MyHttpResponse {
    #[tag("Field description")]
    pub field: String,
}
```

**Enum types** — generates tagged union in TypeScript:

```rust
#[derive(AFastSerialize, AFastDeserialize, Tag)]
#[tag("User role")]
pub enum Role {
    #[tag("Administrator")]
    Admin,
    #[tag("Regular user")]
    User,
    #[tag("Guest")]
    Guest,
}
// Generated TS: type Role = { tag: 'Admin', data: null } | { tag: 'User', data: null } | ...
```

## Error Handling

Use `afast::Error::custom(code, message)` to return custom errors:

```rust
#[handler(desc("Handler with error handling"))]
pub async fn with_error(
    afast::State(state): afast::State<AppState>,
) -> afast::Result<MyResponse> {
    if some_condition {
        return Err(afast::Error::custom(400, "Bad request"));
    }
    
    let data = state.db.lock().await.query().await
        .map_err(|e| afast::Error::custom(500, e.to_string()))?;
    
    Ok(MyResponse { message: "Success".into() })
}
```

## HTTP Methods (Ordinary Routes)

Key differences between HTTP and binary handlers:
- Auth uses `Header<T>` instead of `Custom<T>`
- Returns `afast::HttpResult<afast::Json<T>>` instead of `afast::Result<T>`
- Types need `serde::Deserialize`/`serde::Serialize` + `Tag`
- Paths are defined in `service!` macro, NOT in `#[get]` attributes

```rust
use afast::{get, post, put, delete, Tag};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Tag)]
#[tag("Query parameters")]
pub struct ListUsersQuery {
    #[tag("Page number")]
    pub page: Option<i64>,
    #[tag("Items per page")]
    pub size: Option<i64>,
}

#[derive(Debug, Deserialize, Tag)]
#[tag("Create user request")]
pub struct CreateUserBody {
    #[tag("Username")]
    pub username: String,
    #[tag("Password")]
    pub password: String,
    #[tag("Display name")]
    pub name: String,
}

#[derive(Debug, Deserialize, Tag)]
#[tag("User ID path parameter")]
pub struct UserIdParam {
    #[tag("User ID")]
    pub user_id: i64,
}

// Paths are defined in service! macro, only desc here
#[get(desc("List users"))]
pub async fn list_users_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Query(query): afast::Query<ListUsersQuery>,
) -> afast::HttpResult<afast::Json<ListUsersHttpResponse>> {
    // GET /user?page=1&size=10
    Ok(afast::Json(ListUsersHttpResponse { total: 0, items: vec![] }))
}

#[post(desc("Create user"))]
pub async fn create_user_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Body(body): afast::Body<CreateUserBody>,
) -> afast::HttpResult<afast::Json<CreateUserHttpResponse>> {
    // POST /user with JSON body
    Ok(afast::Json(CreateUserHttpResponse { id: 1 }))
}

// Registration in service!:
// service!("admin", "Admin Service" => {
//     group("user" => {
//         get("", list_users_http),           // GET /user
//         post("", create_user_http),         // POST /user
//         group(":user_id" => {
//             get("", get_user_http),         // GET /user/:user_id
//             put("", update_user_http),      // PUT /user/:user_id
//             delete("", delete_user_http),   // DELETE /user/:user_id
//         })
//     })
// })
```

## WebSocket Handlers

Path is defined in `service!` macro:

```rust
use afast::ws;
use afast::extractors::{WsSender, WsReceiver};

#[ws(desc("Chat WebSocket"))]
pub async fn chat_ws(
    afast::State(state): afast::State<AppState>,
    sender: WsSender,
    receiver: WsReceiver,
) {
    while let Some(msg) = receiver.recv().await {
        sender.send(msg).await;
    }
}

// Registration in service!:
// let chat_svc = service!("chat", "Chat Service" => {
//     ws("/chat/:room", chat_ws),
// });
```

## SSE (Server-Sent Events)

Path is defined in `service!` macro:

```rust
use afast::sse;
use afast::extractors::SseSender;

#[sse(desc("Event stream"))]
pub async fn sse_stream(sender: SseSender) {
    for i in 0..10 {
        sender.send(format!("Event {}", i)).await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

// Registration in service!:
// let chat_svc = service!("chat", "Chat Service" => {
//     sse("/sse", sse_stream),
// });
```

## Lifecycle Hooks

```rust
use afast::hook::{Hook, RequestContext, RequestGuard, ConnectionGuard};

struct MyHook;

impl Hook for MyHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        eprintln!("→ {} ({})", ctx.handler_name, ctx.transport);
        Some(Box::new(MyTimer(std::time::Instant::now())))
    }
    
    fn on_connect(&self, ctx: &RequestContext) -> Option<Box<dyn ConnectionGuard>> {
        eprintln!("↕ connect: {}", ctx.handler_name);
        Some(Box::new(MyConnGuard))
    }
}

struct MyTimer(std::time::Instant);
struct MyConnGuard;

impl RequestGuard for MyTimer {
    fn on_response(&mut self, ctx: &RequestContext, _resp: &[u8]) {
        eprintln!("← {} OK ({:?})", ctx.handler_name, self.0.elapsed());
    }
    fn on_error(&mut self, ctx: &RequestContext, err: &afast::Error) {
        eprintln!("✗ {} error: {}", ctx.handler_name, err);
    }
}

impl ConnectionGuard for MyConnGuard {
    fn on_disconnect(&mut self, ctx: &RequestContext) {
        eprintln!("✕ disconnect: {}", ctx.handler_name);
    }
}

// Register in main:
// AFast::new().hook(MyHook)
```

## Rate Limiting

Use `rate_limit("policy_name")` in `#[handler]` attribute, and configure the policy in `main`:

```rust
use afast::RateLimitConfig;

#[handler(desc("Rate limited endpoint"), rate_limit("api_limit"))]
pub async fn limited_endpoint(
    afast::State(state): afast::State<AppState>,
) -> afast::Result<MyResponse> {
    Ok(MyResponse { message: "Success".into() })
}

// Configure rate limiting in main:
// AFast::new()
//     .rate_limit(RateLimitConfig::new()
//         .policy("api_limit", afast::RateLimitPolicy::fixed_window(100, 60)))
```

## Client Code Generation

After starting the server, generate client code per service:

```bash
# TypeScript
curl http://localhost:5000/code/auth/ts

# JavaScript
curl http://localhost:5000/code/auth/js

# Kotlin
curl http://localhost:5000/code/auth/kt

# Rust
curl http://localhost:5000/code/auth/rs
```

Where `auth` is the service name. Each service generates a separate client file.

## Client Usage Examples

### TypeScript / JavaScript Client

The generated TS client creates a `Client` class per service, exposing all handler methods via the `apis` property.

```typescript
import { AuthClient } from './auth';
import { AdminClient } from './admin';

// ── Binary protocol client (fetch mode, most common) ──
const auth = new AuthClient({
    host: 'localhost',
    port: 5001,
    tls: false,
    transport: 'fetch',  // 'fetch' | 'ws' | 'nodetcp' | 'buntcp'
    debug: true,         // prints request/response logs
    customs: {
        // Custom<T> extractor requires a Promise-returning factory
        AuthCustom: async () => ({ token: myToken })
    }
});
await auth.apis._ready;  // wait for connection (fetch is instant)

// Call binary handler — fully typed params and return values
const reg = await auth.apis.signup({
    username: 'alice',
    password: 'secret',
    name: 'Alice'
});
console.log(reg.user.username);  // 'alice'
console.log(reg.token);

const login = await auth.apis.login({
    username: 'alice',
    password: 'secret'
});

// Handler with no params
const uid = await auth.apis.get_user_id();

// ── Service with both binary and HTTP routes ──
const admin = new AdminClient({
    host: 'localhost',
    port: 5001,
    tls: false,
    transport: 'fetch',
    customs: {
        AuthCustom: async () => ({ token: myToken })
    },
    headers: {
        // Header<T> for HTTP route auth
        AuthHeader: async () => ({ authorization: `Bearer ${myToken}` })
    }
});
await admin.apis._ready;

// Binary handler — accessed via nested group
const users = await admin.apis.user.list_users({ page: 1, size: 10 });
console.log(users.total, users.items);

const newUser = await admin.apis.user.create_user({
    username: 'bob', password: 'pw', name: 'Bob'
});

// HTTP REST handler — accessed via nested group
const httpUsers = await admin.apis.user.list_users_http({
    queries: { page: 1, size: 10 }  // Query params go in queries
});

const created = await admin.apis.user.create_user_http({
    body: { username: 'charlie', password: 'pw', name: 'Charlie' }  // Body goes in body
});

// Path params are extracted from the param object
const updated = await admin.apis.user.user_id.update_user_http({
    user_id: 123,  // path param :user_id
    body: { name: 'Updated', age: 25, active: true }
});

// ── WebSocket mode (supports long connections and push) ──
const wsAuth = new AuthClient({
    host: 'localhost',
    port: 3001,
    tls: false,
    transport: 'ws',
    customs: { AuthCustom: async () => ({ token: myToken }) }
});
await wsAuth.apis._ready;
// Same API, binary frames over WebSocket
const result = await wsAuth.apis.login({ username: 'alice', password: 'secret' });
```

### Kotlin Client

```kotlin
import afast.generated.*

fun main() = runBlocking {
    val auth = AuthClient(
        host = "localhost",
        port = 5001,
        tls = false,
        transport = "http",  // "http" | "ws" | "tcp"
        customFns = AuthCustomFns(
            AuthCustom = { AuthCustom(token = myToken) }
        )
    )

    // suspend functions, call directly in coroutine
    val reg = auth.apis.signup(RegisterRequest(
        username = "alice",
        password = "secret",
        name = "Alice"
    ))
    println(reg.user.username)  // "alice"
    println(reg.token)

    val login = auth.apis.login(LoginRequest(
        username = "alice",
        password = "secret"
    ))

    val uid = auth.apis.get_user_id()

    // Service with HTTP routes
    val admin = AdminClient(
        host = "localhost",
        port = 5001,
        tls = false,
        transport = "http",
        customFns = AdminCustomFns(
            AuthCustom = { AuthCustom(token = myToken) }
        ),
        headerFns = AdminHeaderFns(
            AuthHeader = { AuthHeader(authorization = "Bearer $myToken") }
        )
    )

    // Binary handler
    val users = admin.apis.user.list_users(ListUsersRequest(page = 1, size = 10))

    // HTTP REST handler
    val httpUsers = admin.apis.user.list_users_http(
        queries = UserListUsersHttpQuery(page = 1, size = 10)
    )
}
```

### Client Type Generation Rules

| Rust Type | TypeScript Type | Notes |
|-----------|----------------|-------|
| `String` | `string` | |
| `i32/i64/u32/u64/f32/f64` | `number` | |
| `bool` | `boolean` | |
| `Vec<T>` | `T[]` | |
| `Option<T>` | `T \| null` | |
| `HashMap<K,V>` | `Record<K,V>` | |
| `Vec<u8>` | `Uint8Array` | |
| `enum { A, B }` | `{ tag: 'A', data: null } \| { tag: 'B', data: null }` | Tagged union |
| `#[handler(name("signup"))]` | `apis.signup(...)` | Client method name |
| `group("user" => { ... })` | `apis.user.xxx(...)` | Nested group |

### Supported Client Transports

| Transport | Description | Use Case |
|-----------|-------------|----------|
| `fetch` | HTTP/1.1 or HTTP/2 | Browser, Node.js (most common) |
| `ws` | WebSocket binary frames | Browser, Node.js long connections |
| `nodetcp` | Node.js TCP | Node.js high-performance |
| `buntcp` | Bun TCP | Bun runtime |
| `unirequest` | uni-app HTTP | Mini programs / mobile apps |
| `uniws` | uni-app WebSocket | Mini programs long connections |
| `wxrequest` | WeChat Mini Program HTTP | WeChat mini programs |
| `wxws` | WeChat Mini Program WebSocket | WeChat mini program long connections |

## Common Mistakes to Avoid

1. **Don't forget `Tag` derive** — All types used in handlers must derive `Tag` with `#[tag("description")]` on fields
2. **Don't use `String` for request body** — Always use `Data<T>` with proper struct
3. **Don't forget `Result` return type** — Binary handlers return `afast::Result<T>`, HTTP handlers return `afast::HttpResult<afast::Json<T>>`
4. **Don't manually register routes** — Use `#[handler]` macro
5. **Don't clone State** — `State<T>` holds `&'static T`, just use it directly
6. **Parameters must use destructuring** — Write `afast::State(state): afast::State<AppState>` not `state: State<AppState>`
7. **HTTP auth uses Header** — HTTP handlers use `Header<T>` for auth, not `Custom<T>`
8. **HTTP types need serde** — HTTP handler request/response types need `Deserialize`/`Serialize` + `Tag`
9. **`#[get]` takes desc not path** — Paths are defined in `service!` macro
10. **Path params use `:param` in service!** — e.g. `group(":user_id" => { get("", handler) })`

## Project Structure Template

```
my-project/
├── Cargo.toml
└── src/
    ├── main.rs          # Entry point, service! definitions and AFast config
    ├── state.rs         # AppState + Database definitions
    └── handler/
        ├── mod.rs
        ├── auth.rs      # Authentication handlers
        ├── admin.rs     # Admin handlers (including HTTP routes)
        └── chat.rs      # WebSocket/SSE handlers
```

## Quick Reference

| Macro | Purpose | Example |
|-------|---------|---------|
| `#[handler]` | Binary protocol handler | `#[handler(desc("..."), name("..."), cache(60), rate_limit("..."))]` |
| `#[get]` | HTTP GET | `#[get(desc("..."))]` — path in service! |
| `#[post]` | HTTP POST | `#[post(desc("..."))]` |
| `#[put]` | HTTP PUT | `#[put(desc("..."))]` |
| `#[delete]` | HTTP DELETE | `#[delete(desc("..."))]` |
| `#[ws]` | WebSocket | `#[ws(desc("..."))]` |
| `#[sse]` | SSE | `#[sse(desc("..."))]` |
| `service!` | Create service with routes | `service!("name", "desc" => { h(fn), get("path", fn) })` |
| `h()` | Register binary handler | `h(my_handler)` |
| `group()` | Route grouping | `group("user" => { get("", fn), group(":id" => { get("", fn) }) })` |

| Extractor | Purpose | Destructuring Syntax |
|-----------|---------|---------------------|
| `State<T>` | Shared app state | `afast::State(state): afast::State<AppState>` |
| `Data<T>` | Binary request body | `afast::Data(req): afast::Data<MyRequest>` |
| `Custom<T>` | Binary auth context | `afast::Custom(auth): afast::Custom<AuthCustom>` |
| `Ctx<T>` | Request context (hook-set) | `afast::Ctx(ctx): afast::Ctx<RequestInfo>` |
| `Query<T>` | HTTP query parameters | `afast::Query(q): afast::Query<MyQuery>` |
| `Param<T>` | HTTP path parameters | `afast::Param(p): afast::Param<MyParam>` |
| `Body<T>` | HTTP request body | `afast::Body(b): afast::Body<MyBody>` |
| `Header<T>` | HTTP request headers | `afast::Header(h): afast::Header<AuthHeader>` |
