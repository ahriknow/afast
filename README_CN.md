# AFast

简体中文 | [English](./README.md) | [📖 完整文档](https://ahriknow.github.io/afast)

AFast 是一个高性能 Rust Web 后端框架。它消除了手工定义路由的工作——
只需用 `#[handler]` 标注函数，框架即自动注册和分发请求。数据传输采用紧凑的
二进制协议，比 JSON 更小、更快。同时支持一键生成 TypeScript、JavaScript、
Kotlin 和 Rust 客户端代码，内置交互式 API 文档。

## 特性

- **零路由定义** — `#[handler]` 标注函数即可，无需手动编写路由表
- **紧凑二进制协议** — 专为内部通信设计，比 JSON 体积更小、解析更快
- **自动代码生成** — 生成 TypeScript / JavaScript / Kotlin / Rust 客户端，含完整类型定义
- **交互式 API 文档** — 内置带深色/浅色主题的 Web 文档页面，支持在线测试
- **多传输层** — 同时支持 WebSocket、HTTP/1.1、HTTP/2 和 TCP，可按需组合
- **TLS / HTTPS** — 基于 rustls，支持 ALPN 协商 HTTP/2
- **RESTful 端点** — 支持标准 HTTP 方法，含 Query/Param/Body/Header 提取器
- **长连接** — 通过 Receiver/Sender 支持 WebSocket/TCP 双向持久通信
- **生命周期钩子** — `before_request`/`on_response`/`on_error`/`on_connect`/`on_disconnect`，支持全局和 Service 级别
- **请求限流** — 基于命名策略的限流，支持按 IP/Header/连接/全局维度，可插拔存储后端
- **客户端缓存** — `cache(seconds)` 属性，生成类级别静态缓存

## 快速开始

```toml
[dependencies]
afast = { version = "0.1.10", features = ["http", "ws", "ts"] }
tokio = { version = "1", features = ["full"] }
```

```rust
use afast::{AFast, handler, service, State, Data, Result};
use afast::{AFastDeserialize, AFastSerialize, Tag};

#[derive(Clone)]
struct AppState { db_url: String }

#[derive(AFastDeserialize, Tag)]
#[tag("Request")]
struct HelloReq { name: String }

#[derive(AFastSerialize, Tag)]
#[tag("Response")]
struct HelloResp { message: String }

#[handler(desc("Say hello"))]
async fn hello(state: State<AppState>, req: Data<HelloReq>) -> Result<HelloResp> {
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

```bash
cargo run --features "http,ws,ts"
```

## 文档

📖 **[完整文档](https://ahriknow.github.io/afast)** — 包含完整指南和示例

| 主题 | 说明 |
|------|------|
| [快速开始](https://ahriknow.github.io/afast/quick-start.html) | 入门指南 |
| [核心概念](https://ahriknow.github.io/afast/core-concepts.html) | Handler、Service、Extractor |
| [特性详解](https://ahriknow.github.io/afast/features.html) | 所有可用特性 |
| [生命周期钩子](https://ahriknow.github.io/afast/hooks.html) | 请求/连接钩子 |
| [请求限流](https://ahriknow.github.io/afast/rate-limiting.html) | 限流配置 |
| [传输层](https://ahriknow.github.io/afast/transports.html) | HTTP、WebSocket、TCP |
| [代码生成](https://ahriknow.github.io/afast/code-generation.html) | TS/JS/KT/RS 客户端 |
| [二进制协议](https://ahriknow.github.io/afast/binary-protocol.html) | 线路格式规范 |

## 项目结构

```
afast/           — 主框架 crate
afast-macros/    — 过程宏 (#[handler], register!, #[derive(Tag)])
example/         — 包含完整用法的示例项目
docs/            — 文档源码 (mdbook)
```

## License

MIT
