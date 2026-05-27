# AFast

[简体中文](./README.md) | English

AFast is a high-performance Rust web backend framework. It eliminates manual route
definitions — annotate functions with `#[handler]` and the framework auto-registers
and dispatches requests. Data transport uses a compact binary protocol that is
smaller and faster than JSON. It supports one-click generation of TypeScript,
JavaScript, and Kotlin client code, with built-in interactive API documentation.

## Features

- **Zero Route Definitions** — `#[handler]` annotation, no manual routing table
- **Compact Binary Protocol** — Smaller and faster than JSON, designed for internal communication
- **Auto Code Generation** — TypeScript / JavaScript / Kotlin clients with full type definitions
- **Interactive API Docs** — Built-in Web docs with dark/light theme and online API testing
- **Multiple Transports** — WebSocket, HTTP/1.1, HTTP/2, and TCP, mix and match as needed
- **TLS / HTTPS** — Based on rustls with ALPN for HTTP/2 negotiation. HTTP and HTTPS can run simultaneously
- **HTTP + WS Port Merging** — Same port serves both HTTP and WebSocket simultaneously
- **RESTful Endpoints** — Standard HTTP methods with Query/Param/Body/Header extractors
- **Long Connections** — Bidirectional persistent communication via Receiver/Sender over WS/TCP
- **Type-Safe Extractors** — `State<T>`, `Data<T>`, `Custom<T>` parameter injection
- **Multiple States** — Register multiple State types, keyed by generic type
- **Multiple Data Params** — Accept multiple `Data<T>` params, generating corresponding client args
- **Nested Handler Structure** — Organize APIs with `group`, auto-generating hierarchical paths
- **Recursive Type Discovery** — `#[derive(Tag)]` with function pointers, no global registry
- **Client Strategy Pattern** — Transport chosen at construction, immutable thereafter, zero overhead
- **Client-Side Caching** — `cache(seconds)` attribute, class-level static cache, returns cached data for identical params

## Quick Start

### Add Dependency

```toml
[dependencies]
afast = { version = "0.1.3", features = ["http", "ws", "ts"] }
tokio = { version = "1", features = ["full"] }
```

### Define a Handler

```rust
use afast::{AFast, handler, service, State, Data, Custom, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

#[derive(Clone)]
struct AppState {
    db_url: String,
}

#[derive(Clone)]
struct CacheState {
    redis_url: String,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Auth info")]
struct AuthCustom {
    token: i64,
    platform: String,
}

#[derive(AFastDeserialize, Tag)]
#[tag("Request body")]
struct HelloReq {
    name: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("Response body")]
struct HelloResp {
    message: String,
}

#[handler(desc("Say hello"), name("hello"))]
async fn hello(
    state: State<AppState>,
    cache: State<CacheState>,
    auth: Custom<AuthCustom>,
    req: Data<HelloReq>,
) -> Result<HelloResp> {
    println!("DB: {}, Cache: {}", state.db_url, cache.redis_url);
    Ok(HelloResp {
        message: format!("Hello, {}!", req.name),
    })
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

### Run

```bash
cargo run --features "http,ws,ts"
```

- WebSocket API: `ws://localhost:3000`
- HTTP API: `POST http://localhost:5000/_api`
- Generated TS client code in `./code/api.ts`

## Features (Detailed)

| Feature | Description | Dependencies |
|---------|-------------|-------------|
| `http` | HTTP server | hyper, hyper-util, http-body-util |
| `ws` | WebSocket server | tokio-tungstenite, futures-util |
| `tcp` | TCP server (length-prefix framing) | — |
| `ts` | TypeScript client generation (ESM + full types) | — |
| `js` | JavaScript client generation (ESM + JSDoc) | — |
| `kt` | Kotlin client generation | — |
| `code` | On-demand code generation at `/code/{service}/{lang}` | `http` |
| `doc` | Interactive API docs at `/doc` endpoint | `http`, `js` |
| `ordinary-http` | RESTful JSON endpoints (GET/POST/PUT/DELETE) | `http`, serde, serde_json |
| `seq64` | WS request ID uses `i64` (default `i32`) | — |
| `len64` | WS payload length uses `u64` (default `u32`) | — |
| `tag-u8` | Enum tag uses `u8` (default) | afastdata/tag-u8 |
| `tag-u16` | Enum tag uses `u16` | afastdata/tag-u16 |
| `tag-u32` | Enum tag uses `u32` | afastdata/tag-u32 |

**Note**: If the server uses `seq64` or `len64`, generated client code must use
the same feature, otherwise protocol mismatch will occur.

## Core Concepts

### Handler Registration

The `#[handler]` proc macro generates the following at compile time:

1. The original function unchanged
2. `HandlerMeta` — name, description, parameter list, return type metadata
3. `HandlerInvoker` trait impl — type-erased invoker, deserializes params, calls the function
4. A static invoker instance — referenced by the `register!` macro

```rust
#[handler(desc("Get user"), name("get_user"))]
async fn get_user_handler(
    state: State<AppState>,
    auth: Custom<Auth>,
    req: Data<UserIdRequest>,
) -> Result<UserInfo> {
    // ...
}
```

- `desc("...")` — Sets description used in docs and JSDoc comments
- `name("...")` — Overrides the client-side method name (defaults to the Rust function name)
- `cache(seconds)` — Enables client-side caching; requests with identical params within `seconds` return cached data

### Multiple States

AFast supports registering **multiple State types**. `StateMap` uses `TypeId` as
keys, with one value per type. Handlers access different States via multiple
`State<T>` parameters:

```rust
#[derive(Clone)]
struct DbConfig { url: String }

#[derive(Clone)]
struct RedisConfig { url: String }

#[derive(Clone)]
struct AppConfig { name: String }

// Register multiple State values
let app = AFast::new()
    .state(DbConfig { url: "postgres://...".into() })
    .state(RedisConfig { url: "redis://...".into() })
    .state(AppConfig { name: "my-app".into() });

// Access multiple States in a handler
#[handler(desc("Multiple State example"))]
async fn my_handler(
    db: State<DbConfig>,
    redis: State<RedisConfig>,
    config: State<AppConfig>,
) -> Result<()> {
    println!("DB: {}, Redis: {}, App: {}", db.url, redis.url, config.name);
    Ok(())
}
```

If a handler references a State type that was not registered, it returns a
`CODE_STATE_NOT_FOUND` error at runtime.

### Multiple Data Params

A handler can accept **multiple `Data<T>` parameters**, deserialized sequentially
from the binary payload. The generated client method presents them as separate
arguments:

```rust
#[derive(AFastDeserialize, Tag)]
#[tag("Pagination")]
struct PageRequest { page: i64, size: i64 }

#[derive(AFastDeserialize, Tag)]
#[tag("Filter")]
struct FilterRequest { keyword: String, status: i32 }

#[handler(desc("Search users"))]
async fn search_users(
    page: Data<PageRequest>,
    filter: Data<FilterRequest>,
) -> Result<PageResponse> {
    // ...
}
```

Generated TypeScript client method signature:

```typescript
async searchUsers(page: PageRequest, filter: FilterRequest): Promise<PageResponse>
```

Each `Data<T>` maps to one method parameter, in declaration order.

### Extractor Types

| Extractor | Description | Protocols |
|-----------|-------------|-----------|
| `State<T>` | Injects shared state by type from StateMap (T: Clone) | All |
| `Data<T>` | Deserializes request body from binary payload | HTTP/WS/TCP |
| `Custom<T>` | Deserializes client-side custom context (e.g., auth token) | HTTP/WS/TCP |
| `Receiver` | Receives binary messages from the client (long connection) | WS/TCP |
| `Sender` | Sends binary messages to the client (long connection) | WS/TCP |
| `Query<T>` | Deserializes from URL query string (requires `ordinary-http`) | HTTP |
| `Param<T>` | Deserializes from route path params (`:id`) (requires `ordinary-http`) | HTTP |
| `Body<T>` | Deserializes from HTTP JSON body (requires `ordinary-http`) | HTTP |
| `Header<T>` | Deserializes from HTTP request headers (requires `ordinary-http`) | HTTP |

### Services and Nesting

The `service!` macro builds handler trees with `group` for namespacing:

```rust
let api_svc = service!("api", "User API" => {
    h(health),
    group("user" => {
        h(list_users),
        h(get_user),
        group("posts" => {
            h(list_posts),
        }),
    }),
    group("chat" => {
        h(chat),  // Persistent connection using Receiver/Sender
    }),
});
```

Client namespace paths become `api.user.list_users`, `api.chat.chat`, etc.

Binary and ordinary HTTP routes can be mixed within a `group`:

```rust
group("user" => {
    h(get_user),                 // binary handler
    get(":id", get_user_by_id),  // GET /user/:id
    post("", create_user),       // POST /user
    delete(":id", delete_user),  // DELETE /user/:id
}),
```

### Type Tags

`#[derive(Tag)]` generates runtime type metadata for structs and enums. The
code generator recursively discovers nested types through `FieldMeta.structure`
function pointers — no global registry required.

```rust
use afast::Tag;

#[derive(Tag)]
#[tag("User role")]
enum Role {
    Admin,
    User { level: i32 },
    Guest { expires_at: i64 },
    Custom(String),
}

#[derive(Tag)]
#[tag("User info")]
struct User {
    name: String,
    role: Role,           // Auto-discovers Role fields recursively
    tags: Vec<String>,    // Vec element type auto-expanded
    avatar: Option<Vec<u8>>,
}
```

Validation rules via `#[afast(...)]`, generating client-side preflight checks:

| Rule | Example | Description |
|------|---------|-------------|
| `gt(value, code, "msg")` | `#[afast(gt(0, 400, "must > 0"))]` | Greater than |
| `gte(value, code, "msg")` | `#[afast(gte(1, 400, "must >= 1"))]` | Greater or equal |
| `lt(value, code, "msg")` | `#[afast(lt(100, 400, "must < 100"))]` | Less than |
| `lte(value, code, "msg")` | `#[afast(lte(99, 400, "must <= 99"))]` | Less or equal |
| `len(min, max, code, "msg")` | `#[afast(len(1, 20, 400, "len 1-20"))]` | Length constraint |
| `of(["a","b"], code, "msg")` | `#[afast(of(["a","b"], 400, "a or b"))]` | Enum of values |

## Transport Layer

### HTTP

HTTP server endpoints:

| Method | Path | Description |
|--------|------|-------------|
| POST | `/_api` | Binary handler dispatch |
| GET | `/_ws` | WebSocket upgrade (merged mode) |
| GET | `/code/{service}/{lang}` | On-demand code gen (requires `code`) |
| GET | `/doc` | API docs index (requires `doc`) |
| GET | `/doc/{service}` | Service-specific docs (requires `doc`) |
| * | Ordinary routes | RESTful endpoints (requires `ordinary-http`) |

**HTTP Response Format:**
- Success: `[0u8][0i64][data: bytes]`
- Error: `[1u8][code: i64][message: bytes]`

### WebSocket

WS frame format:

```
Request:  [req_id: SeqId][handler_id: u32][len: Len][payload]
Push:     [0: SeqId][conn_id: u32][len: Len][payload]
Heartbeat:[0xFFFFFFFF: SeqId][len: Len][conn_id1: u32]...
```

**WS Response Format:**
- Success: `[req_id: SeqId][len: Len][0u8][0i64][data]`
- Error: `[req_id: SeqId][len: Len][1u8][code: i64][message_bytes]`

`SeqId` type is controlled by the `seq64` feature (`i32` or `i64`), `Len` type
by the `len64` feature (`u32` or `u64`).

### TCP

TCP uses 4-byte big-endian length-prefix framing with complete binary payloads
per frame. Suitable for embedded devices or raw TCP scenarios.

### HTTP + WS Port Merging

When `ws_addr` and `http_addr` are set to the **same address**, AFast merges
WebSocket into the HTTP server via HTTP Upgrade, skipping a separate WS listener.

```rust
// Same port for both HTTP and WebSocket
let app = AFast::new()
    .service(svc)
    .ws("0.0.0.0:5000")
    .http("0.0.0.0:5000");  // Same address, auto-merged
```

```rust
// Separate ports
let app = AFast::new()
    .service(svc)
    .ws("0.0.0.0:3000")     // Dedicated WS port
    .http("0.0.0.0:5000");  // Dedicated HTTP port
```

In merged mode, clients connect via `ws://host:5000/_ws`. Generated client code
handles both modes automatically.

## Ordinary HTTP

With `ordinary-http`, define RESTful routes using `get`/`post`/`put`/
`patch`/`delete` inside the `service!` macro.

### Defining an Ordinary Handler

```rust
use afast::{get, Query, Param, Body, Header, Json, HttpResult};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct UserQuery {
    page: i64,
    size: i64,
}

#[derive(Serialize)]
struct UserResponse {
    id: i64,
    name: String,
}

#[get(":id")]
async fn get_user(
    state: State<AppState>,
    param: Param<UserPath>,
    query: Query<UserQuery>,
) -> HttpResult<Json<UserResponse>> {
    Ok(Json(UserResponse {
        id: param.id,
        name: format!("User {}", param.id),
    }))
}
```

### Response Types

| Type | HTTP Status | Content-Type |
|------|-------------|-------------|
| `Json<T>` | 200 | `application/json` |
| `Text` | 200 | `text/plain` |
| `Html` | 200 | `text/html` |
| `File` | 200 | Custom + `Content-Disposition: attachment` |
| `Status(code)` | Custom | — |
| `Redirect(url)` | 302 | `Location` header |
| `Result<T>` | 200 / Error code | `application/json` (on error) |

## Long Connections (Chat)

Handlers using `Receiver`/`Sender` are auto-detected as long-connection mode.
The server allocates a `conn_id`, carried in the initial response; subsequent
messages use push frames for bidirectional communication.

```rust
#[handler(desc("Chat"))]
async fn chat(
    state: State<AppState>,
    auth: Custom<Auth>,
    mut receiver: Receiver,
    sender: Sender,
) -> Result<()> {
    sender.send(b"Welcome!".to_vec()).await?;

    while let Some(msg) = receiver.recv().await {
        sender.send(msg).await?;  // echo
    }

    Ok(())
}
```

Generated clients return a `Socket` object for long-connection handlers, with
`send()`/`close()` and `onMessage` callback.

## Binary Protocol

### Type Mapping

| Rust Type | TS/JS Type | Kotlin Type |
|-----------|-----------|------------|
| `i8`~`i64`, `u8`~`u64`, `f32`, `f64` | `number` | `Int`/`Long`/`Float`/`Double` |
| `bool` | `boolean` | `Boolean` |
| `String`, `&str` | `string` | `String` |
| `Vec<u8>` | `Uint8Array` | `ByteArray` |
| `Option<T>` | `T \| null` | `T?` |
| `Vec<T>` | `T[]` | `List<T>` |
| struct | `{ field: Type }` | `data class` |
| enum | `{ tag: 'Variant', data: ... }` | `sealed class` |

### Error Codes

System reserved error codes range from `-90011` to `-90000`. User-defined errors
must not use this range.

| Constant | Value | Description |
|----------|-------|-------------|
| `CODE_SIGNAL` | -90000 | OS signal (e.g., Ctrl+C) |
| `CODE_MSG_TOO_SHORT` | -90001 | Message too short |
| `CODE_PAYLOAD_MISMATCH` | -90002 | Payload length mismatch |
| `CODE_SERIALIZE` | -90003 | Serialization/deserialization error |
| `CODE_STATE_NOT_FOUND` | -90004 | State type not registered |
| `CODE_HANDLER` | -90005 | Handler execution error |
| `CODE_INVALID_PARAM` | -90006 | Invalid parameter |
| `CODE_IO` | -90007 | I/O error |
| `CODE_WS` | -90008 | WebSocket error |
| `CODE_HTTP` | -90009 | HTTP error |
| `CODE_TCP` | -90010 | TCP error |
| `CODE_LONG_CONNECTION_NOT_SUPPORTED` | -90011 | Long connections unsupported in HTTP mode |

```rust
// Custom error (code must be outside the reserved range)
return Err(afast::Error::custom(400, "invalid request parameter"));
```

## Code Generation

### Static Generation (compile-time file output)

```rust
use afast::{GenerateTarget, Lang, JsTsCallType};

let app = AFast::new()
    .service(api_svc)
    .generate(vec![
        GenerateTarget {
            lang: Lang::TS(vec![JsTsCallType::Fetch, JsTsCallType::Ws]),
            path: "./code".into(),
            debug: false,
        },
        GenerateTarget {
            lang: Lang::JS(vec![JsTsCallType::Fetch, JsTsCallType::Ws]),
            path: "./code".into(),
            debug: true,
        },
    ]);
```

### Dynamic Generation (HTTP endpoint)

```
GET /code/api/ts?call=fetch,ws
GET /code/api/js?call=fetch,ws
GET /code/pay/kt?call=http,ws,tcp
```

### Supported Transport Types

**TS/JS:**

| Value | API |
|-------|-----|
| `fetch` | Browser `fetch` |
| `ws` | Browser `WebSocket` |
| `nodetcp` | Node.js `net` |
| `buntcp` | Bun `Bun.connect` |
| `unirequest` | UniApp `uni.request` |
| `uniws` | UniApp `uni.connectSocket` |
| `wxrequest` | WeChat Mini Program `wx.request` |
| `wxws` | WeChat Mini Program `wx.connectSocket` |

**Kotlin:**

| Value | API |
|-------|-----|
| `http` / `fetch` | `java.net.HttpURLConnection` |
| `ws` | `java.net.http.WebSocket` |
| `tcp` | `java.net.Socket` |

### Client Usage

```typescript
import { ApiClient } from './api';

// Dedicated WS port
const wsClient = new ApiClient('ws://localhost:3000');
const wsResult = await wsClient.apis.user.list_users({ page: 1, size: 20 });

// Dedicated HTTP port
const httpClient = new ApiClient('http://localhost:5000');
const httpResult = await httpClient.apis.user.list_users({ page: 1, size: 20 });

// Merged mode (WS and HTTP on the same port)
const mergedClient = new ApiClient('ws://localhost:5000');
// Auto-connects to ws://localhost:5000/_ws
```

The client transport mode is fixed at construction time. Each transport type
has a strategy method (`_callWs` / `_callHttp` / `_callTcp`), dispatched via
function reference at runtime.

**JavaScript** uses JSDoc for type hints:

```javascript
/**
 * @typedef {Object} PageRequest
 * @property {number} page
 * @property {number} size
 */

/**
 * @typedef {Function} UserFn_ListUsers
 * @param {PageRequest} request
 * @returns {Promise<PageResponse>}
 */
```

Nested types are fully expanded — never replaced with `Object`.

## Client-Side Caching

The `cache(seconds)` attribute enables client-side caching for handlers, reducing redundant requests.

### Server-Side

Both `#[handler]` and ordinary HTTP macros (`#[get]`, `#[post]`, etc.) support `cache(seconds)`:

```rust
#[handler(desc("List users"), cache(60))]
async fn list_users(
    state: State<AppState>,
    req: Data<ListUsersRequest>,
) -> Result<ListUsersResponse> {
    // ...
}
```

### Generated Client Methods

```typescript
// Cached for 60 seconds, force defaults to false
const users = await client.apis.admin.listUsers({ page: 1, size: 20 });
// Within 60 seconds, same params return cached data without a network request

// force = true forces a fresh fetch
const fresh = await client.apis.admin.listUsers({ page: 1, size: 20 }, true);
```

### Cache Strategy

- **Class-level** — Cache is stored on the class `static _cache`, shared across all instances
- **Param-aware** — Cache key is composed of method name + serialized params; changed params trigger a fresh request
- **Lazy caching** — `force = false` with valid cache returns immediately; `force = true` bypasses cache
- **Cross-language** — TypeScript, JavaScript, and Kotlin clients use the same caching strategy

TS client cache structure:

```typescript
private static _cache = new Map<string, { data: any; expiry: number }>();
```

JS client:

```javascript
/** @type {Map<string, { data: any; expiry: number }>} */
static _cache = new Map();
```

Kotlin client:

```kotlin
companion object {
    private val _cache = mutableMapOf<String, Pair<Long, Any?>>()
}
```

## About TextEncoder / TextDecoder

The generated client code (including Socket and binary serialization) uses the
`TextEncoder` and `TextDecoder` APIs. These are **unavailable** on:

- **React Native** (all versions)
- **WeChat Mini Programs** (non-standard Web API environment)
- **Older browsers** (Chrome < 38, Firefox < 19, Safari < 10.1, all IE versions)

This can be resolved by manually implementing `TextEncoder` and `TextDecoder` on global.

### Solutions

1. **Use a polyfill** (recommended):

```bash
npm install text-encoding
```

```javascript
import 'text-encoding';  // At your application entry point
```

2. **React Native**: RN 0.72+ has built-in support. For older versions:

```javascript
import { polyfill as polyfillEncoding } from 'react-native-polyfill-globals/src/encoding';
polyfillEncoding();
```

3. **WeChat Mini Programs / UniApp**: Use `wxrequest` / `wxws` / `unirequest` /
`uniws` transport types — generated code uses platform-native APIs.

4. **Custom replacement**: Replace the `_writer` (encode) and `_reader` (decode)
methods in the generated client with your platform's native serialization.

Where TextEncoder/TextDecoder is needed:
- **Socket class** `send()` — encoding strings/Uint8Arrays into binary frames
- **Binary protocol** — `Data<T>` / `Custom<T>` serialization/deserialization
- Plain JSON HTTP (`Body<T>`) does NOT require TextEncoder

## Interactive Documentation

With the `doc` feature, visit `http://host:port/doc` for interactive API docs:

```rust
let app = AFast::new()
    .service(svc)
    .document(afast::DocConfig::new()
        .title("My API Documentation")
        .output("./docs")  // Also write static HTML to disk
    )
    .http("0.0.0.0:5000");
```

- `GET /doc` — Index page listing all services
- `GET /doc/{service}` — Service docs with type definitions and online test panel
- Dark/light theme toggle
- Online test panel can send real requests to the server

## Project Structure

```
afast/           — Main framework crate (core types, State, transports, code generation)
afast-macros/    — Proc macros (#[handler], register!, #[derive(Tag)])
example/         — Example project (full usage including HTTP, WS, TCP, docs)
```

### Dependencies

- `afast` → `afast-macros`, `afastdata`, `tokio`
- `afast-macros` → `syn`, `quote`, `proc-macro2`
- User crates indirectly depend on `afastdata-core` (referenced by `#[derive(Tag)]` expanded code)

## Testing

```bash
# Start the example server
cargo run -p example
```

## License

MIT
