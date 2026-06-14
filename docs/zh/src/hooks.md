# 生命周期钩子

启用 `hook` Feature 可拦截请求生命周期事件，用于可观测性、链路追踪、日志记录或自定义中间件。

## 快速示例

```rust
use afast::hook::{Hook, RequestContext, RequestGuard, ConnectionGuard};

struct LoggingHook;

impl Hook for LoggingHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        println!("→ {} ({})", ctx.handler_name, ctx.transport);
        Some(Box::new(std::time::Instant::now()))
    }

    fn on_connect(&self, ctx: &RequestContext) -> Option<Box<dyn ConnectionGuard>> {
        println!("↕ connect: {} ({})", ctx.handler_name, ctx.transport);
        Some(Box::new(()))
    }
}

impl RequestGuard for std::time::Instant {
    fn on_response(&mut self, ctx: &RequestContext, _resp: &[u8]) {
        println!("← {} OK ({:?})", ctx.handler_name, self.elapsed());
    }
    fn on_error(&mut self, ctx: &RequestContext, err: &afast::Error) {
        println!("✗ {} error: {}", ctx.handler_name, err);
    }
}

impl ConnectionGuard for () {
    fn on_disconnect(&mut self, ctx: &RequestContext) {
        println!("✕ disconnect: {} ({})", ctx.handler_name, ctx.transport);
    }
}
```

## 钩子 Trait

### `Hook` — 入口点

```rust
pub trait Hook: Send + Sync + 'static {
    /// Called before each request for request-response interfaces
    /// (HTTP binary, WS binary, TCP binary, ordinary HTTP).
    /// Return a `RequestGuard` to observe the response.
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> { None }

    /// Called when a connection is established for connection-oriented interfaces
    /// (WS long, TCP long, ordinary WS, SSE).
    /// Return a `ConnectionGuard` to observe disconnection.
    fn on_connect(&self, ctx: &RequestContext) -> Option<Box<dyn ConnectionGuard>> { None }
}
```

### `RequestGuard` — 每请求观察者

```rust
pub trait RequestGuard: Send + 'static {
    /// Called when the handler returns Ok.
    fn on_response(&mut self, ctx: &RequestContext, response: &[u8]) {}

    /// Called when the handler returns Err.
    fn on_error(&mut self, ctx: &RequestContext, error: &afast::Error) {}
}
```

### `ConnectionGuard` — 长连接观察者

```rust
pub trait ConnectionGuard: Send + 'static {
    /// Called when the connection is closed.
    fn on_disconnect(&mut self, ctx: &RequestContext) {}
}
```

## 全局钩子和服务钩子

```rust
let app = AFast::new()
    .hook(LoggingHook)                   // 全局：所有 handler
    .service(
        service!("api" => { h(handler) })
            .hook(ApiSpecificHook)       // 服务级：仅该服务的 handler
    );
```

- **全局钩子**对所有服务中的每个 handler 运行。
- **服务钩子**仅对该服务中的 handler 运行。
- 两者始终执行 — 不会互相替代。
- 执行顺序：先全局，后服务（洋葱模型）。

## 按传输层的钩子生命周期

钩子按接口类型分为两类：

- **`before_request`**（请求-响应）：HTTP 二进制、WS 二进制、TCP 二进制、ordinary HTTP。
- **`on_connect`**（面向连接）：WS 长连接、TCP 长连接、ordinary WS、SSE。

### 二进制协议 (HTTP `POST /_api`、WS `/_ws`、TCP)

普通 handler（请求-响应）：

```
before_request → handler → on_response / on_error
```

长连接 handler (`call_stream`)：

```
on_connect → handler → on_disconnect
```

### Ordinary HTTP (`ordinary-http`)

```
before_request → handler → on_response / on_error
```

ordinary HTTP 不会调用 `on_connect` / `on_disconnect`（无状态请求-响应）。

### Ordinary WebSocket (`ordinary-ws`)

```
on_connect → handler → on_disconnect
```

- `on_connect`：在 WebSocket 握手完成后触发。
- `on_disconnect`：在 handler 返回且转发任务清理完成后触发。

### Ordinary SSE (`ordinary-sse`)

```
on_connect → handler (spawned) → on_disconnect
```

- `on_connect`：在 SSE 响应发送和 handler 生成之前触发。
- `on_disconnect`：在 handler 任务完成后触发。

## 请求上下文集成

钩子可以通过 `RequestContext` 上的 `ctx` 字段读写请求数据。然后 handler 可以通过 `Ctx<T>` 提取器访问这些数据。

```rust
use afast::hook::{Hook, RequestContext};

#[derive(Clone)]
struct RequestId(pub String);

struct CtxHook;

impl Hook for CtxHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        // 写入请求上下文
        ctx.ctx.insert(RequestId(format!("req-{:08x}", /* ... */)));
        None
    }
}
```

Handler 自动获取：

```rust
#[handler(desc("..."))]
async fn my_handler(ctx: afast::Ctx<RequestId>) -> afast::Result<()> {
    println!("request_id = {}", ctx.0 .0);
    Ok(())
}
```

同一个请求的所有钩子和 handler 共享同一个上下文。对于长连接 handler（WS/TCP），上下文在整个连接期间持续存在。

详见 [请求上下文 (`Ctx`)](./context.md)。

## 访问自定义属性

`RequestContext` 通过 `ctx.attrs` 暴露 handler 的自定义属性：

```rust
impl Hook for DeprecationHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        for attr in ctx.attrs {
            match attr.key {
                "deprecated" => eprintln!("WARNING: {} is deprecated", ctx.handler_name),
                "tag" => {
                    if let AttrValue::Str(v) = attr.value {
                        eprintln!("tag: {}", v);
                    }
                }
                _ => {}
            }
        }
        None
    }
}
```

## 获取客户端 IP

`RequestContext` 提供两个字段用于获取客户端 IP：

```rust
impl Hook for IpHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        // TCP 连接的对端 IP
        let peer_ip = &ctx.client_ip;

        // 真实客户端 IP（从 X-Forwarded-For / X-Real-IP 头获取）
        let real_ip = ctx.forwarded_for.as_deref().unwrap_or(&ctx.client_ip);

        println!("client: {}, real: {}", peer_ip, real_ip);
        None
    }
}
```

- `client_ip`：所有传输层均有值
- `forwarded_for`：仅 HTTP/WS 有值，TCP 为 `None`

## 钩子键 — 路由匹配

钩子通过 **`"service_name:route_path"`** 匹配，而不是通过 handler 函数名。这避免了同一服务中不同 group 内出现相同函数名时的冲突：

```rust
service!("admin" => {
    group("users" => {
        get("info", get_info),    // key: "admin:/users/info"
    }),
    group("posts" => {
        get("info", get_info),    // key: "admin:/posts/info" — 无冲突！
    }),
})
```

对于合并的服务（同名服务多次注册），钩子条目会自动去重。

## RequestContext 字段

| 字段 | 类型 | 描述 |
|------|------|------|
| `handler_name` | `&'static str` | Handler 函数名 |
| `handler_desc` | `&'static str` | `#[handler(desc(...))]` 中的描述 |
| `transport` | `&'static str` | `"http-binary"`、`"http"`、`"ws-binary"`、`"ws"`、`"tcp"` 或 `"sse"` |
| `is_binary` | `bool` | 是否为二进制协议 handler |
| `method` | `&'static str` | HTTP 方法 (`"GET"`、`"POST"` 等)，非 HTTP 时为空 |
| `long_connection` | `bool` | 是否为长连接 handler (`Receiver`/`Sender`) |
| `handler_id` | `usize` | Handler 在二进制分发表中的偏移量（ordinary 路由为 0） |
| `state` | `Arc<StateMap>` | 共享应用状态 |
| `ctx` | `RequestCtx` | 请求上下文容器（钩子写入，handler 通过 `Ctx<T>` 读取） |
| `attrs` | `&'static [Attr]` | `#[handler(...)]` 中的自定义 handler 属性 |

## 支持的提取器

所有提取器在所有传输层上均可工作：

| 提取器 | HTTP | WS | SSE | TCP |
|--------|:---:|:---:|:---:|:---:|
| `State<T>` | ✅ | ✅ | ✅ | ✅ |
| `Query<T>` | ✅ | ✅ | ✅ | — |
| `Param<T>` | ✅ | ✅ | ✅ | — |
| `Header<T>` | ✅ | ✅ | ✅ | — |
| `Body<T>` | ✅ | — | — | — |
| `Custom<T>` | — | — | — | ✅ |
| `Data` | — | — | — | ✅ |
| `WsSender` | — | ✅ | — | — |
| `WsReceiver` | — | ✅ | — | — |
| `SseSender` | — | — | ✅ | — |
| `Sender` | — | — | — | ✅ |
| `Receiver` | — | — | — | ✅ |

## 服务端示例输出

```
[hook] ↕ connect: chat_ws (ws)       ← on_connect
[check-svc] ▶ chat_ws                ← 服务钩子
[ws-chat] client joined room: test   ← handler 运行
[ws-chat] client left room: test     ← handler 返回
[hook] ✕ disconnect: chat_ws (ws)    ← on_disconnect
[check-svc] ◀ chat_ws done           ← 服务钩子完成
```
