# Rate Limiting

Enable the `rate-limit` feature to apply named rate-limit policies to handlers. Supports HTTP, WebSocket, and TCP transports.

## Configuration

```rust
use afast::{RateLimitConfig, RateLimitPolicy, RateLimitKey, Algorithm};

let app = AFast::new()
    .rate_limit(
        RateLimitConfig::new()
            .policy(RateLimitPolicy {
                id: "login".into(),
                max_requests: 5,
                window_secs: 60,
                key: RateLimitKey::Ip,
                algorithm: Algorithm::SlidingWindow,
            })
            .default_policy("global")
            .policy(RateLimitPolicy {
                id: "global".into(),
                max_requests: 100,
                window_secs: 1,
                key: RateLimitKey::Ip,
                algorithm: Algorithm::SlidingWindow,
            }),
    )
    .service(svc)
    .http("0.0.0.0:5000");
```

## Binding a Handler

```rust
#[handler(rate_limit("login"), desc("User login"))]
async fn login(
    state: State<AppState>,
    req: Data<LoginRequest>,
) -> Result<LoginResponse> {
    // ...
}
```

Handlers without `rate_limit` automatically use the `default_policy`. If no default is set, they are not rate-limited.

## Rate Limit Keys

| Key | Description | HTTP | WebSocket | TCP |
|-----|-------------|------|-----------|-----|
| `Ip` | Client IP (supports `X-Forwarded-For`) | ✅ | ✅ | ✅ |
| `Header("name")` | HTTP header value (e.g. API Key) | ✅ | ✅ (cached at handshake) | ⏭ skipped |
| `Connection` | Per-connection (WS/TCP message rate) | ⏭ skipped | ✅ | ✅ |
| `Global` | Shared global counter | ✅ | ✅ | ✅ |

## Storage Backend

The default `InMemoryStore` keeps counters in process memory. Implement `RateLimitStore` for a custom backend (e.g. Redis):

```rust
use afast::RateLimitStore;

struct RedisStore { /* ... */ }

impl RateLimitStore for RedisStore {
    fn incr<'a>(&'a self, key: &'a str, ttl_secs: u64)
        -> Pin<Box<dyn Future<Output = u64> + Send + 'a>> { /* INCR + EXPIRE */ }
    fn get<'a>(&'a self, key: &'a str)
        -> Pin<Box<dyn Future<Output = u64> + Send + 'a>> { /* GET */ }
    fn set<'a>(&'a self, key: &'a str, value: u64, ttl_secs: u64)
        -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> { /* SET + EXPIRE */ }
    fn delete<'a>(&'a self, key: &'a str)
        -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> { /* DEL */ }
}
```

## Rejection Response

- **HTTP**: Status `429 Too Many Requests`, body: `{"code":-90012,"message":"Too many requests"}`
- **WebSocket / TCP**: Error frame with code `-90012`

Customize via `RateLimitConfig::rejected_code()` and `rejected_message()`.
