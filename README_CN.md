# AFast

简体中文 | [English](./README.md) | [📖 文档](https://afast.ahriknow.help)

高性能 Rust Web 后端框架。用 `#[handler]` 标注函数即可——框架自动注册路由、通过紧凑二进制协议分发请求，并一键生成 TypeScript / JavaScript / Kotlin / Rust / C# 客户端代码。

## 特性

- **零路由定义** — `#[handler]` 标注函数，无需手动路由表
- **紧凑二进制协议** — 比 JSON 更小更快
- **自动代码生成** — TypeScript / JavaScript / Kotlin / Rust / C# 客户端
- **交互式 API 文档** — 内置 Web 文档，在线测试
- **多传输层** — WebSocket、HTTP/1.1、HTTP/2、TCP
- **TLS / HTTPS** — 基于 rustls，ALPN 协商 HTTP/2，支持 channel 热重载
- **文件上传** — `Multipart`（原始）和 `MultipartForm<T>`（类型化）提取器，支持 `#[derive(FromFormData)]` 宏
- **RESTful 端点** — `#[get]`/`#[post]`/`#[put]`/`#[delete]` + JSON
- **WebSocket & SSE** — `#[ws]` 和 `#[sse]` 路由宏
- **长连接** — 通过 `Receiver`/`Sender` 双向流式通信
- **零拷贝 State** — `State<T>` 持有 `&'static T`，无 per-request clone
- **生命周期钩子** — `before_request`/`on_response`/`on_error`/`on_connect`/`on_disconnect`
- **请求上下文** — `Ctx<T>` 请求级上下文，hook 写入，handler 读取
- **请求限流** — 命名策略 + 可插拔存储后端
- **客户端缓存** — `cache(seconds)` 属性

## 快速开始

```toml
[dependencies]
afast = { version = "0.1.25", features = ["http", "ordinary-http"] }
tokio = { version = "1", features = ["full"] }
```

```rust
use afast::{AFast, Ctx, handler, service, State, Data, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

struct AppState { greeting: String }

#[derive(Clone)]
struct RequestId(pub String);

#[derive(AFastDeserialize, Tag)]
#[tag("Hello request")]
struct HelloReq { name: String }

#[derive(AFastSerialize, Tag)]
#[tag("Hello response")]
struct HelloResp { message: String }

#[handler(desc("Say hello"))]
async fn hello(
    ctx: Ctx<RequestId>,
    state: State<AppState>,
    req: Data<HelloReq>,
) -> Result<HelloResp> {
    println!("request_id={}", ctx.0 .0);
    Ok(HelloResp { message: format!("{}, {}!", state.greeting, req.name) })
}

#[tokio::main]
async fn main() {
    let svc = service!("api" => { h(hello) });
    AFast::new()
        .state(AppState { greeting: "Hello".into() })
        .service(svc)
        .http("0.0.0.0:5000")
        .run().await.unwrap();
}
```

## 文档

📖 **[afast.ahriknow.help](https://afast.ahriknow.help)** — 快速开始、核心概念、特性详解、钩子、请求上下文、限流、代码生成、二进制协议等。

## 项目结构

```
afast/           — 主框架 crate
afast-macros/    — 过程宏 (#[handler], service!, #[derive(Tag)])
example/         — 示例项目
docs/            — 文档源码 (mdbook)
```

## License

MIT
