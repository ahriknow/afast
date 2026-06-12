# Changelog

## [0.1.17]

### Added

- **Catch-all 路由**: 支持 `*` 和 `*name` 通配符语法捕获所有未匹配的路径。用法：`get("*", handler)`、`post("*path", handler)` 等。匹配优先级：精确路由 > 参数路由 > catch-all，确保不会覆盖已注册的具体接口和内置端点（`/_api`、`/_ws`、`/code`、`/doc`）。
- **`FullPath` 提取器**: 新增 `FullPath` 提取器，可在 ordinary HTTP handler 中获取完整请求路径（如 `/users/123`）。

## [0.1.16]

### Changed

- **`Result<T>` 泛型化**: `afast::Result<T>` 改为 `afast::Result<T, E: AFastError = Error>`，所有 handler 宏（`#[handler]`、`#[get]`/`#[post]`/`#[put]`/`#[patch]`/`#[delete]`、`#[ws]`、`#[sse]`）均支持返回自定义错误类型，只要实现 `AFastError` trait。现有 `Result<T>` 用法完全兼容（默认 `E = Error`）。
- **WS/SSE 生成代码错误转换**: `#[ws]` 和 `#[sse]` handler 的宏生成代码增加 `AFastError::into_error(e)` 转换，不再要求返回类型必须是 `Result<(), Error>`。
- **`IntoResponse` 泛型化**: `impl IntoResponse for Result<T>` 改为 `impl<T, E: AFastError> IntoResponse for Result<T, E>`，ordinary HTTP handler 的自定义错误类型自动通过 `AFastError` trait 方法生成错误响应。
- **Stream handler 错误日志**: 长连接 handler 的错误日志改用 `AFastError::message(&e)` 替代 `Display` 格式化。

## [0.1.15]

### Added

- **中英双语文档**: 支持 `/en/` 和 `/zh/` 两套 mdbook 文档，根页面根据浏览器语言自动跳转，页面右上角语言切换按钮。
- **Doc WS/SSE 面板路径参数**: 自动检测 `:param` 路径参数并添加输入框，连接时替换为实际值。
- **Doc WS 面板 Query 输入**: WS 调试面板新增 Query 参数输入框。

### Fixed

- **Doc handler 展开问题**: ordinary HTTP handler 的 `stable_id` 为 0 导致 doc 中只能展开第一个接口，改用路径哈希作为唯一 ID。
- **Doc 端口自动获取**: 默认端口从 server 配置自动注入，不再硬编码 `3000`/`5000`；修复 IPv6 地址端口提取（`rsplit(':').next_back()` → `next()`）。
- **Doc `_` 开头 service 隐藏**: service name 以 `_` 开头的 service 在 index 页面设置 `display:none`。
- **TCP 启动重复 clone**: 移除 TCP server spawn 块中 `rl`/`hn` 的重复 clone。
- **Header 提取代码重复**: 提取 `populate_header_cache()` 辅助函数，替换 6 处重复代码。
- **原始类型检查列表重复**: 提取 `is_primitive_type()` / `is_primitive_or_container_type()` 函数，替换 5 处重复 match。
- **`Box::leak` Safety 文档**: 为 `RoutePattern::parse` 中的 `Box::leak` 添加 `# Safety` 文档。
- **路由收集逻辑重复**: 提取 `AFast::collect_routes()` 辅助函数，merge/add 两条路径统一调用。

## [0.1.14]

### Breaking Changes

- **`State<T>` 改为持有 `&'static T` 引用**: `State<T>` 不再 clone 整个值，而是持有 `'static` 引用。`AFast::state()` 启动时 `Box::leak` 分配一次，handler 每次请求零拷贝提取。
  - **用户代码无需修改** — `State<AppState>` 签名不变，`state.db.lock().await` 用法不变。
  - `State<T>` 不再要求 `T: Clone`，但需要 `T: 'static`。
  - 需要可变内部状态的用户继续用 `Arc<Mutex<...>>` 或 `Arc<RwLock<...>>`。
  - 直接调用 `StateMap::insert` 的用户需改为插入 `&'static T`（通常通过 `Box::leak`）。

### Added

- **`RequestContext` 传输协议信息**: `RequestContext` 新增 `is_binary`、`method`、`long_connection` 字段，hook 可精确判断请求类型。
  - `transport` 字段细化为：`"http-binary"` / `"http"` / `"ws-binary"` / `"ws"` / `"tcp"` / `"sse"`。
  - `is_binary: bool` — 是否为 binary 协议接口。
  - `method: &str` — 普通 HTTP 的方法名（`"GET"`/`"POST"`/`"PUT"`/`"DELETE"`/`"PATCH"`），其他情况为空。
  - `long_connection: bool` — 是否为长连接 handler（`Receiver`/`Sender`）。

### Changed

- **Hook 生命周期职责分离**: 按接口类型明确划分 `before_request` 和 `on_connect` 的职责，不再混用。
  - **`before_request`**（请求-响应模式）：HTTP binary、WS binary、TCP binary、ordinary HTTP。
  - **`on_connect`**（连接模式）：WS long、TCP long、ordinary WS、SSE。
  - 长连接 handler（WS/TCP binary `call_stream`）移除了多余的 `before_request` / `on_response` / `on_error` 调用，只保留 `on_connect` + `on_disconnect`。
  - 普通 WS handler 移除了 `before_request` / `on_response` / `on_error` 调用，只保留 `on_connect` + `on_disconnect`。

### Optimized

- **`State<T>` 零拷贝提取**: `State<T>` 改为持有 `&'static T` 引用，`AFast::state()` 启动时 `Box::leak` 分配一次，handler 每次请求直接获取引用，消除 `T::clone()` 开销。需要可变内部状态的用户用 `Arc<Mutex<...>>`。
- **减少 HTTP 热路径堆分配（6 项）**:
  - `TrieRouter::match_segments` 返回 `Option<usize>`，params 就地填充，消除 `params.clone()`。精确路由返回空 HashMap（Rust HashMap 不预分配堆内存）。
  - Hook key（`"service_name:path"`）在路由编译阶段预计算存入 `Compiled*Route`，运行时直接引用，消除每次请求的 `format!` 分配。
  - SSE/WS 路由从 O(n) 线性扫描改为 `TrieRouter` O(depth) 匹配，复用现有 trie 基础设施（cfg gate 扩展为 `ordinary-http | ordinary-ws | ordinary-sse`）。
  - `handle_request` 中 `uri.path()` 直接借用 `&str`，消除 `.to_string()` 分配。
  - `path.split('/').collect::<Vec>()` 延迟到 fallback 分支（`/_api`、`/code`、`/doc`、`/_ws`），普通路由命中 trie 后跳过此分配。
  - `read_body_bytes` 从 `Content-Length` 头预分配 `Vec::with_capacity`，避免大 body 多次 realloc。

## [0.1.13]

### Added

- **请求级 `Ctx` 上下文系统**: 新增贯穿整个 handler 生命周期的上下文容器，支持所有传输方式。
  - `RequestCtx` 容器：基于 `Arc<RwLock<HashMap<TypeId, Box<dyn Any>>>>` 的类型图，廉价 clone（共享 Arc）。
  - `Ctx<T>` 提取器：handler 参数签名中声明 `ctx: Ctx<MyType>`，宏自动从上下文中提取。与 `State<T>`（应用全局）不同，`Ctx<T>` 是请求/连接级别的。
  - Hook 集成：`RequestContext` 新增 `ctx` 字段，`before_request` 可写入上下文数据，`on_response`/`on_error`/`on_connect`/`on_disconnect` 可读取。
  - 全传输支持：HTTP binary、HTTP ordinary、WS binary、WS ordinary、SSE、TCP 均自动创建和传递 `RequestCtx`。
  - 长连接生命周期：WS/TCP 长连接的 `RequestCtx` 在连接建立时创建，整个连接生命周期共享。
  - 无侵入性：不使用 `Ctx<T>` 的 handler 不受影响，`Ctx<T>` 不计入 binary/ordinary 互斥检查，可与任何提取器组合。
- **WebSocket Origin 校验**: 新增 `AFast::ws_origins(vec!["https://example.com"])` 配置项，校验 WebSocket 升级请求的 `Origin` 头，防止 CSWSH 攻击。空列表（默认）表示允许所有来源。
- **`RateLimitStore::decr` 原子操作**: `RateLimitStore` trait 新增 `decr` 方法，原子递减并返回新值，修复令牌桶算法的竞态条件。`InMemoryStore` 使用写锁内原子递减实现。外部存储后端应覆盖此方法以保证原子性。
- **`HandlerMeta` 自定义属性**（`meta-attrs` feature）: `HandlerMeta` 新增 `attrs: &'static [Attr]` 字段，收集 handler 宏中除内置参数（`desc`/`name`/`cache`/`rate_limit`）以外的所有自定义属性。
  - `Attr` 结构体：`key: &'static str` + `value: AttrValue`。
  - `AttrValue` 枚举：`Str(&'static str)` / `Int(i64)` / `Bool(bool)`，自动推断值类型。
  - 使用方式：`#[handler(desc("..."), tag("admin"), timeout(30), deprecated)]`，`tag`/`timeout`/`deprecated` 被收集到 `attrs`。
  - 启用 `meta-attrs` feature 后可通过 `use afast::{Attr, AttrValue}` 导入类型。
  - 运行时可通过 `invoker.meta().unwrap().attrs` 读取。
  - `RequestContext` 新增 `attrs` 字段，hook 生命周期中可通过 `ctx.attrs` 访问自定义属性。
  - 所有传输方式（HTTP/WS/TCP binary、HTTP/WS/SSE ordinary）均自动传递 attrs。
  - 无自定义属性时为空 `&[]`，零开销。

### Fixed

- **JSON 注入**: 修复 HTTP 错误响应通过 `format!` 直接拼接 JSON 字符串导致的注入风险，改用 `json_error_body()` 对特殊字符（`"`、`\`、`\n` 等）正确转义。
- **XSS**: 修复 `/doc` 端点将服务名和描述直接嵌入 HTML 未转义的存储型 XSS 漏洞，新增 `html_escape()` 函数。
- **TLS 握手超时**: 新增 10 秒超时，防止客户端发起 TCP 连接后不完成 TLS 握手导致的资源耗尽。
- **令牌桶限流竞态条件**: 将 `get` + `set` 两步操作改为 `decr` 原子操作，防止并发请求同时读取相同 token 数双双放行。
- **`service!` 宏支持变量服务名**: `register_with_path!` 过程宏现在支持字符串字面量（编译期哈希）和任意表达式（运行时哈希），`Service::new` 改为 `AsRef<str>` 签名。
- **feature gate 兼容性**: 修复 `util` 模块在 `http` feature 下不可用的问题，扩展 cfg gate 为 `any(feature = "http", ...)`。
- **TCP 启动重复 clone**: 移除 TCP server spawn 块中 `rl`/`hn` 的重复 clone。

## [0.1.12]

### Added

- **`AFastError` trait — 自定义错误类型**: 新增 `AFastError` trait，允许用户定义自己的错误类型并直接从 handler 返回。
  - 实现 `code()` 和 `message()` 方法即可，框架自动调用 `into_error()` 转换为 `Error`。
  - `Error` 自身也实现了 `AFastError`，向后兼容。
  - `Result<T>` 文档更新，说明任何实现 `AFastError` 的类型都可作为 handler 返回值。
- **连接并发限制**: 新增 `AFast::max_connections(n)` 配置项（默认 10,000），通过 Semaphore 限制 HTTP/WS/TCP 并发连接数，防止文件描述符和内存耗尽。
- **连接超时**: 新增 `AFast::request_timeout(secs)` 配置项（默认 30 秒），超时连接被静默关闭，防止 slowloris 攻击。
- **请求体大小限制配置**: 新增 `AFast::body_size_limit(bytes)` 配置项（默认 10 MB），在流式读取时强制检查，防止 OOM。
- **安全响应头**: HTTP 响应自动添加 `x-content-type-options: nosniff`、`x-frame-options: DENY`、`x-xss-protection: 1; mode=block`，可通过 `AFast::security_headers()` 自定义。
- **错误消息脱敏**: 新增 `Error::sanitized_message()` 方法，系统级错误（Io/Serialize/StateNotFound/Ws/Http/Tcp）返回通用消息，用户自定义错误原样返回。通过 `AFast::sanitize_errors(bool)` 控制（默认开启）。

### Changed

- **路由匹配从 O(n) 线性扫描改为 O(depth) Trie 路由器**: 普通 HTTP 路由匹配从遍历所有路由改为 Trie 树匹配，路径深度决定复杂度，大量路由时性能显著提升。
- **`Error::custom()` 的 `assert!` 改为 `debug_assert!`**: Release 构建中不再因保留错误码 panic，防止生产环境崩溃。
- **`SseEvent` 字段类型**: `event` 和 `id` 从 `Option<&'static str>` 改为 `Option<String>`，支持动态构造事件。
- **`SseEvent::to_wire()` 性能优化**: 使用 `String::with_capacity` 和直接 `push_str` 替代 `format!`，减少内存分配。

### Fixed

- **滑动窗口限流竞态条件**: 修复并发请求同时读取计数器再递增的竞态条件，改为先 `incr` 再检查（原子操作）。
- **`InMemoryStore` 内存泄漏**: 新增后台清理任务（每 60 秒），自动移除过期条目，防止内存无限增长。
- **SSE hook 生命周期修复**: 修复 `on_response`/`on_error` guard 被提前 drop 的问题，hook 现在在 handler 返回后正确触发。
- **动态 `Retry-After` 响应头**: 被限流时返回的 `Retry-After` 头现在根据策略的 `window_secs` 动态计算，而非固定值。
- **`Box::leak` 内存增长**: 路由模式参数名不再使用 `Box::leak` 泄漏内存，改为 `'static` 字符串引用。
- **流式读取大小限制**: HTTP body、WebSocket 帧、TCP 帧在流式读取时强制检查大小限制，防止超大 payload 导致 OOM。

## [0.1.11]

### Added

- **稳定 Handler ID（哈希替代顺序索引）**: Handler 分发 ID 从全局顺序索引改为基于路径的 FNV-1a u32 哈希值。
  - `stable_id` 替代 `offset`：每个 handler 的 ID 由 `fnv1a_32(service_name + "/" + handler_name)` 在编译时计算。
  - 添加/删除/重排序 handler 不影响其他 handler 的 ID，客户端无需全部重新部署。
  - 分发表从 `Vec<Option<...>>` 改为 `HashMap<u32, ...>`，启动时自动检测哈希冲突并报错。
  - 新增 `register_with_path!` proc macro，在 `service!` 宏内部传递服务名以计算完整路径哈希。
  - Hook 表和 rate-limit 名称映射同步改为 `HashMap<u32, ...>` 索引。

### Fixed

- **KT WebSocket 初始化**: 修复 KT codegen 未在 `init` 块中初始化 WebSocket 连接导致 NPE 的问题。
  - 使用 `java.net.http.HttpClient` + `WebSocket.Listener` 建立连接，`CountDownLatch` 确保连接就绪后再返回。
  - 修复 `ByteBuffer` 处理：用 `data.get(bytes)` 替代 `data.array().take(data.remaining())`。
- **KT codegen handler ID 类型**: `_call` 参数从 `Int` 改为 `Long`，字面量加 `L` 后缀，`wU32` 调用加 `.toInt()`。修复 u32 哈希值超过 `Int.MAX_VALUE` 时的编译错误。
- **未使用代码警告**: 修复启用 `ordinary-http`/`ordinary-ws`/`ordinary-sse` 但未启用 `hook` 时的 dead_code 和 unused_variables 警告。
  - `CompiledOrdinaryRoute` 的 `path`/`service_name` 字段添加 `cfg_attr(not(feature = "hook"), allow(dead_code))`。
  - `handle_ordinary_ws_upgrade`/`handle_sse` 函数添加 `cfg_attr(not(feature = "hook"), allow(unused_variables))`。
  - `handler_name_static` 变量改为仅在 `hook` 启用时编译，避免无意义的内存泄漏。

## [0.1.10]

### Added

- **`ordinary-ws` feature — WebSocket 路由**: 新增基于路径的 WebSocket 端点，支持路径参数和查询参数提取。
  - `#[ws(desc("..."))]` 宏：声明 WebSocket 路由 handler。
  - `WsSender` / `WsReceiver` 提取器：分离收发，与二进制协议的 `Sender` / `Receiver` 风格一致。
  - `WsQuery<T>` / `WsParam<T>` 提取器（`Query<T>` / `Param<T>` 别名）：从升级请求中提取查询参数和路径参数。
  - `service!` 宏支持 `ws("/path/:param", handler)` 注册 WebSocket 路由。
  - 客户端 codegen 自动生成 WebSocket 连接方法（TS/JS/KT/RS），TS/JS 兼容 uni.connectSocket 和 wx.connectSocket。
  - 支持 rate-limit 和 lifecycle hook（before_request / on_connect / on_disconnect）。
  - `ordinary-ws` 独立于 `ordinary-http`，可单独启用。
- **`ordinary-sse` feature — Server-Sent Events 路由**: 新增基于路径的 SSE 端点，支持路径参数和查询参数提取。
  - `#[sse(desc("..."))]` 宏：声明 SSE 路由 handler。
  - `SseSender` 提取器：通过 `send()` / `send_event()` 推送事件。
  - `SseEvent` 结构体：支持 `event`、`data`、`id`、`retry` 字段。
  - `service!` 宏支持 `sse("/path", handler)` 注册 SSE 路由。
  - 客户端 codegen 自动生成 `EventSource` 方法（TS/JS），Kotlin 使用 OkHttp `EventSource` 或 `java.net.http.HttpClient`。
  - 支持 rate-limit 和 lifecycle hook（before_request / on_response / on_error）。
  - `ordinary-sse` 依赖 `ordinary-http`，可独立启用。
- **统一提取器**: `Query<T>` / `Param<T>` 从 `ordinary-http` 的纯壳子升级为自带 `from_query()` / `from_params()` 方法的完整提取器，所有 transport（HTTP、WS、SSE）共用。`WsQuery` / `WsParam` 保留为类型别名，向后兼容。Feature gate 改为 `any(ordinary-http, ordinary-ws, ordinary-sse)`。
- **`Header<T>` 提取器支持 WS/SSE**: `#[ws]` 和 `#[sse]` handler 现在支持 `Header<T>` 提取器，从 HTTP 升级请求中提取 headers。
- **ordinary 路由 lifecycle hook 支持**: `before_request`、`on_response`、`on_error`、`on_connect`、`on_disconnect` hook 现在对所有 ordinary 路由（HTTP、WS、SSE）生效。
  - Hook key 使用 `"service_name:route_path"` 格式，避免同 service 不同 group 同名 handler 冲突。
  - 合并 service（同名 service）的 hook 自动去重。
  - `on_response` / `on_error` 在 handler 返回后正确触发（修复了 guard 提前 drop 的问题）。
- **HTTP 响应体重构为 `BoxBody`**: 支持固定体（`Full<Bytes>`）和流式体（`StreamBody`），为 SSE 流式推送提供基础。
- **`binary` feature flag**: 二进制协议（`POST /_api`、WS 帧、TCP 帧）现在由 `binary` feature 控制。`ws` 和 `tcp` 依赖 `binary`，`http` 和 `ordinary-http` 不依赖。
- **KT 客户端 `KtCallType::OkHttp`**: 新增 OkHttp 传输方式，兼容 Android 平台。使用 `OkHttpClient` 替代 `java.net.HttpURLConnection` 和 `java.net.http.WebSocket`。
- **KT codegen 每个 service 独立包名**: 解决多 service 编译时类型重复定义问题。每个 service 生成到独立子目录（如 `check/check.kt`），包名如 `afast.generated.check`。
- **TS/JS ordinary-ws codegen 兼容 uni/wx**: 生成的 WebSocket 方法根据 `this._transport` 自动选择 `new WebSocket()`、`uni.connectSocket()` 或 `wx.connectSocket()`。
- **交互式文档支持 WS/SSE 调试**: API 文档页面（`/doc/{service}`）新增 WebSocket 和 SSE 路由的交互式调试面板。
  - WS 路由：Connect/Disconnect 按钮、消息输入框、Send 按钮（支持 Enter 快捷键）、实时日志面板。
  - SSE 路由：Query 参数输入、Connect/Disconnect 按钮、实时事件流日志。
  - WS/SSE 路由渲染为独立 endpoint card，与 binary handler 样式一致（无额外分组标题）。
  - WS/SSE 连接地址使用右上角配置面板的 host/port/TLS 设置，不再写死 `location.hostname`。
  - 文档 schema 新增 `wsRoutes` 和 `sseRoutes` 字段，支持所有 service 的 WS/SSE 路由展示。

### Changed

- **KT 构造函数风格统一**: `url: String` 参数改为 `host: String, port: Int, tls: Boolean = false`，与 RS/JS/TS 客户端一致。
- **KT customs key 改用类型名**: `customs["0"]` 改为 `customs["AuthCustom"]`，与 JS/TS 客户端一致。
- **KT `Option<T>` 返回类型**: 修复 Option 返回类型的反序列化（先读 u8 标志位再反序列化内层类型），返回类型正确标记为 nullable（`GetArticle?`）。
- **KT `chatWs` 修复**: 从 `host`/`port`/`tls` 构建 URL；`WebSocket.Listener` 用匿名对象替代不存在的 `create()`。
- **TS/JS uni/wx `onMessage` 兼容**: `data` 类型从 `ArrayBuffer` 改为 `string | ArrayBuffer`，兼容文本帧。
- **TS/JS codegen WS/SSE 方法改用箭头函数**: 修复 `this` 绑定问题，WS 和 SSE 方法从普通函数改为箭头函数，正确引用 `ChatClient` 实例。
- **Proc macro `expand_ordinary` 简化**: Query/Param 提取从内联解析改为调用 `Query::from_query()` / `Param::from_params()`。
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
