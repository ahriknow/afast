# AFast

简体中文 | [English](./README_EN.md)

AFast 是一个高性能 Rust Web 后端框架。它消除了手工定义路由的工作——
只需用 `#[handler]` 标注函数，框架即自动注册和分发请求。数据传输采用紧凑的
二进制协议，比 JSON 更小、更快。同时支持一键生成 TypeScript、JavaScript 和
Kotlin 客户端代码，内置交互式 API 文档。

## 特性

- **零路由定义** — `#[handler]` 标注函数即可，无需手动编写路由表
- **紧凑二进制协议** — 专为内部通信设计，比 JSON 体积更小、解析更快
- **自动代码生成** — 生成 TypeScript / JavaScript / Kotlin 客户端，含完整类型定义
- **交互式 API 文档** — 内置带深色/浅色主题的 Web 文档页面，支持在线测试
- **多传输层** — 同时支持 WebSocket、HTTP 和 TCP，可按需组合
- **HTTP + WS 端口合并** — 同一端口可同时提供 HTTP 和 WebSocket 服务
- **RESTful 端点** — 支持标准 HTTP 方法（GET/POST/PUT/PATCH/DELETE），含 Query/Param/Body/Header 提取器
- **长连接** — 通过 Receiver/Sender 支持 WebSocket/TCP 双向持久通信
- **类型安全提取器** — `State<T>`、`Data<T>`、`Custom<T>` 等参数注入
- **多个 State** — 支持注册多个不同类型的 State，通过泛型按类型区分
- **多个 Data** — 一个 Handler 可接受多个 `Data<T>` 参数，客户端生成对应的方法参数
- **嵌套 Handler 结构** — 通过 `group` 组织 API 命名空间，自动生成层级路径
- **递归类型发现** — `#[derive(Tag)]` 通过函数指针递归发现嵌套类型，无需全局注册表
- **客户端策略模式** — 构造时选择 WS/HTTP，之后不可切换，运行时零分支开销
- **客户端缓存** — `cache(seconds)` 属性，生成类级别静态缓存，相同参数请求直接返回缓存数据

## 快速开始

### 添加依赖

```toml
[dependencies]
afast = { version = "0.1.3", features = ["http", "ws", "ts"] }
tokio = { version = "1", features = ["full"] }
```

### 定义 Handler

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
#[tag("认证信息")]
struct AuthCustom {
    token: i64,
    platform: String,
}

#[derive(AFastDeserialize, Tag)]
#[tag("请求体")]
struct HelloReq {
    name: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("响应体")]
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
    let svc = service!("api", "示例 API" => {
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

### 运行

```bash
cargo run --features "http,ws,ts"
```

- WebSocket API: `ws://localhost:3000`
- HTTP API: `POST http://localhost:5000/_api`
- 生成的 TS 客户端代码位于 `./code/api.ts`

## Features 详解

| Feature | 说明 | 自动引入 |
|---------|------|---------|
| `http` | 启用 HTTP 服务端 | hyper, hyper-util, http-body-util |
| `ws` | 启用 WebSocket 服务端 | tokio-tungstenite, futures-util |
| `tcp` | 启用 TCP 服务端（长度前缀帧协议） | — |
| `ts` | 生成 TypeScript 客户端（ESM + 完整类型） | — |
| `js` | 生成 JavaScript 客户端（ESM + JSDoc） | — |
| `kt` | 生成 Kotlin 客户端 | — |
| `code` | HTTP `/code/{service}/{lang}` 按需生成端点 | `http` |
| `doc` | 交互式 API 文档（`/doc` 端点） | `http`, `js` |
| `ordinary-http` | RESTful JSON 端点（GET/POST/PUT/DELETE） | `http`, serde, serde_json |
| `seq64` | WS 请求 ID 使用 `i64`（默认 `i32`） | — |
| `len64` | WS 载荷长度使用 `u64`（默认 `u32`） | — |
| `tag-u8` | 枚举标签使用 `u8`（默认） | afastdata/tag-u8 |
| `tag-u16` | 枚举标签使用 `u16` | afastdata/tag-u16 |
| `tag-u32` | 枚举标签使用 `u32` | afastdata/tag-u32 |

**注意**：如果服务器端启用了 `seq64` 或 `len64`，生成的客户端代码也必须使用
相同的 feature，否则协议不匹配。

## 核心概念

### Handler 注册

`#[handler]` 过程宏在编译时自动生成：

1. 原函数保持不变
2. `HandlerMeta` — 函数名、描述、参数列表、返回类型等元数据
3. `HandlerInvoker` trait 实现 — 类型擦除的调用器，负责反序列化参数并调用原函数
4. 静态 invoker 实例 — 供 `register!` 宏引用

```rust
#[handler(desc("获取用户"), name("get_user"))]
async fn get_user_handler(
    state: State<AppState>,
    auth: Custom<Auth>,
    req: Data<UserIdRequest>,
) -> Result<UserInfo> {
    // ...
}
```

- `desc("...")` — 设置描述，用于文档和 JSDoc 注释
- `name("...")` — 覆盖客户端方法名（默认使用 Rust 函数名）
- `cache(seconds)` — 启用客户端缓存，seconds 秒内相同参数的请求返回缓存数据，避免重复请求

### 多 State 支持

AFast 支持注册**多个不同类型的 State**。`StateMap` 以 `TypeId` 为键，
每种类型最多存储一个值。Handler 通过多个 `State<T>` 参数获取不同的 State：

```rust
#[derive(Clone)]
struct DbConfig { url: String }

#[derive(Clone)]
struct RedisConfig { url: String }

#[derive(Clone)]
struct AppConfig { name: String }

// 注册多个 State
let app = AFast::new()
    .state(DbConfig { url: "postgres://...".into() })
    .state(RedisConfig { url: "redis://...".into() })
    .state(AppConfig { name: "my-app".into() });

// Handler 中获取多个 State
#[handler(desc("多 State 示例"))]
async fn my_handler(
    db: State<DbConfig>,
    redis: State<RedisConfig>,
    config: State<AppConfig>,
) -> Result<()> {
    println!("DB: {}, Redis: {}, App: {}", db.url, redis.url, config.name);
    Ok(())
}
```

如果 Handler 引用的 State 类型未注册，运行时会返回 `CODE_STATE_NOT_FOUND` 错误。

### 多 Data 支持

一个 Handler 可以接受**多个 `Data<T>` 参数**。它们依次从二进制载荷中反序列化。
生成的客户端方法以多个参数呈现：

```rust
#[derive(AFastDeserialize, Tag)]
#[tag("分页参数")]
struct PageRequest { page: i64, size: i64 }

#[derive(AFastDeserialize, Tag)]
#[tag("过滤条件")]
struct FilterRequest { keyword: String, status: i32 }

#[handler(desc("搜索用户"))]
async fn search_users(
    page: Data<PageRequest>,
    filter: Data<FilterRequest>,
) -> Result<PageResponse> {
    // ...
}
```

生成的 TypeScript 客户端方法签名：

```typescript
async searchUsers(page: PageRequest, filter: FilterRequest): Promise<PageResponse>
```

每个 `Data<T>` 对应一个方法参数，顺序与 Rust 函数中 `Data<T>` 的声明顺序一致。

### Extractor 类型

| Extractor | 说明 | 适用协议 |
|-----------|------|---------|
| `State<T>` | 从 StateMap 按类型注入共享状态（T: Clone） | 全部 |
| `Data<T>` | 从二进制载荷反序列化请求体 | HTTP/WS/TCP |
| `Custom<T>` | 客户端构造时设置的自定义上下文（如认证令牌） | HTTP/WS/TCP |
| `Receiver` | 接收客户端推送的二进制消息（长连接） | WS/TCP |
| `Sender` | 向客户端发送二进制消息（长连接） | WS/TCP |
| `Query<T>` | 从 URL Query String 反序列化（需 `ordinary-http`） | HTTP |
| `Param<T>` | 从路由路径参数（`:id`）反序列化（需 `ordinary-http`） | HTTP |
| `Body<T>` | 从 HTTP JSON Body 反序列化（需 `ordinary-http`） | HTTP |
| `Header<T>` | 从 HTTP 请求头反序列化（需 `ordinary-http`） | HTTP |

### 服务与嵌套

`service!` 宏用于构建 Handler 树，支持 `group` 命名空间：

```rust
let api_svc = service!("api", "用户 API" => {
    h(health),
    group("user" => {
        h(list_users),
        h(get_user),
        group("posts" => {
            h(list_posts),
        }),
    }),
    group("chat" => {
        h(chat),  // 使用 Receiver/Sender 的持久连接
    }),
});
```

客户端命名空间路径为 `api.user.list_users`、`api.chat.chat` 等。

在 `group` 内可混合使用二进制 handler 和普通 HTTP 路由：

```rust
group("user" => {
    h(get_user),                 // 二进制 handler
    get(":id", get_user_by_id),  // GET /user/:id
    post("", create_user),       // POST /user
    delete(":id", delete_user),  // DELETE /user/:id
}),
```

### 类型标签（Tag）

用 `#[derive(Tag)]` 为 struct/enum 生成运行时类型元数据。代码生成器通过
`FieldMeta.structure` 函数指针递归发现嵌套类型，无需全局注册表。

```rust
use afast::Tag;

#[derive(Tag)]
#[tag("用户角色")]
enum Role {
    Admin,
    User { level: i32 },
    Guest { expires_at: i64 },
    Custom(String),
}

#[derive(Tag)]
#[tag("用户信息")]
struct User {
    name: String,
    role: Role,           // 自动递归发现 Role
    tags: Vec<String>,    // Vec 元素类型自动展开
    avatar: Option<Vec<u8>>,
}
```

验证规则（通过 `#[afast(...)]` 设置，会生成客户端预检代码）：

| 规则 | 示例 | 说明 |
|------|------|------|
| `gt(value, code, "msg")` | `#[afast(gt(0, 400, "必须 > 0"))]` | 大于 |
| `gte(value, code, "msg")` | `#[afast(gte(1, 400, "必须 >= 1"))]` | 大于等于 |
| `lt(value, code, "msg")` | `#[afast(lt(100, 400, "必须 < 100"))]` | 小于 |
| `lte(value, code, "msg")` | `#[afast(lte(99, 400, "必须 <= 99"))]` | 小于等于 |
| `len(min, max, code, "msg")` | `#[afast(len(1, 20, 400, "长度 1-20"))]` | 长度限制 |
| `of(["a","b"], code, "msg")` | `#[afast(of(["a","b"], 400, "须为 a 或 b"))]` | 枚举值 |

## 传输层

### HTTP

HTTP 服务端端点：

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/_api` | 二进制 Handler 分发 |
| GET | `/_ws` | WebSocket 升级（合并模式） |
| GET | `/code/{service}/{lang}` | 按需代码生成（需 `code`） |
| GET | `/doc` | API 文档首页（需 `doc`） |
| GET | `/doc/{service}` | 单服务文档页（需 `doc`） |
| * | 普通路由 | RESTful 端点（需 `ordinary-http`） |

**HTTP 响应格式：**
- 成功：`[0u8][0i64][data: bytes]`
- 错误：`[1u8][code: i64][message: bytes]`

### WebSocket

WS 协议帧格式：

```
请求帧:  [req_id: SeqId][handler_id: u32][len: Len][payload]
推送帧:  [0: SeqId][conn_id: u32][len: Len][payload]
心跳帧:  [0xFFFFFFFF: SeqId][len: Len][conn_id1: u32]...
```

**WS 响应格式：**
- 成功：`[req_id: SeqId][len: Len][0u8][0i64][data]`
- 错误：`[req_id: SeqId][len: Len][1u8][code: i64][message_bytes]`

`SeqId` 类型由 `seq64` feature 控制（`i32` 或 `i64`），`Len` 类型由 `len64`
feature 控制（`u32` 或 `u64`）。

### TCP

TCP 使用 4 字节大端长度前缀帧协议，帧内是完整二进制请求载荷。
适用于嵌入式设备或需要原始 TCP 通信的场景。

### HTTP + WS 端口合并

当 `ws_addr` 和 `http_addr` 设置为**同一个地址**时，AFast 将 WebSocket
合并到 HTTP 服务端，通过 HTTP Upgrade 机制处理 WS 连接，不再启动独立的
WS 监听器。

```rust
// 同一端口同时提供 HTTP 和 WebSocket
let app = AFast::new()
    .service(svc)
    .ws("0.0.0.0:5000")
    .http("0.0.0.0:5000");  // 同一地址，自动合并
```

```rust
// 不同端口分开监听
let app = AFast::new()
    .service(svc)
    .ws("0.0.0.0:3000")     // WS 独立端口
    .http("0.0.0.0:5000");  // HTTP 独立端口
```

合并模式下，客户端自动连接 `ws://host:5000/_ws`。生成的客户端代码已兼容两种模式。

## 普通 HTTP（ordinary-http）

启用 `ordinary-http` feature 后，在 `service!` 宏中使用 `get`/`post`/`put`/
`patch`/`delete` 定义 RESTful 路由。

### 定义普通 Handler

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

### 响应类型

| 类型 | HTTP 状态码 | Content-Type |
|------|-----------|-------------|
| `Json<T>` | 200 | `application/json` |
| `Text` | 200 | `text/plain` |
| `Html` | 200 | `text/html` |
| `File` | 200 | 自定义 + `Content-Disposition: attachment` |
| `Status(code)` | 自定义 | — |
| `Redirect(url)` | 302 | `Location` header |
| `Result<T>` | 200 / 错误码 | `application/json`（错误时） |

## 长连接（Chat）

使用 `Receiver` 和 `Sender` 的 Handler 自动识别为长连接模式。
服务端分配 `conn_id`，初始响应携带 `conn_id`，后续消息通过 push 帧双向通信。

```rust
#[handler(desc("聊天"))]
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

生成的客户端为长连接 Handler 返回 `Socket` 对象，
提供 `send()`/`close()` 方法和 `onMessage` 回调。

## 二进制协议

### 数据类型映射

| Rust 类型 | TS/JS 类型 | Kotlin 类型 |
|-----------|-----------|------------|
| `i8`~`i64`, `u8`~`u64`, `f32`, `f64` | `number` | `Int`/`Long`/`Float`/`Double` |
| `bool` | `boolean` | `Boolean` |
| `String`, `&str` | `string` | `String` |
| `Vec<u8>` | `Uint8Array` | `ByteArray` |
| `Option<T>` | `T \| null` | `T?` |
| `Vec<T>` | `T[]` | `List<T>` |
| struct | `{ field: Type }` | `data class` |
| enum | `{ tag: 'Variant', data: ... }` | `sealed class` |

### 错误码

系统保留错误码范围 `-90011` ~ `-90000`，用户自定义错误不能使用此范围。

| 常量 | 值 | 说明 |
|------|-----|------|
| `CODE_SIGNAL` | -90000 | 系统信号（如 Ctrl+C） |
| `CODE_MSG_TOO_SHORT` | -90001 | 消息太短 |
| `CODE_PAYLOAD_MISMATCH` | -90002 | 载荷长度不匹配 |
| `CODE_SERIALIZE` | -90003 | 序列化/反序列化错误 |
| `CODE_STATE_NOT_FOUND` | -90004 | State 类型未注册 |
| `CODE_HANDLER` | -90005 | Handler 执行错误 |
| `CODE_INVALID_PARAM` | -90006 | 参数无效 |
| `CODE_IO` | -90007 | I/O 错误 |
| `CODE_WS` | -90008 | WebSocket 错误 |
| `CODE_HTTP` | -90009 | HTTP 错误 |
| `CODE_TCP` | -90010 | TCP 错误 |
| `CODE_LONG_CONNECTION_NOT_SUPPORTED` | -90011 | HTTP 模式不支持长连接 |

```rust
// 自定义错误（code 必须在保留范围之外）
return Err(afast::Error::custom(400, "请求参数无效"));
```

## 代码生成

### 静态生成（编译时写文件）

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

### 动态生成（HTTP 端点）

```
GET /code/api/ts?call=fetch,ws
GET /code/api/js?call=fetch,ws
GET /code/pay/kt?call=http,ws,tcp
```

### 支持的传输类型

**TS/JS：**

| 值 | 对应 API |
|----|---------|
| `fetch` | 浏览器 `fetch` |
| `ws` | 浏览器 `WebSocket` |
| `nodetcp` | Node.js `net` |
| `buntcp` | Bun `Bun.connect` |
| `unirequest` | UniApp `uni.request` |
| `uniws` | UniApp `uni.connectSocket` |
| `wxrequest` | 微信小程序 `wx.request` |
| `wxws` | 微信小程序 `wx.connectSocket` |

**Kotlin：**

| 值 | 对应 API |
|----|---------|
| `http` / `fetch` | `java.net.HttpURLConnection` |
| `ws` | `java.net.http.WebSocket` |
| `tcp` | `java.net.Socket` |

### 客户端使用方法

```typescript
import { ApiClient } from './api';

// 独立 WS 端口
const wsClient = new ApiClient('ws://localhost:3000');
const wsResult = await wsClient.apis.user.list_users({ page: 1, size: 20 });

// 独立 HTTP 端口
const httpClient = new ApiClient('http://localhost:5000');
const httpResult = await httpClient.apis.user.list_users({ page: 1, size: 20 });

// 合并模式（WS 和 HTTP 同一端口）
const mergedClient = new ApiClient('ws://localhost:5000');
// 自动连接 ws://localhost:5000/_ws
```

客户端传输模式在构造时确定，之后不可切换。每种传输类型有对应的策略方法
（`_callWs` / `_callHttp` / `_callTcp`），运行时通过函数引用分发。

**JavaScript 版本**使用 JSDoc 提供类型提示：

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

嵌套类型完整展开，不会用 `Object` 带过。

## 客户端缓存

通过 `cache(seconds)` 属性，可以为 Handler 启用客户端缓存，减少重复请求。

### 服务端

`#[handler]` 和普通 HTTP 宏（`#[get]`、`#[post]` 等）均支持 `cache(seconds)`：

```rust
#[handler(desc("用户列表"), cache(60))]
async fn list_users(
    state: State<AppState>,
    req: Data<ListUsersRequest>,
) -> Result<ListUsersResponse> {
    // ...
}
```

### 生成的客户端方法

```typescript
// 缓存 60 秒，force 默认为 false
const users = await client.apis.admin.listUsers({ page: 1, size: 20 });
// 60 秒内相同参数直接返回缓存数据，不发请求

// force = true 强制刷新缓存
const fresh = await client.apis.admin.listUsers({ page: 1, size: 20 }, true);
```

### 缓存策略

- **类级别** — 缓存存储在类的 `static _cache` 上，所有实例共享
- **参数感知** — 缓存键由方法名 + 序列化的参数组成，参数变化自动重新请求
- **惰性缓存** — `force = false` 且缓存未过期时直接返回；`force = true` 忽略缓存
- **多语言一致** — TypeScript、JavaScript、Kotlin 客户端使用相同的缓存策略

TS 客户端缓存存储结构：

```typescript
private static _cache = new Map<string, { data: any; expiry: number }>();
```

JS 客户端：

```javascript
/** @type {Map<string, { data: any; expiry: number }>} */
static _cache = new Map();
```

Kotlin 客户端：

```kotlin
companion object {
    private val _cache = mutableMapOf<String, Pair<Long, Any?>>()
}
```

## 关于 TextEncoder / TextDecoder

生成的客户端代码（包括 Socket 类和二进制序列化）使用了 `TextEncoder` 和
`TextDecoder` API。这两个 API 在以下平台**不可用**：

- **React Native**（所有版本）
- **微信小程序**（非标准 Web API 环境）
- **较旧的浏览器**（Chrome < 38, Firefox < 19, Safari < 10.1, IE 全版本）

可以通过手动在全局变量上实现 `TextEncoder` 和 `TextDecoder` 来解决

### 解决方案

1. **使用 polyfill**（推荐）：

```bash
npm install text-encoding
```

```javascript
import 'text-encoding';  // 应用入口处引入
```

2. **React Native**：RN 0.72+ 已内置支持。旧版本可用 `react-native-polyfill-globals`：

```javascript
import { polyfill as polyfillEncoding } from 'react-native-polyfill-globals/src/encoding';
polyfillEncoding();
```

3. **微信小程序 / UniApp**：使用 `wxrequest` / `wxws` / `unirequest` / `uniws`
传输类型，生成的代码走平台原生 API，不依赖 TextEncoder。

4. **自定义替换**：如果完全无法使用 polyfill，可在生成的客户端中替换 `_writer`
（编码）和 `_reader`（解码）方法。

涉及 TextEncoder/TextDecoder 的位置：
- **Socket 类** `send()` — 将字符串/Uint8Array 编码为二进制帧
- **二进制协议序列化** — `Data<T>` / `Custom<T>` 的参数序列化/反序列化
- 纯 JSON 普通 HTTP 请求（`Body<T>`）不需要 TextEncoder

## 交互式文档

启用 `doc` feature 后，访问 `http://host:port/doc` 获得交互式 API 文档：

```rust
let app = AFast::new()
    .service(svc)
    .document(afast::DocConfig::new()
        .title("我的 API 文档")
        .output("./docs")  // 同时输出静态 HTML 到磁盘
    )
    .http("0.0.0.0:5000");
```

- `GET /doc` — 首页，列出所有服务
- `GET /doc/{service}` — 单服务文档，包含类型定义和在线测试面板
- 支持深色/浅色主题切换
- 在线测试面板可直接发送真实请求到服务器

## 项目结构

```
afast/           — 主框架 crate（核心类型、State、传输层、代码生成）
afast-macros/    — 过程宏（#[handler]、register!、#[derive(Tag)]）
example/         — 示例项目（含 HTTP、WS、TCP、文档等完整用法）
```

### 依赖关系

- `afast` → `afast-macros`, `afastdata`, `tokio`
- `afast-macros` → `syn`, `quote`, `proc-macro2`
- 用户 crate 需要间接依赖 `afastdata-core`（`#[derive(Tag)]` 展开后的代码引用它）

## 测试

```bash
# 启动示例服务器
cargo run -p example
```

## 路线图

- **Middleware** — 计划支持 handler 执行前后的通用中间件（auth、日志、限流等）
- **TLS / HTTPS** — 计划集成 rustls 以支持 HTTPS
- **HTTP/2** — 计划支持 HTTP/2 协议

## License

MIT
