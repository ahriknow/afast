# Feature 列表

## 核心

| Feature | 描述 | 依赖 |
|---------|------|------|
| `binary` | 二进制协议 (POST `/_api`、WS 帧、TCP 帧) | — |
| `http` | HTTP 服务器 | hyper, hyper-util, http-body-util |
| `ws` | WebSocket 服务器 | tokio-tungstenite, futures-util, `binary` |
| `tcp` | TCP 服务器 (长度前缀帧) | `binary` |

## 代码生成

| Feature | 描述 | 依赖 |
|---------|------|------|
| `ts` | TypeScript 客户端生成 (ESM + 完整类型) | — |
| `js` | JavaScript 客户端生成 (ESM + JSDoc) | — |
| `kt` | Kotlin 客户端生成 | — |
| `rs` | Rust 客户端生成 (Tokio 异步 / std 同步 TCP) | — |
| `code` | 按需代码生成，端点 `/code/{service}/{lang}` | `http` |

## 文档

| Feature | 描述 | 依赖 |
|---------|------|------|
| `doc` | 交互式 API 文档，端点 `/doc` | `http`, `js` |

## Ordinary 路由

| Feature | 描述 | 依赖 |
|---------|------|------|
| `ordinary-http` | RESTful JSON 端点 (`#[get]`/`#[post]` 等) | `http`, serde, serde_json |
| `ordinary-ws` | 基于路径的 WebSocket 端点 (`#[ws]`) | `ws`, `ordinary-http` |
| `ordinary-sse` | Server-Sent Events 端点 (`#[sse]`) | `ordinary-http`, futures-util |

## 协议选项

| Feature | 描述 |
|---------|------|
| `seq64` | WS 请求 ID 使用 `i64`（默认 `i32`） |
| `len64` | WS 负载长度使用 `u64`（默认 `u32`） |
| `tag-u8` | 枚举标签使用 `u8`（默认） |
| `tag-u16` | 枚举标签使用 `u16` |
| `tag-u32` | 枚举标签使用 `u32` |

## TLS

| Feature | 描述 | 依赖 |
|---------|------|------|
| `tls` | 基于 rustls 的 HTTPS，ALPN 协商 HTTP/2，支持 channel 热重载 | `http`, tokio-rustls, rustls, rustls-pemfile |

## 可选能力

| Feature | 描述 |
|---------|------|
| `marker` | 基于标记的条件序列化，通过 `AFast::marker()` 设置 |
| `hook` | 生命周期钩子 (`before_request`/`on_connect` 等)，支持全局和按服务配置 |
| `rate-limit` | 命名策略的速率限制 (FixedWindow/SlidingWindow/TokenBucket) |
| `tls` | TLS/HTTPS 支持，基于 rustls 并支持 ALPN |

> **注意**: 如果服务端使用了 `seq64` 或 `len64`，生成的客户端代码必须使用相同的 Feature，否则会出现协议不匹配。
