//! Shared wire-format helpers for WS and TCP binary transports.
//!
//! These functions build raw byte frames for the afast binary protocol.
//! Each transport wraps the result in its own message type:
//! - WS: `Message::Binary(buf.into())`
//! - TCP: raw `Vec<u8>` written to the stream

#[cfg(feature = "seq64")]
pub(crate) type SeqId = i64;
#[cfg(not(feature = "seq64"))]
pub(crate) type SeqId = i32;

#[cfg(feature = "len64")]
pub(crate) type Len = u64;
#[cfg(not(feature = "len64"))]
pub(crate) type Len = u32;

/// Byte size of the sequence-id field in wire frames.
pub(crate) const SEQ_BYTES: usize = std::mem::size_of::<SeqId>();
/// Byte size of the length field in wire frames.
pub(crate) const LEN_BYTES: usize = std::mem::size_of::<Len>();
/// Reserved sequence ID that identifies heartbeat frames.
pub(crate) const HEARTBEAT_ID: SeqId = -1;

/// Safely reads N bytes from `data` at `offset` and converts to `[u8; N]`.
/// Returns `None` if the slice is too short.
#[inline]
pub(crate) fn read_array<const N: usize>(data: &[u8], offset: usize) -> Option<[u8; N]> {
    data.get(offset..offset + N)?.try_into().ok()
}

/// Builds a success response frame for a regular (non-long-connection)
/// handler result.
///
/// Wire format: `[req_id: SeqId][len: Len][0u8][0i64][data]`
pub(crate) fn make_success_raw(req_id: SeqId, data: &[u8]) -> Vec<u8> {
    let len: Len = (1 + 8 + data.len()) as Len;
    let mut buf = Vec::with_capacity(SEQ_BYTES + LEN_BYTES + 1 + 8 + data.len());
    buf.extend_from_slice(&req_id.to_le_bytes());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.push(0);
    buf.extend_from_slice(&0i64.to_le_bytes());
    buf.extend_from_slice(data);
    buf
}

/// Builds a success response frame that includes a connection ID,
/// used for the initial response during long-connection handshake.
///
/// Wire format: `[req_id: SeqId][len: Len][0u8][0i64][conn_id: u32][data]`
pub(crate) fn make_success_with_conn_raw(req_id: SeqId, conn_id: u32, data: &[u8]) -> Vec<u8> {
    let payload_len = 4 + data.len();
    let len: Len = (1 + 8 + payload_len) as Len;
    let mut buf = Vec::with_capacity(SEQ_BYTES + LEN_BYTES + 1 + 8 + payload_len);
    buf.extend_from_slice(&req_id.to_le_bytes());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.push(0);
    buf.extend_from_slice(&0i64.to_le_bytes());
    buf.extend_from_slice(&conn_id.to_le_bytes());
    buf.extend_from_slice(data);
    buf
}

/// Builds an error response frame.
///
/// Wire format: `[req_id: SeqId][len: Len][1u8][code: i64][message]`
pub(crate) fn make_error_raw(req_id: SeqId, code: i64, message: &str) -> Vec<u8> {
    let msg_bytes = message.as_bytes();
    let len: Len = (1 + 8 + msg_bytes.len()) as Len;
    let mut buf = Vec::with_capacity(SEQ_BYTES + LEN_BYTES + 1 + 8 + msg_bytes.len());
    buf.extend_from_slice(&req_id.to_le_bytes());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.push(1);
    buf.extend_from_slice(&code.to_le_bytes());
    buf.extend_from_slice(msg_bytes);
    buf
}

/// Builds a push frame for forwarding long-connection handler output to
/// the client.
///
/// Wire format: `[0: SeqId][conn_id: u32][len: Len][payload]`
pub(crate) fn make_push_raw(conn_id: u32, data: &[u8]) -> Vec<u8> {
    let zero: SeqId = 0;
    let len: Len = data.len() as Len;
    let mut buf = Vec::with_capacity(SEQ_BYTES + 4 + LEN_BYTES + data.len());
    buf.extend_from_slice(&zero.to_le_bytes());
    buf.extend_from_slice(&conn_id.to_le_bytes());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(data);
    buf
}
