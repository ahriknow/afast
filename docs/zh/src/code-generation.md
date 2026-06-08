# 代码生成

## 静态生成（编译时文件输出）

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

## 动态生成（HTTP 端点）

```
GET /code/api/ts?call=fetch,ws
GET /code/api/js?call=fetch,ws
GET /code/pay/kt?call=http,ws,tcp
GET /code/api/rs?call=tcp-async
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

## 客户端使用

```typescript
import { ApiClient } from './api';

// 专用 WS 端口
const wsClient = new ApiClient('ws://localhost:3001');
const wsResult = await wsClient.apis.user.list_users({ page: 1, size: 20 });

// 合并模式（WS 和 HTTP 在同一端口）
const mergedClient = new ApiClient('ws://localhost:5001');
// 自动连接到 ws://localhost:5001/_ws
```

客户端传输模式在构造时确定。

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
