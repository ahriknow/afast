use crate::handler::Handler;
#[cfg(any(feature = "ws", feature = "http", feature = "tcp"))]
use crate::handler::HandlerInvoker;
#[cfg(feature = "ordinary-http")]
use crate::service::OrdinaryRouteInfo;
use crate::{Error, Service, StateMap};
use std::path::PathBuf;

/// Transport protocol variant for TypeScript and JavaScript generated clients.
///
/// Each variant corresponds to a specific network transport API available in
/// the target runtime (browser, Node.js, Bun, UniApp, or WeChat Mini Program).
#[derive(Clone, Debug, PartialEq)]
pub enum JsTsCallType {
    /// Browser [`fetch`](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API) API.
    Fetch,
    /// Standard [`WebSocket`](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket) API.
    Ws,
    /// Node.js [`net`](https://nodejs.org/api/net.html) TCP socket.
    NodeTcp,
    /// Bun [`Bun.connect`](https://bun.sh/docs/api/tcp) TCP socket.
    BunTcp,
    /// UniApp [`uni.request`](https://uniapp.dcloud.net.cn/api/request/request.html) HTTP API.
    UniRequest,
    /// UniApp [`uni.connectSocket`](https://uniapp.dcloud.net.cn/api/request/websocket.html) WebSocket API.
    UniWs,
    /// WeChat Mini Program [`wx.request`](https://developers.weixin.qq.com/miniprogram/en/dev/api/network/request/wx.request.html) HTTP API.
    WxRequest,
    /// WeChat Mini Program [`wx.connectSocket`](https://developers.weixin.qq.com/miniprogram/en/dev/api/network/websocket/wx.connectSocket.html) WebSocket API.
    WxWs,
}

impl JsTsCallType {
    /// Parses a call-type string from the `?call=` query parameter.
    ///
    /// Returns `None` if the string does not match any known transport.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fetch" => Some(JsTsCallType::Fetch),
            "ws" => Some(JsTsCallType::Ws),
            "nodetcp" => Some(JsTsCallType::NodeTcp),
            "buntcp" => Some(JsTsCallType::BunTcp),
            "unirequest" => Some(JsTsCallType::UniRequest),
            "uniws" => Some(JsTsCallType::UniWs),
            "wxrequest" => Some(JsTsCallType::WxRequest),
            "wxws" => Some(JsTsCallType::WxWs),
            _ => None,
        }
    }

    /// Returns the canonical string representation for use in query parameters.
    pub fn as_str(&self) -> &'static str {
        match self {
            JsTsCallType::Fetch => "fetch",
            JsTsCallType::Ws => "ws",
            JsTsCallType::NodeTcp => "nodetcp",
            JsTsCallType::BunTcp => "buntcp",
            JsTsCallType::UniRequest => "unirequest",
            JsTsCallType::UniWs => "uniws",
            JsTsCallType::WxRequest => "wxrequest",
            JsTsCallType::WxWs => "wxws",
        }
    }
}

/// Transport protocol variant for Kotlin generated clients.
#[derive(Clone, Debug, PartialEq)]
pub enum KtCallType {
    /// HTTP via `java.net.HttpURLConnection`.
    Http,
    /// WebSocket via `java.net.http.WebSocket`.
    Ws,
    /// Raw TCP via `java.net.Socket`.
    Tcp,
}

impl KtCallType {
    /// Parses a call-type string from the `?call=` query parameter.
    ///
    /// Accepts `"http"` and `"fetch"` as aliases for [`KtCallType::Http`].
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "http" | "fetch" => Some(KtCallType::Http),
            "ws" => Some(KtCallType::Ws),
            "tcp" => Some(KtCallType::Tcp),
            _ => None,
        }
    }

    /// Returns the canonical string representation for use in query parameters.
    pub fn as_str(&self) -> &'static str {
        match self {
            KtCallType::Http => "http",
            KtCallType::Ws => "ws",
            KtCallType::Tcp => "tcp",
        }
    }
}

/// Transport protocol variant for Rust generated clients.
#[derive(Clone, Debug, PartialEq)]
pub enum RsCallType {
    /// Async TCP via `tokio::net::TcpStream` (requires tokio runtime).
    TcpAsync,
    /// Synchronous TCP via `std::net::TcpStream`.
    TcpSync,
}

impl RsCallType {
    /// Parses a call-type string from the `?call=` query parameter.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tcp-async" | "tcp_async" => Some(RsCallType::TcpAsync),
            "tcp-sync" | "tcp_sync" => Some(RsCallType::TcpSync),
            "tcp" => Some(RsCallType::TcpAsync), // default to async
            _ => None,
        }
    }

    /// Returns the canonical string representation for use in query parameters.
    pub fn as_str(&self) -> &'static str {
        match self {
            RsCallType::TcpAsync => "tcp-async",
            RsCallType::TcpSync => "tcp-sync",
        }
    }
}

/// Target language for client code generation.
pub enum Lang {
    /// TypeScript (`.ts` files with full type annotations).
    #[cfg(feature = "ts")]
    TS(Vec<JsTsCallType>),
    /// Stub variant when the `ts` feature is disabled.
    #[cfg(not(feature = "ts"))]
    TS,
    /// JavaScript (`.js` files with JSDoc type annotations).
    #[cfg(feature = "js")]
    JS(Vec<JsTsCallType>),
    /// Stub variant when the `js` feature is disabled.
    #[cfg(not(feature = "js"))]
    JS,
    /// Kotlin (`.kt` files with full type annotations).
    #[cfg(feature = "kt")]
    KT(Vec<KtCallType>),
    /// Stub variant when the `kt` feature is disabled.
    #[cfg(not(feature = "kt"))]
    KT,
    /// Rust (`.rs` files with full type annotations).
    #[cfg(feature = "rs")]
    RS(Vec<RsCallType>),
    /// Stub variant when the `rs` feature is disabled.
    #[cfg(not(feature = "rs"))]
    RS,
}

/// Specifies a code generation output target.
///
/// Each target controls the language, output directory, and debug mode for
/// one generated client. Passed to [`AFast::generate`] to produce client
/// code at startup.
pub struct GenerateTarget {
    /// Target language and transport types.
    pub lang: Lang,
    /// Output directory path for the generated files.
    pub path: PathBuf,
    /// When `true`, the generated client logs parameter details before each
    /// request and response details after each request. For long-connection
    /// handlers, each sent message is also logged. Defaults to `false`.
    pub debug: bool,
}

/// Configuration for the interactive API documentation endpoint.
///
/// Passed to [`AFast::document`] to enable the `/doc` HTTP route and
/// optionally write static HTML files to disk on startup.
#[cfg(feature = "doc")]
pub struct DocConfig {
    /// Page title for the generated HTML. Defaults to `"afast — API Documentation"`.
    pub title: Option<String>,
    /// If set, HTML documentation files are written to this directory
    /// at application startup, in addition to being served via HTTP.
    pub output: Option<PathBuf>,
}

#[cfg(feature = "doc")]
impl DocConfig {
    /// Creates a `DocConfig` with default settings.
    pub fn new() -> Self {
        Self {
            title: None,
            output: None,
        }
    }

    /// Creates a `DocConfig` with the given page title and no output directory.
    pub fn with_title(title: &str) -> Self {
        Self {
            title: Some(title.to_string()),
            output: None,
        }
    }

    /// Creates a `DocConfig` with both a title and an output directory.
    pub fn with(title: &str, output: &str) -> Self {
        Self {
            title: Some(title.to_string()),
            output: Some(output.into()),
        }
    }

    /// Sets the page title for the generated documentation.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets an output directory. HTML documentation will be written to this
    /// directory on startup.
    pub fn output(mut self, path: impl Into<PathBuf>) -> Self {
        self.output = Some(path.into());
        self
    }
}

#[cfg(feature = "doc")]
impl Default for DocConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// TLS configuration for HTTPS server.
///
/// Passed to [`AFast::https`] to enable TLS on the HTTP server.
/// Supports PEM-encoded certificate chains and private keys.
#[cfg(feature = "tls")]
#[derive(Clone)]
pub struct TlsConfig {
    /// Path to the PEM-encoded certificate chain file.
    pub cert_path: String,
    /// Path to the PEM-encoded private key file.
    pub key_path: String,
}

/// The top-level application builder and runtime.
///
/// `AFast` configures shared state, services, and transport servers, then
/// starts the application via [`run`](AFast::run). All configuration follows
/// the builder pattern.
///
/// # Example
///
/// ```ignore
/// let app = AFast::new()
///     .state(AppState { db_url: "localhost".into() })
///     .service(api_svc)
///     .http("0.0.0.0:5000");
/// app.run().await.unwrap();
/// ```
pub struct AFast {
    /// Registered service containers.
    services: Vec<Service>,
    /// Pre-built handler invoker lookup table, indexed by handler offset.
    /// Built incrementally as services are registered via [`service`](AFast::service).
    #[cfg(any(feature = "ws", feature = "http", feature = "tcp"))]
    handler_table: Vec<Option<&'static dyn HandlerInvoker>>,
    /// Typed application state shared across all handlers.
    state: StateMap,
    /// Marker string for conditional serialization (`skip_with`).
    /// Stored here and passed to handlers via `Arc<String>` in `StateMap`.
    marker: String,
    /// WebSocket listen address (`"host:port"`), if bound.
    #[cfg(any(feature = "ws", feature = "doc"))]
    ws_addr: Option<String>,
    /// HTTP listen address (`"host:port"`), if bound.
    #[cfg(any(feature = "http", feature = "doc"))]
    http_addr: Option<String>,
    /// TCP listen address (`"host:port"`), if bound.
    #[cfg(feature = "tcp")]
    tcp_addr: Option<String>,
    /// Documentation configuration, if the `/doc` endpoint is enabled.
    #[cfg(feature = "doc")]
    doc_config: Option<DocConfig>,
    /// Client code generation targets, if any.
    #[cfg(any(feature = "ts", feature = "js", feature = "kt", feature = "rs"))]
    code_config: Option<Vec<GenerateTarget>>,
    /// Ordinary (REST-style) HTTP routes extracted from registered services.
    #[cfg(feature = "ordinary-http")]
    ordinary_routes: Vec<OrdinaryRouteInfo>,
    /// TLS configuration for HTTPS, if enabled.
    #[cfg(feature = "tls")]
    tls_config: Option<TlsConfig>,
    /// HTTPS listen address (`"host:port"`), if bound.
    #[cfg(feature = "tls")]
    https_addr: Option<String>,
    /// Rate-limit configuration, if enabled.
    #[cfg(feature = "rate-limit")]
    rate_limit_config: Option<crate::rate_limit::RateLimitConfig>,
    /// Registered lifecycle hooks.
    #[cfg(feature = "hook")]
    hooks: Vec<std::sync::Arc<dyn crate::hook::Hook>>,
}

impl AFast {
    /// Creates an empty application with no state, services, or transport
    /// bindings configured. Use the builder methods to add these before
    /// calling [`run`](AFast::run).
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            #[cfg(any(feature = "ws", feature = "http", feature = "tcp"))]
            handler_table: Vec::new(),
            state: StateMap::new(),
            marker: "afast".to_string(),
            #[cfg(any(feature = "ws", feature = "doc"))]
            ws_addr: None,
            #[cfg(any(feature = "http", feature = "doc"))]
            http_addr: None,
            #[cfg(feature = "tcp")]
            tcp_addr: None,
            #[cfg(feature = "doc")]
            doc_config: None,
            #[cfg(any(feature = "ts", feature = "js", feature = "kt", feature = "rs"))]
            code_config: None,
            #[cfg(feature = "ordinary-http")]
            ordinary_routes: Vec::new(),
            #[cfg(feature = "tls")]
            tls_config: None,
            #[cfg(feature = "tls")]
            https_addr: None,
            #[cfg(feature = "rate-limit")]
            rate_limit_config: None,
            #[cfg(feature = "hook")]
            hooks: Vec::new(),
        }
    }

    /// Sets the marker string used for conditional serialization.
    ///
    /// The marker is stored in the application and passed to handlers via
    /// `Arc<String>` in the [`StateMap`](crate::StateMap). It controls which
    /// `#[afast(skip_with("marker"))]` fields are skipped during
    /// serialization/deserialization.
    ///
    /// Defaults to `"afast"` if never called.
    pub fn marker(mut self, marker: &str) -> Self {
        self.marker = marker.to_string();
        self
    }

    /// Registers a lifecycle hook.
    ///
    /// Hooks receive callbacks before/after handler execution and on
    /// long-connection events.  Multiple hooks can be registered; they
    /// are called in registration order.
    #[cfg(feature = "hook")]
    pub fn hook(mut self, hook: impl crate::hook::Hook + 'static) -> Self {
        self.hooks.push(std::sync::Arc::new(hook));
        self
    }

    /// Registers a shared application state value.
    ///
    /// Each Rust type may have at most one value in the state map. A later
    /// call with the same type replaces the previous value. Handlers access
    /// state through the [`State<T>`](crate::State) extractor.
    pub fn state<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.state.insert(value);
        self
    }

    /// Binds the WebSocket server to the given address.
    ///
    /// Requires the `ws` feature. The address must use the form
    /// `"host:port"` (e.g. `"0.0.0.0:3000"`).
    #[cfg(feature = "ws")]
    pub fn ws(mut self, addr: &str) -> Self {
        self.ws_addr = Some(addr.to_string());
        self
    }

    /// Binds the HTTP server to the given address.
    ///
    /// Requires the `http` feature. The server exposes:
    /// - `POST /_api` for binary handler dispatch.
    /// - `GET /code/{service}/{lang}` for client code generation (requires `code`).
    /// - `GET /doc` for interactive documentation (requires `doc`).
    #[cfg(feature = "http")]
    pub fn http(mut self, addr: &str) -> Self {
        self.http_addr = Some(addr.to_string());
        self
    }

    /// Binds the HTTP server with TLS to the given address.
    ///
    /// Requires the `tls` feature. The server uses `rustls` for TLS and
    /// supports ALPN negotiation for HTTP/2. The address must use the form
    /// `"host:port"` (e.g. `"0.0.0.0:443"`).
    #[cfg(feature = "tls")]
    pub fn https(mut self, addr: &str, cert_path: &str, key_path: &str) -> Self {
        self.https_addr = Some(addr.to_string());
        self.tls_config = Some(TlsConfig {
            cert_path: cert_path.to_string(),
            key_path: key_path.to_string(),
        });
        self
    }

    /// Binds the TCP server to the given address.
    ///
    /// Requires the `tcp` feature. Uses length-prefix framing over raw TCP
    /// sockets. The address must use the form `"host:port"`.
    #[cfg(feature = "tcp")]
    pub fn tcp(mut self, addr: &str) -> Self {
        self.tcp_addr = Some(addr.to_string());
        self
    }

    /// Enables interactive API documentation.
    ///
    /// Requires the `doc` feature. When combined with the HTTP server,
    /// documentation is served at `GET /doc` and `GET /doc/{service}`.
    #[cfg(feature = "doc")]
    pub fn document(mut self, config: DocConfig) -> Self {
        self.doc_config = Some(config);
        self
    }

    /// Registers a service and its handlers with the application.
    ///
    /// Handler offsets are assigned automatically, continuing from the
    /// highest offset of previously registered services. Ordinary HTTP
    /// routes are extracted and stored separately for the HTTP server.
    ///
    /// If a service with the same name already exists (and the name is
    /// non-empty), the new service's handlers and ordinary routes are
    /// merged into the existing one instead of adding a duplicate.
    pub fn service(mut self, mut service: Service) -> Self {
        // Check for duplicate service name — merge if found.
        if !service.name.is_empty()
            && let Some(existing) = self.services.iter_mut().find(|s| s.name == service.name)
        {
            // Merge handlers: assign offsets for new handlers continuing
            // from the existing handler_table length.
            #[cfg(any(feature = "ws", feature = "http", feature = "tcp"))]
            build_handler_table(&mut service.handlers, &mut self.handler_table);
            #[cfg(not(any(feature = "ws", feature = "http", feature = "tcp")))]
            build_handler_table(&mut service.handlers, &mut 0);

            #[cfg(feature = "ordinary-http")]
            {
                // Add routes to the global HTTP dispatch table
                for route in &service.ordinary_routes {
                    self.ordinary_routes.push(route.clone());
                }
                collect_ordinary_from_tree(&service.handlers, "", &mut self.ordinary_routes);
                // Also add to the existing service for code generation
                existing
                    .ordinary_routes
                    .append(&mut service.ordinary_routes);
            }

            existing.handlers.append(&mut service.handlers);

            // Update description only if the existing one is empty
            if existing.desc.is_empty() && !service.desc.is_empty() {
                existing.desc = service.desc;
            }

            return self;
        }

        #[cfg(any(feature = "ws", feature = "http", feature = "tcp"))]
        build_handler_table(&mut service.handlers, &mut self.handler_table);
        #[cfg(not(any(feature = "ws", feature = "http", feature = "tcp")))]
        build_handler_table(&mut service.handlers, &mut 0);

        #[cfg(feature = "ordinary-http")]
        {
            for route in &service.ordinary_routes {
                self.ordinary_routes.push(route.clone());
            }
            collect_ordinary_from_tree(&service.handlers, "", &mut self.ordinary_routes);
        }

        self.services.push(service);
        self
    }

    /// Sets the client code generation targets.
    ///
    /// Requires at least one of the `ts`, `js`, `kt`, or `rs` features. Code is
    /// generated when [`run`](AFast::run) is called.
    #[cfg(any(feature = "ts", feature = "js", feature = "kt", feature = "rs"))]
    pub fn generate(mut self, targets: Vec<GenerateTarget>) -> Self {
        self.code_config = Some(targets);
        self
    }

    /// Sets the rate-limit configuration.
    ///
    /// Requires the `rate-limit` feature. Each handler can bind to a named
    /// policy via `#[handler(rate_limit("policy_id"))]`.
    #[cfg(feature = "rate-limit")]
    pub fn rate_limit(mut self, config: crate::rate_limit::RateLimitConfig) -> Self {
        self.rate_limit_config = Some(config);
        self
    }

    /// Writes interactive API documentation as static HTML files to the
    /// given directory.
    ///
    /// Requires the `doc` feature. Produces an `index.html` and one
    /// `{service}.html` file per registered service.
    #[cfg(feature = "doc")]
    pub fn generate_doc(&self, dir: &std::path::Path) -> Result<(), Error> {
        let title = self.doc_config.as_ref().and_then(|c| c.title.clone());
        codegen::doc::write_docs(&self.services, dir, title.as_deref()).map_err(|e| Error::Io {
            message: e.to_string(),
        })
    }

    /// Starts all configured servers and runs until Ctrl+C.
    ///
    /// Server startup ordering:
    /// 1. Documentation HTML is written to disk if an output directory is set.
    /// 2. Client code is generated for each configured target.
    /// 3. Transport servers (WS, HTTP, TCP) are launched on their respective
    ///    addresses.
    /// 4. The main task blocks on `ctrl_c()`. On shutdown, a signal is
    ///    broadcast to all servers, which stop accepting new connections.
    ///
    /// If no transport server is configured (code generation only), the
    /// application waits for Ctrl+C to keep the process alive.
    pub async fn run(self) -> Result<(), Error> {
        // Set the code-generation marker before any codegen runs.
        // This is read immutably by should_include_field during code generation.
        crate::marker::set_codegen_marker(&self.marker);

        // Write doc HTML to output directory if configured
        #[cfg(feature = "doc")]
        if let Some(doc_cfg) = &self.doc_config
            && let Some(dir) = &doc_cfg.output
        {
            let title = doc_cfg.title.clone();
            codegen::doc::write_docs(&self.services, dir, title.as_deref()).map_err(|e| {
                Error::Io {
                    message: e.to_string(),
                }
            })?;
            println!("afast: docs written to {}", dir.display());
        }

        #[cfg(any(feature = "ts", feature = "js", feature = "kt", feature = "rs"))]
        if let Some(code_config) = &self.code_config {
            for target in code_config {
                match &target.lang {
                    #[cfg(feature = "ts")]
                    Lang::TS(calls) => self.generate_ts(&target.path, calls, target.debug)?,
                    #[cfg(feature = "js")]
                    Lang::JS(calls) => self.generate_js(&target.path, calls, target.debug)?,
                    #[cfg(feature = "kt")]
                    Lang::KT(calls) => self.generate_kt(&target.path, calls, target.debug)?,
                    #[cfg(feature = "rs")]
                    Lang::RS(calls) => self.generate_rs(&target.path, calls, target.debug)?,
                    #[allow(unreachable_patterns)]
                    _ => panic!("warning: language generation not enabled"),
                }
            }
        }

        #[cfg(any(feature = "ws", feature = "http", feature = "tcp"))]
        {
            let mut has_server = false;
            #[cfg(feature = "ws")]
            if self.ws_addr.is_some() {
                has_server = true;
            }
            #[cfg(feature = "http")]
            if self.http_addr.is_some() {
                has_server = true;
            }
            #[cfg(feature = "tcp")]
            if self.tcp_addr.is_some() {
                has_server = true;
            }
            #[cfg(feature = "tls")]
            if self.https_addr.is_some() {
                has_server = true;
            }

            if has_server {
                let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

                // Insert marker into state so handlers can access it via Arc<String>
                let mut state = self.state;
                state.insert(std::sync::Arc::new(self.marker));

                let state = std::sync::Arc::new(state);

                #[cfg(feature = "hook")]
                let hooks = {
                    // Build a per-handler hook table: handler_hooks[handler_id] is
                    // the hooks for that handler (service-specific or global).
                    let total = self.handler_table.len();
                    let mut handler_hooks: Vec<Vec<std::sync::Arc<dyn crate::hook::Hook>>> =
                        vec![Vec::new(); total];

                    // Walk each service's handler tree and assign hooks.
                    for svc in &self.services {
                        let svc_hooks: &Vec<std::sync::Arc<dyn crate::hook::Hook>> = &svc.hooks;
                        assign_hooks_recursive(
                            &svc.handlers,
                            svc_hooks,
                            &self.hooks,
                            &mut handler_hooks,
                        );
                    }

                    std::sync::Arc::new(handler_hooks)
                };

                let handlers = std::sync::Arc::new(self.handler_table);

                #[cfg(any(feature = "code", feature = "doc"))]
                let http_services = self.services.clone();
                #[cfg(all(feature = "tls", any(feature = "code", feature = "doc")))]
                let https_services = http_services.clone();

                #[cfg(feature = "doc")]
                let doc_title = self.doc_config.and_then(|c| c.title);

                // Build the rate limiter (if configured).
                #[cfg(feature = "rate-limit")]
                let (rate_limiter, rate_handler_names) = {
                    // Collect all HandlerMeta references for the limiter to scan.
                    let all_meta: Vec<&crate::handler::HandlerMeta> = self
                        .services
                        .iter()
                        .flat_map(|svc| svc.handlers.iter())
                        .map(|h| h.meta)
                        .collect();
                    match self.rate_limit_config {
                        Some(cfg) => (
                            Some(std::sync::Arc::new(crate::rate_limit::RateLimiter::new(
                                cfg, &all_meta,
                            ))),
                            {
                                // Build handler_id → handler_name mapping.
                                let mut names = Vec::with_capacity(handlers.len());
                                for i in 0..handlers.len() {
                                    let name = self
                                        .services
                                        .iter()
                                        .flat_map(|svc| svc.handlers.iter())
                                        .find(|h| h.offset == i)
                                        .map(|h| h.meta.name)
                                        .unwrap_or("");
                                    names.push(name.to_string());
                                }
                                names
                            },
                        ),
                        None => (None, Vec::new()),
                    }
                };

                let mut handles: Vec<tokio::task::JoinHandle<Result<(), Error>>> = Vec::new();

                // Pre-clone rate limiter for use across multiple server spawns.
                #[cfg(feature = "rate-limit")]
                let rate_limiter_outer = rate_limiter.clone();
                #[cfg(feature = "rate-limit")]
                let rate_handler_names_outer = rate_handler_names.clone();

                // Detect if WS and HTTP/HTTPS share the same address
                #[cfg(all(feature = "ws", feature = "http"))]
                let ws_merged = self
                    .ws_addr
                    .as_ref()
                    .map(|w| {
                        #[cfg(feature = "tls")]
                        if self.https_addr.as_ref().is_some_and(|h| w == h) {
                            return true;
                        }
                        self.http_addr.as_ref().is_some_and(|h| w == h)
                    })
                    .unwrap_or(false);
                #[cfg(all(feature = "ws", not(feature = "http")))]
                let ws_merged = false;

                // Start WS server (standalone, only if not merged with HTTP)
                #[cfg(feature = "ws")]
                if let Some(addr) = &self.ws_addr {
                    if ws_merged {
                        println!("afast: ws merged with http on {}", addr);
                    } else {
                        use tokio::net::TcpListener;

                        let listener = TcpListener::bind(addr).await.map_err(|e| Error::Ws {
                            message: e.to_string(),
                        })?;
                        println!("afast: ws server listening on {}", addr);

                        let state_clone = state.clone();
                        let handlers_clone = handlers.clone();
                        #[cfg(feature = "hook")]
                        let hooks_clone = hooks.clone();
                        #[cfg(feature = "rate-limit")]
                        let rl_base = rate_limiter_outer.clone();
                        #[cfg(feature = "rate-limit")]
                        let hn_base = rate_handler_names_outer.clone();
                        let mut shutdown_rx = shutdown_tx.subscribe();

                        let server = tokio::spawn(async move {
                            loop {
                                tokio::select! {
                                    accept = listener.accept() => {
                                        match accept {
                                            Ok((stream, _)) => {
                                                let state = state_clone.clone();
                                                let handlers = handlers_clone.clone();
                                                #[cfg(feature = "hook")]
                                                let hooks = hooks_clone.clone();
                                                #[cfg(feature = "rate-limit")]
                                                let rl = rl_base.clone();
                                                #[cfg(feature = "rate-limit")]
                                                let hn = hn_base.clone();
                                                tokio::spawn(async move {
                                                    crate::app::transport::handle_connection(
                                                        stream,
                                                        state,
                                                        handlers,
                                                        #[cfg(feature = "hook")]
                                                        hooks,
                                                        #[cfg(feature = "rate-limit")]
                                                        rl,
                                                        #[cfg(feature = "rate-limit")]
                                                        hn,
                                                    )
                                                    .await;
                                                });
                                            }
                                            Err(e) => eprintln!("afast: accept error: {}", e),
                                        }
                                    }
                                    _ = shutdown_rx.recv() => break,
                                }
                            }
                            Ok(())
                        });
                        handles.push(server);
                    } // end else (standalone WS)
                } // end if let Some(addr)

                // Start HTTP server (plain, no TLS)
                #[cfg(feature = "http")]
                if let Some(addr) = &self.http_addr {
                    let addr: std::net::SocketAddr = addr.parse().map_err(|e| Error::Http {
                        message: format!("invalid http address: {}", e),
                    })?;

                    let state_clone = state.clone();
                    let handlers_clone = handlers.clone();
                    #[cfg(feature = "hook")]
                    let hooks_clone = hooks.clone();
                    let shutdown_rx = shutdown_tx.subscribe();
                    #[cfg(feature = "doc")]
                    let doc_title_clone = doc_title.clone();
                    #[cfg(feature = "doc")]
                    let ws_addr_clone = self.ws_addr.clone();
                    #[cfg(feature = "ordinary-http")]
                    let ordinary_routes = self.ordinary_routes.clone();
                    #[cfg(feature = "rate-limit")]
                    let rl = rate_limiter_outer.clone();
                    #[cfg(feature = "rate-limit")]
                    let hn = rate_handler_names_outer.clone();

                    let server = tokio::spawn(async move {
                        crate::app::transport::serve(
                            crate::app::transport::HttpConfig {
                                addr,
                                state: state_clone,
                                handlers: handlers_clone,
                                #[cfg(feature = "hook")]
                                hooks: hooks_clone,
                                #[cfg(any(feature = "code", feature = "doc"))]
                                services: http_services,
                                #[cfg(feature = "doc")]
                                doc_title: doc_title_clone,
                                #[cfg(feature = "doc")]
                                ws_addr_str: ws_addr_clone,
                                #[cfg(feature = "ordinary-http")]
                                ordinary_routes,
                                #[cfg(feature = "ws")]
                                enable_ws_upgrade: ws_merged,
                                #[cfg(feature = "tls")]
                                tls_config: None,
                                #[cfg(feature = "rate-limit")]
                                rate_limiter: rl,
                                #[cfg(feature = "rate-limit")]
                                handler_names: hn,
                            },
                            shutdown_rx,
                        )
                        .await
                    });
                    handles.push(server);
                }

                // Start HTTPS server (TLS)
                #[cfg(feature = "tls")]
                if let Some(addr) = &self.https_addr {
                    let addr: std::net::SocketAddr = addr.parse().map_err(|e| Error::Http {
                        message: format!("invalid https address: {}", e),
                    })?;

                    let state_clone = state.clone();
                    let handlers_clone = handlers.clone();
                    #[cfg(feature = "hook")]
                    let hooks_clone = hooks.clone();
                    let shutdown_rx = shutdown_tx.subscribe();
                    #[cfg(feature = "doc")]
                    let doc_title_clone = doc_title.clone();
                    #[cfg(feature = "doc")]
                    let ws_addr_clone = self.ws_addr.clone();
                    #[cfg(feature = "ordinary-http")]
                    let ordinary_routes = self.ordinary_routes.clone();
                    let tls_config = self.tls_config.clone();
                    #[cfg(feature = "rate-limit")]
                    let rl = rate_limiter_outer.clone();
                    #[cfg(feature = "rate-limit")]
                    let hn = rate_handler_names_outer.clone();

                    let server = tokio::spawn(async move {
                        crate::app::transport::serve(
                            crate::app::transport::HttpConfig {
                                addr,
                                state: state_clone,
                                handlers: handlers_clone,
                                #[cfg(feature = "hook")]
                                hooks: hooks_clone,
                                #[cfg(any(feature = "code", feature = "doc"))]
                                services: https_services,
                                #[cfg(feature = "doc")]
                                doc_title: doc_title_clone,
                                #[cfg(feature = "doc")]
                                ws_addr_str: ws_addr_clone,
                                #[cfg(feature = "ordinary-http")]
                                ordinary_routes,
                                #[cfg(feature = "ws")]
                                enable_ws_upgrade: ws_merged,
                                #[cfg(feature = "tls")]
                                tls_config,
                                #[cfg(feature = "rate-limit")]
                                rate_limiter: rl,
                                #[cfg(feature = "rate-limit")]
                                handler_names: hn,
                            },
                            shutdown_rx,
                        )
                        .await
                    });
                    handles.push(server);
                }

                // Start TCP server
                #[cfg(feature = "tcp")]
                if let Some(addr) = &self.tcp_addr {
                    use tokio::net::TcpListener;

                    let listener = TcpListener::bind(addr).await.map_err(|e| Error::Tcp {
                        message: e.to_string(),
                    })?;
                    println!("afast: tcp server listening on {}", addr);

                    let state_clone = state.clone();
                    let handlers_clone = handlers.clone();
                    #[cfg(feature = "hook")]
                    let hooks_clone = hooks.clone();
                    let mut shutdown_rx = shutdown_tx.subscribe();
                    #[cfg(feature = "rate-limit")]
                    let rl = rate_limiter_outer.clone();
                    #[cfg(feature = "rate-limit")]
                    let hn = rate_handler_names_outer.clone();

                    let server = tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                accept = listener.accept() => {
                                    match accept {
                                        Ok((stream, _)) => {
                                            let state = state_clone.clone();
                                            let handlers = handlers_clone.clone();
                                            #[cfg(feature = "hook")]
                                            let hooks = hooks_clone.clone();
                                            #[cfg(feature = "rate-limit")]
                                            let rl = rl.clone();
                                            #[cfg(feature = "rate-limit")]
                                            let hn = hn.clone();
                                            #[cfg(feature = "rate-limit")]
                                            let rl = rl.clone();
                                            #[cfg(feature = "rate-limit")]
                                            let hn = hn.clone();
                                            tokio::spawn(async move {
                                                crate::app::transport::handle_tcp_connection(
                                                    stream,
                                                    state,
                                                    handlers,
                                                    #[cfg(feature = "hook")]
                                                    hooks,
                                                    #[cfg(feature = "rate-limit")]
                                                    rl,
                                                    #[cfg(feature = "rate-limit")]
                                                    hn,
                                                )
                                                .await;
                                            });
                                        }
                                        Err(e) => eprintln!("afast: tcp accept error: {}", e),
                                    }
                                }
                                _ = shutdown_rx.recv() => break,
                            }
                        }
                        Ok(())
                    });
                    handles.push(server);
                }

                // Wait for Ctrl+C
                tokio::signal::ctrl_c().await.map_err(|e| Error::Signal {
                    message: e.to_string(),
                })?;
                println!("\nafast: shutting down...");
                let _ = shutdown_tx.send(());

                for handle in handles {
                    let _ = handle.await;
                }

                return Ok(());
            }
        }

        // No transport servers configured — wait for Ctrl+C to keep the
        // process alive (e.g. code-generation-only mode).
        tokio::signal::ctrl_c().await.map_err(|e| Error::Signal {
            message: e.to_string(),
        })?;
        println!("\nafast: shutting down...");
        Ok(())
    }
}

/// Assigns offsets and builds the handler invoker lookup table in a single
/// depth-first pass over the handler tree.
///
/// Each handler receives an `offset` field equal to its position in the
/// global table. Binary-protocol handlers (non-ordinary leaves) have their
/// invoker stored at that offset in the table; group nodes and ordinary
/// leaves store `None`. Nesting is accounted for: parents before children.
#[cfg(any(feature = "ws", feature = "http", feature = "tcp"))]
fn build_handler_table(
    handlers: &mut [Handler],
    table: &mut Vec<Option<&'static dyn HandlerInvoker>>,
) {
    for h in handlers.iter_mut() {
        h.offset = table.len();
        let is_binary_leaf = !h.meta.name.is_empty() && !h.meta.is_ordinary;
        table.push(if is_binary_leaf {
            Some(h.invoker)
        } else {
            None
        });
        build_handler_table(&mut h.children, table);
    }
}

/// Assigns offsets only (no table building), used when no transport feature
/// is enabled that requires the invoker lookup table.
#[cfg(not(any(feature = "ws", feature = "http", feature = "tcp")))]
fn build_handler_table(handlers: &mut [Handler], counter: &mut usize) {
    for h in handlers.iter_mut() {
        h.offset = *counter;
        *counter += 1;
        build_handler_table(&mut h.children, counter);
    }
}

/// Recursively assigns hooks to each handler's slot in the `handler_hooks`
/// table.  If the service has its own hooks, they are **appended** after the
/// global hooks so that both run (global first, then service-specific).
#[cfg(feature = "hook")]
fn assign_hooks_recursive(
    handlers: &[Handler],
    svc_hooks: &[std::sync::Arc<dyn crate::hook::Hook>],
    global_hooks: &[std::sync::Arc<dyn crate::hook::Hook>],
    table: &mut [Vec<std::sync::Arc<dyn crate::hook::Hook>>],
) {
    for h in handlers {
        let idx = h.offset;
        if idx < table.len() {
            let mut hooks = global_hooks.to_vec();
            hooks.extend(svc_hooks.iter().cloned());
            table[idx] = hooks;
        }
        assign_hooks_recursive(&h.children, svc_hooks, global_hooks, table);
    }
}

impl Default for AFast {
    fn default() -> Self {
        Self::new()
    }
}

/// Recursively walks the handler tree to collect ordinary HTTP route
/// definitions, building full paths from group prefixes.
///
/// Group nodes contribute their name to a path prefix for children.
/// Leaf nodes with an `ordinary_def` emit a route record.
#[cfg(feature = "ordinary-http")]
fn collect_ordinary_from_tree(
    handlers: &[Handler],
    prefix: &str,
    routes: &mut Vec<OrdinaryRouteInfo>,
) {
    for h in handlers {
        if let Some(def) = &h.ordinary_def {
            // Ordinary leaf: prefix comes from parent groups only, not handler name
            let normalized_path = if h.path.is_empty() || h.path.starts_with('/') {
                h.path.to_string()
            } else {
                format!("/{}", h.path)
            };
            let full_path = if normalized_path.is_empty() || normalized_path == "/" {
                if prefix.is_empty() {
                    "/".to_string()
                } else {
                    prefix.to_string()
                }
            } else {
                format!("{}{}", prefix, normalized_path)
            };
            routes.push(OrdinaryRouteInfo {
                method: h.method,
                path: full_path,
                handler_entry: def.clone(),
            });
        } else {
            // Group or binary leaf: name contributes to child path prefix
            let child_prefix = if h.name.is_empty() {
                prefix.to_string()
            } else {
                format!("{}/{}", prefix, h.name)
            };
            collect_ordinary_from_tree(&h.children, &child_prefix, routes);
        }
    }
}

mod codegen;
#[cfg(feature = "ordinary-http")]
pub mod ordinary;
mod transport;
