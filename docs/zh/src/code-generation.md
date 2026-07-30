# 代码生成

## 静态生成（编译时文件输出）

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

## 动态生成（HTTP 端点）

```
GET /code/api/ts?call=fetch,ws
GET /code/api/js?call=fetch,ws
GET /code/pay/kt?call=http,ws,tcp
GET /code/api/rs?call=tcp-async
GET /code/api/cs?call=http,ws,tcp
```

## 支持的传输类型

### TS/JS

| 值 | API |
|----|-----|
| `fetch` | 浏览器 `fetch` |
| `ws` | 浏览器 `WebSocket` |
| `nodetcp` | Node.js `net` |
| `buntcp` | Bun `Bun.connect` |
| `unirequest` | UniApp `uni.request` |
| `uniws` | UniApp `uni.connectSocket` |
| `wxrequest` | 微信小程序 `wx.request` |
| `wxws` | 微信小程序 `wx.connectSocket` |

### Kotlin

| 值 | API |
|----|-----|
| `http` / `fetch` | `java.net.HttpURLConnection` |
| `ws` | `java.net.http.WebSocket` |
| `tcp` | `java.net.Socket` |

### Rust

| 值 | API |
|----|-----|
| `tcp-async` | `tokio::net::TcpStream` (异步) |
| `tcp-sync` | `std::net::TcpStream` (同步) |

### C# / .NET

| 值 | API |
|----|-----|
| `http` / `fetch` | `System.Net.Http.HttpClient` |
| `ws` | `System.Net.WebSockets.ClientWebSocket` |
| `tcp` | `System.Net.Sockets.TcpClient` |

## 客户端使用

### TypeScript / JavaScript

```typescript
import { ApiClient } from './api';

// 专用 WS 端口
const wsClient = new ApiClient({
  host: 'localhost',
  port: 3001,
  tls: false,
  transport: 'ws',
  debug: false,
});
await wsClient.apis._ready;
const result = await wsClient.apis.user.list_users({ page: 1, size: 20 });

// HTTP (fetch) 模式
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

### Custom 提取器

通过 `customs` 为 `Custom<T>` 提取器提供值：

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

### Header 提取器

通过 `headers` 为 `Header<T>` 提取器提供值：

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
// HTTP 模式
val client = ApiClient(host = "localhost", port = 5001, tls = false)
val users = client.userListUsers(page = 1, size = 20)

// OkHttp 模式（Android 兼容）
val client = ApiClient(host = "localhost", port = 5001, tls = false, callType = KtCallType.OkHttp)

// WebSocket 模式
val wsClient = ApiClient(host = "localhost", port = 3001, tls = false, callType = KtCallType.Ws)
```

### Rust

```rust
// 异步 TCP 客户端
let mut client = AfastSocket::connect("localhost:4001").await?;
let users: ListUsersResp = client.call(1, &req).await?;

// 同步 TCP 客户端
let mut client = AfastSocketSync::connect("localhost:4001")?;
let users: ListUsersResp = client.call(1, &req)?;
```

### C# / .NET

```csharp
// HTTP 模式
await using var client = new ApiClient("localhost", 5001, false, ApiClient.Transport.Http);
var users = await client.Apis.User.ListUsers(new UserListUsersRequest { Page = 1, Size = 20 });

// WebSocket 模式
await using var wsClient = new ApiClient("localhost", 3001, false, ApiClient.Transport.Ws);
var result = await wsClient.Apis.User.ListUsers(new UserListUsersRequest { Page = 1, Size = 20 });

// TCP 模式
await using var tcpClient = new ApiClient("localhost", 4001, false, ApiClient.Transport.Tcp);
var result = await tcpClient.Apis.User.ListUsers(new UserListUsersRequest { Page = 1, Size = 20 });

// Custom 提取器
client.Customs["AuthCustom"] = async () => new AuthCustom { Token = "my-token" };
```

客户端传输模式在构造时确定。

> **注意**：普通 HTTP 路由（如 `#[get]`、`#[post]`）仅在 `fetch`/`http` 传输模式下可用。WS/TCP 传输仅支持二进制协议 handler（`#[handler]`）。

## 客户端缓存

`cache(seconds)` 属性启用客户端缓存：

```rust
#[handler(desc("List users"), cache(60))]
async fn list_users(...) -> Result<ListUsersResponse> { /* ... */ }
```

生成的客户端：

```typescript
const users = await client.apis.admin.listUsers({ page: 1, size: 20 });
// 60 秒内，相同参数返回缓存数据

const fresh = await client.apis.admin.listUsers({ page: 1, size: 20 }, true);
// force = true 跳过缓存
```

## 关于 TextEncoder / TextDecoder

生成的客户端代码使用 `TextEncoder` 和 `TextDecoder` API。这些在 React Native（旧版本）、微信小程序和旧浏览器中**不可用**。

解决方案：
1. **Polyfill**: `npm install text-encoding` + `import 'text-encoding'`
2. **React Native 0.72+**: 内置支持
3. **微信/UniApp**: 使用 `wxrequest`/`wxws`/`unirequest`/`uniws` 传输类型
