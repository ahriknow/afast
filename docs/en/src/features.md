# Features

## Core

| Feature | Description | Dependencies |
|---------|-------------|-------------|
| `binary` | Binary protocol (POST `/_api`, WS framing, TCP framing) | — |
| `http` | HTTP server | hyper, hyper-util, http-body-util |
| `ws` | WebSocket server | tokio-tungstenite, futures-util, `binary` |
| `tcp` | TCP server (length-prefix framing) | `binary` |

## Code Generation

| Feature | Description | Dependencies |
|---------|-------------|-------------|
| `ts` | TypeScript client generation (ESM + full types) | — |
| `js` | JavaScript client generation (ESM + JSDoc) | — |
| `kt` | Kotlin client generation | — |
| `rs` | Rust client generation (Tokio async / std sync TCP) | — |
| `code` | On-demand code generation at `/code/{service}/{lang}` | `http` |

## Documentation

| Feature | Description | Dependencies |
|---------|-------------|-------------|
| `doc` | Interactive API docs at `/doc` endpoint | `http`, `js` |

## Ordinary Routes

| Feature | Description | Dependencies |
|---------|-------------|-------------|
| `ordinary-http` | RESTful JSON endpoints (`#[get]`/`#[post]`/etc.) | `http`, serde, serde_json |
| `ordinary-ws` | Path-based WebSocket endpoints (`#[ws]`) | `ws`, `ordinary-http` |
| `ordinary-sse` | Server-Sent Events endpoints (`#[sse]`) | `ordinary-http`, futures-util |

## Protocol Options

| Feature | Description |
|---------|-------------|
| `seq64` | WS request ID uses `i64` (default `i32`) |
| `len64` | WS payload length uses `u64` (default `u32`) |
| `tag-u8` | Enum tag uses `u8` (default) |
| `tag-u16` | Enum tag uses `u16` |
| `tag-u32` | Enum tag uses `u32` |

## TLS

| Feature | Description | Dependencies |
|---------|-------------|-------------|
| `tls` | HTTPS via rustls with ALPN for HTTP/2, hot-reload via channel | `http`, tokio-rustls, rustls, rustls-pemfile |

## Optional Capabilities

| Feature | Description |
|---------|-------------|
| `marker` | Marker-based conditional serialization via `AFast::marker()` |
| `hook` | Lifecycle hooks (`before_request`/`on_connect`/etc.), global and per-service |
| `rate-limit` | Named-policy rate limiting (FixedWindow/SlidingWindow/TokenBucket) |
| `tls` | TLS/HTTPS support via rustls with ALPN |

> **Note**: If the server uses `seq64` or `len64`, generated client code must use
> the same feature, otherwise protocol mismatch will occur.
