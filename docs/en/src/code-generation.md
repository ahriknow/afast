# Code Generation

## Static Generation (compile-time file output)

```rust
use afast::{GenerateTarget, Lang, JsTsCallType, RsCallType, NetCallType};

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
        GenerateTarget {
            lang: Lang::CS(vec![NetCallType::Http, NetCallType::Ws, NetCallType::Tcp]),
            path: "./client".into(),
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
GET /code/api/cs?call=http,ws,tcp
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

### C# / .NET

| Value | API |
|-------|-----|
| `http` / `fetch` | `System.Net.Http.HttpClient` |
| `ws` | `System.Net.WebSockets.ClientWebSocket` |
| `tcp` | `System.Net.Sockets.TcpClient` |

## Client Usage

### TypeScript / JavaScript

```typescript
import { ApiClient } from './api';

// Dedicated WS port
const wsClient = new ApiClient({
  host: 'localhost',
  port: 3001,
  tls: false,
  transport: 'ws',
  debug: false,
});
await wsClient.apis._ready;
const result = await wsClient.apis.user.list_users({ page: 1, size: 20 });

// HTTP (fetch) mode
const httpClient = new ApiClient({
  host: 'localhost',
  port: 5001,
  tls: false,
  transport: 'fetch',
  debug: false,
});
await httpClient.apis._ready;
const users = await httpClient.apis.user.list_users({ page: 1, size: 20 });
```

### Custom Extractors

Pass `customs` to provide values for `Custom<T>` extractors:

```typescript
const client = new ApiClient({
  host: 'localhost',
  port: 5001,
  tls: false,
  transport: 'fetch',
  customs: {
    AuthCustom: () => ({ token: 'my-token' }),
  },
});
```

### Header Extractors

Pass `headers` to provide values for `Header<T>` extractors:

```typescript
const client = new ApiClient({
  host: 'localhost',
  port: 5001,
  tls: false,
  transport: 'fetch',
  headers: {
    AuthHeader: async () => ({ authorization: 'Bearer my-token' }),
  },
});
```

### Kotlin

```kotlin
// HTTP mode
val client = ApiClient(host = "localhost", port = 5001, tls = false)
val users = client.userListUsers(page = 1, size = 20)

// OkHttp mode (Android-compatible)
val client = ApiClient(host = "localhost", port = 5001, tls = false, callType = KtCallType.OkHttp)

// WebSocket mode
val wsClient = ApiClient(host = "localhost", port = 3001, tls = false, callType = KtCallType.Ws)
```

### Rust

```rust
// Async TCP client
let mut client = AfastSocket::connect("localhost:4001").await?;
let users: ListUsersResp = client.call(1, &req).await?;

// Sync TCP client
let mut client = AfastSocketSync::connect("localhost:4001")?;
let users: ListUsersResp = client.call(1, &req)?;
```

### C# / .NET

```csharp
// HTTP mode
await using var client = new ApiClient("localhost", 5001, false, ApiClient.Transport.Http);
var users = await client.Apis.User.ListUsers(new UserListUsersRequest { Page = 1, Size = 20 });

// WebSocket mode
await using var wsClient = new ApiClient("localhost", 3001, false, ApiClient.Transport.Ws);
var result = await wsClient.Apis.User.ListUsers(new UserListUsersRequest { Page = 1, Size = 20 });

// TCP mode
await using var tcpClient = new ApiClient("localhost", 4001, false, ApiClient.Transport.Tcp);
var result = await tcpClient.Apis.User.ListUsers(new UserListUsersRequest { Page = 1, Size = 20 });

// Custom extractors
client.Customs["AuthCustom"] = async () => new AuthCustom { Token = "my-token" };
```

The client transport mode is fixed at construction time.

> **Note**: Ordinary HTTP routes (e.g., `#[get]`, `#[post]`) are only available with `fetch`/`http` transport. WS/TCP transport only supports binary protocol handlers (`#[handler]`).

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
