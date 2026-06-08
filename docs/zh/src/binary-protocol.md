# 二进制协议

## 类型映射

| Rust 类型 | TS/JS 类型 | Kotlin 类型 |
|-----------|-----------|------------|
| `i8` ~ `i64`, `u8` ~ `u64`, `f32`, `f64` | `number` | `Int`/`Long`/`Float`/`Double` |
| `bool` | `boolean` | `Boolean` |
| `String`, `&str` | `string` | `String` |
| `Vec<u8>` | `Uint8Array` | `ByteArray` |
| `Option<T>` | `T \| null` | `T?` |
| `Vec<T>` | `T[]` | `List<T>` |
| struct | `{ field: Type }` | `data class` |
| enum | `{ tag: 'Variant', data: ... }` | `sealed class` |

## 错误码

系统保留错误码范围为 `-90011` 到 `-90000`。用户自定义错误不得使用此范围。

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
