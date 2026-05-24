# Changelog

## [0.1.0] — Unreleased

Initial release.

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
