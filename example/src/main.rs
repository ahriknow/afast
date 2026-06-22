//! # Example Application
//!
//! This is a complete example demonstrating all afast framework features:
//! - Binary protocol handlers with `#[handler]`
//! - Ordinary HTTP REST routes with `#[get]`, `#[post]`, etc.
//! - WebSocket routes with `#[ws]`
//! - SSE (Server-Sent Events) routes with `#[sse]`
//! - Bidirectional long connections with `Receiver`/`Sender`
//! - Lifecycle hooks for observability
//! - Rate limiting with named policies
//! - Client code generation for TypeScript, JavaScript, Kotlin, and Rust
//! - Interactive API documentation
//!
//! ## Running
//!
//! ```bash
//! cargo run -p example --bin example
//! ```
//!
//! This starts:
//! - WebSocket server on port 3001
//! - HTTP server on port 5001
//! - TCP server on port 4001
//!
//! ## Testing
//!
//! After starting the server, run the client tests:
//!
//! ```bash
//! # JavaScript test (Node.js)
//! cd client && node test_js.mjs
//!
//! # TypeScript test (Bun)
//! cd client && bun run test_ts.ts
//!
//! # Kotlin test (Gradle)
//! cd client/kt-test && gradle run
//!
//! # Rust test client
//! cargo run -p example --bin test_client
//! ```

use afast::{
    AFast, Algorithm, CorsConfig, DocConfig, GenerateTarget, JsTsCallType, KtCallType, Lang,
    RateLimitConfig, RateLimitKey, RateLimitPolicy, RsCallType, service,
};

#[cfg(feature = "hook")]
use afast::hook::{ConnectionGuard, Hook, RequestContext, RequestGuard};

mod handler;
mod state;

// ─── Request Context Example ─────────────────────────────────────
//
// `Ctx<T>` is a per-request context extractor. A hook inserts values
// into the context, and handlers retrieve them automatically.
// Unlike `State<T>` (app-global), `Ctx<T>` is scoped to a single
// request (HTTP) or connection (WS/TCP/SSE).

/// Per-request metadata — inserted by CtxHook, read by handlers.
#[derive(Clone, Debug)]
pub struct RequestInfo {
    pub request_id: String,
    pub started_at: std::time::Instant,
}

/// Hook that injects `RequestInfo` into every request's context.
///
/// Handlers can retrieve it via `Ctx<RequestInfo>` without any
/// manual wiring — the framework extracts it automatically.
#[cfg(feature = "hook")]
struct CtxHook;

#[cfg(feature = "hook")]
impl Hook for CtxHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        // Insert per-request data into the context.
        // The RwLock is uncontended here (sequential write then read).
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        ctx.ctx.insert(RequestInfo {
            request_id: format!("req-{:08x}", nanos),
            started_at: std::time::Instant::now(),
        });
        None
    }

    fn on_connect(&self, ctx: &RequestContext) -> Option<Box<dyn ConnectionGuard>> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        ctx.ctx.insert(RequestInfo {
            request_id: format!("conn-{:08x}", nanos),
            started_at: std::time::Instant::now(),
        });
        Some(Box::new(CtxConnGuard))
    }
}

#[cfg(feature = "hook")]
struct CtxConnGuard;

#[cfg(feature = "hook")]
impl ConnectionGuard for CtxConnGuard {
    fn on_disconnect(&mut self, ctx: &RequestContext) {
        // Read the context value that was set in on_connect.
        if let Some(info) = ctx.ctx.get::<RequestInfo>() {
            eprintln!(
                "[ctx-hook] ✕ {} disconnected after {:?} (id: {})",
                ctx.handler_name,
                info.started_at.elapsed(),
                info.request_id,
            );
        }
    }
}

use handler::admin::{
    delete_user, delete_user_http, get_user_http, list_users, list_users_http, update_user,
    update_user_http,
};
use handler::article::{
    create_article, delete_article, get_article, list_articles, update_article,
};
use handler::auth::{create_token, get_user_id, login, register};
use handler::chat::chat_echo;
use handler::sse_stream::sse_stream;
use handler::{catch_all_get, health, info, ping};
use state::AppState;

// ─── Hook Implementations ────────────────────────────────────────
//
// Hooks intercept handler execution for observability, tracing, logging,
// or custom middleware. Each hook returns a _guard_ object whose methods
// are called at the corresponding lifecycle point.
//
// Lifecycle:
//   1. before_request() → returns a RequestGuard
//   2. Handler executes
//   3. on_response() or on_error() is called on the guard
//   4. Guard is dropped
//
// For long connections:
//   1. on_connect() → returns a ConnectionGuard
//   2. Connection stays open
//   3. on_disconnect() is called when connection closes

/// Global logging hook — logs every request with handler name and duration.
///
/// This hook is registered on the AFast application and runs for ALL handlers
/// across ALL services.
#[cfg(feature = "hook")]
struct LoggingHook;

/// Guard that records the start time of a request.
/// Used by LoggingHook to measure request duration.
#[cfg(feature = "hook")]
struct HookTimer(std::time::Instant);

/// Guard for long-connection lifecycle tracking.
#[cfg(feature = "hook")]
struct HookConn;

#[cfg(feature = "hook")]
impl Hook for LoggingHook {
    /// Called before every binary-protocol handler executes.
    /// Returns a HookTimer guard that will receive on_response/on_error callbacks.
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        eprintln!("[hook] → {} ({})", ctx.handler_name, ctx.transport);
        Some(Box::new(HookTimer(std::time::Instant::now())))
    }

    /// Called before a long-connection handler spawns.
    /// Returns a HookConn guard that will receive on_disconnect callbacks.
    fn on_connect(&self, ctx: &RequestContext) -> Option<Box<dyn ConnectionGuard>> {
        eprintln!("[hook] ↕ connect: {} ({})", ctx.handler_name, ctx.transport);
        Some(Box::new(HookConn))
    }
}

#[cfg(feature = "hook")]
impl RequestGuard for HookTimer {
    /// Called when the handler returns Ok(bytes).
    fn on_response(&mut self, ctx: &RequestContext, _resp: &[u8]) {
        eprintln!("[hook] ← {} OK ({:?})", ctx.handler_name, self.0.elapsed());
    }

    /// Called when the handler returns Err(e).
    fn on_error(&mut self, ctx: &RequestContext, err: &afast::Error) {
        eprintln!(
            "[hook] ✗ {} error: {} ({:?})",
            ctx.handler_name,
            err,
            self.0.elapsed()
        );
    }
}

#[cfg(feature = "hook")]
impl ConnectionGuard for HookConn {
    /// Called when the long connection is dropped or closed.
    fn on_disconnect(&mut self, ctx: &RequestContext) {
        eprintln!(
            "[hook] ✕ disconnect: {} ({})",
            ctx.handler_name, ctx.transport
        );
    }
}

/// Service-level hook for the check service.
///
/// This hook is registered on a specific service and runs AFTER the global
/// LoggingHook for handlers belonging to that service. Both global and
/// service hooks always execute — they never replace each other.
#[cfg(feature = "hook")]
struct CheckServiceHook;

#[cfg(feature = "hook")]
struct CheckGuard(&'static str);

#[cfg(feature = "hook")]
impl Hook for CheckServiceHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        eprintln!("[check-svc] ▶ {}", ctx.handler_name);
        Some(Box::new(CheckGuard(ctx.handler_name)))
    }
}

#[cfg(feature = "hook")]
impl RequestGuard for CheckGuard {
    fn on_response(&mut self, _ctx: &RequestContext, _resp: &[u8]) {
        eprintln!("[check-svc] ◀ {} done", self.0);
    }
    fn on_error(&mut self, _ctx: &RequestContext, err: &afast::Error) {
        eprintln!("[check-svc] ✗ {} error: {}", self.0, err);
    }
}

// ─── Entry Point ──────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // ── Service Definitions ──────────────────────────────────────
    //
    // A service groups handlers into a namespace. Each service generates
    // a separate client code file (e.g., check.ts, admin.ts, auth.ts).

    // "check" service: health check and system info
    let check_svc = service!("check", "Check Service" => {
        h(health),
        group("inner" => {
            h(info),
        }),
        // Catch-all route: matches any GET path not matched by other routes.
        // Has the lowest priority — exact routes and param routes take precedence.
        // Built-in endpoints (/_api, /code, /doc, /_ws) are never intercepted.
        get("*", catch_all_get),
    });
    // Add a service-level hook (runs after global hooks for this service's handlers)
    #[cfg(feature = "hook")]
    let check_svc = check_svc.hook(CheckServiceHook);

    // "admin" service: user CRUD with both binary and HTTP ordinary routes
    //
    // Binary handlers (h(...)) use the afast binary protocol.
    // HTTP ordinary handlers (get/post/put/delete(...)) use standard REST.
    //
    // Route structure:
    //   POST /_api  → binary handlers (create_user, list_users, etc.)
    //   GET  /user  → list_users_http (REST)
    //   POST /user  → create_user_http (REST)
    //   GET  /user/:user_id  → get_user_http (REST)
    //   PUT  /user/:user_id  → update_user_http (REST)
    //   DELETE /user/:user_id → delete_user_http (REST)
    let admin_svc_name = "admin"; // &str or String works for service name
    let admin_svc = service!(admin_svc_name, "Admin Service" => {
        group("user" => {
            // Binary protocol handlers
            h(handler::admin::create_user),
            h(list_users),
            h(update_user),
            h(delete_user),
            // Ordinary HTTP handlers (REST)
            get("", list_users_http),
            post("", handler::admin::create_user_http),
            group(":user_id" => {
                get("", get_user_http),
                put("", update_user_http),
                delete("", delete_user_http),
            })
        })
    })
    .hook(CheckServiceHook);

    // "auth" service: registration, login, token management
    let auth_svc = service!("auth", "Auth Service" => {
        h(register),
        h(login),
        h(create_token),
        h(get_user_id),
    })
    .hook(CheckServiceHook);

    // "article" service: article CRUD
    let article_svc = service!("article", "Article Service" => {
        h(create_article),
        h(list_articles),
        h(get_article),
        h(update_article),
        h(delete_article),
    })
    .hook(CheckServiceHook);

    // "chat" service: bidirectional streaming (binary), WebSocket, and SSE
    //
    // - chat_echo: binary protocol long-connection handler (works over WS/TCP)
    // - /chat/:room: ordinary WebSocket route (text/JSON frames)
    // - /sse: Server-Sent Events route
    let chat_svc = service!("chat", "Chat Service" => {
        h(chat_echo),
        ws("/chat/:room", handler::ws_chat::chat_ws),
        sse("/sse", sse_stream),
    })
    .hook(CheckServiceHook);

    // Service merging: if two services have the same name, their handlers
    // are merged into the first one. This is useful for organizing handlers
    // across multiple files.
    let admin_extra_svc = service!("admin", "Admin Extra" => {
        h(health),  // This handler is added to the "admin" service
    });

    // Empty service name: handlers are registered and callable via binary
    // protocol, but excluded from client code generation and API documentation.
    // Useful for internal/debug endpoints.
    let internal_svc = service!("", "Internal" => {
        h(info),
        get("ping", ping),
    });

    // ── Application Builder ──────────────────────────────────────
    //
    // AFast::new() creates the application builder. Use the builder pattern
    // to configure state, services, code generation, rate limiting, and
    // transport servers. Call .run() to start all servers.

    #[allow(unused_mut)]
    let mut app = AFast::new()
        // Register shared application state (accessible via State<T> in handlers)
        .state(AppState::new())
        // Enable interactive API documentation at /doc
        .document(DocConfig::with("Blog API Docs", "./client/doc"));

    // Register global hooks (only when "hook" feature is enabled)
    #[cfg(feature = "hook")]
    {
        app = app.hook(LoggingHook);
        app = app.hook(CtxHook);
    }

    let app = app
        // ── Code Generation ──────────────────────────────────────
        //
        // Generate client code for multiple languages and transports.
        // Code is generated at application startup and written to the
        // specified paths. Set debug: true to log all requests/responses.
        .generate(vec![
            // TypeScript client supporting all JS/TS transport types
            GenerateTarget {
                debug: true,
                lang: Lang::TS(vec![
                    JsTsCallType::Fetch,      // Browser fetch API
                    JsTsCallType::Ws,         // Browser WebSocket API
                    JsTsCallType::BunTcp,     // Bun TCP socket
                    JsTsCallType::NodeTcp,    // Node.js TCP socket
                    JsTsCallType::UniRequest, // UniApp HTTP API
                    JsTsCallType::UniWs,      // UniApp WebSocket API
                    JsTsCallType::WxRequest,  // WeChat Mini Program HTTP
                    JsTsCallType::WxWs,       // WeChat Mini Program WS
                ]),
                path: "./client".into(),
            },
            // JavaScript client (same transport options as TypeScript)
            GenerateTarget {
                debug: true,
                lang: Lang::JS(vec![
                    JsTsCallType::Fetch,
                    JsTsCallType::Ws,
                    JsTsCallType::BunTcp,
                    JsTsCallType::NodeTcp,
                    JsTsCallType::UniRequest,
                    JsTsCallType::UniWs,
                    JsTsCallType::WxRequest,
                    JsTsCallType::WxWs,
                ]),
                path: "./client".into(),
            },
            // Kotlin client
            GenerateTarget {
                debug: true,
                lang: Lang::KT(vec![
                    KtCallType::Http, // java.net.HttpURLConnection
                    KtCallType::Ws,   // java.net.http.WebSocket
                    KtCallType::Tcp,  // java.net.Socket
                ]),
                path: "./client".into(),
            },
            // Rust client (TCP async via tokio)
            GenerateTarget {
                debug: true,
                lang: Lang::RS(vec![RsCallType::TcpAsync]),
                path: "./example/src/bin/client".into(),
            },
        ])
        // Register all services
        .service(check_svc)
        .service(admin_svc)
        .service(auth_svc)
        .service(article_svc)
        .service(chat_svc)
        .service(admin_extra_svc) // merges into "admin"
        .service(internal_svc) // empty name: excluded from codegen/docs
        // Marker for conditional field skipping (afastdata 0.0.7+)
        // Fields with #[afast(skip_with("afast"))] are excluded from
        // serialization when this marker is set.
        .marker("afast")
        // ── CORS ──────────────────────────────────────────────────
        //
        // Enable CORS for all HTTP endpoints (including /_api).
        // Use CorsConfig::permissive() for development or
        // CorsConfig::new(vec!["https://example.com"]) for production.
        .cors(CorsConfig::permissive())
        // ── Rate Limiting ────────────────────────────────────────
        //
        // Named policies that handlers reference by ID via
        // #[handler(rate_limit("policy_id"))].
        .rate_limit(
            RateLimitConfig::new()
                // "login" policy: max 5 requests per 60 seconds per IP
                // Uses sliding window to avoid burst at window boundary
                .policy(RateLimitPolicy {
                    id: "login".into(),
                    max_requests: 100,
                    window_secs: 60,
                    key: RateLimitKey::Ip,
                    algorithm: Algorithm::SlidingWindow,
                })
                // "global" policy: max 100 requests per second per IP
                // Applied to handlers that don't specify rate_limit("...")
                .default_policy("global")
                .policy(RateLimitPolicy {
                    id: "global".into(),
                    max_requests: 100,
                    window_secs: 60,
                    key: RateLimitKey::Ip,
                    algorithm: Algorithm::SlidingWindow,
                }),
        )
        // ── Transport Servers ────────────────────────────────────
        //
        // Bind transport servers to addresses. You can run multiple
        // transports simultaneously.
        .ws("[::]:3001") // WebSocket server on port 3001
        .http("[::]:5001") // HTTP server on port 5001
        .tcp("[::]:4001"); // TCP server on port 4001

    // TLS/HTTPS (only when "tls" feature is enabled)
    #[cfg(feature = "tls")]
    let app = app.https("[::]:5443", "./cert.pem", "./key.pem");

    // Start all servers and block until Ctrl+C
    app.run().await.unwrap();
}
