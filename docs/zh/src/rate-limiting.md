# 速率限制

启用 `rate-limit` Feature 可为 handler 应用命名的速率限制策略。支持 HTTP、WebSocket 和 TCP 传输层。

## 配置

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

## 绑定 Handler

```rust
#[handler(rate_limit("login"), desc("User login"))]
async fn login(
    state: State<AppState>,
    req: Data<LoginRequest>,
) -> Result<LoginResponse> {
    // ...
}
```

没有 `rate_limit` 的 handler 自动使用 `default_policy`。如果未设置默认策略，则不受速率限制。

## 速率限制键

| 键 | 描述 | HTTP | WebSocket | TCP |
|----|------|------|-----------|-----|
| `Ip` | 客户端 IP（支持 `X-Forwarded-For`） | ✅ | ✅ | ✅ |
| `Header("name")` | HTTP 头值（如 API Key） | ✅ | ✅ (握手时缓存) | ⏭ 跳过 |
| `Connection` | 按连接（WS/TCP 消息速率） | ⏭ 跳过 | ✅ | ✅ |
| `Global` | 共享全局计数器 | ✅ | ✅ | ✅ |

## 存储后端

默认的 `InMemoryStore` 在进程内存中保存计数器。实现 `RateLimitStore` 可使用自定义后端（如 Redis）：

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

## 拒绝响应

- **HTTP**: 状态码 `429 Too Many Requests`，响应体: `{"code":-90012,"message":"Too many requests"}`
- **WebSocket / TCP**: 错误帧，错误码 `-90012`

可通过 `RateLimitConfig::rejected_code()` 和 `rejected_message()` 自定义。
