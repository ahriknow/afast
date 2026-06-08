# Code Generation

## Static Generation (compile-time file output)

```rust
use afast::{GenerateTarget, Lang, JsTsCallType, RsCallType};

let app = AFast::new()
    .service(api_svc)
    .generate(vec![
        GenerateTarget {
            lang: Lang::TS(vec![JsTsCallType::Fetch, JsTsCallType::Ws]),
            path: "./code".into(),
            debug: false,
        },
        GenerateTarget {
            lang: Lang::RS(vec![RsCallType::TcpAsync]),
            path: "./src/bin/client".into(),
            debug: true,
        },
    ]);
```

## Dynamic Generation (HTTP endpoint)

```
GET /code/api/ts?call=fetch,ws
GET /code/api/js?call=fetch,ws
GET /code/pay/kt?call=http,ws,tcp
GET /code/api/rs?call=tcp-async
```

## Supported Transport Types

### TS/JS

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

### Kotlin

| Value | API |
|-------|-----|
| `http` / `fetch` | `java.net.HttpURLConnection` |
| `ws` | `java.net.http.WebSocket` |
| `tcp` | `java.net.Socket` |

### Rust

| Value | API |
|-------|-----|
| `tcp-async` | `tokio::net::TcpStream` (async) |
| `tcp-sync` | `std::net::TcpStream` (sync) |

## Client Usage

```typescript
import { ApiClient } from './api';

// Dedicated WS port
const wsClient = new ApiClient('ws://localhost:3001');
const wsResult = await wsClient.apis.user.list_users({ page: 1, size: 20 });

// Merged mode (WS and HTTP on the same port)
const mergedClient = new ApiClient('ws://localhost:5001');
// Auto-connects to ws://localhost:5001/_ws
```

The client transport mode is fixed at construction time.

## Client-Side Caching

The `cache(seconds)` attribute enables client-side caching:

```rust
#[handler(desc("List users"), cache(60))]
async fn list_users(...) -> Result<ListUsersResponse> { /* ... */ }
```

Generated client:

```typescript
const users = await client.apis.admin.listUsers({ page: 1, size: 20 });
// Within 60 seconds, same params return cached data

const fresh = await client.apis.admin.listUsers({ page: 1, size: 20 }, true);
// force = true bypasses cache
```

## About TextEncoder / TextDecoder

The generated client code uses `TextEncoder` and `TextDecoder` APIs. These are **unavailable** on React Native (older versions), WeChat Mini Programs, and older browsers.

Solutions:
1. **Polyfill**: `npm install text-encoding` + `import 'text-encoding'`
2. **React Native 0.72+**: Built-in support
3. **WeChat/UniApp**: Use `wxrequest`/`wxws`/`unirequest`/`uniws` transport types
