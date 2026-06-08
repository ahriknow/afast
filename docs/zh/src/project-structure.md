# 项目结构

```
afast/           — 主框架 crate（核心类型、State、传输层、代码生成）
afast-macros/    — 过程宏（#[handler]、register!、#[derive(Tag)]）
example/         — 示例项目（完整用法，包括 HTTP、WS、TCP、文档）
```

## 依赖关系

- `afast` → `afast-macros`、`afastdata`、`tokio`
- `afast-macros` → `syn`、`quote`、`proc-macro2`
- 用户 crate 间接依赖 `afastdata-core`（由 `#[derive(Tag)]` 展开代码引用）
