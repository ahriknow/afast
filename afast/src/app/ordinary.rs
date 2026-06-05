//! Route pattern matching, query/path parsing and lenient deserialization
//! shared by `ordinary-http` and `ordinary-ws` features.
//!
//! This module provides:
//! - Path pattern compilation and matching (exact and parameterized).
//! - Query-string and path-parameter parsing into `serde_json::Value`.
//! - A lenient JSON deserializer that coerces string values into numbers and
//!   booleans, so that query/path string parameters can populate typed struct fields.
//! - (`ordinary-http`) Header name normalization and JSON conversion.
//! - (`ordinary-http`) Request body reading.

use std::collections::HashMap;

// ─── Body Size Limit ─────────────────────────────────────────────

/// Global body size limit (bytes) set once at startup.
/// Read by [`read_body_bytes`] to enforce request body size limits
/// during streaming, preventing OOM from oversized payloads.
#[cfg(feature = "ordinary-http")]
static BODY_SIZE_LIMIT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(10 * 1024 * 1024); // 10 MB default

/// Sets the global body size limit. Called once at application startup.
#[cfg(feature = "ordinary-http")]
pub fn set_body_size_limit(limit: usize) {
    BODY_SIZE_LIMIT.store(limit, std::sync::atomic::Ordering::Relaxed);
}

// ─── Route Pattern Matching ────────────────────────────────────────

/// A single segment within a compiled route pattern.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) enum RouteSegment {
    /// A literal string segment that must match exactly.
    Static(String),
    /// A named parameter segment (e.g. `":id"` matches any value).
    Param(&'static str),
}

/// A compiled route pattern for matching request paths.
///
/// Constructed from a pattern string like `"/users/:id/posts/:post_id"`,
/// this is used by the HTTP server to dispatch ordinary (REST-style)
/// requests to the correct handler.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) enum RoutePattern {
    /// A literal path with no parameters. Match is a simple string comparison.
    Exact(String),
    /// A path containing colon-prefixed parameter segments. Matching
    /// produces a map of parameter names to captured values.
    Parametric { segments: Vec<RouteSegment> },
}

#[allow(dead_code)]
impl RoutePattern {
    /// Compiles a pattern string into a [`RoutePattern`].
    ///
    /// Segments beginning with `:` become [`RouteSegment::Param`]; all
    /// others become [`RouteSegment::Static`]. Trailing and leading
    /// slashes are stripped before compilation.
    pub fn parse(pattern: &str) -> Self {
        let pattern = pattern.trim_start_matches('/').trim_end_matches('/');

        if pattern.is_empty() {
            return RoutePattern::Exact(String::new());
        }

        let segments: Vec<&str> = pattern.split('/').collect();
        let has_params = segments.iter().any(|s| s.starts_with(':'));

        if !has_params {
            return RoutePattern::Exact(pattern.to_string());
        }

        // Parameter names are leaked into static memory to avoid lifetime
        // management on the pattern, which lives for the duration of the process.
        let route_segments: Vec<RouteSegment> = segments
            .iter()
            .map(|s| {
                if let Some(param_name) = s.strip_prefix(':') {
                    RouteSegment::Param(Box::leak(param_name.to_string().into_boxed_str()))
                } else {
                    RouteSegment::Static(s.to_string())
                }
            })
            .collect();

        RoutePattern::Parametric {
            segments: route_segments,
        }
    }

    /// Attempts to match a request path against this pattern.
    ///
    /// Returns `Some(HashMap)` with captured parameter values if the path
    /// matches, or `None` if it does not. Both the pattern and the path
    /// have leading and trailing slashes stripped before comparison.
    pub fn matches(&self, path: &str) -> Option<HashMap<String, String>> {
        let path = path.trim_start_matches('/').trim_end_matches('/');

        match self {
            RoutePattern::Exact(exact) => {
                if exact == path {
                    Some(HashMap::new())
                } else {
                    None
                }
            }
            RoutePattern::Parametric { segments } => {
                let path_segments: Vec<&str> = path.split('/').collect();

                if path_segments.len() != segments.len() {
                    return None;
                }

                let mut params = HashMap::new();

                for (route_seg, path_seg) in segments.iter().zip(path_segments.iter()) {
                    match route_seg {
                        RouteSegment::Static(s) => {
                            if s != path_seg {
                                return None;
                            }
                        }
                        RouteSegment::Param(name) => {
                            params.insert(name.to_string(), path_seg.to_string());
                        }
                    }
                }

                Some(params)
            }
        }
    }
}

// ─── Trie Router ─────────────────────────────────────────────────

/// A trie-based router for O(path_depth) route matching.
///
/// Routes are organized into a tree where each node corresponds to a
/// path segment. Static segments branch via a `HashMap`, while
/// parameter segments (`:id`) use a single wildcard child.
///
/// This replaces the previous O(n) linear scan over all routes.
#[cfg(feature = "ordinary-http")]
pub(crate) struct TrieRouter {
    /// Per-method trie roots: `"GET"`, `"POST"`, `"PUT"`, `"DELETE"`, `"PATCH"`.
    roots: std::collections::HashMap<&'static str, TrieNode>,
}

#[cfg(feature = "ordinary-http")]
struct TrieNode {
    /// Static segment children: `"users"` → child node.
    static_children: std::collections::HashMap<String, TrieNode>,
    /// Parameter segment child (`:id`, `:name`, etc.).
    /// Only one param child is allowed per node.
    param_child: Option<Box<ParamChild>>,
    /// The route stored at this node (if it's a leaf).
    route: Option<usize>,
}

#[cfg(feature = "ordinary-http")]
impl TrieNode {
    fn new() -> Self {
        Self {
            static_children: std::collections::HashMap::new(),
            param_child: None,
            route: None,
        }
    }
}

#[cfg(feature = "ordinary-http")]
struct ParamChild {
    /// The parameter name (e.g. `"id"`).
    name: &'static str,
    /// The child node for the segment after the parameter.
    node: TrieNode,
}

#[cfg(feature = "ordinary-http")]
impl TrieRouter {
    /// Creates an empty trie router.
    pub fn new() -> Self {
        Self {
            roots: std::collections::HashMap::new(),
        }
    }

    /// Inserts a route into the trie.
    ///
    /// `method` — HTTP method (e.g. `"GET"`).
    /// `pattern` — the compiled route pattern.
    /// `index` — index into the external route storage (e.g. `Vec<CompiledOrdinaryRoute>`).
    pub fn insert(&mut self, method: &'static str, pattern: &RoutePattern, index: usize) {
        let root = self.roots.entry(method).or_insert_with(TrieNode::new);
        match pattern {
            RoutePattern::Exact(path) => {
                // Insert exact path as a chain of static segment nodes.
                let segments: Vec<&str> = if path.is_empty() {
                    vec![""]
                } else {
                    path.split('/').collect()
                };
                let mut node = root;
                for seg in segments {
                    node = node
                        .static_children
                        .entry(seg.to_string())
                        .or_insert_with(TrieNode::new);
                }
                node.route = Some(index);
            }
            RoutePattern::Parametric { segments } => {
                let mut node = root;
                for seg in segments {
                    match seg {
                        RouteSegment::Static(s) => {
                            node = node
                                .static_children
                                .entry(s.clone())
                                .or_insert_with(TrieNode::new);
                        }
                        RouteSegment::Param(name) => {
                            let param = node.param_child.get_or_insert_with(|| {
                                Box::new(ParamChild {
                                    name,
                                    node: TrieNode::new(),
                                })
                            });
                            // If param name differs, update it (last write wins).
                            param.name = name;
                            node = &mut param.node;
                        }
                    }
                }
                node.route = Some(index);
            }
        }
    }

    /// Matches a request path against the trie for the given method.
    ///
    /// Returns `(route_index, path_params)` on match, or `None`.
    pub fn match_path(
        &self,
        method: &str,
        path: &str,
    ) -> Option<(usize, std::collections::HashMap<String, String>)> {
        let root = self.roots.get(method)?;
        let path = path.trim_start_matches('/').trim_end_matches('/');

        // Special case: root path "/" or empty
        if path.is_empty() {
            // Check for empty-string static child (root route)
            if let Some(child) = root.static_children.get("")
                && let Some(idx) = child.route
            {
                return Some((idx, std::collections::HashMap::new()));
            }
            // Also check for param child at root
            // (Root path has no segment for a param — skip)
            return None;
        }

        let segments: Vec<&str> = path.split('/').collect();
        Self::match_segments(root, &segments, 0, &mut std::collections::HashMap::new())
    }

    fn match_segments(
        node: &TrieNode,
        segments: &[&str],
        idx: usize,
        params: &mut std::collections::HashMap<String, String>,
    ) -> Option<(usize, std::collections::HashMap<String, String>)> {
        if idx >= segments.len() {
            // All segments consumed — check if this node has a route.
            return node.route.map(|r| (r, params.clone()));
        }

        let seg = segments[idx];

        // Try static child first (more specific).
        if let Some(child) = node.static_children.get(seg)
            && let Some(result) = Self::match_segments(child, segments, idx + 1, params)
        {
            return Some(result);
        }

        // Try param child (wildcard).
        if let Some(ref param) = node.param_child {
            params.insert(param.name.to_string(), seg.to_string());
            if let Some(result) = Self::match_segments(&param.node, segments, idx + 1, params) {
                return Some(result);
            }
            params.remove(param.name);
        }

        None
    }
}

// ─── Header Helpers (ordinary-http only) ─────────────────────────

/// Converts an HTTP header name to a `snake_case` field name.
///
/// `"Content-Type"` becomes `"content_type"`, `"Authorization"` becomes
/// `"authorization"`. Hyphens are replaced with underscores and all
/// characters are lowercased.
#[cfg(feature = "ordinary-http")]
pub fn header_name_to_field(header_name: &str) -> String {
    let mut result = String::with_capacity(header_name.len());
    for c in header_name.chars() {
        if c == '-' {
            result.push('_');
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }
    result
}

/// Converts all HTTP request headers into a `serde_json::Value::Object`.
///
/// Each header name is normalized via [`header_name_to_field`] so that
/// the resulting JSON keys match Rust struct field names (e.g.
/// `content_type` rather than `Content-Type`). Header values that cannot
/// be converted to UTF-8 are silently omitted.
#[cfg(feature = "ordinary-http")]
pub fn req_headers_to_json(headers: &hyper::HeaderMap) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (name, value) in headers.iter() {
        let field_name = header_name_to_field(name.as_str());
        if let Ok(s) = value.to_str() {
            map.insert(field_name, serde_json::Value::String(s.to_string()));
        }
    }
    serde_json::Value::Object(map)
}

/// Inserts empty-string defaults for standard HTTP header fields that are
/// missing from a JSON object.
///
/// Standard headers (e.g. `Content-Type`) are not always present in a
/// request (GET requests lack a Content-Type). This ensures that the JSON
/// object fed to the lenient deserializer has every field the target
/// struct expects, so deserialization succeeds even when optional standard
/// headers are absent.
#[cfg(feature = "ordinary-http")]
pub fn fill_standard_header_defaults(
    json: &mut serde_json::Value,
    structure: fn() -> &'static crate::handler::TagMeta,
) {
    if let serde_json::Value::Object(map) = json {
        let meta = structure();
        if let crate::handler::TagKind::Struct(fields) = meta.kind {
            for field in fields {
                let header_name = field.name.replace('_', "-");
                if crate::is_standard_header(&header_name) {
                    map.entry(field.name.to_string())
                        .or_insert_with(|| serde_json::Value::String(String::new()));
                }
            }
        }
    }
}

// ─── Query String Helpers ──────────────────────────────────────────

/// Performs percent-decoding and plus-to-space conversion on a
/// URL-encoded string.
///
/// Decoded bytes are collected into a buffer and validated as UTF-8
/// at the end, so multi-byte sequences (e.g. `%C3%A9` → `é`) are
/// handled correctly. Invalid UTF-8 bytes are replaced with the
/// Unicode replacement character (U+FFFD).
fn percent_decode(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                16,
            ) {
                result.push(byte);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            // `+` in query strings represents a space (application/x-www-form-urlencoded).
            result.push(b' ');
            i += 1;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

/// Parses a URL query string into a `serde_json::Value::Object`.
///
/// All values are stored as JSON strings. Type coercion (string to
/// number, string to boolean) is deferred to the lenient deserializer
/// ([`from_value_lenient`]), which runs when the parsed JSON is
/// deserialized into a typed Rust struct.
pub fn parse_query_to_json(query: &str) -> serde_json::Value {
    let query = query.strip_prefix('?').unwrap_or(query);
    let mut map = serde_json::Map::new();
    if query.is_empty() {
        return serde_json::Value::Object(map);
    }
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next().unwrap_or("");
        let val = kv.next().unwrap_or("");
        if !key.is_empty() {
            map.insert(
                percent_decode(key).to_string(),
                serde_json::Value::String(percent_decode(val)),
            );
        }
    }
    serde_json::Value::Object(map)
}

/// Converts a map of path parameters into a `serde_json::Value::Object`.
///
/// All values are stored as JSON strings. Type coercion is deferred to
/// the lenient deserializer.
pub fn path_params_to_json(params: &HashMap<String, String>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in params {
        map.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    serde_json::Value::Object(map)
}

// ─── Lenient Deserializer ───────────────────────────────────────────

use serde::de::{self, DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor};

/// Deserializes a `serde_json::Value` into `T` with lenient type coercion.
///
/// Standard `serde_json::from_value` rejects type mismatches (e.g. a JSON
/// string for an integer field). This deserializer accepts strings where
/// numbers or booleans are expected, and vice versa, which is required
/// because HTTP query-string and path-parameter values are always strings
/// at the protocol level.
pub fn from_value_lenient<T: DeserializeOwned>(value: serde_json::Value) -> serde_json::Result<T> {
    T::deserialize(LenientValue {
        value: &value,
        key: None,
    })
}

struct LenientValue<'v> {
    value: &'v serde_json::Value,
    key: Option<&'static str>,
}

macro_rules! lenient_int_signed {
    ($self:ident, $visitor:ident, $visit_method:ident, $ty:ty) => {{
        match $self.value {
            serde_json::Value::Number(n) => {
                if let Some(raw) = n.as_i64() {
                    if let Ok(v) = <$ty>::try_from(raw) {
                        return $visitor.$visit_method(v);
                    }
                }
                Err(de::Error::custom(format_args!(
                    "expected {} at {:?}",
                    stringify!($ty),
                    $self.key
                )))
            }
            serde_json::Value::String(s) => {
                if let Ok(v) = s.parse::<$ty>() {
                    $visitor.$visit_method(v)
                } else {
                    Err(de::Error::custom(format_args!(
                        "expected {} at {:?}",
                        stringify!($ty),
                        $self.key
                    )))
                }
            }
            _ => Err(de::Error::custom(format_args!(
                "expected {} at {:?}",
                stringify!($ty),
                $self.key
            ))),
        }
    }};
}

macro_rules! lenient_int_unsigned {
    ($self:ident, $visitor:ident, $visit_method:ident, $ty:ty) => {{
        match $self.value {
            serde_json::Value::Number(n) => {
                if let Some(raw) = n.as_u64() {
                    if let Ok(v) = <$ty>::try_from(raw) {
                        return $visitor.$visit_method(v);
                    }
                }
                Err(de::Error::custom(format_args!(
                    "expected {} at {:?}",
                    stringify!($ty),
                    $self.key
                )))
            }
            serde_json::Value::String(s) => {
                if let Ok(v) = s.parse::<$ty>() {
                    $visitor.$visit_method(v)
                } else {
                    Err(de::Error::custom(format_args!(
                        "expected {} at {:?}",
                        stringify!($ty),
                        $self.key
                    )))
                }
            }
            _ => Err(de::Error::custom(format_args!(
                "expected {} at {:?}",
                stringify!($ty),
                $self.key
            ))),
        }
    }};
}

impl<'de, 'v> Deserializer<'de> for LenientValue<'v> {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::Null => visitor.visit_unit(),
            serde_json::Value::Bool(b) => visitor.visit_bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    visitor.visit_i64(i)
                } else if let Some(u) = n.as_u64() {
                    visitor.visit_u64(u)
                } else if let Some(f) = n.as_f64() {
                    visitor.visit_f64(f)
                } else {
                    Err(de::Error::custom("invalid number"))
                }
            }
            serde_json::Value::String(s) => visitor.visit_string(s.clone()),
            serde_json::Value::Array(arr) => visitor.visit_seq(LenientSeq {
                iter: arr.iter(),
                key: self.key,
            }),
            serde_json::Value::Object(obj) => visitor.visit_map(LenientMap {
                iter: obj.iter(),
                value: None,
                key: self.key,
            }),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::String(s) => visitor.visit_string(s.clone()),
            serde_json::Value::Number(n) => visitor.visit_string(n.to_string()),
            serde_json::Value::Bool(b) => visitor.visit_string(b.to_string()),
            _ => visitor.visit_string(String::new()),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::Bool(b) => visitor.visit_bool(*b),
            serde_json::Value::String(s) => {
                if s.eq_ignore_ascii_case("true") {
                    visitor.visit_bool(true)
                } else if s.eq_ignore_ascii_case("false") {
                    visitor.visit_bool(false)
                } else {
                    Err(de::Error::custom(format_args!(
                        "expected boolean at {:?}",
                        self.key
                    )))
                }
            }
            serde_json::Value::Number(n) => {
                // A number coerces to `true` iff it is non-zero.
                visitor.visit_bool(n.as_f64().is_some_and(|f| f != 0.0))
            }
            _ => Err(de::Error::custom(format_args!(
                "expected boolean at {:?}",
                self.key
            ))),
        }
    }

    fn deserialize_i8<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        lenient_int_signed!(self, v, visit_i8, i8)
    }
    fn deserialize_i16<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        lenient_int_signed!(self, v, visit_i16, i16)
    }
    fn deserialize_i32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        lenient_int_signed!(self, v, visit_i32, i32)
    }
    fn deserialize_i64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::Number(n) => {
                if let Some(raw) = n.as_i64() {
                    return v.visit_i64(raw);
                }
                Err(de::Error::custom(format_args!(
                    "expected i64 at {:?}",
                    self.key
                )))
            }
            serde_json::Value::String(s) => {
                if let Ok(val) = s.parse::<i64>() {
                    v.visit_i64(val)
                } else {
                    Err(de::Error::custom(format_args!(
                        "expected i64 at {:?}",
                        self.key
                    )))
                }
            }
            _ => Err(de::Error::custom(format_args!(
                "expected i64 at {:?}",
                self.key
            ))),
        }
    }
    fn deserialize_u8<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        lenient_int_unsigned!(self, v, visit_u8, u8)
    }
    fn deserialize_u16<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        lenient_int_unsigned!(self, v, visit_u16, u16)
    }
    fn deserialize_u32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        lenient_int_unsigned!(self, v, visit_u32, u32)
    }
    fn deserialize_u64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::Number(n) => {
                if let Some(raw) = n.as_u64() {
                    return v.visit_u64(raw);
                }
                Err(de::Error::custom(format_args!(
                    "expected u64 at {:?}",
                    self.key
                )))
            }
            serde_json::Value::String(s) => {
                if let Ok(val) = s.parse::<u64>() {
                    v.visit_u64(val)
                } else {
                    Err(de::Error::custom(format_args!(
                        "expected u64 at {:?}",
                        self.key
                    )))
                }
            }
            _ => Err(de::Error::custom(format_args!(
                "expected u64 at {:?}",
                self.key
            ))),
        }
    }
    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::Number(n) => {
                if let Some(raw) = n.as_f64() {
                    return v.visit_f32(raw as f32);
                }
                Err(de::Error::custom(format_args!(
                    "expected f32 at {:?}",
                    self.key
                )))
            }
            serde_json::Value::String(s) => {
                if let Ok(val) = s.parse::<f32>() {
                    v.visit_f32(val)
                } else {
                    Err(de::Error::custom(format_args!(
                        "expected f32 at {:?}",
                        self.key
                    )))
                }
            }
            _ => Err(de::Error::custom(format_args!(
                "expected f32 at {:?}",
                self.key
            ))),
        }
    }
    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::Number(n) => {
                if let Some(raw) = n.as_f64() {
                    return v.visit_f64(raw);
                }
                Err(de::Error::custom(format_args!(
                    "expected f64 at {:?}",
                    self.key
                )))
            }
            serde_json::Value::String(s) => {
                if let Ok(val) = s.parse::<f64>() {
                    v.visit_f64(val)
                } else {
                    Err(de::Error::custom(format_args!(
                        "expected f64 at {:?}",
                        self.key
                    )))
                }
            }
            _ => Err(de::Error::custom(format_args!(
                "expected f64 at {:?}",
                self.key
            ))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self.value {
            serde_json::Value::Object(obj) => visitor.visit_map(LenientMap {
                iter: obj.iter(),
                value: None,
                key: self.key,
            }),
            _ => Err(de::Error::custom("expected object")),
        }
    }

    serde::forward_to_deserialize_any! {
        char bytes byte_buf unit unit_struct seq tuple tuple_struct map identifier ignored_any enum
    }
}

struct LenientMap<'v> {
    iter: serde_json::map::Iter<'v>,
    value: Option<&'v serde_json::Value>,
    key: Option<&'static str>,
}

impl<'de, 'v> MapAccess<'de> for LenientMap<'v> {
    type Error = serde_json::Error;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        match self.iter.next() {
            Some((k, v)) => {
                self.value = Some(v);
                seed.deserialize(de::value::StrDeserializer::new(k.as_str()))
                    .map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        let value = self.value.take().unwrap();
        seed.deserialize(LenientValue {
            value,
            key: self.key,
        })
    }
}

struct LenientSeq<'v> {
    iter: std::slice::Iter<'v, serde_json::Value>,
    key: Option<&'static str>,
}

impl<'de, 'v> SeqAccess<'de> for LenientSeq<'v> {
    type Error = serde_json::Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        match self.iter.next() {
            Some(value) => seed
                .deserialize(LenientValue {
                    value,
                    key: self.key,
                })
                .map(Some),
            None => Ok(None),
        }
    }
}

/// Reads the full body of an HTTP request into a byte vector.
///
/// The body is streamed chunk-by-chunk and the total size is checked
/// against the global body size limit (set via [`set_body_size_limit`]).
/// This prevents OOM from oversized payloads — the check happens during
/// reading, not after full allocation.
#[cfg(feature = "ordinary-http")]
pub async fn read_body_bytes(
    req: hyper::Request<hyper::body::Incoming>,
) -> Result<Vec<u8>, crate::Error> {
    use http_body_util::BodyExt;
    let max = BODY_SIZE_LIMIT.load(std::sync::atomic::Ordering::Relaxed);
    let mut collected = Vec::new();
    let mut stream = req.into_body();
    while let Some(chunk) = stream.frame().await {
        let frame = chunk.map_err(|e| crate::Error::Custom {
            code: 400,
            message: format!("body read error: {}", e),
        })?;
        if let Some(data) = frame.data_ref() {
            if collected.len() + data.len() > max {
                return Err(crate::Error::Http {
                    message: format!("request body too large (limit: {} bytes)", max),
                });
            }
            collected.extend_from_slice(data);
        }
    }
    Ok(collected)
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_pattern() {
        let pattern = RoutePattern::parse("/users/list");
        assert!(pattern.matches("/users/list").is_some());
        assert!(pattern.matches("/users/123").is_none());
    }

    #[test]
    fn test_parametric_pattern() {
        let pattern = RoutePattern::parse("/users/:id");
        let params = pattern.matches("/users/123").unwrap();
        assert_eq!(params.get("id").unwrap(), "123");
    }

    #[test]
    fn test_multi_param_pattern() {
        let pattern = RoutePattern::parse("/users/:id/posts/:post_id");
        let params = pattern.matches("/users/123/posts/456").unwrap();
        assert_eq!(params.get("id").unwrap(), "123");
        assert_eq!(params.get("post_id").unwrap(), "456");
    }

    #[test]
    fn test_root_pattern() {
        let pattern = RoutePattern::parse("/");
        assert!(pattern.matches("").is_some());
        assert!(pattern.matches("/").is_some());
    }

    #[test]
    fn test_header_name_to_field() {
        assert_eq!(header_name_to_field("Content-Type"), "content_type");
        assert_eq!(header_name_to_field("Authorization"), "authorization");
        assert_eq!(header_name_to_field("X-Real-Ip"), "x_real_ip");
    }

    #[test]
    fn test_parse_query_to_json() {
        let json = parse_query_to_json("page=1&size=10&name=hello%20world");
        let obj = json.as_object().unwrap();
        assert_eq!(obj.get("page").unwrap().as_str().unwrap(), "1");
        assert_eq!(obj.get("size").unwrap().as_str().unwrap(), "10");
        assert_eq!(obj.get("name").unwrap().as_str().unwrap(), "hello world");
    }

    #[test]
    fn test_empty_query() {
        let json = parse_query_to_json("");
        assert!(json.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_percent_decode_utf8() {
        // Multi-byte UTF-8: é = %C3%A9
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        // CJK: 中 = %E4%B8%AD
        assert_eq!(percent_decode("%E4%B8%AD"), "中");
        // Plus to space
        assert_eq!(percent_decode("hello+world"), "hello world");
        // Plain ASCII passthrough
        assert_eq!(percent_decode("hello"), "hello");
        // Mixed
        assert_eq!(percent_decode("name%3D%E5%BC%A0%E4%B8%89"), "name=张三");
    }

    #[test]
    fn test_percent_decode_invalid_utf8() {
        // Invalid UTF-8 sequence → replacement character
        let result = percent_decode("%FF%FE");
        assert!(result.contains('\u{FFFD}'));
    }
}
