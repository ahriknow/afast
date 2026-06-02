//! Rate limiting module.
//!
//! Provides named rate-limit policies that handlers can reference by ID.
//! Supports per-IP, per-header, per-connection, and global rate limiting
//! with fixed-window, sliding-window, and token-bucket algorithms.
//!
//! The underlying storage is pluggable via the [`RateLimitStore`] trait.
//! A built-in [`InMemoryStore`] is provided; users can supply their own
//! implementation (e.g. Redis) through [`RateLimitConfig::store`].

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::{CODE_RATE_LIMITED, Error};

// ─── Configuration ────────────────────────────────────────────────

/// The rate limit key extraction method.
#[derive(Debug)]
pub enum RateLimitKey {
    /// Rate limit by client IP.
    Ip,
    /// Rate limit by HTTP header value.
    Header(&'static str),
    /// Rate limit per connection (WS/TCP message rate).
    Connection,
    /// Global shared counter across all connections and IPs.
    Global,
}

/// Rate limit algorithm.
#[derive(Debug, Clone)]
pub enum Algorithm {
    /// Fixed time window; resets at window boundary.
    FixedWindow,
    /// Smooth sliding window; avoids boundary burst.
    SlidingWindow,
    /// Token bucket; allows short bursts up to bucket capacity.
    TokenBucket,
}

/// A named rate-limit policy.
pub struct RateLimitPolicy {
    /// Unique policy identifier (handlers reference this).
    pub id: String,
    /// Maximum requests allowed per window.
    pub max_requests: u64,
    /// Window duration in seconds.
    pub window_secs: u64,
    /// Key extraction method.
    pub key: RateLimitKey,
    /// Rate limit algorithm.
    pub algorithm: Algorithm,
}

/// Top-level rate-limit configuration, built via the builder pattern.
pub struct RateLimitConfig {
    policies: Vec<RateLimitPolicy>,
    /// Optional default policy ID applied to handlers without explicit configuration.
    default_policy: Option<String>,
    rejected_code: i64,
    rejected_message: String,
    /// Pluggable counter store (defaults to [`InMemoryStore`]).
    store: Arc<dyn RateLimitStore>,
}

impl RateLimitConfig {
    /// Creates an empty configuration with default rejection settings
    /// and an [`InMemoryStore`] backend.
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
            default_policy: None,
            rejected_code: CODE_RATE_LIMITED,
            rejected_message: "Too many requests".to_string(),
            store: Arc::new(InMemoryStore::new()),
        }
    }

    /// Adds a policy to this configuration.
    pub fn policy(mut self, policy: RateLimitPolicy) -> Self {
        self.policies.push(policy);
        self
    }

    /// Sets the default policy applied to handlers that do not specify one.
    ///
    /// At most one default policy can be set. If the referenced policy ID
    /// does not exist in the policy list, no default will be applied.
    pub fn default_policy(mut self, id: impl Into<String>) -> Self {
        self.default_policy = Some(id.into());
        self
    }

    /// Sets the error code returned when a request is rejected.
    pub fn rejected_code(mut self, code: i64) -> Self {
        self.rejected_code = code;
        self
    }

    /// Sets the error message returned when a request is rejected.
    pub fn rejected_message(mut self, msg: impl Into<String>) -> Self {
        self.rejected_message = msg.into();
        self
    }

    /// Replaces the default [`InMemoryStore`] with a custom store
    /// implementation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let config = RateLimitConfig::new()
    ///     .store(Arc::new(MyRedisStore::new("redis://localhost")));
    /// ```
    pub fn store(mut self, store: Arc<dyn RateLimitStore>) -> Self {
        self.store = store;
        self
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ─── RateLimitStore trait ─────────────────────────────────────────

/// Pluggable storage backend for rate-limit counters.
///
/// Implement this trait to use a custom store such as Redis, Memcached, or
/// a database. The built-in [`InMemoryStore`] is used when no custom store
/// is provided.
///
/// # Contract
///
/// * `try_acquire` must be **atomic** — concurrent calls with the same
///   `(policy_id, key)` pair must not over-count.
/// * Returning `true` means the request is **allowed**; `false` means it
///   should be rejected.
/// * `is_connection == true` indicates a per-connection scoped counter.
///   Implementations that do not support connection scoping may ignore
///   this flag.
pub trait RateLimitStore: Send + Sync + 'static {
    /// Attempts to consume one request slot.
    ///
    /// Returns `true` if the request is within the rate limit, `false` if
    /// it should be rejected.
    fn try_acquire<'a>(
        &'a self,
        policy_id: &'a str,
        key: &'a str,
        max_requests: u64,
        window_secs: u64,
        algorithm: &'a Algorithm,
        is_connection: bool,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
}

// ─── InMemoryStore ────────────────────────────────────────────────

/// Internal sliding-window / token-bucket counter.
struct WindowCounter {
    slots: Vec<(u64, u64)>,
    count: u64,
    tokens: f64,
    last_refill: u64,
}

impl WindowCounter {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            count: 0,
            tokens: 0.0,
            last_refill: 0,
        }
    }

    fn try_fixed_window(&mut self, max_requests: u64, window_secs: u64) -> bool {
        let now = now_secs();
        let window_start = now / window_secs * window_secs;

        if self.slots.first().is_some_and(|&(ts, _)| ts < window_start) {
            self.slots.clear();
            self.count = 0;
        }

        if self.count >= max_requests {
            return false;
        }
        self.count += 1;
        self.slots.push((now, 1));
        true
    }

    fn try_sliding_window(&mut self, max_requests: u64, window_secs: u64) -> bool {
        let now = now_secs();
        let cutoff = now.saturating_sub(window_secs);

        let mut removed = 0u64;
        while self.slots.first().is_some_and(|&(ts, _)| ts < cutoff) {
            if let Some((_, cnt)) = self.slots.first() {
                removed += cnt;
            }
            self.slots.remove(0);
        }
        self.count = self.count.saturating_sub(removed);

        if self.count >= max_requests {
            return false;
        }
        self.count += 1;
        self.slots.push((now, 1));
        true
    }

    fn try_token_bucket(&mut self, max_requests: u64, window_secs: u64) -> bool {
        let now = now_secs();
        let rate = max_requests as f64 / window_secs as f64;

        if self.last_refill == 0 {
            self.tokens = max_requests as f64;
            self.last_refill = now;
        } else {
            let elapsed = (now.saturating_sub(self.last_refill)) as f64;
            self.tokens = (self.tokens + elapsed * rate).min(max_requests as f64);
            self.last_refill = now;
        }

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Returns the current time in seconds since the Unix epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// `policy_id` → `(composite_key → counter)`.
#[allow(clippy::type_complexity)]
type CounterMap = Arc<RwLock<HashMap<String, Arc<RwLock<HashMap<String, WindowCounter>>>>>>;

/// Default in-memory rate-limit store.
///
/// Counters are stored in a two-level concurrent map:
/// `policy_id → (composite_key → WindowCounter)`.
///
/// This store is **not** shared across processes. For distributed
/// rate limiting, implement [`RateLimitStore`] backed by Redis or a
/// similar external store.
#[derive(Clone)]
pub struct InMemoryStore {
    counters: CounterMap,
}

impl InMemoryStore {
    /// Creates a new empty in-memory store.
    pub fn new() -> Self {
        Self {
            counters: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitStore for InMemoryStore {
    fn try_acquire<'a>(
        &'a self,
        policy_id: &'a str,
        key: &'a str,
        max_requests: u64,
        window_secs: u64,
        algorithm: &'a Algorithm,
        is_connection: bool,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            let composite_key = if is_connection {
                format!("__conn:{}:{}", policy_id, key)
            } else {
                format!("{}:{}", policy_id, key)
            };

            let inner_arc = {
                let mut outer = self.counters.write().await;
                outer
                    .entry(policy_id.to_string())
                    .or_insert_with(|| Arc::new(RwLock::new(HashMap::new())))
                    .clone()
            };

            let mut inner = inner_arc.write().await;
            let counter = inner
                .entry(composite_key)
                .or_insert_with(WindowCounter::new);

            match algorithm {
                Algorithm::FixedWindow => counter.try_fixed_window(max_requests, window_secs),
                Algorithm::SlidingWindow => counter.try_sliding_window(max_requests, window_secs),
                Algorithm::TokenBucket => counter.try_token_bucket(max_requests, window_secs),
            }
        })
    }
}

// ─── ConnectionContext ────────────────────────────────────────────

/// Per-connection context for rate-limit key extraction.
///
/// Created at connection time (HTTP: per request; WS/TCP: per connection).
pub struct ConnectionContext {
    /// Client IP address.
    pub client_ip: String,
    /// Header values cached at connection/handshake time.
    pub header_cache: HashMap<String, String>,
    /// Monotonically increasing counter used to generate unique connection IDs.
    conn_counter: u64,
}

impl ConnectionContext {
    /// Creates a new context with the given client IP and no cached headers.
    pub fn new(client_ip: String) -> Self {
        Self {
            client_ip,
            header_cache: HashMap::new(),
            conn_counter: 0,
        }
    }

    /// Returns a unique connection ID string for this context.
    #[allow(dead_code)]
    fn next_conn_id(&mut self) -> String {
        self.conn_counter += 1;
        format!("{}#{}", self.client_ip, self.conn_counter)
    }

    /// Extracts the rate-limit key and an optional per-connection identifier.
    ///
    /// Returns `(key, conn_id)` where `conn_id` is `Some(...)` only for
    /// `RateLimitKey::Connection`. Returns `None` when the key type is
    /// unsupported for the current transport.
    #[allow(dead_code)]
    fn extract_key(&mut self, key: &RateLimitKey) -> Option<(String, Option<String>)> {
        match key {
            RateLimitKey::Ip => Some((self.client_ip.clone(), None)),
            RateLimitKey::Header(name) => self.header_cache.get(*name).cloned().map(|v| (v, None)),
            RateLimitKey::Connection => {
                let conn_id = self.next_conn_id();
                Some((self.client_ip.clone(), Some(conn_id)))
            }
            RateLimitKey::Global => Some(("_global".to_string(), None)),
        }
    }
}

// ─── RateLimiter ──────────────────────────────────────────────────

/// Thread-safe rate limiter shared across all transport layers.
#[allow(dead_code)]
pub(crate) struct RateLimiter {
    /// `policy_id` → policy definition.
    policies: HashMap<String, RateLimitPolicy>,
    /// `handler_name` → `policy_id` for quick lookup at dispatch time.
    name_to_policy: HashMap<String, String>,
    /// Default policy ID for handlers without explicit configuration.
    default_policy: Option<String>,
    /// Error code returned on rejection.
    #[allow(dead_code)]
    rejected_code: i64,
    /// Error message returned on rejection.
    rejected_message: String,
    /// Pluggable counter store.
    store: Arc<dyn RateLimitStore>,
}

impl RateLimiter {
    /// Builds a `RateLimiter` from a configuration and the full handler table.
    #[allow(dead_code)]
    pub(crate) fn new(
        config: RateLimitConfig,
        all_handlers: &[&crate::handler::HandlerMeta],
    ) -> Self {
        let mut policies = HashMap::new();

        for p in config.policies {
            policies.insert(p.id.clone(), p);
        }

        // Validate that the default policy ID actually exists.
        let default_policy = config.default_policy.and_then(|id| {
            if policies.contains_key(&id) {
                Some(id)
            } else {
                None
            }
        });

        let mut name_to_policy = HashMap::new();
        for meta in all_handlers {
            if !meta.rate_limit_policy.is_empty() {
                name_to_policy.insert(meta.name.to_string(), meta.rate_limit_policy.to_string());
            }
        }

        Self {
            policies,
            name_to_policy,
            default_policy,
            rejected_code: config.rejected_code,
            rejected_message: config.rejected_message,
            store: config.store,
        }
    }

    /// Returns the policy ID for a handler name, if one is configured.
    #[allow(dead_code)]
    pub(crate) fn policy_for_handler(&self, handler_name: &str) -> Option<&str> {
        self.name_to_policy.get(handler_name).map(|s| s.as_str())
    }

    /// Checks whether a request is allowed under the rate-limit policy
    /// associated with the given handler name.
    ///
    /// Returns `Ok(())` if allowed, or a rate-limit `Error` if rejected.
    #[allow(dead_code)]
    pub(crate) async fn check(
        &self,
        handler_name: &str,
        ctx: &mut ConnectionContext,
    ) -> Result<(), Error> {
        // Resolve policy: explicit per-handler → default → no limit.
        let policy_id = match self.name_to_policy.get(handler_name) {
            Some(id) => id.as_str(),
            None => match self.default_policy.as_deref() {
                Some(dp) => dp,
                None => return Ok(()),
            },
        };

        let policy = match self.policies.get(policy_id) {
            Some(p) => p,
            None => return Ok(()),
        };

        let is_connection = matches!(policy.key, RateLimitKey::Connection);

        let (key, conn_id) = match ctx.extract_key(&policy.key) {
            Some(v) => v,
            None => return Ok(()), // Key unsupported on this transport → skip.
        };

        // For Connection keys, use the per-connection ID so the store
        // scopes the counter to this connection.
        let store_key = match conn_id {
            Some(cid) => cid,
            None => key,
        };

        let allowed = self
            .store
            .try_acquire(
                policy_id,
                &store_key,
                policy.max_requests,
                policy.window_secs,
                &policy.algorithm,
                is_connection,
            )
            .await;

        if !allowed {
            return Err(Error::RateLimited {
                message: self.rejected_message.clone(),
            });
        }

        Ok(())
    }
}
