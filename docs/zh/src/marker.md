# 条件序列化 (Marker)

当启用 `marker` Feature 时，`AFast::marker()` 设置一个全局标记字符串（默认 `"afast"`），传递给 afastdata 的 `to_bytes_with` / `from_bytes_with`。使用 `#[afast(skip_with("marker"))]` 标注的字段会根据当前激活的标记在序列化/反序列化时被条件跳过。

## 跳过模式

- **`#[afast(skip)]`** — 字段始终跳过，永不序列化/反序列化。必须有 `Default` 实现或初始化函数。
- **`#[afast(skip_with("marker"))]`** — 当标记匹配时跳过；否则正常序列化。

标记会递归传播到嵌套类型（`Vec<T>`、`Option<T>` 等）。

生成的客户端代码（TS/JS/KT/RS）和 API 文档会自动排除被跳过的字段。

## 示例

```rust
#[derive(AFastSerialize, AFastDeserialize, Tag)]
#[tag("User info")]
struct User {
    name: String,
    #[afast(skip)]
    internal_secret: String,        // 始终跳过
    #[afast(skip_with("afast"))]
    internal_note: String,          // 当 marker 为 "afast" 时跳过
}

let app = AFast::new()
    .marker("afast")  // 设置标记；默认已经是 "afast"
    .service(svc)
    .http("0.0.0.0:5000");
```

不启用 `marker` Feature 时，`serialize` / `deserialize` 使用普通的 `to_bytes` / `from_bytes`，所有字段始终包含。但 `#[afast(skip)]` 字段仍会从生成的客户端代码中排除。
