# Binary Protocol

## Type Mapping

| Rust Type | TS/JS Type | Kotlin Type |
|-----------|-----------|------------|
| `i8` ~ `i64`, `u8` ~ `u64`, `f32`, `f64` | `number` | `Int`/`Long`/`Float`/`Double` |
| `bool` | `boolean` | `Boolean` |
| `String`, `&str` | `string` | `String` |
| `Vec<u8>` | `Uint8Array` | `ByteArray` |
| `Option<T>` | `T \| null` | `T?` |
| `Vec<T>` | `T[]` | `List<T>` |
| struct | `{ field: Type }` | `data class` |
| enum | `{ tag: 'Variant', data: ... }` | `sealed class` |

## Error Codes

System reserved error codes range from `-90011` to `-90000`. User-defined errors must not use this range.

| Constant | Value | Description |
|----------|-------|-------------|
| `CODE_SIGNAL` | -90000 | OS signal (e.g., Ctrl+C) |
| `CODE_MSG_TOO_SHORT` | -90001 | Message too short |
| `CODE_PAYLOAD_MISMATCH` | -90002 | Payload length mismatch |
| `CODE_SERIALIZE` | -90003 | Serialization/deserialization error |
| `CODE_STATE_NOT_FOUND` | -90004 | State type not registered |
| `CODE_HANDLER` | -90005 | Handler execution error |
| `CODE_INVALID_PARAM` | -90006 | Invalid parameter |
| `CODE_IO` | -90007 | I/O error |
| `CODE_WS` | -90008 | WebSocket error |
| `CODE_HTTP` | -90009 | HTTP error |
| `CODE_TCP` | -90010 | TCP error |
| `CODE_LONG_CONNECTION_NOT_SUPPORTED` | -90011 | Long connections unsupported in HTTP mode |
| `CODE_RATE_LIMITED` | -90012 | Rate limit exceeded |

```rust
// Custom error (code must be outside the reserved range)
return Err(afast::Error::custom(400, "invalid request parameter"));
```

## Custom Error Types

Implement the `AFastError` trait to define your own error types and return `Result<T, MyError>` from handlers:

```rust
use afast::AFastError;

enum MyError {
    NotFound(String),
    Unauthorized,
}

impl AFastError for MyError {
    fn code(&self) -> i64 {
        match self {
            MyError::NotFound(_) => 404,
            MyError::Unauthorized => 401,
        }
    }

    fn message(&self) -> String {
        match self {
            MyError::NotFound(name) => format!("{} not found", name),
            MyError::Unauthorized => "unauthorized".into(),
        }
    }
}

#[handler(desc("Get user"))]
async fn get_user(id: Data<UserId>) -> afast::Result<UserInfo, MyError> {
    find_user(id).ok_or(MyError::NotFound("user".into()))
}
```

All handler macros (`#[handler]`, `#[get]`/`#[post]`/`#[put]`/`#[delete]`, `#[ws]`, `#[sse]`) support custom error types. `afast::Result<T>` defaults to `Error` and is fully backward compatible.
