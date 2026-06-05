//! WebSocket transport implementation.
//!
//! # Binary protocol wire format
//!
//! All messages are [`Message::Binary`] frames. Multi-byte integers are
//! little-endian. The sizes of `SeqId` and `Len` depend on feature flags
//! (`seq64`/`len64`).
//!
//! ## Request frame (client to server)
//!
//! ```text
//! [req_id: SeqId][handler_id: u32][len: Len][payload: bytes]
//! ```
//!
//! ## Success response frame (server to client)
//!
//! ```text
//! [req_id: SeqId][len: Len][0u8][0i64][data: bytes]
//! ```
//!
//! ## Error response frame (server to client)
//!
//! ```text
//! [req_id: SeqId][len: Len][1u8][code: i64][message: bytes]
//! ```
//!
//! ## Success response with connection ID (long-connection init)
//!
//! ```text
//! [req_id: SeqId][len: Len][0u8][0i64][conn_id: u32][data: bytes]
//! ```
//!
//! ## Push frame (server to client, long-connection data)
//!
//! ```text
//! [0: SeqId][conn_id: u32][len: Len][payload: bytes]
//! ```
//!
//! ## Push frame from client (client to server, long-connection data)
//!
//! ```text
//! [0: SeqId][conn_id: u32][len: Len][payload: bytes]
//! ```
//! A `len` of 0 signals connection close.
//!
//! ## Heartbeat frame (client to server)
//!
//! ```text
//! [-1: SeqId][len: Len][conn_id1: u32][conn_id2: u32]...
//! ```
//! The client sends the set of connection IDs it considers alive. The
//! server removes any tracked connections not in that set.

use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::accept_async_with_config;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

use crate::error::{CODE_MSG_TOO_SHORT, CODE_PAYLOAD_MISMATCH};
use crate::handler::HandlerInvoker;
#[cfg(feature = "rate-limit")]
use crate::rate_limit::{ConnectionContext, RateLimiter};
use crate::state::StateMap;

use super::util::*;

/// Tracks a single long-connection handler's inbound communication channel.
///
/// The `to_handler` sender delivers client push messages to the handler's
/// [`Receiver`](crate::Receiver) extractor.
struct ConnectionState {
    to_handler: mpsc::Sender<Vec<u8>>,
}

/// Builds a success response frame wrapped in a WS `Message`.
fn make_success(req_id: SeqId, data: &[u8]) -> Message {
    Message::Binary(make_success_raw(req_id, data).into())
}

/// Builds a success response with conn_id wrapped in a WS `Message`.
fn make_success_with_conn(req_id: SeqId, conn_id: u32, data: &[u8]) -> Message {
    Message::Binary(make_success_with_conn_raw(req_id, conn_id, data).into())
}

/// Builds an error response frame wrapped in a WS `Message`.
fn make_error(req_id: SeqId, code: i64, message: &str) -> Message {
    Message::Binary(make_error_raw(req_id, code, message).into())
}

/// Builds a push frame wrapped in a WS `Message`.
fn make_push(conn_id: u32, data: &[u8]) -> Message {
    Message::Binary(make_push_raw(conn_id, data).into())
}

/// Drives the WebSocket message loop for a single connection.
///
/// Dispatches incoming binary frames to the appropriate handler (regular
/// or long-connection) and forwards push messages from handler tasks back
/// to the client. This function is the core of the WS transport and may
/// be called either from a standalone WS server or from an HTTP upgrade.
pub async fn handle_websocket<S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    state: Arc<StateMap>,
    handlers: Arc<HashMap<u32, &'static dyn HandlerInvoker>>,
    #[cfg(feature = "hook")] hooks: Arc<HashMap<u32, Vec<std::sync::Arc<dyn crate::hook::Hook>>>>,
    #[cfg(feature = "rate-limit")] mut conn_ctx: Option<ConnectionContext>,
    #[cfg(feature = "rate-limit")] rate_limiter: Option<Arc<RateLimiter>>,
    #[cfg(feature = "rate-limit")] handler_names: HashMap<u32, String>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut source) = ws.split();
    let connections: Arc<Mutex<HashMap<u32, ConnectionState>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut next_conn_id: u32 = 1;

    // Buffered channel for push messages from spawned handler tasks.
    // A bounded channel (capacity 32) provides back-pressure to slow
    // producers without unbounded memory growth.
    let (push_tx, mut push_rx) = mpsc::channel::<(u32, Vec<u8>)>(32);

    loop {
        tokio::select! {
            msg = source.next() => {
                let msg = match msg {
                    Some(Ok(msg)) => msg,
                    Some(Err(e)) => {
                        match &e {
                            // ConnectionClosed and AlreadyClosed are normal
                            // termination paths — do not log them as errors.
                            WsError::ConnectionClosed | WsError::AlreadyClosed => {}
                            WsError::Io(io_err)
                                if io_err.kind() == std::io::ErrorKind::ConnectionReset
                                    || io_err.kind() == std::io::ErrorKind::ConnectionAborted => {}
                            _ => eprintln!("afast: ws receive error: {}", e),
                        }
                        break;
                    }
                    None => break,
                };

                match msg {
                    Message::Binary(data) => {
                        let data = data.to_vec();
                        if data.len() < SEQ_BYTES {
                            let _ = sink.send(make_error(0, CODE_MSG_TOO_SHORT, "message too short")).await;
                            continue;
                        }

                        let Some(req_id_bytes) = read_array::<SEQ_BYTES>(&data, 0) else {
                            let _ = sink.send(make_error(0, CODE_MSG_TOO_SHORT, "frame too short")).await;
                            continue;
                        };
                        let req_id = SeqId::from_le_bytes(req_id_bytes);

                        if req_id == 0 {
                            // Push message from client: [0 SeqId][conn_id u32][len Len][payload]
                            let push_hdr = SEQ_BYTES + 4 + LEN_BYTES;
                            if data.len() < push_hdr {
                                continue;
                            }
                            let Some(conn_id_bytes) = read_array::<4>(&data, SEQ_BYTES) else { continue; };
                            let conn_id = u32::from_le_bytes(conn_id_bytes);
                            let Some(len_bytes) = read_array::<LEN_BYTES>(&data, SEQ_BYTES + 4) else { continue; };
                            let len = Len::from_le_bytes(len_bytes) as usize;

                            if push_hdr + len > data.len() {
                                continue;
                            }

                            if len == 0 {
                                // An empty payload signals the client wants to close
                                // this long connection. Drop the channel entry and
                                // forward a close notification to the push queue so
                                // any pending handler can terminate.
                                let mut conns = connections.lock().await;
                                if conns.remove(&conn_id).is_some() {
                                    drop(conns);
                                    let _ = push_tx.send((conn_id, vec![])).await;
                                }
                            } else {
                                let payload = data[push_hdr..push_hdr + len].to_vec();
                                let conns = connections.lock().await;
                                if let Some(conn) = conns.get(&conn_id) {
                                    let _ = conn.to_handler.send(payload).await;
                                }
                            }
                        } else if req_id == HEARTBEAT_ID {
                            // Heartbeat: [HEARTBEAT_ID SeqId][len Len][conn_id1 u32][conn_id2 u32]...
                            //
                            // The client periodically reports which connection IDs it
                            // considers alive. The server drops any tracked connection
                            // not present in the heartbeat, preventing resource leaks
                            // from stale long connections whose handlers may have
                            // terminated without a clean close signal.
                            let hdr = SEQ_BYTES;
                            if data.len() >= hdr + LEN_BYTES {
                                let Some(len_bytes) = read_array::<LEN_BYTES>(&data, hdr) else { continue; };
                                let len = Len::from_le_bytes(len_bytes) as usize;
                                if data.len() >= hdr + LEN_BYTES + len {
                                    let mut alive = std::collections::HashSet::new();
                                    let mut off = hdr + LEN_BYTES;
                                    while off + 4 <= hdr + LEN_BYTES + len {
                                        let Some(cid_bytes) = read_array::<4>(&data, off) else { break; };
                                        let cid = u32::from_le_bytes(cid_bytes);
                                        alive.insert(cid);
                                        off += 4;
                                    }
                                    let mut conns = connections.lock().await;
                                    let stale: Vec<u32> = conns.keys().filter(|id| !alive.contains(id)).copied().collect();
                                    for id in stale {
                                        if conns.remove(&id).is_some() {
                                            let _ = push_tx.send((id, vec![])).await;
                                        }
                                    }
                                }
                            }
                        } else {
                            // Regular request: [req_id SeqId][handler_id u32][len Len][payload]
                            let req_hdr = SEQ_BYTES + 4 + LEN_BYTES;
                            if data.len() < req_hdr {
                                let _ = sink.send(make_error(req_id, CODE_MSG_TOO_SHORT, "message too short")).await;
                                continue;
                            }

                            let Some(handler_id_bytes) = read_array::<4>(&data, SEQ_BYTES) else {
                                let _ = sink.send(make_error(req_id, CODE_MSG_TOO_SHORT, "frame too short")).await;
                                continue;
                            };
                            let handler_id = u32::from_le_bytes(handler_id_bytes);
                            let Some(len_bytes) = read_array::<LEN_BYTES>(&data, SEQ_BYTES + 4) else {
                                let _ = sink.send(make_error(req_id, CODE_MSG_TOO_SHORT, "frame too short")).await;
                                continue;
                            };
                            let len = Len::from_le_bytes(len_bytes) as usize;

                            if req_hdr + len > data.len() {
                                let _ = sink.send(make_error(req_id, CODE_PAYLOAD_MISMATCH, "payload length mismatch")).await;
                                continue;
                            }

                            let payload = &data[req_hdr..req_hdr + len];

                            let invoker = match handlers.get(&handler_id) {
                                Some(invoker) => *invoker,
                                None => {
                                    let _ = sink.send(make_error(req_id, 102, &format!("handler not found (id={})", handler_id))).await;
                                    continue;
                                }
                            };

                            // Rate-limit check.
                            #[cfg(feature = "rate-limit")]
                            if let (Some(limiter), Some(ctx)) = (&rate_limiter, &mut conn_ctx) {
                                let handler_name = handler_names.get(&handler_id)
                                    .map(|s| s.as_str())
                                    .unwrap_or("");
                                if !handler_name.is_empty()
                                    && let Err(e) = limiter.check(handler_name, ctx).await
                                {
                                    let _ = sink.send(make_error(req_id, e.code(), e.message())).await;
                                    continue;
                                }
                            }

                            if invoker.is_long_connection() {
                                // Long-connection handler: create a bidirectional
                                // channel pair, assign a connection ID, and spawn
                                // the handler. The initial response includes the
                                // conn_id so the client can address subsequent
                                // push messages to this connection.
                                let (to_handler_tx, to_handler_rx) = mpsc::channel::<Vec<u8>>(32);
                                let (from_handler_tx, mut from_handler_rx) = mpsc::channel::<Vec<u8>>(32);

                                let conn_id = next_conn_id;
                                next_conn_id += 1;

                                {
                                    let mut conns = connections.lock().await;
                                    conns.insert(conn_id, ConnectionState { to_handler: to_handler_tx });
                                }

                                let resp = make_success_with_conn(req_id, conn_id, &[]);
                                let _ = sink.send(resp).await;

                                let state = state.clone();
                                let payload = payload.to_vec();
                                let push_tx = push_tx.clone();
                                let connections = connections.clone();
                                #[cfg(feature = "hook")]
                                let hooks = hooks.clone();

                                // Hook: on_connect
                                #[cfg(feature = "hook")]
                                let mut _conn_guards: Vec<Box<dyn crate::hook::ConnectionGuard>> = {
                                    let ctx = crate::hook::RequestContext {
                                        handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                        handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                        transport: "ws",
                                        handler_id,
                                        state: state.clone(),
                                    };
                                    hooks.get(&handler_id).map(|v| v.as_slice()).unwrap_or(&[]).iter().filter_map(|h| h.on_connect(&ctx)).collect()
                                };

                                tokio::spawn(async move {
                                    // Hook: before_request for call_stream
                                    #[cfg(feature = "hook")]
                                    let mut _guards: Vec<Box<dyn crate::hook::RequestGuard>> = {
                                        let ctx = crate::hook::RequestContext {
                                            handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                            handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                            transport: "ws",
                                            handler_id,
                                            state: state.clone(),
                                        };
                                        hooks.get(&handler_id).map(|v| v.as_slice()).unwrap_or(&[]).iter().filter_map(|h| h.before_request(&ctx)).collect()
                                    };

                                    let result = invoker.call_stream(&state, &payload, from_handler_tx, to_handler_rx).await;

                                    // Hook: on_response / on_error for call_stream
                                    #[cfg(feature = "hook")]
                                    {
                                        let ctx = crate::hook::RequestContext {
                                            handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                            handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                            transport: "ws",
                                            handler_id,
                                            state: state.clone(),
                                        };
                                        match &result {
                                            Ok(bytes) => for g in _guards.iter_mut().rev() { g.on_response(&ctx, bytes); },
                                            Err(e) => for g in _guards.iter_mut().rev() { g.on_error(&ctx, e); },
                                        }
                                    }

                                    if let Err(ref e) = result {
                                        eprintln!("afast: stream handler error: {}", e);
                                    }

                                    // Drain handler output into the push channel so it
                                    // arrives as a push frame on the WebSocket.
                                    if result.is_ok() {
                                        while let Some(bytes) = from_handler_rx.recv().await {
                                            let _ = push_tx.send((conn_id, bytes)).await;
                                        }
                                    }

                                    // Hook: on_disconnect
                                    #[cfg(feature = "hook")]
                                    {
                                        let ctx = crate::hook::RequestContext {
                                            handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                            handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                            transport: "ws",
                                            handler_id,
                                            state: state.clone(),
                                        };
                                        for g in &mut _conn_guards { g.on_disconnect(&ctx); }
                                    }

                                    // Remove the connection when the handler
                                    // completes (normal exit or error).
                                    {
                                        let mut conns = connections.lock().await;
                                        conns.remove(&conn_id);
                                    }
                                });
                            } else {
                                // Regular handler: invoke synchronously within the
                                // connection loop and write the response directly.

                                // Hook: before_request
                                #[cfg(feature = "hook")]
                                let mut _guards: Vec<Box<dyn crate::hook::RequestGuard>> = {
                                    let ctx = crate::hook::RequestContext {
                                        handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                        handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                        transport: "ws",
                                        handler_id,
                                        state: state.clone(),
                                    };
                                    hooks.get(&handler_id).map(|v| v.as_slice()).unwrap_or(&[]).iter().filter_map(|h| h.before_request(&ctx)).collect()
                                };

                                let result = invoker.call(&state, payload).await;

                                // Hook: on_response / on_error
                                #[cfg(feature = "hook")]
                                {
                                    let ctx = crate::hook::RequestContext {
                                        handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                        handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                        transport: "ws",
                                        handler_id,
                                        state: state.clone(),
                                    };
                                    match &result {
                                        Ok(bytes) => for g in _guards.iter_mut().rev() { g.on_response(&ctx, bytes); },
                                        Err(e) => for g in _guards.iter_mut().rev() { g.on_error(&ctx, e); },
                                    }
                                }

                                let msg = match result {
                                    Ok(bytes) => make_success(req_id, &bytes),
                                    Err(e) => make_error(req_id, e.code(), e.message()),
                                };
                                let _ = sink.send(msg).await;
                            }
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            // Forward push messages from handler tasks to the WebSocket sink.
            // This runs concurrently with source processing so that long-running
            // handlers can push data without blocking request handling.
            push = push_rx.recv() => {
                if let Some((conn_id, data)) = push {
                    let msg = make_push(conn_id, &data);
                    if sink.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

/// Accepts an incoming TCP connection and performs the WebSocket handshake.
///
/// On successful upgrade, delegates to [`handle_websocket`] for the
/// remainder of the connection lifetime.
pub async fn handle_connection(
    stream: TcpStream,
    state: Arc<StateMap>,
    handlers: Arc<HashMap<u32, &'static dyn HandlerInvoker>>,
    #[cfg(feature = "hook")] hooks: Arc<HashMap<u32, Vec<std::sync::Arc<dyn crate::hook::Hook>>>>,
    #[cfg(feature = "rate-limit")] rate_limiter: Option<Arc<RateLimiter>>,
    #[cfg(feature = "rate-limit")] handler_names: HashMap<u32, String>,
    body_size_limit: usize,
) {
    let peer_ip = stream
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let mut ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    ws_config.max_message_size = Some(body_size_limit);
    ws_config.max_frame_size = Some(body_size_limit);
    let ws = match accept_async_with_config(stream, Some(ws_config)).await {
        Ok(ws) => ws,
        Err(_) => return,
    };

    #[cfg(feature = "rate-limit")]
    let conn_ctx = Some(ConnectionContext::new(peer_ip));
    #[cfg(not(feature = "rate-limit"))]
    let _ = peer_ip;

    handle_websocket(
        ws,
        state,
        handlers,
        #[cfg(feature = "hook")]
        hooks,
        #[cfg(feature = "rate-limit")]
        conn_ctx,
        #[cfg(feature = "rate-limit")]
        rate_limiter,
        #[cfg(feature = "rate-limit")]
        handler_names,
    )
    .await;
}
