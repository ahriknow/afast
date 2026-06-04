#![doc(html_favicon_url = "https://raw.githubusercontent.com/ahriknow/afast/develop/favicon.svg")]
#![doc(html_logo_url = "https://raw.githubusercontent.com/ahriknow/afast/develop/favicon.svg")]

//! # afast
//!
//! A high-performance Rust web framework that eliminates manual route definitions.
//! Handlers are registered declaratively and the framework generates TypeScript and
//! JavaScript client code automatically. Data transport uses a compact binary protocol
//! over HTTP, WebSocket, or TCP.
//!
//! ## Quick Start
//!
//! ```no_run
//! use afast::{AFast, handler, service, State, Data, Result};
//! use afast::{AFastDeserialize, AFastSerialize, Tag};
//!
//! #[derive(Clone)]
//! struct AppState { db_url: String }
//!
//! #[derive(AFastDeserialize, Tag)]
//! #[tag("Request body")]
//! struct HelloReq { name: String }
//!
//! #[derive(AFastSerialize, Tag)]
//! #[tag("Response body")]
//! struct HelloResp { message: String }
//!
//! #[handler(desc("Say hello"))]
//! async fn hello(
//!     state: State<AppState>,
//!     req: Data<HelloReq>,
//! ) -> Result<HelloResp> {
//!     Ok(HelloResp { message: format!("Hello, {}!", req.name) })
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let svc = service!("api" => { h(hello) });
//!     let app = AFast::new()
//!         .state(AppState { db_url: "localhost".into() })
//!         .service(svc)
//!         .http("0.0.0.0:5000");
//!     app.run().await.unwrap();
//! }
//! ```
//!
//! ## Features
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `ws` | WebSocket server via `tokio-tungstenite` |
//! | `http` | HTTP server via `hyper` |
//! | `tcp` | TCP server with length-prefix framing |
//! | `ts` | TypeScript client code generation |
//! | `js` | JavaScript client code generation |
//! | `kt` | Kotlin client code generation |
//! | `code` | HTTP endpoint for on-demand client code generation |
//! | `doc` | Interactive API documentation page |
//! | `ordinary-http` | REST-style endpoints with JSON request/response bodies |
//! | `tls` | HTTPS support via rustls with ALPN for HTTP/2 |
//! | `seq64` | Use `i64` for WebSocket request IDs (default `i32`) |
//! | `len64` | Use `u64` for WebSocket payload lengths (default `u32`) |
//! | `rate-limit` | Request rate limiting with named policies and pluggable store |

pub mod error;
pub mod handler;
#[cfg(feature = "hook")]
pub mod hook;
pub mod marker;
#[cfg(feature = "rate-limit")]
pub mod rate_limit;
pub mod service;
pub mod state;

pub use error::Error;
#[cfg(feature = "ordinary-http")]
pub use handler::OrdinaryHandlerInvoker;
pub use handler::{DummyInvoker, Handler};
pub use handler::{
    EnumVariantMeta, FieldMeta, Structure, TagKind, TagMeta, ValidateRule, no_structure,
};
pub use handler::{HandlerEntry, HandlerInvoker, HandlerMeta, ParamMeta};
#[cfg(feature = "binary")]
pub use handler::{Receiver, Sender};
pub use service::Service;
pub use state::StateMap;

#[cfg(feature = "rate-limit")]
pub use rate_limit::{
    Algorithm, InMemoryStore, RateLimitConfig, RateLimitKey, RateLimitPolicy, RateLimitStore,
};

pub use afastdata::{AFastDeserialize, AFastSerialize};

/// Injects shared application state into a handler.
///
/// The value is retrieved from the [`StateMap`] by type. It must have been
/// registered previously via [`AFast::state`](crate::AFast::state).
///
/// # Examples
///
/// ```no_run
/// #[handler(desc("Uses shared state"))]
/// async fn my_handler(
///     state: afast::State<AppState>,
/// ) -> afast::Result<()> {
///     let db_url = &state.db_url;
///     Ok(())
/// }
/// ```
pub struct State<T>(pub T);

/// Deserializes the binary request payload into `T`.
///
/// `T` must implement [`AFastDeserialize`] and [`Structure`] (via `#[derive(Tag)]`)
/// so that code generators can emit the corresponding type definitions.
///
/// Only available with the `binary` feature.
///
/// # Examples
///
/// ```no_run
/// #[derive(AFastDeserialize, Tag)]
/// #[tag("Request body")]
/// struct MyReq { name: String }
///
/// #[handler(desc("Accepts binary payload"))]
/// async fn my_handler(
///     req: afast::Data<MyReq>,
/// ) -> afast::Result<()> {
///     Ok(())
/// }
/// ```
#[cfg(feature = "binary")]
pub struct Data<T>(pub T);

/// Deserializes custom authentication or context data into `T`.
///
/// Custom data is supplied by the client at construction time (e.g. auth tokens,
/// tenant identifiers). Unlike [`Data`], which comes from the binary payload,
/// `Custom` values are set once per client instance.
///
/// `T` must implement [`Structure`] via `#[derive(Tag)]` so the code generators
/// can produce the corresponding type definitions.
///
/// # Examples
///
/// ```no_run
/// #[derive(AFastDeserialize, Tag)]
/// #[tag("Credentials")]
/// struct Auth { token: i64 }
///
/// #[handler(desc("Requires authentication"))]
/// async fn protected_handler(
///     auth: afast::Custom<Auth>,
/// ) -> afast::Result<()> {
///     Ok(())
/// }
/// ```
pub struct Custom<T>(pub T);

impl<T: Structure> Custom<T> {
    /// Returns the [`TagMeta`] for `T`, used by code generators.
    pub fn structure() -> &'static TagMeta {
        T::structure()
    }
}

#[cfg(feature = "binary")]
impl<T: Structure> Data<T> {
    /// Returns the [`TagMeta`] for `T`, used by code generators.
    pub fn structure() -> &'static TagMeta {
        T::structure()
    }
}

// ─── Ordinary HTTP extractors ─────────────────────────────────────

#[cfg(feature = "ordinary-http")]
pub use serde;
#[cfg(feature = "ordinary-http")]
pub use serde_json;

/// Extracts URL query parameters into `T`.
///
/// `T` must implement `serde::de::DeserializeOwned`. The query string is parsed
/// as `key=value&key=value` and deserialized with lenient coercion
/// (e.g. string-to-int conversion).
///
/// Use [`Query::from_query`] to parse from a raw query string.
#[cfg(any(
    feature = "ordinary-http",
    feature = "ordinary-ws",
    feature = "ordinary-sse"
))]
pub use crate::app::extractors::Query;

/// Extracts route path parameters (`:id`, `:name`) into `T`.
///
/// `T` must implement `serde::de::DeserializeOwned`. Path parameters are
/// collected into a JSON object and deserialized with lenient coercion.
///
/// Use [`Param::from_params`] to parse from a path parameter map.
#[cfg(any(
    feature = "ordinary-http",
    feature = "ordinary-ws",
    feature = "ordinary-sse"
))]
pub use crate::app::extractors::Param;

/// Extracts the JSON request body into `T`.
///
/// `T` must implement `serde::de::DeserializeOwned`. The body bytes are read
/// and deserialized with `serde_json::from_slice`.
#[cfg(feature = "ordinary-http")]
pub struct Body<T>(pub T);

/// Extracts HTTP request headers into `T`.
///
/// Field names are converted from snake_case to Header-Case:
/// `authorization` becomes `Authorization`, `content_type` becomes `Content-Type`.
///
/// Headers listed in [`is_standard_header`] are sent automatically by the
/// browser and require no user callback. Non-standard headers require a
/// callback function in the generated client.
#[cfg(feature = "ordinary-http")]
pub struct Header<T>(pub T);

/// Returns `true` if the given header name is in the standard set.
///
/// Standard headers are sent automatically by browsers and HTTP clients,
/// so the generated code does not require a user callback for them.
pub fn is_standard_header(name: &str) -> bool {
    matches!(
        name,
        "content-type"
            | "content-length"
            | "accept"
            | "accept-encoding"
            | "accept-language"
            | "user-agent"
            | "host"
            | "connection"
            | "cache-control"
            | "pragma"
            | "origin"
            | "referer"
            | "cookie"
            | "etag"
            | "if-none-match"
            | "if-modified-since"
            | "last-modified"
            | "server"
            | "date"
            | "vary"
            | "allow"
            | "location"
            | "content-disposition"
            | "content-encoding"
            | "transfer-encoding"
            | "upgrade"
            | "via"
            | "warning"
            | "dnt"
            | "sec-fetch-dest"
            | "sec-fetch-mode"
            | "sec-fetch-site"
            | "sec-fetch-user"
    )
}

#[cfg(feature = "ordinary-http")]
pub use crate::app::ordinary::{
    fill_standard_header_defaults, header_name_to_field, read_body_bytes, req_headers_to_json,
};
#[cfg(any(
    feature = "ordinary-http",
    feature = "ordinary-ws",
    feature = "ordinary-sse"
))]
pub use crate::app::ordinary::{from_value_lenient, parse_query_to_json, path_params_to_json};

// ─── Ordinary HTTP response types ─────────────────────────────────

/// JSON response. Sets status `200 OK` and `Content-Type: application/json`.
#[cfg(feature = "ordinary-http")]
pub struct Json<T>(pub T);

/// Plain text response. Sets `Content-Type: text/plain`.
#[cfg(feature = "ordinary-http")]
pub struct Text(pub String);

/// HTML response. Sets `Content-Type: text/html`.
#[cfg(feature = "ordinary-http")]
pub struct Html(pub String);

/// File download response. Sets `Content-Disposition: attachment`.
#[cfg(feature = "ordinary-http")]
pub struct File {
    pub data: Vec<u8>,
    pub filename: String,
    pub content_type: String,
}

/// Status-only response with an empty body.
#[cfg(feature = "ordinary-http")]
pub struct Status(pub hyper::StatusCode);

/// Redirect response. Sets `302 Found` with a `Location` header.
#[cfg(feature = "ordinary-http")]
pub struct Redirect(pub String);

/// Type alias for ordinary HTTP handler return types.
#[cfg(feature = "ordinary-http")]
pub type HttpResult<T> = Result<T>;

// ─── IntoResponse trait ───────────────────────────────────────────

/// Converts a response type into a [`hyper::Response`].
///
/// Implemented for [`Json`], [`Text`], [`Html`], [`File`], [`Status`],
/// [`Redirect`], and [`Result`].
#[cfg(feature = "ordinary-http")]
pub trait IntoResponse {
    fn into_response(self) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>>;
}

#[cfg(feature = "ordinary-http")]
impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
        use http_body_util::Full;
        use hyper::body::Bytes;
        match serde_json::to_string(&self.0) {
            Ok(json) => hyper::Response::builder()
                .header("content-type", "application/json; charset=utf-8")
                .body(Full::new(Bytes::from(json)))
                .unwrap(),
            Err(e) => hyper::Response::builder()
                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json; charset=utf-8")
                .body(Full::new(Bytes::from(format!(
                    "{{\"code\":-1,\"message\":\"JSON error: {}\"}}",
                    e
                ))))
                .unwrap(),
        }
    }
}

#[cfg(feature = "ordinary-http")]
impl IntoResponse for Text {
    fn into_response(self) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
        use http_body_util::Full;
        use hyper::body::Bytes;
        hyper::Response::builder()
            .header("content-type", "text/plain; charset=utf-8")
            .body(Full::new(Bytes::from(self.0)))
            .unwrap()
    }
}

#[cfg(feature = "ordinary-http")]
impl IntoResponse for Html {
    fn into_response(self) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
        use http_body_util::Full;
        use hyper::body::Bytes;
        hyper::Response::builder()
            .header("content-type", "text/html; charset=utf-8")
            .body(Full::new(Bytes::from(self.0)))
            .unwrap()
    }
}

#[cfg(feature = "ordinary-http")]
impl IntoResponse for File {
    fn into_response(self) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
        use http_body_util::Full;
        use hyper::body::Bytes;
        let filename = self.filename.replace('"', "\\\"");
        hyper::Response::builder()
            .header("content-type", self.content_type)
            .header(
                "content-disposition",
                format!("attachment; filename=\"{}\"", filename),
            )
            .body(Full::new(Bytes::from(self.data)))
            .unwrap()
    }
}

#[cfg(feature = "ordinary-http")]
impl IntoResponse for Status {
    fn into_response(self) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
        use http_body_util::Full;
        use hyper::body::Bytes;
        hyper::Response::builder()
            .status(self.0)
            .body(Full::new(Bytes::new()))
            .unwrap()
    }
}

#[cfg(feature = "ordinary-http")]
impl IntoResponse for Redirect {
    fn into_response(self) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
        use http_body_util::Full;
        use hyper::body::Bytes;
        hyper::Response::builder()
            .status(hyper::StatusCode::FOUND)
            .header("location", self.0)
            .body(Full::new(Bytes::new()))
            .unwrap()
    }
}

#[cfg(feature = "ordinary-http")]
impl<T: IntoResponse> IntoResponse for Result<T> {
    fn into_response(self) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
        use http_body_util::Full;
        use hyper::body::Bytes;
        match self {
            Ok(val) => val.into_response(),
            Err(e) => {
                let code = e.code();
                let message = e.message();
                let status = if (400..600).contains(&code) {
                    hyper::StatusCode::from_u16(code as u16)
                        .unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                } else {
                    hyper::StatusCode::INTERNAL_SERVER_ERROR
                };
                let body = format!("{{\"code\":{},\"message\":\"{}\"}}", code, message);
                hyper::Response::builder()
                    .status(status)
                    .header("content-type", "application/json; charset=utf-8")
                    .body(Full::new(Bytes::from(body)))
                    .unwrap()
            }
        }
    }
}

#[cfg(feature = "ordinary-sse")]
pub use afast_macros::register_sse;
#[cfg(feature = "ordinary-ws")]
pub use afast_macros::register_ws;
#[cfg(feature = "ordinary-sse")]
pub use afast_macros::sse;
#[cfg(feature = "ordinary-ws")]
pub use afast_macros::ws;
pub use afast_macros::{Tag, handler, register, register_ordinary};
#[cfg(feature = "ordinary-http")]
pub use afast_macros::{delete, get, patch, post, put};

/// Builds a [`Service`] with handlers and nested groups.
///
/// Each service contains binary handlers registered with `h(name)` and
/// optional nested `group("prefix" => { ... })` scopes. Ordinary HTTP
/// routes use `get(path, fn)`, `post(path, fn)`, etc.
///
/// # Syntax
///
/// ```text
/// service!("api", "API description" => {
///     h(health),
///     group("user" => {
///         h(list_users),
///         h(get_user),
///         get(":id", get_user_by_id),
///     }),
/// })
/// ```
#[macro_export]
macro_rules! service {
    // Empty service
    ($name:expr) => {
        $crate::Service::new($name)
    };

    // Service with description
    ($name:expr, $desc:expr) => {
        $crate::Service::new($name).desc($desc)
    };

    // Service with items
    ($name:expr => { $($item:tt)* }) => {
        $crate::service!(@svc $crate::Service::new($name), $($item)*)
    };

    // Service with description and items
    ($name:expr, $desc:expr => { $($item:tt)* }) => {
        $crate::service!(@svc $crate::Service::new($name).desc($desc), $($item)*)
    };

    // ── Service-level accumulation ─────────────────────────────

    (@svc $svc:expr) => { $svc };
    (@svc $svc:expr,) => { $svc };

    (@svc $svc:expr, h($($handler:tt)+) $($rest:tt)*) => {
        $crate::service!(@svc
            $svc.handler($crate::Handler::from_entry(register!($($handler)+)))
            $($rest)*
        )
    };

    (@svc $svc:expr, group($group_name:expr => { $($inner:tt)* }) $($rest:tt)*) => {
        $crate::service!(@svc
            $svc.handler($crate::service!(@group $group_name, $($inner)*))
            $($rest)*
        )
    };

    // Ordinary HTTP routes at service level
    (@svc $svc:expr, get($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@svc
            $svc.ordinary_route("GET", $path, $crate::register_ordinary!($($fn)+))
            $($rest)*
        )
    };
    (@svc $svc:expr, post($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@svc
            $svc.ordinary_route("POST", $path, $crate::register_ordinary!($($fn)+))
            $($rest)*
        )
    };
    (@svc $svc:expr, put($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@svc
            $svc.ordinary_route("PUT", $path, $crate::register_ordinary!($($fn)+))
            $($rest)*
        )
    };
    (@svc $svc:expr, patch($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@svc
            $svc.ordinary_route("PATCH", $path, $crate::register_ordinary!($($fn)+))
            $($rest)*
        )
    };
    (@svc $svc:expr, delete($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@svc
            $svc.ordinary_route("DELETE", $path, $crate::register_ordinary!($($fn)+))
            $($rest)*
        )
    };
    (@svc $svc:expr, ws($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@svc
            {
                let (__ws_invoker, __ws_name) = $crate::register_ws!($($fn)+);
                $svc.ws_route($path, __ws_invoker, __ws_name)
            }
            $($rest)*
        )
    };
    (@svc $svc:expr, sse($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@svc
            {
                let (__sse_invoker, __sse_name) = $crate::register_sse!($($fn)+);
                $svc.sse_route($path, __sse_invoker, __sse_name)
            }
            $($rest)*
        )
    };

    // ── Group building ────────────────────────────────────────

    (@group $name:expr, $($item:tt)*) => {
        $crate::service!(@group_chain $crate::Handler::group($name), $($item)*)
    };

    (@group_chain $grp:expr) => { $grp };
    (@group_chain $grp:expr,) => { $grp };

    (@group_chain $grp:expr, h($($handler:tt)+) $($rest:tt)*) => {
        $crate::service!(@group_chain
            $grp.handler($crate::Handler::from_entry(register!($($handler)+)))
            $($rest)*
        )
    };

    (@group_chain $grp:expr, group($group_name:expr => { $($inner:tt)* }) $($rest:tt)*) => {
        $crate::service!(@group_chain
            $grp.handler($crate::service!(@group $group_name, $($inner)*))
            $($rest)*
        )
    };

    // Ordinary HTTP routes inside a group
    (@group_chain $grp:expr, get($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@group_chain
            $grp.handler($crate::Handler::ordinary_leaf($path, "GET", $crate::register_ordinary!($($fn)+)))
            $($rest)*
        )
    };
    (@group_chain $grp:expr, post($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@group_chain
            $grp.handler($crate::Handler::ordinary_leaf($path, "POST", $crate::register_ordinary!($($fn)+)))
            $($rest)*
        )
    };
    (@group_chain $grp:expr, put($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@group_chain
            $grp.handler($crate::Handler::ordinary_leaf($path, "PUT", $crate::register_ordinary!($($fn)+)))
            $($rest)*
        )
    };
    (@group_chain $grp:expr, patch($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@group_chain
            $grp.handler($crate::Handler::ordinary_leaf($path, "PATCH", $crate::register_ordinary!($($fn)+)))
            $($rest)*
        )
    };
    (@group_chain $grp:expr, delete($path:expr, $($fn:tt)+) $($rest:tt)*) => {
        $crate::service!(@group_chain
            $grp.handler($crate::Handler::ordinary_leaf($path, "DELETE", $crate::register_ordinary!($($fn)+)))
            $($rest)*
        )
    };
}

/// A `Result` type alias using [`Error`] as the error type.
pub type Result<T> = std::result::Result<T, Error>;

pub mod app;

pub use app::AFast;
#[cfg(feature = "doc")]
pub use app::DocConfig;
#[cfg(feature = "kt")]
pub use app::KtCallType;
#[cfg(feature = "rs")]
pub use app::RsCallType;
#[cfg(feature = "tls")]
pub use app::TlsConfig;
#[cfg(any(
    feature = "ordinary-http",
    feature = "ordinary-ws",
    feature = "ordinary-sse"
))]
pub use app::ordinary;
#[cfg(feature = "ordinary-sse")]
pub use app::ordinary_sse::{SseEvent, SseSender};
#[cfg(feature = "ordinary-ws")]
pub use app::ordinary_ws;
#[cfg(feature = "ordinary-ws")]
pub use app::ordinary_ws::{WsMessage, WsParam, WsQuery, WsReceiver, WsSender};
pub use app::{GenerateTarget, JsTsCallType, Lang};
