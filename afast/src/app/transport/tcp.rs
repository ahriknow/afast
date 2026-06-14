//! TCP transport implementation.
//!
//! # Binary protocol wire format
//!
//! Every TCP frame is length-prefixed: a `Len`-byte little-endian length
//! followed by that many bytes of payload. The sizes of `SeqId` and `Len`
//! depend on feature flags (`seq64`/`len64`).
//!
//! ## Request frame
//!
//! ```text
//! [req_id: SeqId][handler_id: u32][len: Len][payload: bytes]
//! ```
//!
//! ## Success response frame
//!
//! ```text
//! [req_id: SeqId][len: Len][0u8][0i64][data: bytes]
//! ```
//!
//! ## Error response frame
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
//! ## Push frame (bidirectional, long-connection data)
//!
//! ```text
//! [0: SeqId][conn_id: u32][len: Len][payload: bytes]
//! ```
//! A `len` of 0 signals connection close.
//!
//! ## Heartbeat frame
//!
//! ```text
//! [-1: SeqId][len: Len][conn_id1: u32][conn_id2: u32]...
//! ```
//! The client periodically reports which connection IDs it considers alive.
//! The server drops any tracked connection not included in the set.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};

use crate::error::{CODE_MSG_TOO_SHORT, CODE_PAYLOAD_MISMATCH};
use crate::handler::HandlerInvoker;
#[cfg(feature = "rate-limit")]
use crate::rate_limit::{ConnectionContext, RateLimiter};
use crate::state::StateMap;

use super::util::*;

/// Tracks a single long-connection handler's inbound communication channel.
struct ConnectionState {
    to_handler: mpsc::Sender<Vec<u8>>,
}

/// Builds a success response for a regular handler.
fn make_success(req_id: SeqId, data: &[u8]) -> Vec<u8> {
    make_success_raw(req_id, data)
}

/// Builds a success response with conn_id for long-connection init.
fn make_success_with_conn(req_id: SeqId, conn_id: u32, data: &[u8]) -> Vec<u8> {
    make_success_with_conn_raw(req_id, conn_id, data)
}

/// Builds an error response frame.
fn make_error(req_id: SeqId, code: i64, message: &str) -> Vec<u8> {
    make_error_raw(req_id, code, message)
}

/// Builds a push frame for forwarding handler output to the client.
fn make_push(conn_id: u32, data: &[u8]) -> Vec<u8> {
    make_push_raw(conn_id, data)
}

/// Reads a length-prefixed frame from the TCP stream.
///
/// Returns `None` if the stream has ended (EOF) or if a recoverable
/// connection error occurs. A zero-length payload is valid and returns
/// `Some(Vec::new())`.
async fn read_frame(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    max_frame_size: usize,
) -> Option<Vec<u8>> {
    let mut len_buf = [0u8; std::mem::size_of::<Len>()];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
        Err(e)
            if e.kind() == std::io::ErrorKind::ConnectionReset
                || e.kind() == std::io::ErrorKind::ConnectionAborted =>
        {
            return None;
        }
        Err(e) => {
            eprintln!("afast: tcp read length error: {}", e);
            return None;
        }
    }
    let len = Len::from_le_bytes(len_buf) as usize;
    if len == 0 {
        return Some(Vec::new());
    }
    // Reject oversized frames BEFORE allocating memory.
    if len > max_frame_size {
        eprintln!(
            "afast: tcp frame too large ({} bytes, limit: {})",
            len, max_frame_size
        );
        return None;
    }
    let mut data = vec![0u8; len];
    match reader.read_exact(&mut data).await {
        Ok(_) => Some(data),
        Err(e) => {
            eprintln!("afast: tcp read data error: {}", e);
            None
        }
    }
}

/// Writes a length-prefixed frame to the TCP stream.
///
/// Returns `false` if the write fails, signaling the caller to close the
/// connection.
async fn write_frame(writer: &mut tokio::net::tcp::OwnedWriteHalf, data: &[u8]) -> bool {
    let len = data.len() as Len;
    if writer.write_all(&len.to_le_bytes()).await.is_err() {
        return false;
    }
    if writer.write_all(data).await.is_err() {
        return false;
    }
    true
}

/// Handles an accepted TCP connection for the duration of its lifetime.
///
/// Reads length-prefixed frames, dispatches requests to handlers, manages
/// long-connection state, and forwards push messages. The connection is
/// split into independent read and write halves to support concurrent
/// push delivery while reading requests.
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

    #[cfg(feature = "rate-limit")]
    let mut conn_ctx = ConnectionContext::new(peer_ip.clone());
    let client_ip = peer_ip;
    #[cfg(not(feature = "rate-limit"))]
    let client_ip = peer_ip;

    let (mut reader, mut writer) = stream.into_split();
    let connections: Arc<Mutex<HashMap<u32, ConnectionState>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut next_conn_id: u32 = 1;

    // Buffered channel for push messages from spawned handler tasks.
    let (push_tx, mut push_rx) = mpsc::channel::<(u32, Vec<u8>)>(32);

    loop {
        tokio::select! {
            frame = read_frame(&mut reader, body_size_limit) => {
                let data = match frame {
                    Some(d) => d,
                    None => break,
                };

                if data.is_empty() {
                    continue;
                }

                if data.len() < SEQ_BYTES {
                    let resp = make_error(0, CODE_MSG_TOO_SHORT, "message too short");
                    let _ = write_frame(&mut writer, &resp).await;
                    continue;
                }

                let Some(req_id_bytes) = read_array::<SEQ_BYTES>(&data, 0) else {
                    let resp = make_error(0, CODE_MSG_TOO_SHORT, "frame too short");
                    let _ = write_frame(&mut writer, &resp).await;
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
                        // An empty payload signals that the client wants to close
                        // this long connection. Remove the channel entry and send
                        // a close notification through the push queue.
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
                    // The client periodically sends the set of connection IDs it
                    // considers alive. The server drops any tracked connection
                    // not in that set, preventing resource leaks from stale
                    // long connections.
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
                        let resp = make_error(req_id, CODE_MSG_TOO_SHORT, "message too short");
                        let _ = write_frame(&mut writer, &resp).await;
                        continue;
                    }

                    let Some(handler_id_bytes) = read_array::<4>(&data, SEQ_BYTES) else {
                        let resp = make_error(req_id, CODE_MSG_TOO_SHORT, "frame too short");
                        let _ = write_frame(&mut writer, &resp).await;
                        continue;
                    };
                    let handler_id = u32::from_le_bytes(handler_id_bytes);
                    let Some(len_bytes) = read_array::<LEN_BYTES>(&data, SEQ_BYTES + 4) else {
                        let resp = make_error(req_id, CODE_MSG_TOO_SHORT, "frame too short");
                        let _ = write_frame(&mut writer, &resp).await;
                        continue;
                    };
                    let len = Len::from_le_bytes(len_bytes) as usize;

                    if req_hdr + len > data.len() {
                        let resp = make_error(req_id, CODE_PAYLOAD_MISMATCH, "payload length mismatch");
                        let _ = write_frame(&mut writer, &resp).await;
                        continue;
                    }

                    let payload = &data[req_hdr..req_hdr + len];

                    let invoker = match handlers.get(&handler_id) {
                        Some(invoker) => *invoker,
                        None => {
                            let resp = make_error(req_id, 102, &format!("handler not found (id={})", handler_id));
                            let _ = write_frame(&mut writer, &resp).await;
                            continue;
                        }
                    };

                    // Rate-limit check.
                    #[cfg(feature = "rate-limit")]
                    if let Some(ref limiter) = rate_limiter {
                        let handler_name = handler_names.get(&handler_id)
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        if !handler_name.is_empty()
                            && let Err(e) = limiter.check(handler_name, &mut conn_ctx).await
                        {
                            let resp = make_error(req_id, e.code(), e.message());
                            let _ = write_frame(&mut writer, &resp).await;
                            continue;
                        }
                    }

                    if invoker.is_long_connection() {
                        // Long-connection handler: create channels, assign a
                        // connection ID, and spawn the handler task. The initial
                        // response includes the conn_id for addressing.
                        let (to_handler_tx, to_handler_rx) = mpsc::channel::<Vec<u8>>(32);
                        let (from_handler_tx, mut from_handler_rx) = mpsc::channel::<Vec<u8>>(32);

                        let conn_id = next_conn_id;
                        next_conn_id += 1;

                        {
                            let mut conns = connections.lock().await;
                            conns.insert(conn_id, ConnectionState { to_handler: to_handler_tx });
                        }

                        let resp = make_success_with_conn(req_id, conn_id, &[]);
                        let _ = write_frame(&mut writer, &resp).await;

                        let state = state.clone();
                        let payload = payload.to_vec();
                        let push_tx = push_tx.clone();
                        let connections = connections.clone();
                        #[cfg(feature = "hook")]
                        let hooks = hooks.clone();
                        let req_ctx = crate::ctx::RequestCtx::new();

                        // Hook: on_connect
                        #[cfg(feature = "hook")]
                        let mut _conn_guards: Vec<Box<dyn crate::hook::ConnectionGuard>> = {
                            let ctx = crate::hook::RequestContext {
                                handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                transport: "tcp",
                                is_binary: true,
                                method: "",
                                long_connection: invoker.is_long_connection(),
                                handler_id,
                                state: state.clone(),
                                ctx: req_ctx.clone(),
                                attrs: invoker.meta().map(|m| m.attrs).unwrap_or(&[]),
                                client_ip: client_ip.clone(),
                                forwarded_for: None,
                            };
                            hooks.get(&handler_id).map(|v| v.as_slice()).unwrap_or(&[]).iter().filter_map(|h| h.on_connect(&ctx)).collect()
                        };

                        let client_ip_for_spawn = client_ip.clone();
                        tokio::spawn(async move {
                            let result = invoker.call_stream(&state, &req_ctx, &payload, from_handler_tx, to_handler_rx).await;

                            if let Err(ref e) = result {
                                eprintln!("afast: stream handler error: {}", e);
                            }

                            // Drain handler output into the push channel so it
                            // is written as a push frame on the TCP stream.
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
                                    transport: "tcp",
                                    is_binary: true,
                                    method: "",
                                    long_connection: invoker.is_long_connection(),
                                    handler_id,
                                    state: state.clone(),
                                    ctx: req_ctx.clone(),
                                    attrs: invoker.meta().map(|m| m.attrs).unwrap_or(&[]),
                                    client_ip: client_ip_for_spawn.clone(),
                                    forwarded_for: None,
                                };
                                for g in &mut _conn_guards { g.on_disconnect(&ctx); }
                            }

                            // Clean up the connection entry when the handler ends.
                            {
                                let mut conns = connections.lock().await;
                                conns.remove(&conn_id);
                            }
                        });
                    } else {
                        // Regular handler: invoke and write the response
                        // directly to the TCP stream.
                        let req_ctx = crate::ctx::RequestCtx::new();

                        // Hook: before_request
                        #[cfg(feature = "hook")]
                        let mut _guards: Vec<Box<dyn crate::hook::RequestGuard>> = {
                            let ctx = crate::hook::RequestContext {
                                handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                transport: "tcp",
                                is_binary: true,
                                method: "",
                                long_connection: invoker.is_long_connection(),
                                handler_id,
                                state: state.clone(),
                                ctx: req_ctx.clone(),
                                attrs: invoker.meta().map(|m| m.attrs).unwrap_or(&[]),
                                client_ip: client_ip.clone(),
                                forwarded_for: None,
                            };
                            hooks.get(&handler_id).map(|v| v.as_slice()).unwrap_or(&[]).iter().filter_map(|h| h.before_request(&ctx)).collect()
                        };

                        let result = invoker.call(&state, &req_ctx, payload).await;

                        // Hook: on_response / on_error
                        #[cfg(feature = "hook")]
                        {
                            let ctx = crate::hook::RequestContext {
                                handler_name: invoker.meta().map(|m| m.name).unwrap_or("unknown"),
                                handler_desc: invoker.meta().map(|m| m.desc).unwrap_or(""),
                                transport: "tcp",
                                is_binary: true,
                                method: "",
                                long_connection: invoker.is_long_connection(),
                                handler_id,
                                state: state.clone(),
                                ctx: req_ctx.clone(),
                                attrs: invoker.meta().map(|m| m.attrs).unwrap_or(&[]),
                                client_ip: client_ip.clone(),
                                forwarded_for: None,
                            };
                            match &result {
                                Ok(bytes) => for g in _guards.iter_mut().rev() { g.on_response(&ctx, bytes); },
                                Err(e) => for g in _guards.iter_mut().rev() { g.on_error(&ctx, e); },
                            }
                        }

                        let resp = match result {
                            Ok(bytes) => make_success(req_id, &bytes),
                            Err(e) => make_error(req_id, e.code(), e.message()),
                        };
                        let _ = write_frame(&mut writer, &resp).await;
                    }
                }
            }
            // Forward push messages from handler tasks to the TCP stream.
            // This select branch runs concurrently with frame reading so
            // that long-running handlers can push data as it is produced.
            push = push_rx.recv() => {
                if let Some((conn_id, data)) = push {
                    let msg = make_push(conn_id, &data);
                    if !write_frame(&mut writer, &msg).await {
                        break;
                    }
                }
            }
        }
    }
}
