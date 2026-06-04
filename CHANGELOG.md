# Changelog

## [0.1.10]

### Added

- **`ordinary-ws` feature — WebSocket 路由**: 新增基于路径的 WebSocket 端点，支持路径参数和查询参数提取。
  - `#[ws(desc("..."))]` 宏：声明 WebSocket 路由 handler。
  - `WsSender` / `WsReceiver` 提取器：分离收发，与二进制协议的 `Sender` / `Receiver` 风格一致。
  - `WsQuery<T>` / `WsParam<T>` 提取器：从升级请求中提取查询参数和路径参数。
  - `service!` 宏支持 `ws("/path/:param", handler)` 注册 WebSocket 路由。
  - 客户端 codegen 自动生成 WebSocket 连接方法（TS/JS/KT/RS），TS/JS 兼容 uni.connectSocket 和 wx.connectSocket。
  - 支持 rate-limit 和 lifecycle hook（before_request / on_connect / on_disconnect）。
  - `ordinary-ws` 独立于 `ordinary-http`，可单独启用。
- **`binary` feature flag**: 二进制协议（`POST /_api`、WS 帧、TCP 帧）现在由 `binary` feature 控制。`ws` 和 `tcp` 依赖 `binary`，`http` 和 `ordinary-http` 不依赖。
- **KT 客户端 `KtCallType::OkHttp`**: 新增 OkHttp 传输方式，兼容 Android 平台。使用 `OkHttpClient` 替代 `java.net.HttpURLConnection` 和 `java.net.http.WebSocket`。
- **KT codegen 每个 service 独立包名**: 解决多 service 编译时类型重复定义问题。每个 service 生成到独立子目录（如 `check/check.kt`），包名如 `afast.generated.check`。
- **TS/JS ordinary-ws codegen 兼容 uni/wx**: 生成的 WebSocket 方法根据 `this._transport` 自动选择 `new WebSocket()`、`uni.connectSocket()` 或 `wx.connectSocket()`。

### Changed

- **KT 构造函数风格统一**: `url: String` 参数改为 `host: String, port: Int, tls: Boolean = false`，与 RS/JS/TS 客户端一致。
- **KT customs key 改用类型名**: `customs["0"]` 改为 `customs["AuthCustom"]`，与 JS/TS 客户端一致。
- **KT `Option<T>` 返回类型**: 修复 Option 返回类型的反序列化（先读 u8 标志位再反序列化内层类型），返回类型正确标记为 nullable（`GetArticle?`）。
- **KT `chatWs` 修复**: 从 `host`/`port`/`tls` 构建 URL；`WebSocket.Listener` 用匿名对象替代不存在的 `create()`。
- **TS/JS uni/wx `onMessage` 兼容**: `data` 类型从 `ArrayBuffer` 改为 `string | ArrayBuffer`，兼容文本帧。
- **GitHub Actions**: 移除第三方 `peaceiris/actions-mdbook`，改用 `cargo install mdbook` + `actions/cache`，消除 Node.js 废弃警告。

## [0.1.9]

### Added

- **`hook` feature — 生命周期钩子**: 新增请求/连接生命周期钩子系统，支持全局和 Service 级别。
  - `Hook` trait：实现 `before_request` 和 `on_connect` 方法。
  - `RequestGuard` trait：实现 `on_response` 和 `on_error` 方法。
  - `ConnectionGuard` trait：实现 `on_disconnect` 方法。
  - `RequestContext` 提供 handler 名称、描述、传输层类型、handler_id、共享状态等上下文。
  - 全局钩子：`AFast::hook(my_hook)` 注册，所有 handler 生效。
  - Service 级钩子：`service!("name" => { ... }).hook(my_hook)` 注册，仅该 service 的 handler 生效。
  - 多个钩子按注册顺序执行（入方向正序，出方向反序 — 洋葱模型）。
  - 长连接 handler 支持完整生命周期：`on_connect` → `before_request` → `on_response`/`on_error` → `on_disconnect`。
- **`HandlerInvoker::meta()` 方法**: `HandlerInvoker` trait 新增 `meta()` 方法，返回 `Option<&'static HandlerMeta>`，供钩子获取 handler 元数据。

### Changed

- **`RateLimitStore` trait 重构**: 简化为 `incr`/`get`/`set`/`delete` 四个基础 KV 操作，算法逻辑（固定窗口/滑动窗口/令牌桶）由框架内部实现。用户实现 Redis 等外部存储时只需实现四个原子操作。
- **文档重构**: 将 README 拆分为 mdbook 多章节文档，README 精简为概览页并链接到完整文档。
  - 新增 `docs/` 目录，包含 13 个章节（Quick Start、Core Concepts、Hooks、Rate Limiting、Code Generation 等）。
  - 新增 `.github/workflows/docs.yml`，自动部署到 GitHub Pages。
  - README.md / README_CN.md 精简为约 90 行概览，原始完整版备份为 README_FULL.md / README_CN_FULL.md。

## [0.1.8]

### Added

- **`rate-limit` feature — 请求限流**: 新增基于命名策略的限流模块，支持 HTTP / WebSocket / TCP 全传输层。
  - `RateLimitConfig` 声明命名策略，handler 通过 `#[handler(rate_limit("policy_id"))]` 绑定。
  - `default_policy("id")` 设置默认策略：未显式配置限流的接口自动使用默认策略。
  - 支持四种限流键：`Ip`（按客户端 IP）、`Header`（按 HTTP 头）、`Connection`（按单连接，WS/TCP 专用）、`Global`（全局共享）。
  - 三种算法：`FixedWindow`（固定窗口）、`SlidingWindow`（滑动窗口）、`TokenBucket`（令牌桶）。
  - 可插拔存储后端：`RateLimitStore` trait 允许用户自定义存储（如 Redis），内置 `InMemoryStore` 默认实现。
  - 被限流时返回 `Error::RateLimited`（code: `-90012`），HTTP 层返回 `429 Too Many Requests`。

### Changed

- `HandlerMeta` 新增 `rate_limit_policy` 字段，`#[handler]` 宏支持 `rate_limit("...")` 属性。
- `AFast` builder 新增 `.rate_limit(config)` 方法。
- `error` 模块新增 `CODE_RATE_LIMITED` 常量和 `Error::RateLimited` 变体。
- `Algorithm` 新增 `Debug + Clone` derive，`RateLimitKey` 新增 `Debug` derive。

## [0.1.7]

### Added

- **`CodeBuf` code-builder utility** (`codegen::buf`): New internal `CodeBuf` struct with ergonomic `.l()` / `.f()` / `.b()` shorthand methods, replacing raw `Vec<String>` + `push` patterns in all code generators (TS/JS/KT/RS/doc).
- **`HttpConfig` struct**: Consolidates all HTTP server parameters (addr, state, handlers, services, TLS config, etc.) into a single configuration struct, replacing 10+ positional arguments in `transport::http::serve()`.
- **Marker always available**: `AFast::marker()` no longer requires the `marker` feature gate — the marker string is stored in the `AFast` struct and injected into `StateMap` via `Arc<String>` so handlers can access it at runtime.

### Changed

- Upgraded `afastdata` from 0.0.8 to 0.0.10.
- Updated workspace description to include `rs` (Rust client).
- `JsTsCallType::from_str()`, `KtCallType::from_str()`, `RsCallType::from_str()` renamed to `.parse()` following Rust naming conventions.
- Refactored nested `if let` chains to Rust 2024 edition **let-chains** syntax throughout the codebase.
- HTTP transport: replaced `unwrap()` with `expect()` for better panic context.
- HTTP transport: replaced `splitn(2, '=')` with `split_once('=')` and range comparisons with `Range::contains()`.
- Code generators: added `#[allow(clippy::too_many_arguments, clippy::type_complexity)]` attributes to suppress noisy lints.
- Code generators: refactored to use `CodeBuf` instead of raw `Vec<String>` for line accumulation.

### Fixed

- Various clippy warnings across codegen modules (unnecessary borrows, type complexity, `only_used_in_recursion`).

## [0.1.6]

### Added

- **Rust client code generator** (`rs` feature): New Rust TCP client code generation supporting `RsCallType::TcpAsync` (tokio) and `RsCallType::TcpSync` (std). Generates complete struct/enum definitions, `Option<T>`/`Vec<T>` handling, `Custom` extractors with `pub` visibility, and `AfastSocket` long-connection handle with `Debug` derive. Available via `Lang::RS(vec![RsCallType::TcpAsync])` in `GenerateTarget` or `/code/{service}/rs` HTTP endpoint.
- **`service!` macro group nesting**: Handlers can now be organized into nested `group` blocks for hierarchical API namespacing.
- **Marker-based conditional serialization** (`marker` feature): When enabled, `AFast::marker("name")` sets a global marker string (default `"afast"`) used by `to_bytes_with` / `from_bytes_with` from afastdata 0.0.7+. Fields annotated with `#[afast(skip_with("marker"))]` are conditionally skipped during serialization/deserialization based on the active marker.
- **Codegen `skip` / `skip_with` support**: Generated client code (TS/JS/KT/RS) and API documentation now respect `#[afast(skip)]` (always excluded) and `#[afast(skip_with("marker"))]` (excluded when marker matches) attributes. Skipped fields are omitted from type definitions, serialization, deserialization, and validation code.
- **`FieldMeta` extended with `skip` / `skip_with`**: The `#[derive(Tag)]` proc macro now parses `#[afast(skip)]` and `#[afast(skip_with("marker"))]` on struct fields, exposing them in `FieldMeta` for code generators.

### Fixed

- **Container type marker propagation** (afastdata): `Vec<T>`, `Option<T>`, `[T; N]`, tuples, and `Box<T>` now correctly propagate the marker through `to_bytes_with` / `from_bytes_with`, fixing nested type serialization with `skip_with`.

- **Codegen `Option<T>` return type**: Generated client methods now correctly wrap `Option<T>` return types (e.g. `Result<Option<GetArticle>, AfastError>`) instead of always using the inner type.
- **Codegen `Option<T>` deserialization**: The code generator now correctly propagates override type names through `Option<T>` wrappers during deserialization code generation.
- **Codegen `customs` field visibility**: Generated client structs now expose `pub customs` field, allowing external auth token injection.

### Changed

- Upgraded `afastdata` from 0.0.6 to 0.0.8 (adds `to_bytes_with`/`from_bytes_with` for marker-based conditional serialization, and propagates marker through `Vec`/`Option`/`Array`/`Tuple`/`Box` container types).
- Upgraded `tokio-tungstenite` from 0.26 to 0.29.

## [0.1.5]

### Fixed

- **`service!` macro ordinary-http feature gate**: Removed `#[cfg(feature = "ordinary-http")]` checks from inside the `service!` macro, which incorrectly evaluated against the caller's crate features instead of afast's. The macro now delegates directly to `ordinary_route` / `ordinary_leaf` methods whose cfg gates correctly evaluate within the afast library.

## [0.1.4]

### Added

- **Service merge on duplicate name**: When registering a service with a name that already exists, the new service's handlers and ordinary routes are automatically merged into the existing one instead of creating a duplicate. Handler offsets continue from the previous service, and the description is preserved from the first registration.
- **Empty-name service support**: Services with an empty string name (`""`) are now fully supported. Their handlers are registered and callable via the binary protocol (HTTP/WS/TCP), but are excluded from client code generation (TS/JS/KT) and API documentation (`/doc`). This is useful for internal-only endpoints that should not be exposed to clients.

## [0.1.3]

### Added

- HTTP/1.1 + HTTP/2 dual support with automatic protocol detection. The server auto-negotiates the protocol by sniffing the connection preface (h2c for cleartext HTTP/2, standard HTTP/1.1 otherwise). WebSocket upgrades continue to work via HTTP/1.1.
- TLS/HTTPS support via `rustls` with ALPN negotiation for HTTP/2. Feature-gated behind `tls`. Supports running HTTP and HTTPS simultaneously on different ports.

## [0.1.2]

### Added

- `cache(seconds)` attribute for `#[handler]` and ordinary HTTP macros (`#[get]`, `#[post]`, etc.). When `cache_seconds > 0`, generated client methods accept a `force = false` parameter. Cache is stored at class level (shared across all instances). TypeScript, JavaScript, and Kotlin clients all support this feature.

## [0.1.1]

### Changed

- `service!` macro now supports full paths: `h(handler::admin::create_user)`, `get(":id", handler::admin::get_user)`.
- `OrdinaryHandlerInvoker` is embedded directly in `HandlerEntry`; `OrdinaryHandlerDef` removed. Hidden `__ordinary_entry_*` symbols are no longer needed, so ordinary HTTP handlers no longer require `use ...::*` wildcard imports.

### Fixed

- `DocConfig::withe()` typo → `DocConfig::with()`.
- Doc page long-connection connect button errors (null client, missing Data inputs).
- `cargo publish --token` deprecation → `cargo login` in publish workflow.

### Optimized

- Single tree walk for handler table construction (was two passes).

## [0.1.0]

Rewrite of 0.0.x — first public release.

### Features

- **Binary protocol** — high-performance RPC via length-prefixed frames with handler dispatch.
- **WebSocket transport** — bidirectional binary messages, long-connection handlers (`Receiver`/`Sender`), heartbeat, and connection multiplexing.
- **HTTP transport** — binary handler dispatch at `POST /_api` and WebSocket upgrade at `/_ws`.
- **TCP transport** — raw TCP socket with the same binary protocol, supporting both regular and long-connection handlers.
- **Ordinary HTTP** — REST-style endpoints with `Query<T>`, `Param<T>`, `Body<T>` extractors and `Json<T>`, `Text`, `Html`, `File`, `Status`, `Redirect` response types.
- **Shared application state** — type-keyed state with `State<T>` extractor, supporting multiple state types.
- **Multiple data parameters** — handlers can accept multiple `Data<T>` extractors.
- **Custom extractors** — user-defined `Custom<T>` for auth, headers, etc.
- **Client code generation** — TypeScript (`.ts`), JavaScript (`.js`), and Kotlin (`.kt`) clients with full type annotations.
- **Interactive documentation** — `/doc` endpoint with type-aware request builder and live test panel.
- **Service grouping** — hierarchical handler namespaces via `service!` macro with `group()`.
- **Type tags** — `#[derive(Tag)]` for automatic client-side type definitions and validation.
- **Multiple transport protocols** — Fetch, WebSocket, and TCP clients for multiple platforms (browser, Node.js, Bun, UniApp, WeChat Mini Program).
- **Port merging** — HTTP and WebSocket can share the same port.
