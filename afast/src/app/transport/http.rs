//! HTTP transport implementation.
//!
//! # Server endpoints
//!
//! | Method | Path                       | Description                          |
//! |--------|----------------------------|--------------------------------------|
//! | POST   | `/_api`                    | Binary handler dispatch.             |
//! | GET    | `/_ws`                     | WebSocket upgrade (merged mode).     |
//! | GET    | `/code/{service}/{lang}`   | Client code generation (`code`).     |
//! | GET    | `/doc`                     | API documentation index (`doc`).     |
//! | GET    | `/doc/{service}`           | Service-specific docs (`doc`).       |
//! | *      | (ordinary routes)          | REST-style handlers (`ordinary-http`).|
//!
//! # Response wire format (binary endpoints)
//!
//! **Success**: `[0u8][0i64][data: bytes]`
//!
//! **Error**: `[1u8][code: i64][message: bytes]`

use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::server::conn::auto;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

#[cfg(any(feature = "code", feature = "doc"))]
use crate::Service;
use crate::StateMap;
#[cfg(any(feature = "ordinary-http", feature = "ordinary-ws"))]
use crate::app::ordinary::RoutePattern;
use crate::error::{CODE_HTTP, CODE_LONG_CONNECTION_NOT_SUPPORTED, Error};
use crate::handler::HandlerInvoker;
#[cfg(feature = "rate-limit")]
use crate::rate_limit::{ConnectionContext, RateLimiter};
#[cfg(feature = "ordinary-http")]
use crate::service::OrdinaryRouteInfo;

/// Configuration for the HTTP server.
///
/// Aggregates all parameters needed by [`serve`] to avoid excessive
/// function arguments.
pub struct HttpConfig {
    pub addr: SocketAddr,
    pub state: Arc<StateMap>,
    pub handlers: Arc<Vec<Option<&'static dyn HandlerInvoker>>>,
    #[cfg(any(feature = "code", feature = "doc"))]
    pub services: Vec<Service>,
    #[cfg(feature = "doc")]
    pub doc_title: Option<String>,
    #[cfg(feature = "doc")]
    pub ws_addr_str: Option<String>,
    #[cfg(feature = "ordinary-http")]
    pub ordinary_routes: Vec<OrdinaryRouteInfo>,
    #[cfg(feature = "ordinary-ws")]
    pub ws_routes: Vec<crate::app::ordinary_ws::WsRouteInfo>,
    #[cfg(all(feature = "ws", feature = "binary"))]
    pub enable_ws_upgrade: bool,
    #[cfg(feature = "tls")]
    pub tls_config: Option<crate::app::TlsConfig>,
    #[cfg(feature = "rate-limit")]
    pub rate_limiter: Option<Arc<RateLimiter>>,
    /// Handler names indexed by handler ID, for rate-limit lookups.
    #[cfg(feature = "rate-limit")]
    pub handler_names: Vec<String>,
    /// Registered lifecycle hooks.
    #[cfg(feature = "hook")]
    pub hooks: Arc<Vec<Vec<std::sync::Arc<dyn crate::hook::Hook>>>>,
}

/// Starts the HTTP server and blocks until a shutdown signal is received.
///
/// This function binds a TCP listener, compiles ordinary HTTP routes
/// (if enabled), and enters an accept loop that spawns a task per
/// connection. Each connection is auto-negotiated (HTTP/1.1 or HTTP/2)
/// with upgrade support (for merged WebSocket mode). When TLS is
/// enabled via the `tls` feature, connections are wrapped with TLS
/// and support ALPN negotiation for HTTP/2.
pub async fn serve(
    config: HttpConfig,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<(), Error> {
    let listener = TcpListener::bind(config.addr)
        .await
        .map_err(|e| Error::Http {
            message: e.to_string(),
        })?;

    println!("afast: http server listening on {}", config.addr);

    // Compile ordinary route patterns once at startup to avoid
    // re-parsing on every request.
    #[cfg(feature = "ordinary-http")]
    let compiled_routes: Vec<CompiledOrdinaryRoute> = config
        .ordinary_routes
        .iter()
        .map(|r| CompiledOrdinaryRoute {
            method: r.method,
            pattern: RoutePattern::parse(&r.path),
            invoker: r
                .handler_entry
                .ordinary_invoker
                .expect("ordinary_invoker must be set for ordinary routes"),
            handler_name: r.handler_entry.name,
        })
        .collect();

    // Compile ordinary-ws route patterns once at startup.
    #[cfg(feature = "ordinary-ws")]
    let compiled_ws_routes: Vec<CompiledWsRoute> = config
        .ws_routes
        .iter()
        .map(|r| CompiledWsRoute {
            pattern: RoutePattern::parse(r.path),
            invoker: r.invoker,
            handler_name: r.handler_name,
        })
        .collect();

    let shared = Arc::new(SharedState {
        state: config.state,
        handlers: config.handlers,
        #[cfg(any(feature = "code", feature = "doc"))]
        services: config.services,
        #[cfg(feature = "doc")]
        doc_title: config.doc_title,
        #[cfg(feature = "doc")]
        http_addr: config.addr.to_string(),
        #[cfg(feature = "doc")]
        ws_addr: config.ws_addr_str,
        #[cfg(feature = "ordinary-http")]
        ordinary_routes: compiled_routes,
        #[cfg(feature = "ordinary-ws")]
        ws_routes: compiled_ws_routes,
        #[cfg(all(feature = "ws", feature = "binary"))]
        enable_ws_upgrade: config.enable_ws_upgrade,
        #[cfg(feature = "rate-limit")]
        rate_limiter: config.rate_limiter,
        #[cfg(feature = "rate-limit")]
        handler_names: config.handler_names,
        #[cfg(feature = "hook")]
        hooks: config.hooks,
    });

    // Set up TLS acceptor if TLS config is provided.
    #[cfg(feature = "tls")]
    let tls_acceptor = if let Some(ref cfg) = config.tls_config {
        // Install the ring crypto provider (idempotent — safe to call multiple times).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certs = load_certs(&cfg.cert_path)?;
        let key = load_key(&cfg.key_path)?;
        let mut server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| Error::Http {
                message: e.to_string(),
            })?;
        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Some(tokio_rustls::TlsAcceptor::from(Arc::new(server_config)))
    } else {
        None
    };

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        let shared = shared.clone();
                        #[cfg(feature = "tls")]
                        let tls = tls_acceptor.clone();
                        tokio::spawn(async move {
                            // Extract client IP from the TCP peer address.
                            let peer_ip = stream.peer_addr()
                                .map(|a| a.ip().to_string())
                                .unwrap_or_else(|_| "unknown".to_string());

                            #[cfg(feature = "tls")]
                            match tls {
                                Some(ref acceptor) => {
                                    match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            let client_ip = tls_stream.get_ref().0
                                                .peer_addr()
                                                .map(|a| a.ip().to_string())
                                                .unwrap_or(peer_ip);
                                            let io = hyper_util::rt::TokioIo::new(tls_stream);
                                            serve_connection(io, shared, client_ip).await;
                                        }
                                        Err(e) => {
                                            eprintln!("afast: tls handshake error: {}", e);
                                        }
                                    }
                                }
                                None => {
                                    let io = hyper_util::rt::TokioIo::new(stream);
                                    serve_connection(io, shared, peer_ip).await;
                                }
                            };
                            #[cfg(not(feature = "tls"))]
                            {
                                let io = hyper_util::rt::TokioIo::new(stream);
                                serve_connection(io, shared, peer_ip).await;
                            }
                        });
                    }
                    Err(e) => eprintln!("afast: http accept error: {}", e),
                }
            }
            _ = shutdown_rx.recv() => break,
        }
    }

    Ok(())
}

/// Serves a single HTTP connection with auto-detected protocol.
///
/// Generic over the IO type to support both plain TCP and TLS streams.
async fn serve_connection<S>(
    io: hyper_util::rt::TokioIo<S>,
    shared: Arc<SharedState>,
    client_ip: String,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req| {
        let shared = shared.clone();
        let client_ip = client_ip.clone();
        async move { handle_request(req, &shared, &client_ip).await }
    });
    if let Err(e) = auto::Builder::new(hyper_util::rt::TokioExecutor::new())
        .serve_connection_with_upgrades(io, service)
        .await
    {
        let msg = e.to_string();
        if !msg.contains("incomplete message") {
            eprintln!("afast: http connection error: {}", e);
        }
    }
}

/// Immutable shared state for all HTTP request handlers.
struct SharedState {
    state: Arc<StateMap>,
    handlers: Arc<Vec<Option<&'static dyn HandlerInvoker>>>,
    #[cfg(any(feature = "code", feature = "doc"))]
    services: Vec<Service>,
    #[cfg(feature = "doc")]
    doc_title: Option<String>,
    #[cfg(feature = "doc")]
    http_addr: String,
    #[cfg(feature = "doc")]
    ws_addr: Option<String>,
    #[cfg(feature = "ordinary-http")]
    ordinary_routes: Vec<CompiledOrdinaryRoute>,
    #[cfg(feature = "ordinary-ws")]
    ws_routes: Vec<CompiledWsRoute>,
    #[cfg(all(feature = "ws", feature = "binary"))]
    enable_ws_upgrade: bool,
    #[cfg(feature = "rate-limit")]
    rate_limiter: Option<Arc<RateLimiter>>,
    #[cfg(feature = "rate-limit")]
    handler_names: Vec<String>,
    #[cfg(feature = "hook")]
    hooks: Arc<Vec<Vec<std::sync::Arc<dyn crate::hook::Hook>>>>,
}

/// A pre-compiled ordinary HTTP route ready for request matching.
#[cfg(feature = "ordinary-http")]
struct CompiledOrdinaryRoute {
    method: &'static str,
    pattern: RoutePattern,
    invoker: &'static dyn crate::handler::OrdinaryHandlerInvoker,
    #[cfg_attr(not(feature = "rate-limit"), allow(dead_code))]
    handler_name: &'static str,
}

/// A pre-compiled ordinary-ws route ready for WebSocket upgrade matching.
#[cfg(feature = "ordinary-ws")]
struct CompiledWsRoute {
    pattern: RoutePattern,
    invoker: &'static dyn crate::app::ordinary_ws::WsHandlerInvoker,
    handler_name: &'static str,
}

/// Dispatches an incoming HTTP request to the correct handler.
///
/// Resolution order:
/// 1. Ordinary HTTP routes (compiled patterns, first match wins).
/// 2. `POST /_api` for binary handler dispatch.
/// 3. `GET /code/{service}/{lang}` for client code generation.
/// 4. `GET /doc[/{service}]` for interactive documentation.
/// 5. `GET /_ws` for WebSocket upgrade (merged mode only).
/// 6. 404 for unmatched paths.
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    shared: &SharedState,
    client_ip: &str,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let segments: Vec<&str> = path
        .trim_start_matches('/')
        .trim_end_matches('/')
        .split('/')
        .collect();

    // Try ordinary HTTP routes first
    #[cfg(feature = "ordinary-http")]
    {
        let method_str = method.as_str();
        for compiled in &shared.ordinary_routes {
            if compiled.method != method_str {
                continue;
            }
            if let Some(path_params) = compiled.pattern.matches(&path) {
                // Rate-limit check for ordinary routes.
                #[cfg(feature = "rate-limit")]
                if let Some(ref limiter) = shared.rate_limiter {
                    let handler_name = compiled.handler_name;
                    let mut ctx = ConnectionContext::new(client_ip.to_string());
                    // Pre-populate header cache from the request.
                    for (name, value) in req.headers().iter() {
                        if let Ok(v) = value.to_str() {
                            ctx.header_cache
                                .insert(name.as_str().to_lowercase(), v.to_string());
                        }
                    }
                    if let Err(e) = limiter.check(handler_name, &mut ctx).await {
                        let status = StatusCode::TOO_MANY_REQUESTS;
                        let body =
                            format!("{{\"code\":{},\"message\":\"{}\"}}", e.code(), e.message());
                        return Ok(Response::builder()
                            .status(status)
                            .header("content-type", "application/json; charset=utf-8")
                            .header("retry-after", "60")
                            .body(Full::new(Bytes::from(body)))
                            .expect("valid response builder"));
                    }
                }

                let query_string = uri.query().unwrap_or("");
                let state = shared.state.clone();
                let invoker = compiled.invoker;
                return match invoker
                    .call_ordinary(req, &path_params, query_string, &state)
                    .await
                {
                    Ok(response) => Ok(response),
                    Err(e) => {
                        let code = e.code();
                        let message = e.message();
                        // Map user error codes in the 4xx–5xx range to HTTP status
                        // codes so the client receives a semantically correct response.
                        let status = if (400..600).contains(&code) {
                            StatusCode::from_u16(code as u16)
                                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
                        } else {
                            StatusCode::INTERNAL_SERVER_ERROR
                        };
                        let body = format!("{{\"code\":{},\"message\":\"{}\"}}", code, message);
                        Ok(Response::builder()
                            .status(status)
                            .header("content-type", "application/json; charset=utf-8")
                            .body(Full::new(Bytes::from(body)))
                            .expect("valid response builder"))
                    }
                };
            }
        }
    }

    // Try ordinary-ws routes (WebSocket upgrade with path matching)
    #[cfg(feature = "ordinary-ws")]
    if method == hyper::Method::GET {
        // Check if this is a WebSocket upgrade request.
        let is_ws_upgrade = req
            .headers()
            .get(hyper::header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_lowercase().contains("websocket"))
            .unwrap_or(false);

        if is_ws_upgrade {
            for compiled in &shared.ws_routes {
                if let Some(path_params) = compiled.pattern.matches(&path) {
                    // Rate-limit check for ordinary-ws routes.
                    #[cfg(feature = "rate-limit")]
                    if let Some(ref limiter) = shared.rate_limiter {
                        let mut ctx = ConnectionContext::new(client_ip.to_string());
                        for (name, value) in req.headers().iter() {
                            if let Ok(v) = value.to_str() {
                                ctx.header_cache
                                    .insert(name.as_str().to_lowercase(), v.to_string());
                            }
                        }
                        if let Err(e) = limiter.check(compiled.handler_name, &mut ctx).await {
                            let status = StatusCode::TOO_MANY_REQUESTS;
                            let body = format!(
                                "{{\"code\":{},\"message\":\"{}\"}}",
                                e.code(),
                                e.message()
                            );
                            return Ok(Response::builder()
                                .status(status)
                                .header("content-type", "application/json; charset=utf-8")
                                .header("retry-after", "60")
                                .body(Full::new(Bytes::from(body)))
                                .expect("valid response builder"));
                        }
                    }

                    let query_string = uri.query().unwrap_or("");
                    return handle_ordinary_ws_upgrade(
                        req,
                        shared,
                        compiled.invoker,
                        compiled.handler_name,
                        query_string,
                        path_params,
                        client_ip,
                    )
                    .await;
                }
            }
        }
    }

    match segments.first().copied() {
        #[cfg(feature = "binary")]
        Some("_api") => {
            if method != hyper::Method::POST {
                return method_not_allowed();
            }
            handle_api(req, shared, client_ip).await
        }
        #[cfg(feature = "code")]
        Some("code") => {
            if method != hyper::Method::GET {
                return method_not_allowed();
            }
            handle_code(&segments, uri.query(), shared)
        }
        #[cfg(feature = "doc")]
        Some("doc") => {
            if method != hyper::Method::GET {
                return method_not_allowed();
            }
            handle_doc(&segments, shared)
        }
        #[cfg(all(feature = "ws", feature = "binary"))]
        Some("_ws") => {
            if shared.enable_ws_upgrade {
                return handle_ws_upgrade(req, shared, client_ip).await;
            }
            not_found()
        }
        _ => not_found(),
    }
}

/// Handles `POST /_api` — binary handler dispatch.
///
/// The request body contains a 4-byte little-endian handler ID followed
/// by the handler payload. The handler is looked up in the offset table
/// and invoked directly. Long-connection handlers are rejected (they
/// require WebSocket).
///
/// Response format:
/// - Success: `[0u8][0i64][data]` with `content-type: application/octet-stream`.
/// - Error: `[1u8][code i64][message bytes]` with the same content type.
#[cfg(feature = "binary")]
#[allow(unused_variables)]
async fn handle_api(
    req: Request<hyper::body::Incoming>,
    shared: &SharedState,
    client_ip: &str,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    use http_body_util::BodyExt;
    let headers = req.headers().clone();
    let body = req.into_body().collect().await?.to_bytes();

    if body.len() < 4 {
        return error_response(
            StatusCode::BAD_REQUEST,
            CODE_HTTP,
            "request body too short, expected handler_id",
        );
    }

    let handler_id = body[0] as usize
        | (body[1] as usize) << 8
        | (body[2] as usize) << 16
        | (body[3] as usize) << 24;

    let invoker = match shared.handlers.get(handler_id).and_then(|h| *h) {
        Some(invoker) => invoker,
        None => {
            return error_response(
                StatusCode::NOT_FOUND,
                CODE_HTTP,
                &format!("handler not found (id={})", handler_id),
            );
        }
    };

    if invoker.is_long_connection() {
        return error_response(
            StatusCode::BAD_REQUEST,
            CODE_LONG_CONNECTION_NOT_SUPPORTED,
            "long connection handlers are not supported in HTTP mode, use WebSocket instead",
        );
    }

    // Rate-limit check for binary API handlers.
    #[cfg(feature = "rate-limit")]
    if let Some(ref limiter) = shared.rate_limiter {
        let handler_name = shared
            .handler_names
            .get(handler_id)
            .map(|s| s.as_str())
            .unwrap_or("");
        if !handler_name.is_empty() {
            let mut ctx = ConnectionContext::new(client_ip.to_string());
            for (name, value) in headers.iter() {
                if let Ok(v) = value.to_str() {
                    ctx.header_cache
                        .insert(name.as_str().to_lowercase(), v.to_string());
                }
            }
            if let Err(e) = limiter.check(handler_name, &mut ctx).await {
                return error_response(StatusCode::TOO_MANY_REQUESTS, e.code(), e.message());
            }
        }
    }

    let payload = &body[4..];

    // Hook: before_request
    #[cfg(feature = "hook")]
    let mut _guards: Vec<Box<dyn crate::hook::RequestGuard>> = {
        let ctx = crate::hook::RequestContext {
            handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
            handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
            transport: "http",
            handler_id,
            state: shared.state.clone(),
        };
        shared.hooks[handler_id]
            .iter()
            .filter_map(|h| h.before_request(&ctx))
            .collect()
    };

    let result = invoker.call(&shared.state, payload).await;

    // Hook: on_response / on_error
    #[cfg(feature = "hook")]
    {
        let ctx = crate::hook::RequestContext {
            handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
            handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
            transport: "http",
            handler_id,
            state: shared.state.clone(),
        };
        match &result {
            Ok(bytes) => {
                for g in _guards.iter_mut().rev() {
                    g.on_response(&ctx, bytes);
                }
            }
            Err(e) => {
                for g in _guards.iter_mut().rev() {
                    g.on_error(&ctx, e);
                }
            }
        }
    }

    match result {
        Ok(data) => {
            let mut resp = Vec::with_capacity(1 + 8 + data.len());
            resp.push(0u8);
            resp.extend_from_slice(&0i64.to_le_bytes());
            resp.extend_from_slice(&data);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/octet-stream")
                .body(Full::new(Bytes::from(resp)))
                .expect("valid response builder"))
        }
        Err(e) => error_response(StatusCode::OK, e.code(), e.message()),
    }
}

/// Handles `GET /code/{service}/{lang}?call=...` — client code generation.
///
/// Generates client code for the named service in the requested language.
/// The `?call=` query parameter specifies which transport types to include
/// (comma-separated: `fetch,ws,nodetcp,buntcp,unirequest,uniws,wxrequest,wxws`).
/// If omitted, defaults to `fetch,ws` for TS/JS and `http,ws,tcp` for Kotlin.
#[cfg(feature = "code")]
#[allow(unreachable_code, unused_variables)]
fn handle_code(
    segments: &[&str],
    query: Option<&str>,
    shared: &SharedState,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // segments: ["code", service_name, lang]
    if segments.len() != 3 {
        return error_response(
            StatusCode::BAD_REQUEST,
            CODE_HTTP,
            "usage: /code/{service}/{lang} where lang is 'ts', 'js', 'kt', or 'rs'",
        );
    }

    let service_name = segments[1];
    let lang_str = segments[2];

    // Parse ?call= parameter into raw string list
    let raw_calls: Vec<String> = if let Some(q) = query {
        q.split('&')
            .filter_map(|part| {
                let (key, val) = part.split_once('=')?;

                if key == "call" { Some(val) } else { None }
            })
            .next()
            .map(|val| {
                val.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let result: Option<(crate::Lang, &str)> = match lang_str {
        #[cfg(feature = "ts")]
        "ts" => {
            let calls: Vec<crate::JsTsCallType> = raw_calls
                .iter()
                .filter_map(|s| crate::JsTsCallType::parse(s))
                .collect();
            let calls = if calls.is_empty() {
                vec![crate::JsTsCallType::Fetch, crate::JsTsCallType::Ws]
            } else {
                calls
            };
            Some((crate::Lang::TS(calls), "text/typescript; charset=utf-8"))
        }
        #[cfg(not(feature = "ts"))]
        "ts" => {
            return error_response(
                StatusCode::BAD_REQUEST,
                CODE_HTTP,
                "typescript code generation is not enabled (rebuild with the 'ts' feature)",
            );
        }
        #[cfg(feature = "js")]
        "js" => {
            let calls: Vec<crate::JsTsCallType> = raw_calls
                .iter()
                .filter_map(|s| crate::JsTsCallType::parse(s))
                .collect();
            let calls = if calls.is_empty() {
                vec![crate::JsTsCallType::Fetch, crate::JsTsCallType::Ws]
            } else {
                calls
            };
            Some((
                crate::Lang::JS(calls),
                "application/javascript; charset=utf-8",
            ))
        }
        #[cfg(not(feature = "js"))]
        "js" => {
            return error_response(
                StatusCode::BAD_REQUEST,
                CODE_HTTP,
                "javascript code generation is not enabled (rebuild with the 'js' feature)",
            );
        }
        #[cfg(feature = "kt")]
        "kt" => {
            let calls: Vec<crate::KtCallType> = raw_calls
                .iter()
                .filter_map(|s| crate::KtCallType::parse(s))
                .collect();
            let calls = if calls.is_empty() {
                vec![
                    crate::KtCallType::Http,
                    crate::KtCallType::Ws,
                    crate::KtCallType::Tcp,
                ]
            } else {
                calls
            };
            Some((crate::Lang::KT(calls), "text/x-kotlin; charset=utf-8"))
        }
        #[cfg(not(feature = "kt"))]
        "kt" => {
            return error_response(
                StatusCode::BAD_REQUEST,
                CODE_HTTP,
                "kotlin code generation is not enabled (rebuild with the 'kt' feature)",
            );
        }
        #[cfg(feature = "rs")]
        "rs" => {
            let calls: Vec<crate::RsCallType> = raw_calls
                .iter()
                .filter_map(|s| crate::RsCallType::parse(s))
                .collect();
            let calls = if calls.is_empty() {
                vec![crate::RsCallType::TcpAsync]
            } else {
                calls
            };
            Some((crate::Lang::RS(calls), "text/x-rust; charset=utf-8"))
        }
        #[cfg(not(feature = "rs"))]
        "rs" => {
            return error_response(
                StatusCode::BAD_REQUEST,
                CODE_HTTP,
                "rust code generation is not enabled (rebuild with the 'rs' feature)",
            );
        }
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                CODE_HTTP,
                &format!(
                    "unsupported lang '{}', expected 'ts', 'js', 'kt', or 'rs'",
                    lang_str
                ),
            );
        }
    };

    let (lang, content_type) = result.expect("lang must be set by match above");

    match crate::app::codegen::code::generate_code(&shared.services, service_name, &lang) {
        Ok(code) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", content_type)
            .body(Full::new(Bytes::from(code)))
            .expect("valid response builder")),
        Err(e) => error_response(StatusCode::NOT_FOUND, CODE_HTTP, &e.to_string()),
    }
}

/// Handles `GET /doc` and `GET /doc/{service}` — interactive documentation.
///
/// `GET /doc` returns the index page listing all services. `GET /doc/{name}`
/// returns the documentation page for a single service, including an inline
/// API tester that can send real requests to the server.
#[cfg(feature = "doc")]
fn handle_doc(
    segments: &[&str],
    shared: &SharedState,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    match segments.len() {
        1 => {
            // GET /doc → index.html
            let html = crate::app::codegen::doc::generate_index_html(
                &shared.services,
                shared.doc_title.as_deref(),
            );
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/html; charset=utf-8")
                .body(Full::new(Bytes::from(html)))
                .expect("valid response builder"))
        }
        2 => {
            let service_name = segments[1];
            match crate::app::codegen::doc::generate_service_html(
                &shared.services,
                service_name,
                shared.doc_title.as_deref(),
                &shared.http_addr,
                shared.ws_addr.as_deref(),
            ) {
                Ok(html) => Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Full::new(Bytes::from(html)))
                    .expect("valid response builder")),
                Err(e) => error_response(StatusCode::NOT_FOUND, CODE_HTTP, &e.to_string()),
            }
        }
        _ => error_response(
            StatusCode::BAD_REQUEST,
            CODE_HTTP,
            "usage: /doc or /doc/{service_name}",
        ),
    }
}

/// Handles `GET /_ws` — WebSocket upgrade endpoint in merged mode.
///
/// When WS and HTTP share the same address, this endpoint performs the
/// WebSocket handshake on the existing HTTP connection. After the 101
/// Switching Protocols response, the upgraded connection is handed off
/// to [`handle_websocket`](crate::app::transport::handle_websocket).
#[cfg(all(feature = "ws", feature = "binary"))]
async fn handle_ws_upgrade(
    req: Request<hyper::body::Incoming>,
    shared: &SharedState,
    _client_ip: &str,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::tungstenite::protocol::Role;

    let headers = req.headers();
    let ws_key = headers
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok());

    let is_ws_upgrade = headers
        .get(hyper::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_lowercase().contains("websocket"))
        .unwrap_or(false);

    if !is_ws_upgrade || ws_key.is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            CODE_HTTP,
            "websocket upgrade required",
        );
    }

    let accept_key = tokio_tungstenite::tungstenite::handshake::derive_accept_key(
        ws_key.expect("ws_key checked above").as_bytes(),
    );

    // Extract headers for WS rate-limiting before moving `req`.
    #[cfg(feature = "rate-limit")]
    let mut header_cache = std::collections::HashMap::new();
    #[cfg(feature = "rate-limit")]
    for (name, value) in req.headers().iter() {
        if let Ok(v) = value.to_str() {
            header_cache.insert(name.as_str().to_lowercase(), v.to_string());
        }
    }

    let on_upgrade = hyper::upgrade::on(req);
    let state = shared.state.clone();
    let handlers = shared.handlers.clone();
    #[cfg(feature = "hook")]
    let hooks = shared.hooks.clone();

    #[cfg(feature = "rate-limit")]
    let client_ip_owned = _client_ip.to_string();
    #[cfg(feature = "rate-limit")]
    let rate_limiter = shared.rate_limiter.clone();
    #[cfg(feature = "rate-limit")]
    let handler_names = shared.handler_names.clone();

    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => {
                let io = hyper_util::rt::TokioIo::new(upgraded);
                let ws = WebSocketStream::from_raw_socket(io, Role::Server, None).await;
                #[cfg(feature = "rate-limit")]
                let mut ctx = ConnectionContext::new(client_ip_owned);
                #[cfg(feature = "rate-limit")]
                {
                    ctx.header_cache = header_cache;
                }
                crate::app::transport::handle_websocket(
                    ws,
                    state,
                    handlers,
                    #[cfg(feature = "hook")]
                    hooks,
                    #[cfg(feature = "rate-limit")]
                    Some(ctx),
                    #[cfg(feature = "rate-limit")]
                    rate_limiter,
                    #[cfg(feature = "rate-limit")]
                    handler_names,
                )
                .await;
            }
            Err(e) => {
                eprintln!("afast: ws upgrade error: {}", e);
            }
        }
    });

    Ok(Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("upgrade", "websocket")
        .header("connection", "upgrade")
        .header("sec-websocket-accept", accept_key)
        .body(Full::new(Bytes::new()))
        .expect("valid response builder"))
}

/// Handles WebSocket upgrade for ordinary-ws routes.
///
/// This performs the HTTP→WebSocket upgrade, then creates `WsSender` and
/// `WsReceiver` and invokes the matched handler. The handler runs in a
/// spawned task so the 101 response is returned immediately.
#[cfg(feature = "ordinary-ws")]
async fn handle_ordinary_ws_upgrade(
    req: Request<hyper::body::Incoming>,
    shared: &SharedState,
    invoker: &'static dyn crate::app::ordinary_ws::WsHandlerInvoker,
    handler_name: &str,
    query_string: &str,
    path_params: std::collections::HashMap<String, String>,
    _client_ip: &str,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::tungstenite::protocol::Role;

    let headers = req.headers();
    let ws_key = headers
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok());

    let is_ws_upgrade = headers
        .get(hyper::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_lowercase().contains("websocket"))
        .unwrap_or(false);

    if !is_ws_upgrade || ws_key.is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            CODE_HTTP,
            "websocket upgrade required",
        );
    }

    let accept_key = tokio_tungstenite::tungstenite::handshake::derive_accept_key(
        ws_key.expect("ws_key checked above").as_bytes(),
    );

    let on_upgrade = hyper::upgrade::on(req);
    let state = shared.state.clone();
    let query_owned = query_string.to_string();
    let handler_name_owned = handler_name.to_string();

    // TODO: Add hook support (before_request, on_connect, on_disconnect)
    // when the lifetime issues with spawned tasks are resolved.

    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => {
                let io = hyper_util::rt::TokioIo::new(upgraded);
                let ws = WebSocketStream::from_raw_socket(io, Role::Server, None).await;

                // Split the WebSocket stream into sender/receiver.
                let (ws_tx, mut ws_rx) = ws.split();
                use futures_util::{SinkExt, StreamExt};

                // Create channels for the WsSender/WsReceiver wrapper.
                let (user_tx, mut user_rx) =
                    tokio::sync::mpsc::channel::<crate::app::ordinary_ws::WsMessage>(32);
                let (incoming_tx, incoming_rx) =
                    tokio::sync::mpsc::channel::<crate::app::ordinary_ws::WsMessage>(32);

                let ws_sender = crate::app::ordinary_ws::WsSender::new(user_tx);
                let ws_receiver = crate::app::ordinary_ws::WsReceiver::new(incoming_rx);

                // Spawn a task to forward outgoing messages from user → WebSocket.
                let mut ws_tx = ws_tx;
                let send_task = tokio::spawn(async move {
                    while let Some(msg) = user_rx.recv().await {
                        let ws_msg = match msg {
                            crate::app::ordinary_ws::WsMessage::Text(t) => {
                                tokio_tungstenite::tungstenite::Message::Text(t.into())
                            }
                            crate::app::ordinary_ws::WsMessage::Binary(b) => {
                                tokio_tungstenite::tungstenite::Message::Binary(b.into())
                            }
                            crate::app::ordinary_ws::WsMessage::Close(reason) => {
                                tokio_tungstenite::tungstenite::Message::Close(
                                    reason.map(|r| {
                                        tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                                            reason: r.into(),
                                        }
                                    }),
                                )
                            }
                            _ => continue,
                        };
                        if ws_tx.send(ws_msg).await.is_err() {
                            break;
                        }
                    }
                });

                // Spawn a task to forward incoming messages from WebSocket → user.
                let recv_task = tokio::spawn(async move {
                    while let Some(result) = ws_rx.next().await {
                        match result {
                            Ok(tokio_tungstenite::tungstenite::Message::Text(t)) => {
                                let _ = incoming_tx
                                    .send(crate::app::ordinary_ws::WsMessage::Text(t.to_string()))
                                    .await;
                            }
                            Ok(tokio_tungstenite::tungstenite::Message::Binary(b)) => {
                                let _ = incoming_tx
                                    .send(crate::app::ordinary_ws::WsMessage::Binary(b.to_vec()))
                                    .await;
                            }
                            Ok(tokio_tungstenite::tungstenite::Message::Ping(d)) => {
                                let _ = incoming_tx
                                    .send(crate::app::ordinary_ws::WsMessage::Ping(d.to_vec()))
                                    .await;
                            }
                            Ok(tokio_tungstenite::tungstenite::Message::Pong(d)) => {
                                let _ = incoming_tx
                                    .send(crate::app::ordinary_ws::WsMessage::Pong(d.to_vec()))
                                    .await;
                            }
                            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_) => {
                                let _ = incoming_tx
                                    .send(crate::app::ordinary_ws::WsMessage::Close(None))
                                    .await;
                                break;
                            }
                            _ => {}
                        }
                    }
                });

                // Call the user's handler.
                if let Err(e) = invoker
                    .call_ws(&query_owned, &path_params, ws_sender, ws_receiver, state)
                    .await
                {
                    eprintln!("afast: ws handler '{}' error: {}", handler_name_owned, e);
                }

                // Clean up forwarding tasks.
                send_task.abort();
                recv_task.abort();
            }
            Err(e) => {
                eprintln!(
                    "afast: ws upgrade error for '{}': {}",
                    handler_name_owned, e
                );
            }
        }
    });

    Ok(Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("upgrade", "websocket")
        .header("connection", "upgrade")
        .header("sec-websocket-accept", accept_key)
        .body(Full::new(Bytes::new()))
        .expect("valid response builder"))
}

/// Builds a 405 Method Not Allowed response.
fn method_not_allowed() -> Result<Response<Full<Bytes>>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Full::new(Bytes::from("method not allowed")))
        .expect("valid response builder"))
}

/// Builds a 404 Not Found response.
fn not_found() -> Result<Response<Full<Bytes>>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Full::new(Bytes::from("not found")))
        .expect("valid response builder"))
}

/// Builds a binary-format error response.
///
/// The body uses the same `[1u8][code i64][message bytes]` format as
/// WebSocket error frames, providing a consistent error representation
/// across both transports.
fn error_response(
    status: StatusCode,
    code: i64,
    message: &str,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let msg_bytes = message.as_bytes();
    let mut resp = Vec::with_capacity(1 + 8 + msg_bytes.len());
    resp.push(1u8);
    resp.extend_from_slice(&code.to_le_bytes());
    resp.extend_from_slice(msg_bytes);
    Ok(Response::builder()
        .status(status)
        .header("content-type", "application/octet-stream")
        .body(Full::new(Bytes::from(resp)))
        .expect("valid response builder"))
}

/// Loads PEM-encoded certificates from a file.
#[cfg(feature = "tls")]
fn load_certs(path: &str) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, Error> {
    let file = std::fs::File::open(path).map_err(|e| Error::Http {
        message: format!("failed to open cert file '{}': {}", path, e),
    })?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::Http {
            message: format!("failed to parse cert file: {}", e),
        })
}

/// Loads a PEM-encoded private key from a file.
#[cfg(feature = "tls")]
fn load_key(path: &str) -> Result<rustls::pki_types::PrivateKeyDer<'static>, Error> {
    let file = std::fs::File::open(path).map_err(|e| Error::Http {
        message: format!("failed to open key file '{}': {}", path, e),
    })?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| Error::Http {
            message: format!("failed to parse key file: {}", e),
        })?
        .ok_or_else(|| Error::Http {
            message: "no private key found in key file".into(),
        })
}
