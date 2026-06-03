# Features

| Feature | Description | Dependencies |
|---------|-------------|-------------|
| `http` | HTTP server | hyper, hyper-util, http-body-util |
| `ws` | WebSocket server | tokio-tungstenite, futures-util |
| `tcp` | TCP server (length-prefix framing) | — |
| `ts` | TypeScript client generation (ESM + full types) | — |
| `js` | JavaScript client generation (ESM + JSDoc) | — |
| `kt` | Kotlin client generation | — |
| `rs` | Rust client generation (Tokio async / std sync TCP) | — |
| `code` | On-demand code generation at `/code/{service}/{lang}` | `http` |
| `doc` | Interactive API docs at `/doc` endpoint | `http`, `js` |
| `ordinary-http` | RESTful JSON endpoints (GET/POST/PUT/DELETE) | `http`, serde, serde_json |
| `seq64` | WS request ID uses `i64` (default `i32`) | — |
| `len64` | WS payload length uses `u64` (default `u32`) | — |
| `tag-u8` | Enum tag uses `u8` (default) | afastdata/tag-u8 |
| `tag-u16` | Enum tag uses `u16` | afastdata/tag-u16 |
| `tag-u32` | Enum tag uses `u32` | afastdata/tag-u32 |
| `marker` | Enable marker-based conditional serialization; set marker via `AFast::marker()` (default `"afast"`) | — |
| `hook` | Lifecycle hooks (`before_request`/`on_response`/`on_error`/`on_connect`/`on_disconnect`), global and per-service | — |
| `rate-limit` | Named-policy rate limiting (FixedWindow/SlidingWindow/TokenBucket), pluggable store (built-in InMemoryStore) | — |

> **Note**: If the server uses `seq64` or `len64`, generated client code must use
> the same feature, otherwise protocol mismatch will occur.
