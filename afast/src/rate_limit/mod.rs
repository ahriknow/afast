//! Rate limiting module.
//!
//! Provides named rate-limit policies that handlers can reference by ID.
//! Supports per-IP, per-header, per-connection, and global rate limiting
//! with fixed-window, sliding-window, and token-bucket algorithms.
//!
//! The underlying storage is pluggable via the [`RateLimitStore`] trait.
//! Algorithm logic is handled internally by the framework; users only
//! need to implement simple key-value operations (`incr`, `get`, `set`,
//! `delete`).  A built-in [`InMemoryStore`] is provided.

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
/// a database.  The built-in [`InMemoryStore`] is used when no custom store
/// is provided.
///
/// Users only need to implement simple key-value operations.  The rate-limit
/// algorithms (fixed window, sliding window, token bucket) are handled
/// internally by the framework.
///
/// # Contract
///
/// * `incr` must be **atomic** — concurrent calls with the same key must
///   not over-count.
/// * `get` returns `0` for non-existent keys.
/// * `set` overwrites the value and resets the TTL.
/// * `ttl_secs > 0` means the key expires after that many seconds.
pub trait RateLimitStore: Send + Sync + 'static {
    /// Atomically increments `key` by 1 and returns the new value.
    /// If the key does not exist, it is created starting from 0 (so the
    /// first `incr` returns 1).  If `ttl_secs > 0`, the key expires after
    /// that many seconds.
    fn incr<'a>(
        &'a self,
        key: &'a str,
        ttl_secs: u64,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + 'a>>;

    /// Returns the current value of `key`, or `0` if it does not exist.
    fn get<'a>(&'a self, key: &'a str) -> Pin<Box<dyn Future<Output = u64> + Send + 'a>>;

    /// Sets `key` to `value` with an optional TTL.  Overwrites any
    /// existing value.  If `ttl_secs > 0`, the key expires after that
    /// many seconds.
    fn set<'a>(
        &'a self,
        key: &'a str,
        value: u64,
        ttl_secs: u64,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    /// Deletes `key`.  No-op if the key does not exist.
    fn delete<'a>(&'a self, key: &'a str) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    /// Atomically decrements `key` by 1 if the value is > 0, and returns
    /// the new value.  If the key does not exist or is already 0, returns 0
    /// without modifying the key.  If `ttl_secs > 0`, resets the TTL.
    ///
    /// The default implementation uses `get` + `set` (non-atomic).  Custom
    /// store backends should override this with an atomic operation to
    /// prevent race conditions in the token-bucket algorithm.
    fn decr<'a>(
        &'a self,
        key: &'a str,
        ttl_secs: u64,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + 'a>> {
        Box::pin(async move {
            let current = self.get(key).await;
            if current > 0 {
                let new_val = current - 1;
                self.set(key, new_val, ttl_secs).await;
                new_val
            } else {
                0
            }
        })
    }
}

// ─── InMemoryStore ────────────────────────────────────────────────

/// Default in-memory rate-limit store backed by a concurrent `HashMap`.
///
/// This store is **not** shared across processes.  For distributed
/// rate limiting, implement [`RateLimitStore`] backed by Redis or a
/// similar external store.
#[derive(Clone)]
pub struct InMemoryStore {
    /// `(value, Option<expires_at_secs>)`
    #[allow(clippy::type_complexity)]
    data: Arc<RwLock<HashMap<String, (u64, Option<u64>)>>>,
}

impl InMemoryStore {
    /// Creates a new empty in-memory store and spawns a background
    /// cleanup task that removes expired entries every 60 seconds.
    pub fn new() -> Self {
        #[allow(clippy::type_complexity)]
        let data: Arc<RwLock<HashMap<String, (u64, Option<u64>)>>> =
            Arc::new(RwLock::new(HashMap::new()));
        // Background task: periodically purge expired keys so memory
        // does not grow unboundedly for keys that are never accessed again.
        let cleanup_data = data.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let now = now_secs();
                let mut map = cleanup_data.write().await;
                map.retain(|_, (_, exp)| exp.is_none_or(|e| now < e));
            }
        });
        Self { data }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitStore for InMemoryStore {
    fn incr<'a>(
        &'a self,
        key: &'a str,
        ttl_secs: u64,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + 'a>> {
        Box::pin(async move {
            let now = now_secs();
            let mut map = self.data.write().await;
            let entry = map.entry(key.to_string()).or_insert((0, None));
            // Expired → reset
            if let Some(exp) = entry.1
                && now >= exp
            {
                *entry = (0, None);
            }
            entry.0 += 1;
            if ttl_secs > 0 {
                entry.1 = Some(now + ttl_secs);
            }
            entry.0
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> Pin<Box<dyn Future<Output = u64> + Send + 'a>> {
        Box::pin(async move {
            let now = now_secs();
            let map = self.data.read().await;
            match map.get(key) {
                Some((val, Some(exp))) if now < *exp => *val,
                Some((_, Some(_))) => 0, // expired
                Some((val, None)) => *val,
                None => 0,
            }
        })
    }

    fn set<'a>(
        &'a self,
        key: &'a str,
        value: u64,
        ttl_secs: u64,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let now = now_secs();
            let mut map = self.data.write().await;
            let exp = if ttl_secs > 0 {
                Some(now + ttl_secs)
            } else {
                None
            };
            map.insert(key.to_string(), (value, exp));
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let mut map = self.data.write().await;
            map.remove(key);
        })
    }

    fn decr<'a>(
        &'a self,
        key: &'a str,
        ttl_secs: u64,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + 'a>> {
        Box::pin(async move {
            let now = now_secs();
            let mut map = self.data.write().await;
            let entry = map.entry(key.to_string()).or_insert((0, None));
            // Expired → reset
            if let Some(exp) = entry.1
                && now >= exp
            {
                *entry = (0, None);
            }
            if entry.0 > 0 {
                entry.0 -= 1;
            }
            if ttl_secs > 0 {
                entry.1 = Some(now + ttl_secs);
            }
            entry.0
        })
    }
}

// ─── Algorithm implementation ─────────────────────────────────────

/// Returns the current time in seconds since the Unix epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Attempts to acquire one request slot using the specified algorithm.
///
/// This function implements the rate-limit algorithms on top of the simple
/// [`RateLimitStore`] operations (`incr`, `get`, `set`, `delete`).
async fn try_acquire(
    store: &dyn RateLimitStore,
    policy_id: &str,
    key: &str,
    max_requests: u64,
    window_secs: u64,
    algorithm: &Algorithm,
    is_connection: bool,
) -> bool {
    let prefix = if is_connection {
        format!("__conn:{}:{}", policy_id, key)
    } else {
        format!("{}:{}", policy_id, key)
    };

    match algorithm {
        Algorithm::FixedWindow => {
            let now = now_secs();
            let window_start = now / window_secs * window_secs;
            let store_key = format!("{}:{}", prefix, window_start);
            let count = store.incr(&store_key, window_secs).await;
            count <= max_requests
        }
        Algorithm::SlidingWindow => {
            let now = now_secs();
            let current_window = now / window_secs * window_secs;
            let previous_window = current_window.saturating_sub(window_secs);
            let current_key = format!("{}:{}", prefix, current_window);
            let previous_key = format!("{}:{}", prefix, previous_window);

            // Atomic: incr first, then check. This eliminates the race
            // condition where concurrent requests both read the same count
            // before either increments. The trade-off is that rejected
            // requests also increment the counter (slightly strict), which is
            // acceptable for rate limiting.
            let current_count = store.incr(&current_key, window_secs * 2).await;
            let previous_count = store.get(&previous_key).await;

            let elapsed_in_window = now.saturating_sub(current_window);
            let weight = 1.0 - (elapsed_in_window as f64 / window_secs as f64);
            let estimated = previous_count as f64 * weight + current_count as f64;

            estimated <= max_requests as f64
        }
        Algorithm::TokenBucket => {
            let now = now_secs();
            let tokens_key = format!("{}:tokens", prefix);
            let refill_key = format!("{}:refill", prefix);

            let current_tokens = store.get(&tokens_key).await;
            let last_refill = store.get(&refill_key).await;
            let rate = max_requests as f64 / window_secs as f64;

            let new_tokens = if last_refill == 0 {
                max_requests as f64
            } else {
                let elapsed = now.saturating_sub(last_refill) as f64;
                (current_tokens as f64 + elapsed * rate).min(max_requests as f64)
            };

            // Refill tokens and update the timestamp.
            store
                .set(&tokens_key, new_tokens as u64, window_secs * 2)
                .await;
            store.set(&refill_key, now, window_secs * 2).await;

            // Atomically consume one token.  `decr` is a single operation
            // on the store backend, so concurrent requests cannot both read
            // the same token count and both succeed.
            let remaining = store.decr(&tokens_key, window_secs * 2).await;
            remaining < new_tokens as u64
        }
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

    /// Returns the `window_secs` for the policy that applies to
    /// `handler_name`, used to compute the `Retry-After` header.
    /// Returns `60` as a safe default when no policy is found.
    #[allow(dead_code)]
    pub(crate) fn retry_after_secs(&self, handler_name: &str) -> u64 {
        let policy_id = match self.name_to_policy.get(handler_name) {
            Some(id) => id.as_str(),
            None => match self.default_policy.as_deref() {
                Some(dp) => dp,
                None => return 60,
            },
        };
        self.policies
            .get(policy_id)
            .map(|p| p.window_secs)
            .unwrap_or(60)
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

        let allowed = try_acquire(
            self.store.as_ref(),
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

    /// Checks whether a request is allowed under a specific policy ID.
    ///
    /// Unlike [`check`](Self::check), this method takes the policy ID
    /// directly instead of looking it up by handler name. Used for
    /// framework-level endpoints like `/code` and `/doc`.
    #[allow(dead_code)]
    pub(crate) async fn check_by_policy(
        &self,
        policy_id: &str,
        ctx: &mut ConnectionContext,
    ) -> Result<(), Error> {
        let policy = match self.policies.get(policy_id) {
            Some(p) => p,
            None => return Ok(()),
        };

        let is_connection = matches!(policy.key, RateLimitKey::Connection);
        let (key, conn_id) = match ctx.extract_key(&policy.key) {
            Some(v) => v,
            None => return Ok(()),
        };

        let store_key = match conn_id {
            Some(cid) => cid,
            None => key,
        };

        let allowed = try_acquire(
            self.store.as_ref(),
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
