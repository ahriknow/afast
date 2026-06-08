# AFast

简体中文 | [English](/en/)

AFast 是一个高性能 Rust Web 后端框架。用 `#[handler]` 标注函数即可——框架自动注册路由、通过紧凑二进制协议分发请求，并一键生成 TypeScript / JavaScript / Kotlin / Rust 客户端代码。

## 亮点

- **零路由定义** — `#[handler]` 标注函数，无需手动路由表
- **紧凑二进制协议** — 比 JSON 更小更快，专为内部通信设计
- **零拷贝 State** — `State<T>` 持有 `&'static T`，无 per-request clone，不要求 `T: Clone`
- **自动代码生成** — TypeScript / JavaScript / Kotlin / Rust 客户端，完整类型定义
- **交互式 API 文档** — 内置 Web 文档，暗色/亮色主题，在线测试，WS/SSE 调试
- **多传输层** — WebSocket、HTTP/1.1、HTTP/2、TCP，按需组合
- **TLS / HTTPS** — 基于 rustls，ALPN 协商 HTTP/2
- **RESTful 端点** — `#[get]`/`#[post]`/`#[put]`/`#[delete]` + JSON
- **WebSocket & SSE** — `#[ws]` 和 `#[sse]` 路由宏
- **长连接** — 通过 `Receiver`/`Sender` 双向流式通信
- **生命周期钩子** — `before_request`/`on_response`/`on_error`/`on_connect`/`on_disconnect`
- **请求上下文** — `Ctx<T>` 请求级上下文，hook 写入，handler 读取
- **请求限流** — 命名策略 + 可插拔存储后端

## 快速示例

```rust
use afast::{AFast, handler, service, State, Data, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

struct AppState { db_url: String }

#[derive(AFastDeserialize, Tag)]
#[tag("请求体")]
struct HelloReq { name: String }

#[derive(AFastSerialize, Tag)]
#[tag("响应体")]
struct HelloResp { message: String }

#[handler(desc("打招呼"))]
async fn hello(
    state: State<AppState>,
    req: Data<HelloReq>,
) -> Result<HelloResp> {
    Ok(HelloResp { message: format!("Hello, {}!", req.name) })
}

#[tokio::main]
async fn main() {
    let svc = service!("api" => { h(hello) });
    AFast::new()
        .state(AppState { db_url: "localhost".into() })
        .service(svc)
        .http("0.0.0.0:5000")
        .run().await.unwrap();
}
```
