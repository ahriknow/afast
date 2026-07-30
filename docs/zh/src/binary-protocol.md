# 二进制协议

## 类型映射

| Rust 类型 | TS/JS 类型 | Kotlin 类型 | C# 类型 |
|-----------|-----------|------------|----------|
| `i8` | `number` | `Byte` | `sbyte` |
| `u8` | `number` | `Byte` | `byte` |
| `i16` | `number` | `Short` | `short` |
| `u16` | `number` | `Short` | `ushort` |
| `i32` | `number` | `Int` | `int` |
| `u32` | `number` | `Int` | `uint` |
| `i64` | `number` | `Long` | `long` |
| `u64` | `number` | `Long` | `ulong` |
| `f32` | `number` | `Float` | `float` |
| `f64` | `number` | `Double` | `double` |
| `bool` | `boolean` | `Boolean` | `bool` |
| `String`, `&str` | `string` | `String` | `string` |
| `Vec<u8>` | `Uint8Array` | `ByteArray` | `byte[]` |
| `Option<T>` | `T \| null` | `T?` | `T?` |
| `Vec<T>` | `T[]` | `List<T>` | `List<T>` |
| struct | `{ field: Type }` | `data class` | `class` |
| enum | `{ tag: 'Variant', data: ... }` | `sealed class` | `abstract record` + `sealed record` |

## 错误码

系统保留错误码范围为 `-90012` 到 `-90000`。用户自定义错误不得使用此范围。

| 常量 | 值 | 描述 |
|------|-----|------|
| `CODE_SIGNAL` | -90000 | 操作系统信号（如 Ctrl+C） |
| `CODE_MSG_TOO_SHORT` | -90001 | 消息过短 |
| `CODE_PAYLOAD_MISMATCH` | -90002 | 负载长度不匹配 |
| `CODE_SERIALIZE` | -90003 | 序列化/反序列化错误 |
| `CODE_STATE_NOT_FOUND` | -90004 | State 类型未注册 |
| `CODE_HANDLER` | -90005 | Handler 执行错误 |
| `CODE_INVALID_PARAM` | -90006 | 无效参数 |
| `CODE_IO` | -90007 | I/O 错误 |
| `CODE_WS` | -90008 | WebSocket 错误 |
| `CODE_HTTP` | -90009 | HTTP 错误 |
| `CODE_TCP` | -90010 | TCP 错误 |
| `CODE_LONG_CONNECTION_NOT_SUPPORTED` | -90011 | HTTP 模式不支持长连接 |
| `CODE_RATE_LIMITED` | -90012 | 超出速率限制 |

```rust
// 自定义错误（错误码必须在保留范围之外）
return Err(afast::Error::custom(400, "invalid request parameter"));
```

## 自定义错误类型

实现 `AFastError` trait 即可定义自己的错误类型，handler 直接返回 `Result<T, MyError>`：

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

所有 handler 宏（`#[handler]`、`#[get]`/`#[post]`/`#[put]`/`#[delete]`、`#[ws]`、`#[sse]`）均支持自定义错误类型。`afast::Result<T>` 默认使用 `Error`，完全向后兼容。
