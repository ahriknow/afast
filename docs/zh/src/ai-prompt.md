# AFast AI 开发指南

本文档专为 AI 助手设计，帮助 AI 理解如何使用 AFast 框架进行开发。

## 核心规则

1. **始终使用 `#[handler]` 宏** — 不要手动注册路由
2. **使用 `Result<T>` 处理错误** — 所有 handler 必须返回 `Result<T>`
3. **使用 `State<T>` 管理共享状态** — 零拷贝，无需 clone
4. **使用 `Data<T>` 接收请求体** — 自动从二进制负载反序列化
5. **使用 `Custom<T>` 获取认证信息** — 客户端提供的认证上下文
6. **使用 `Ctx<T>` 获取请求上下文** — 由 hooks 写入，handler 读取
7. **类型必须派生 `Tag`** — 所有请求/响应类型需要 `#[derive(Tag)]` 和 `#[tag("描述")]`
8. **HTTP handler 使用 `Header<T>` 做认证** — 不要用 `Custom<T>`

## Handler 签名模式

**重要：参数必须使用解构语法 `afast::X(name): afast::X<Type>`**

### 二进制协议 Handler（基础）

```rust
use afast::{AFastDeserialize, AFastSerialize, Tag, handler};
use crate::state::AppState;

#[derive(AFastDeserialize, Tag)]
#[tag("请求体描述")]
pub struct MyRequest {
    #[tag("字段描述")]
    pub name: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("响应体描述")]
pub struct MyResponse {
    #[tag("字段描述")]
    pub message: String,
}

#[handler(desc("描述这个 handler 的功能"))]
pub async fn my_handler(
    afast::State(state): afast::State<AppState>,
    afast::Data(req): afast::Data<MyRequest>,
) -> afast::Result<MyResponse> {
    let db = state.db.lock().await;
    Ok(MyResponse { message: format!("Hello, {}!", req.name) })
}
```

### 带认证的二进制 Handler（Custom）

```rust
use afast::{AFastDeserialize, AFastSerialize, Tag, handler};

#[derive(AFastDeserialize, Tag)]
#[tag("认证令牌")]
pub struct AuthCustom {
    #[tag("Bearer token")]
    pub token: String,
}

#[handler(desc("需要认证的接口"))]
pub async fn protected(
    afast::State(state): afast::State<AppState>,
    afast::Custom(auth): afast::Custom<AuthCustom>,
    afast::Data(req): afast::Data<MyRequest>,
) -> afast::Result<MyResponse> {
    // auth.token 可用
    Ok(MyResponse { message: "已授权".into() })
}
```

### 带请求上下文的 Handler（Ctx）

```rust
use afast::{Tag, handler};

#[derive(Clone, Debug)]
pub struct RequestInfo {
    pub request_id: String,
}

#[handler(desc("带上下文的接口"))]
pub async fn with_context(
    afast::Ctx(ctx): afast::Ctx<RequestInfo>,
    afast::State(state): afast::State<AppState>,
) -> afast::Result<MyResponse> {
    println!("请求 ID: {}", ctx.request_id);
    Ok(MyResponse { message: "完成".into() })
}
```

### HTTP REST Handler（Ordinary HTTP）

HTTP handler 使用 `Header<T>` 做认证，返回 `HttpResult<Json<T>>`：

```rust
use afast::{get, post, put, delete, Tag};
use serde::{Deserialize, Serialize};

// HTTP 认证用 Header，不用 Custom
#[derive(Debug, Deserialize, Tag)]
#[tag("HTTP 认证头")]
pub struct AuthHeader {
    #[tag("Authorization 头")]
    pub authorization: String,
}

impl AuthHeader {
    pub fn token(&self) -> &str {
        self.authorization.strip_prefix("Bearer ").unwrap_or(&self.authorization)
    }
}

#[derive(Debug, Deserialize, Tag)]
#[tag("查询参数")]
pub struct ListUsersQuery {
    #[tag("页码")]
    pub page: Option<i64>,
    #[tag("每页数量")]
    pub size: Option<i64>,
}

#[derive(Debug, Serialize, Tag)]
#[tag("用户 HTTP 响应")]
pub struct UserHttp {
    #[tag("用户 ID")]
    pub id: i64,
    #[tag("用户名")]
    pub username: String,
}

#[derive(Debug, Serialize, Tag)]
#[tag("用户列表响应")]
pub struct ListUsersHttpResponse {
    #[tag("总数")]
    pub total: i64,
    #[tag("用户列表")]
    pub items: Vec<UserHttp>,
}

// 注意：#[get] 里是 desc，不是路径！路径在 service! 宏里定义
#[get(desc("通过 HTTP 列出用户"))]
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

## 服务注册

路径在 `service!` 宏中定义，不在 handler 属性中：

```rust
use afast::{AFast, service};

// 二进制 handler 用 h() 包裹
// HTTP handler 路径在 service! 中定义
let admin_svc = service!("admin", "管理服务" => {
    group("user" => {
        // 二进制协议 handler
        h(create_user),
        h(list_users),
        // HTTP REST handler — 路径在这里定义
        get("", list_users_http),
        post("", create_user_http),
        group(":user_id" => {
            get("", get_user_http),
            put("", update_user_http),
            delete("", delete_user_http),
        })
    })
});

// catch-all 路由
let check_svc = service!("check", "检查服务" => {
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

## AppState 模式

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

## 类型派生规则

**二进制协议类型** — 用于 `#[handler]` 的 `Data<T>` 和返回值：

```rust
#[derive(AFastDeserialize, Tag)]
#[tag("请求描述")]
pub struct MyRequest {
    #[tag("字段描述")]
    pub field: String,
}

#[derive(AFastSerialize, Tag)]
#[tag("响应描述")]
pub struct MyResponse {
    #[tag("字段描述")]
    pub field: String,
}
```

**HTTP 类型** — 用于 `#[get]`/`#[post]` 等的 `Body<T>`、`Query<T>` 和返回值（需要 serde）：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Tag)]
#[tag("HTTP 请求体")]
pub struct MyBody {
    #[tag("字段描述")]
    pub field: String,
}

#[derive(Debug, Serialize, Tag)]
#[tag("HTTP 响应")]
pub struct MyHttpResponse {
    #[tag("字段描述")]
    pub field: String,
}
```

**枚举类型** — 生成的 TypeScript 为 tagged union：

```rust
#[derive(AFastSerialize, AFastDeserialize, Tag)]
#[tag("用户角色")]
pub enum Role {
    #[tag("管理员")]
    Admin,
    #[tag("普通用户")]
    User,
    #[tag("访客")]
    Guest,
}
// 生成的 TS: type Role = { tag: 'Admin', data: null } | { tag: 'User', data: null } | ...
```

## 错误处理

使用 `afast::Error::custom(code, message)` 返回自定义错误：

```rust
#[handler(desc("带错误处理的 handler"))]
pub async fn with_error(
    afast::State(state): afast::State<AppState>,
) -> afast::Result<MyResponse> {
    if some_condition {
        return Err(afast::Error::custom(400, "请求参数错误"));
    }
    
    let data = state.db.lock().await.query().await
        .map_err(|e| afast::Error::custom(500, e.to_string()))?;
    
    Ok(MyResponse { message: "成功".into() })
}
```

## HTTP 方法（RESTful 路由）

HTTP handler 与二进制 handler 的关键区别：
- 认证用 `Header<T>` 而不是 `Custom<T>`
- 返回 `afast::HttpResult<afast::Json<T>>` 而不是 `afast::Result<T>`
- 类型需要 `serde::Deserialize`/`serde::Serialize` + `Tag`
- 路径在 `service!` 宏中定义，不在 `#[get]` 属性中

```rust
use afast::{get, post, put, delete, Tag};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Tag)]
#[tag("查询参数")]
pub struct ListUsersQuery {
    #[tag("页码")]
    pub page: Option<i64>,
    #[tag("每页数量")]
    pub size: Option<i64>,
}

#[derive(Debug, Deserialize, Tag)]
#[tag("创建用户请求")]
pub struct CreateUserBody {
    #[tag("用户名")]
    pub username: String,
    #[tag("密码")]
    pub password: String,
    #[tag("显示名")]
    pub name: String,
}

#[derive(Debug, Deserialize, Tag)]
#[tag("用户 ID 路径参数")]
pub struct UserIdParam {
    #[tag("用户 ID")]
    pub user_id: i64,
}

#[derive(Debug, Deserialize, Tag)]
#[tag("更新用户请求")]
pub struct UpdateUserBody {
    #[tag("显示名")]
    pub name: String,
    #[tag("年龄")]
    pub age: i32,
    #[tag("是否激活")]
    pub active: bool,
}

// 路径在 service! 宏中定义，这里只写 desc
#[get(desc("列出用户"))]
pub async fn list_users_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Query(query): afast::Query<ListUsersQuery>,
) -> afast::HttpResult<afast::Json<ListUsersHttpResponse>> {
    // GET /user?page=1&size=10
    Ok(afast::Json(ListUsersHttpResponse { total: 0, items: vec![] }))
}

#[post(desc("创建用户"))]
pub async fn create_user_http(
    afast::State(state): afast::State<AppState>,
    afast::Header(auth): afast::Header<AuthHeader>,
    afast::Body(body): afast::Body<CreateUserBody>,
) -> afast::HttpResult<afast::Json<CreateUserHttpResponse>> {
    // POST /user 带 JSON 请求体
    Ok(afast::Json(CreateUserHttpResponse { id: 1 }))
}

// 在 service! 中的注册方式：
// service!("admin", "管理服务" => {
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

## WebSocket 处理器

路径在 `service!` 宏中定义：

```rust
use afast::ws;
use afast::extractors::{WsSender, WsReceiver};

#[ws(desc("聊天 WebSocket"))]
pub async fn chat_ws(
    afast::State(state): afast::State<AppState>,
    sender: WsSender,
    receiver: WsReceiver,
) {
    while let Some(msg) = receiver.recv().await {
        sender.send(msg).await;
    }
}

// 在 service! 中注册：
// let chat_svc = service!("chat", "聊天服务" => {
//     ws("/chat/:room", chat_ws),
// });
```

## SSE（服务器推送事件）

路径在 `service!` 宏中定义：

```rust
use afast::sse;
use afast::extractors::SseSender;

#[sse(desc("事件流"))]
pub async fn sse_stream(sender: SseSender) {
    for i in 0..10 {
        sender.send(format!("事件 {}", i)).await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

// 在 service! 中注册：
// let chat_svc = service!("chat", "聊天服务" => {
//     sse("/sse", sse_stream),
// });
```

## 生命周期钩子

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

// 在 main 中注册：
// AFast::new().hook(MyHook)
```

## 速率限制

在 `#[handler]` 属性中使用 `rate_limit("policy_name")`，并在 `main` 中配置策略：

```rust
use afast::RateLimitConfig;

#[handler(desc("带限流的接口"), rate_limit("api_limit"))]
pub async fn limited_endpoint(
    afast::State(state): afast::State<AppState>,
) -> afast::Result<MyResponse> {
    Ok(MyResponse { message: "成功".into() })
}

// 在 main 中配置限流策略：
// AFast::new()
//     .rate_limit(RateLimitConfig::new()
//         .policy("api_limit", afast::RateLimitPolicy::fixed_window(100, 60)))
```

## 客户端代码生成

启动服务后，可以生成客户端代码：

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

其中 `auth` 是 service 名称。每个 service 生成一个独立的客户端文件。

## 客户端使用示例

### TypeScript / JavaScript 客户端

生成的 TS 客户端为每个 service 创建一个 `Client` 类，通过 `apis` 属性暴露所有 handler 方法。

```typescript
import { AuthClient } from './auth';
import { AdminClient } from './admin';

// ── 二进制协议客户端（fetch 模式，最常用）──
const auth = new AuthClient({
    host: 'localhost',
    port: 5001,
    tls: false,
    transport: 'fetch',  // 'fetch' | 'ws' | 'nodetcp' | 'buntcp'
    debug: true,         // 开启后会打印请求/响应日志
    customs: {
        // Custom<T> 提取器需要提供一个返回 Promise 的工厂函数
        AuthCustom: async () => ({ token: myToken })
    }
});
await auth.apis._ready;  // 等待连接就绪（fetch 模式立即就绪）

// 调用二进制 handler — 参数和返回值都有完整类型
const reg = await auth.apis.signup({
    username: 'alice',
    password: 'secret',
    name: 'Alice'
});
console.log(reg.user.username);  // 'alice'
console.log(reg.token);          // 自动生成的 token

const login = await auth.apis.login({
    username: 'alice',
    password: 'secret'
});

// 无参数的 handler
const uid = await auth.apis.get_user_id();

// ── 同时有二进制和 HTTP 路由的客户端 ──
const admin = new AdminClient({
    host: 'localhost',
    port: 5001,
    tls: false,
    transport: 'fetch',
    customs: {
        AuthCustom: async () => ({ token: myToken })
    },
    headers: {
        // HTTP 路由的 Header<T> 认证
        AuthHeader: async () => ({ authorization: `Bearer ${myToken}` })
    }
});
await admin.apis._ready;

// 二进制 handler — 通过 apis 下的分组访问
const users = await admin.apis.user.list_users({ page: 1, size: 10 });
console.log(users.total, users.items);

const newUser = await admin.apis.user.create_user({
    username: 'bob', password: 'pw', name: 'Bob'
});

// HTTP REST handler — 通过 apis 下的分组访问
const httpUsers = await admin.apis.user.list_users_http({
    queries: { page: 1, size: 10 }  // Query 参数放在 queries 中
});

const created = await admin.apis.user.create_user_http({
    body: { username: 'charlie', password: 'pw', name: 'Charlie' }  // Body 放在 body 中
});

// 路径参数自动从参数对象中提取
const updated = await admin.apis.user.user_id.update_user_http({
    user_id: 123,  // 路径参数 :user_id
    body: { name: 'Updated', age: 25, active: true }
});

// ── WebSocket 模式（支持长连接和推送）──
const wsAuth = new AuthClient({
    host: 'localhost',
    port: 3001,
    tls: false,
    transport: 'ws',
    customs: { AuthCustom: async () => ({ token: myToken }) }
});
await wsAuth.apis._ready;
// 使用方式完全相同，底层自动走 WebSocket 二进制帧
const result = await wsAuth.apis.login({ username: 'alice', password: 'secret' });
```

### Kotlin 客户端

```kotlin
import afast.generated.*

fun main() = runBlocking {
    // 创建客户端
    val auth = AuthClient(
        host = "localhost",
        port = 5001,
        tls = false,
        transport = "http",  // "http" | "ws" | "tcp"
        customFns = AuthCustomFns(
            AuthCustom = { AuthCustom(token = myToken) }
        )
    )

    // 调用 handler — suspend 函数，直接在协程中调用
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

    // 无参数
    val uid = auth.apis.get_user_id()

    // 带 HTTP 路由的 service
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

    // 二进制 handler
    val users = admin.apis.user.list_users(ListUsersRequest(page = 1, size = 10))

    // HTTP REST handler
    val httpUsers = admin.apis.user.list_users_http(
        queries = UserListUsersHttpQuery(page = 1, size = 10)
    )
}
```

### 客户端类型自动生成规则

| Rust 类型 | TypeScript 类型 | 说明 |
|-----------|----------------|------|
| `String` | `string` | |
| `i32/i64/u32/u64/f32/f64` | `number` | |
| `bool` | `boolean` | |
| `Vec<T>` | `T[]` | |
| `Option<T>` | `T \| null` | |
| `HashMap<K,V>` | `Record<K,V>` | |
| `Vec<u8>` | `Uint8Array` | |
| `enum { A, B }` | `{ tag: 'A', data: null } \| { tag: 'B', data: null }` | Tagged union |
| `#[handler(name("signup"))]` | `apis.signup(...)` | 客户端方法名 |
| `group("user" => { ... })` | `apis.user.xxx(...)` | 嵌套分组 |

### 客户端支持的传输方式

| 传输方式 | 说明 | 适用场景 |
|----------|------|----------|
| `fetch` | HTTP/1.1 或 HTTP/2 | 浏览器、Node.js（最常用） |
| `ws` | WebSocket 二进制帧 | 浏览器、Node.js 长连接 |
| `nodetcp` | Node.js TCP | Node.js 高性能场景 |
| `buntcp` | Bun TCP | Bun 运行时 |
| `unirequest` | uni-app HTTP | 小程序/APP |
| `uniws` | uni-app WebSocket | 小程序/APP 长连接 |
| `wxrequest` | 微信小程序 HTTP | 微信小程序 |
| `wxws` | 微信小程序 WebSocket | 微信小程序长连接 |

## 常见错误

1. **忘记 `Tag` 派生** — 所有在 handler 中使用的类型必须派生 `Tag`，字段需要 `#[tag("描述")]`
2. **不要用 `String` 接收请求体** — 始终使用 `Data<T>` 配合结构体
3. **忘记 `Result` 返回类型** — 二进制 handler 返回 `afast::Result<T>`，HTTP handler 返回 `afast::HttpResult<afast::Json<T>>`
4. **不要手动注册路由** — 使用 `#[handler]` 宏
5. **不要 clone State** — `State<T>` 持有 `&'static T`，直接使用即可
6. **参数必须解构** — 写 `afast::State(state): afast::State<AppState>` 而不是 `state: State<AppState>`
7. **HTTP 认证用 Header** — HTTP handler 用 `Header<T>` 做认证，不要用 `Custom<T>`
8. **HTTP 类型需要 serde** — HTTP handler 的请求/响应类型需要 `Deserialize`/`Serialize` + `Tag`
9. **`#[get]` 里写 desc 不写路径** — 路径在 `service!` 宏中定义
10. **路径参数在 service! 中用 `:param`** — 如 `group(":user_id" => { get("", handler) })`

## 项目结构模板

```
my-project/
├── Cargo.toml
└── src/
    ├── main.rs          # 入口，service! 定义和 AFast 配置
    ├── state.rs         # AppState + Database 定义
    └── handler/
        ├── mod.rs
        ├── auth.rs      # 认证相关 handler
        ├── admin.rs     # 管理相关 handler（含 HTTP 路由）
        └── chat.rs      # WebSocket/SSE handler
```

## 快速参考

| 宏 | 用途 | 示例 |
|-----|------|------|
| `#[handler]` | 定义二进制协议 handler | `#[handler(desc("..."), name("..."), cache(60), rate_limit("..."))]` |
| `#[get]` | 定义 HTTP GET | `#[get(desc("..."))]` — 路径在 service! 中 |
| `#[post]` | 定义 HTTP POST | `#[post(desc("..."))]` |
| `#[put]` | 定义 HTTP PUT | `#[put(desc("..."))]` |
| `#[delete]` | 定义 HTTP DELETE | `#[delete(desc("..."))]` |
| `#[ws]` | 定义 WebSocket | `#[ws(desc("..."))]` |
| `#[sse]` | 定义 SSE | `#[sse(desc("..."))]` |
| `service!` | 创建服务并定义路由 | `service!("name", "desc" => { h(fn), get("path", fn) })` |
| `h()` | 注册二进制 handler | `h(my_handler)` |
| `group()` | 路由分组 | `group("user" => { get("", fn), group(":id" => { get("", fn) }) })` |

| 提取器 | 用途 | 解构语法 |
|--------|------|----------|
| `State<T>` | 共享应用状态 | `afast::State(state): afast::State<AppState>` |
| `Data<T>` | 二进制请求体 | `afast::Data(req): afast::Data<MyRequest>` |
| `Custom<T>` | 二进制认证上下文 | `afast::Custom(auth): afast::Custom<AuthCustom>` |
| `Ctx<T>` | 请求上下文（hook 写入） | `afast::Ctx(ctx): afast::Ctx<RequestInfo>` |
| `Query<T>` | HTTP 查询参数 | `afast::Query(q): afast::Query<MyQuery>` |
| `Param<T>` | HTTP 路径参数 | `afast::Param(p): afast::Param<MyParam>` |
| `Body<T>` | HTTP 请求体 | `afast::Body(b): afast::Body<MyBody>` |
| `Header<T>` | HTTP 请求头 | `afast::Header(h): afast::Header<AuthHeader>` |
