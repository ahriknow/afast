# 请求上下文 (`Ctx`)

`Ctx<T>` 提取器提供按请求（或按连接）的类型化数据，在整个 handler 生命周期中流转。与应用全局的 `State<T>` 不同，`Ctx<T>` 的作用域限于单个请求。

## 核心概念

| | `State<T>` | `Ctx<T>` |
|---|---|---|
| 作用域 | 应用全局 | 按请求 / 按连接 |
| 存储 | `StateMap`（启动时设置一次） | `RequestCtx`（每个请求创建） |
| 生命周期 | 整个应用 | 请求开始 → 完全结束 |
| 用途 | 数据库连接池、配置 | 请求 ID、认证信息、计时 |
| 设置方 | `AFast::state()` | 钩子 (`before_request`、`on_connect`) |

## 工作原理

```
请求到达
    │
    ▼
RequestCtx::new()          ← 框架创建空上下文
    │
    ▼
Hook: before_request()     ← 钩子插入值: ctx.ctx.insert(RequestId(...))
    │
    ▼
Handler 执行               ← 框架提取: Ctx<RequestId> 从上下文中
    │
    ▼
Hook: on_response()        ← 钩子读取值: ctx.ctx.get::<RequestId>()
    │
    ▼
RequestCtx 释放             ← 所有值被释放
```

对于长连接 handler（WS/TCP），`RequestCtx` 在整个连接期间持续存在：

```
连接建立
    │
    ▼
RequestCtx::new()          ← 创建一次
    │
    ▼
Hook: on_connect()         ← 钩子插入连接级数据
    │
    ▼
消息 1: handler 执行        ← 读取 Ctx<T>
消息 2: handler 执行        ← 相同的 Ctx<T>（消息间共享）
    ...
    │
    ▼
Hook: on_disconnect()      ← 钩子读取最终状态
    │
    ▼
RequestCtx 释放
```

## 快速示例

### 1. 定义上下文数据

```rust
#[derive(Clone)]
pub struct RequestInfo {
    pub request_id: String,
    pub started_at: std::time::Instant,
}
```

类型必须实现 `Clone + Send + Sync + 'static`（用于通过 `Ctx<T>` 提取）。

### 2. 创建插入数据的钩子

```rust
use afast::hook::{Hook, RequestContext, RequestGuard};

struct CtxHook;

impl Hook for CtxHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        ctx.ctx.insert(RequestInfo {
            request_id: format!("req-{:08x}", /* ... */),
            started_at: std::time::Instant::now(),
        });
        None  // 如果只是写入上下文，不需要 guard
    }
}
```

### 3. 在 handler 中使用

```rust
use afast::{Ctx, handler};

#[handler(desc("My handler"))]
async fn my_handler(ctx: Ctx<RequestInfo>) -> afast::Result<MyResp> {
    let elapsed = ctx.0.started_at.elapsed();
    println!("request {} took {:?}", ctx.0.request_id, elapsed);
    // ...
}
```

`ctx.0` 给你内部的 `RequestInfo` 值。

## 支持的 Handler 类型

`Ctx<T>` 在**所有** handler 类型中均可使用：

| Handler 类型 | 宏 | 示例 |
|---|---|---|
| 二进制协议 | `#[handler]` | `async fn h(ctx: Ctx<Info>) -> Result<T>` |
| HTTP ordinary | `#[get]` / `#[post]` 等 | `async fn h(ctx: Ctx<Info>) -> HttpResult<Json<T>>` |
| WebSocket | `#[ws]` | `async fn h(ctx: Ctx<Info>, sender: WsSender) -> Result<()>` |
| SSE | `#[sse]` | `async fn h(ctx: Ctx<Info>, sender: SseSender) -> Result<()>` |
| 长连接 | `#[handler]` + `Receiver`/`Sender` | `async fn h(ctx: Ctx<Info>, rx: Receiver, tx: Sender)` |

`Ctx<T>` **不参与**二进制/ordinary 互斥检查，因此可以与任何其他提取器组合使用。

## 参数位置

`Ctx<T>` 可以放在参数列表的任意位置。按惯例放在第一位：

```rust
#[handler(desc("..."))]
async fn my_handler(
    ctx: Ctx<RequestInfo>,          // ← 上下文在前
    state: State<AppState>,          // ← 然后是 state
    data: Data<MyReq>,               // ← 最后是 data
) -> afast::Result<MyResp> {
    // ...
}
```

## 在钩子中读写

钩子通过 `RequestContext::ctx` 与上下文交互：

```rust
impl Hook for MyHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        // 写入
        ctx.ctx.insert(MyData { value: 42 });

        // 读取（如果其他钩子之前写入了数据）
        // ctx.ctx.get::<OtherData>()

        Some(Box::new(MyGuard))
    }
}

impl RequestGuard for MyGuard {
    fn on_response(&mut self, ctx: &RequestContext, _resp: &[u8]) {
        // 读取 handler 或之前钩子写入的数据
        if let Some(data) = ctx.ctx.get::<MyData>() {
            println!("value was {}", data.value);
        }
    }
}
```

## 获取客户端 IP

`RequestContext` 提供两个字段用于获取客户端 IP 地址：

| 字段 | 类型 | 说明 |
|---|---|---|
| `client_ip` | `String` | TCP 连接的对端 IP（`peer_addr`） |
| `forwarded_for` | `Option<String>` | 从 `X-Forwarded-For` / `X-Real-IP` 头获取的真实 IP |

```rust
impl Hook for MyHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        // 直接连接的 IP（可能是代理 IP）
        let ip = &ctx.client_ip;

        // 真实客户端 IP（仅 HTTP/WS 有值）
        let real_ip = ctx.forwarded_for.as_deref().unwrap_or(&ctx.client_ip);

        println!("client: {} (real: {})", ip, real_ip);
        None
    }
}
```

**说明**：
- `client_ip` 在所有传输层（HTTP、WS、TCP、SSE）均有值
- `forwarded_for` 仅在 HTTP 和 WebSocket 传输层有值，TCP 始终为 `None`
- 当处于反向代理后面时，建议优先使用 `forwarded_for`

## API 参考

### `RequestCtx` — 容器

```rust
// 创建空上下文
let ctx = RequestCtx::new();

// 插入值（按类型为键）
ctx.insert(my_value);

// 获取克隆的值
let val: Option<MyType> = ctx.get::<MyType>();

// 克隆（廉价 — 共享 Arc）
let ctx2 = ctx.clone();
```

### `Ctx<T>` — 提取器

```rust
pub struct Ctx<T>(pub T);

// 访问内部值
let inner: T = my_ctx.0;
```

## 性能

- `RequestCtx::new()` 分配单个 `Arc<RwLock<HashMap>>` — 非常廉价。
- `insert()` 和 `get()` 获取 `RwLock` — 在典型场景（先顺序写入再读取）下无竞争，开销可忽略。
- `RequestCtx::clone()` 是 `Arc` 克隆 — O(1)。
- 对于大型上下文值，用 `Arc<T>` 包装可使 `get()` 的克隆更廉价。
- 不使用 `Ctx<T>` 的 handler 零提取成本 — 空上下文被创建但不会被读取。
