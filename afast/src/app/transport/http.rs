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
#[cfg(feature = "ordinary-http")]
use crate::app::ordinary::RoutePattern;
use crate::error::{CODE_HTTP, CODE_LONG_CONNECTION_NOT_SUPPORTED, Error};
use crate::handler::HandlerInvoker;
#[cfg(feature = "ordinary-http")]
use crate::service::OrdinaryRouteInfo;

/// Starts the HTTP server and blocks until a shutdown signal is received.
///
/// This function binds a TCP listener, compiles ordinary HTTP routes
/// (if enabled), and enters an accept loop that spawns a task per
/// connection. Each connection is auto-negotiated (HTTP/1.1 or HTTP/2)
/// with upgrade support (for merged WebSocket mode). When TLS is
/// enabled via the `tls` feature, connections are wrapped with TLS
/// and support ALPN negotiation for HTTP/2.
pub async fn serve(
    addr: SocketAddr,
    state: Arc<StateMap>,
    handlers: Arc<Vec<Option<&'static dyn HandlerInvoker>>>,
    #[cfg(any(feature = "code", feature = "doc"))] services: Vec<Service>,
    mut shutdown_rx: broadcast::Receiver<()>,
    #[cfg(feature = "doc")] doc_title: Option<String>,
    #[cfg(feature = "doc")] ws_addr_str: Option<String>,
    #[cfg(feature = "ordinary-http")] ordinary_routes: Vec<OrdinaryRouteInfo>,
    #[cfg(feature = "ws")] enable_ws_upgrade: bool,
    #[cfg(feature = "tls")] tls_config: Option<crate::app::TlsConfig>,
) -> Result<(), Error> {
    let listener = TcpListener::bind(addr).await.map_err(|e| Error::Http {
        message: e.to_string(),
    })?;

    println!("afast: http server listening on {}", addr);

    // Compile ordinary route patterns once at startup to avoid
    // re-parsing on every request.
    #[cfg(feature = "ordinary-http")]
    let compiled_routes: Vec<CompiledOrdinaryRoute> = ordinary_routes
        .iter()
        .map(|r| CompiledOrdinaryRoute {
            method: r.method,
            pattern: RoutePattern::parse(&r.path),
            invoker: r.handler_entry.ordinary_invoker.unwrap(),
        })
        .collect();

    let shared = Arc::new(SharedState {
        state,
        handlers,
        #[cfg(any(feature = "code", feature = "doc"))]
        services,
        #[cfg(feature = "doc")]
        doc_title,
        #[cfg(feature = "doc")]
        http_addr: addr.to_string(),
        #[cfg(feature = "doc")]
        ws_addr: ws_addr_str,
        #[cfg(feature = "ordinary-http")]
        ordinary_routes: compiled_routes,
        #[cfg(feature = "ws")]
        enable_ws_upgrade,
    });

    // Set up TLS acceptor if TLS config is provided.
    #[cfg(feature = "tls")]
    let tls_acceptor = if let Some(ref cfg) = tls_config {
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
                            #[cfg(feature = "tls")]
                            match tls {
                                Some(ref acceptor) => {
                                    match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            let io = hyper_util::rt::TokioIo::new(tls_stream);
                                            serve_connection(io, shared).await;
                                        }
                                        Err(e) => {
                                            eprintln!("afast: tls handshake error: {}", e);
                                        }
                                    }
                                }
                                None => {
                                    let io = hyper_util::rt::TokioIo::new(stream);
                                    serve_connection(io, shared).await;
                                }
                            };
                            #[cfg(not(feature = "tls"))]
                            {
                                let io = hyper_util::rt::TokioIo::new(stream);
                                serve_connection(io, shared).await;
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
async fn serve_connection<S>(io: hyper_util::rt::TokioIo<S>, shared: Arc<SharedState>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req| {
        let shared = shared.clone();
        async move { handle_request(req, &shared).await }
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
    #[cfg(feature = "ws")]
    enable_ws_upgrade: bool,
}

/// A pre-compiled ordinary HTTP route ready for request matching.
#[cfg(feature = "ordinary-http")]
struct CompiledOrdinaryRoute {
    method: &'static str,
    pattern: RoutePattern,
    invoker: &'static dyn crate::handler::OrdinaryHandlerInvoker,
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
                        let status = if code >= 400 && code < 600 {
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
                            .unwrap())
                    }
                };
            }
        }
    }

    match segments.first().copied() {
        Some("_api") => {
            if method != hyper::Method::POST {
                return method_not_allowed();
            }
            handle_api(req, shared).await
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
        #[cfg(feature = "ws")]
        Some("_ws") => {
            if shared.enable_ws_upgrade {
                return handle_ws_upgrade(req, shared).await;
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
async fn handle_api(
    req: Request<hyper::body::Incoming>,
    shared: &SharedState,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    use http_body_util::BodyExt;
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

    let payload = &body[4..];
    let result = invoker.call(&shared.state, payload).await;

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
                .unwrap())
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
            "usage: /code/{service}/{lang} where lang is 'ts', 'js', or 'kt'",
        );
    }

    let service_name = segments[1];
    let lang_str = segments[2];

    // Parse ?call= parameter into raw string list
    let raw_calls: Vec<String> = if let Some(q) = query {
        q.split('&')
            .filter_map(|part| {
                let mut kv = part.splitn(2, '=');
                let key = kv.next()?;
                let val = kv.next()?;
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

    let (lang, content_type) = match lang_str {
        "ts" => {
            let calls: Vec<crate::JsTsCallType> = raw_calls
                .iter()
                .filter_map(|s| crate::JsTsCallType::from_str(s))
                .collect();
            let calls = if calls.is_empty() {
                vec![crate::JsTsCallType::Fetch, crate::JsTsCallType::Ws]
            } else {
                calls
            };
            (crate::Lang::TS(calls), "text/typescript; charset=utf-8")
        }
        "js" => {
            let calls: Vec<crate::JsTsCallType> = raw_calls
                .iter()
                .filter_map(|s| crate::JsTsCallType::from_str(s))
                .collect();
            let calls = if calls.is_empty() {
                vec![crate::JsTsCallType::Fetch, crate::JsTsCallType::Ws]
            } else {
                calls
            };
            (
                crate::Lang::JS(calls),
                "application/javascript; charset=utf-8",
            )
        }
        #[cfg(feature = "kt")]
        "kt" => {
            let calls: Vec<crate::KtCallType> = raw_calls
                .iter()
                .filter_map(|s| crate::KtCallType::from_str(s))
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
            (crate::Lang::KT(calls), "text/x-kotlin; charset=utf-8")
        }
        #[cfg(not(feature = "kt"))]
        "kt" => {
            return error_response(
                StatusCode::BAD_REQUEST,
                CODE_HTTP,
                "kotlin code generation is not enabled (rebuild with the 'kt' feature)",
            );
        }
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                CODE_HTTP,
                &format!(
                    "unsupported lang '{}', expected 'ts', 'js', or 'kt'",
                    lang_str
                ),
            );
        }
    };

    match crate::app::codegen::code::generate_code(&shared.services, service_name, &lang) {
        Ok(code) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", content_type)
            .body(Full::new(Bytes::from(code)))
            .unwrap()),
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
                .unwrap())
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
                    .unwrap()),
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
#[cfg(feature = "ws")]
async fn handle_ws_upgrade(
    req: Request<hyper::body::Incoming>,
    shared: &SharedState,
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

    let accept_key =
        tokio_tungstenite::tungstenite::handshake::derive_accept_key(ws_key.unwrap().as_bytes());

    let on_upgrade = hyper::upgrade::on(req);
    let state = shared.state.clone();
    let handlers = shared.handlers.clone();

    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => {
                let io = hyper_util::rt::TokioIo::new(upgraded);
                let ws = WebSocketStream::from_raw_socket(io, Role::Server, None).await;
                crate::app::transport::handle_websocket(ws, state, handlers).await;
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
        .unwrap())
}

/// Builds a 405 Method Not Allowed response.
fn method_not_allowed() -> Result<Response<Full<Bytes>>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Full::new(Bytes::from("method not allowed")))
        .unwrap())
}

/// Builds a 404 Not Found response.
fn not_found() -> Result<Response<Full<Bytes>>, hyper::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Full::new(Bytes::from("not found")))
        .unwrap())
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
        .unwrap())
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
