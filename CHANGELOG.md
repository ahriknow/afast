「方案选单」# Changelog

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
