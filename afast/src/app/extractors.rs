//! Shared HTTP parameter extractors for ordinary routes.
//!
//! These extractors are used by all ordinary transport types (HTTP, WS, SSE):
//! - `Query<T>` — URL query parameter extractor (`?key=value`)
//! - `Param<T>` — path parameter extractor (`:id`, `:name`)

use std::collections::HashMap;

use crate::app::ordinary::{from_value_lenient, parse_query_to_json, path_params_to_json};
use crate::error::Error;

// ─── Query extractor ──────────────────────────────────────────────

/// Extracts URL query parameters into `T`.
///
/// `T` must implement `serde::de::DeserializeOwned`. The query string is parsed
/// as `key=value&key=value` and deserialized with lenient coercion
/// (e.g. string-to-int conversion).
pub struct Query<T>(pub T);

impl<T: serde::de::DeserializeOwned> Query<T> {
    /// Parses a query string into a `Query<T>`.
    pub fn from_query(query: &str) -> Result<Self, Error> {
        let json = parse_query_to_json(query);
        from_value_lenient(json)
            .map(Query)
            .map_err(|e| Error::InvalidParam {
                code: 400,
                message: format!("query parse error: {e}"),
            })
    }
}

// ─── Param extractor ──────────────────────────────────────────────

/// Extracts route path parameters (`:id`, `:name`) into `T`.
///
/// `T` must implement `serde::de::DeserializeOwned`. Path parameters are
/// collected into a JSON object and deserialized with lenient coercion.
pub struct Param<T>(pub T);

impl<T: serde::de::DeserializeOwned> Param<T> {
    /// Parses path parameters into a `Param<T>`.
    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, Error> {
        let json = path_params_to_json(params);
        from_value_lenient(json)
            .map(Param)
            .map_err(|e| Error::InvalidParam {
                code: 400,
                message: format!("path param parse error: {e}"),
            })
    }
}
