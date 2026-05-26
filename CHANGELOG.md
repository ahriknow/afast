# Changelog

## [0.1.3] — Unreleased

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

### Planned

- Middleware support
- TLS / HTTPS (rustls)
- HTTP/2
