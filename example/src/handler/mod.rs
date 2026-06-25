//! Handler module — contains all example handler functions.
//!
//! Each sub-module demonstrates a different handler pattern:
//! - `admin` — Binary protocol + ordinary HTTP REST routes with auth
//! - `auth` — Registration, login, token management
//! - `article` — CRUD operations with caching
//! - `chat` — Bidirectional long-connection streaming
//! - `sse_stream` — Server-Sent Events
//! - `ws_chat` — WebSocket chat with path parameters

pub mod admin;
pub mod article;
pub mod auth;
pub mod chat;
pub mod sse_stream;
pub mod ws_chat;

use afast::{AFastDeserialize, AFastSerialize, Tag, get, handler, post};
use serde::Serialize;

// ─── Health Check Handler ────────────────────────────────────────
//
// This is the simplest handler pattern: no parameters, no state,
// just returns a response. Used for liveness probes.

/// Health check response — returns the server version.
#[derive(AFastSerialize, Tag)]
#[tag("Health check result")]
pub struct TokenResponse {
    #[tag("Version")]
    version: String,
}

/// Health check endpoint.
///
/// This handler demonstrates the `Ctx<T>` extractor for per-request context.
/// The `RequestInfo` is injected by `CtxHook` before the handler runs.
/// It's registered in the "check" service.
#[handler(desc("Health check"))]
pub async fn health(ctx: afast::Ctx<crate::RequestInfo>) -> afast::Result<TokenResponse> {
    eprintln!(
        "[health] request_id={}, elapsed={:?}",
        ctx.0.request_id,
        ctx.0.started_at.elapsed(),
    );
    Ok(TokenResponse {
        version: "0.0.0".to_string(),
    })
}

// ─── Multiple Data Extractors ────────────────────────────────────
//
// A handler can accept multiple Data<T> parameters. The binary payload
// is split in declaration order — the first Data<T> reads its bytes,
// then the second reads the remaining bytes, and so on.
//
// This is useful for combining multiple request parts without creating
// a wrapper struct.

/// First part of the info request.
#[derive(AFastDeserialize, Tag)]
#[tag("First data")]
pub struct FirstData {
    id: i64,
}

/// Second part of the info request.
#[derive(AFastDeserialize, Tag)]
#[tag("Second data")]
pub struct SecondData {
    name: String,
}

/// Info response.
#[derive(AFastSerialize, Tag)]
#[tag("Info response")]
pub struct InfoResponst {
    message: String,
}

/// System info endpoint — demonstrates multiple `Data<T>` extractors.
///
/// The binary payload is split into two parts:
/// 1. FirstData (id: i64)
/// 2. SecondData (name: String)
///
/// This is equivalent to having a single struct with both fields,
/// but allows more flexible composition.
#[handler(desc("System info"))]
pub async fn info(
    afast::Data(first): afast::Data<FirstData>, // Reads first 8 bytes (i64)
    afast::Data(second): afast::Data<SecondData>, // Reads remaining bytes (String)
) -> afast::Result<InfoResponst> {
    println!("first => {}, second => {}", first.id, second.name);
    Ok(InfoResponst {
        message: "message".to_string(),
    })
}

// ─── Ordinary HTTP Handler ───────────────────────────────────────
//
// This demonstrates the #[get] macro for REST-style HTTP endpoints.
// Unlike binary protocol handlers (#[handler]), ordinary HTTP handlers
// use JSON request/response bodies and standard HTTP methods.
//
// For serde::Serialize types, you don't need AFastSerialize — just
// derive Serialize and the framework serializes to JSON automatically.

/// Pong response — simple JSON response for the ping endpoint.
#[derive(Serialize, Tag)]
#[tag("Pong response")]
pub struct PongResponse {
    #[tag("Whether the server is alive")]
    pong: bool,
}

/// Simple ping endpoint (ordinary HTTP GET).
///
/// This handler demonstrates `Ctx<T>` with ordinary HTTP routes.
/// It returns `Json<T>` which serializes to JSON automatically.
///
/// Access via: GET http://localhost:5001/ping
/// Response: {"pong": true}
#[get(desc("Simple ping endpoint"))]
pub async fn ping(
    ctx: afast::Ctx<crate::RequestInfo>,
) -> afast::HttpResult<afast::Json<PongResponse>> {
    eprintln!(
        "[ping] request_id={}, elapsed={:?}",
        ctx.0.request_id,
        ctx.0.started_at.elapsed(),
    );
    Ok(afast::Json(PongResponse { pong: true }))
}

// ─── Catch-all Route Handler ─────────────────────────────────────
//
// Demonstrates the `*` catch-all syntax that captures all requests
// not matched by other routes. Uses `FullPath` extractor to get
// the full request path and `Param<HashMap>` to get the captured
// remaining path segments.

/// Catch-all response — echoes the captured path information.
#[derive(Serialize, Tag)]
#[tag("Catch-all response")]
pub struct CatchAllResponse {
    #[tag("The full request path")]
    pub full_path: String,
    #[tag("The captured catch-all segments")]
    pub caught: String,
    #[tag("Message indicating this is the catch-all handler")]
    pub message: String,
}

/// Catch-all handler — matches any GET request not matched by other routes.
///
/// This demonstrates:
/// - `FullPath` extractor: gets the full request path (e.g., `/unknown/path`)
/// - Catch-all routes have the lowest priority: exact > param > catch-all
///
/// Access via: GET http://localhost:5001/any/unknown/path
/// Response: {"full_path":"/any/unknown/path","caught":"any/unknown/path","message":"catch-all"}
#[get(desc("Catch-all handler"))]
pub async fn catch_all_get(
    path: afast::FullPath,
) -> afast::HttpResult<afast::Json<CatchAllResponse>> {
    let p = path.0.clone();
    Ok(afast::Json(CatchAllResponse {
        full_path: p.clone(),
        caught: p.trim_start_matches('/').to_string(),
        message: "catch-all".to_string(),
    }))
}

// ─── TLS Reload Handler ──────────────────────────────────────────

/// TLS reload response.
#[derive(Serialize, Tag)]
#[tag("TLS reload result")]
pub struct TlsReloadResponse {
    #[tag("Whether the reload signal was sent successfully")]
    pub success: bool,
    #[tag("Status message")]
    pub message: String,
}

/// Reload TLS certificates at runtime via GET request.
///
/// Access via: GET http://localhost:5443/tls/reload
#[cfg(feature = "tls")]
#[get(desc("Reload TLS certificates"))]
pub async fn reload_tls(
    afast::State(state): afast::State<crate::state::AppState>,
) -> afast::HttpResult<afast::Json<TlsReloadResponse>> {
    match state.reload_tx.send(None) {
        Ok(_) => Ok(afast::Json(TlsReloadResponse {
            success: true,
            message: "TLS reload signal sent".to_string(),
        })),
        Err(_) => Ok(afast::Json(TlsReloadResponse {
            success: false,
            message: "no receivers (TLS server not running)".to_string(),
        })),
    }
}

// ─── File Upload Handler ─────────────────────────────────────────

/// Upload response.
#[derive(Serialize, Tag)]
#[tag("File upload result")]
pub struct UploadResponse {
    #[tag("Uploaded file name")]
    pub filename: String,
    #[tag("Field name")]
    pub field_name: String,
    #[tag("Content type")]
    pub content_type: String,
    #[tag("File size in bytes")]
    pub size: usize,
}

/// Upload a file via multipart/form-data (raw multer access).
///
/// Access via: POST http://localhost:5001/upload
/// Content-Type: multipart/form-data
///
/// # Example (curl)
///
/// ```bash
/// curl -F "file=@test.txt" http://localhost:5001/upload
/// ```
#[post(desc("Upload file (raw)"))]
pub async fn upload_file(
    mut form: afast::Multipart,
) -> afast::HttpResult<afast::Json<UploadResponse>> {
    if let Some(field) = form.next_field().await.map_err(|e| afast::Error::Custom {
        code: 400,
        message: format!("multipart error: {}", e),
    })? {
        let field_name = field.name().unwrap_or("unknown").to_string();
        let filename = field.file_name().unwrap_or("unknown").to_string();
        let content_type = field
            .content_type()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let data = field.bytes().await.map_err(|e| afast::Error::Custom {
            code: 400,
            message: format!("read field error: {}", e),
        })?;
        return Ok(afast::Json(UploadResponse {
            filename,
            field_name,
            content_type,
            size: data.len(),
        }));
    }
    Err(afast::Error::Custom {
        code: 400,
        message: "no file provided".to_string(),
    })
}

/// Form data for typed file upload.
#[derive(afast::FromFormData, Tag)]
#[tag("Upload form data")]
pub struct UploadForm {
    #[tag("Number field")]
    pub id: i64,
    #[tag("Boolean field")]
    pub validate: bool,
    #[tag("Description text field")]
    pub description: String,
    #[tag("The uploaded file")]
    pub file: afast::FileField,
}

/// Upload a file with typed form data extraction via `#[derive(FromFormData)]`.
///
/// Demonstrates the `MultipartForm<T>` extractor with `#[derive(FromFormData)]`
/// macro. Fields are automatically extracted from the multipart stream by name.
/// `String` fields are parsed as text, `FileField` fields collect file bytes.
///
/// Access via: POST http://localhost:5001/upload/typed
/// Content-Type: multipart/form-data
///
/// # Example (curl)
///
/// ```bash
/// curl -F "description=My file" -F "file=@test.txt" http://localhost:5001/upload/typed
/// ```
#[post(desc("Upload file (typed)"))]
pub async fn upload_file_typed(
    form: afast::MultipartForm<UploadForm>,
) -> afast::HttpResult<afast::Json<UploadResponse>> {
    let data = form.0;
    println!(
        "upload_file_typed: {} {} {}",
        data.id, data.validate, data.description
    );
    Ok(afast::Json(UploadResponse {
        filename: data.file.filename.unwrap_or_else(|| "unknown".to_string()),
        field_name: data.file.name,
        content_type: data
            .file
            .content_type
            .unwrap_or_else(|| "application/octet-stream".to_string()),
        size: data.file.bytes.len(),
    }))
}
