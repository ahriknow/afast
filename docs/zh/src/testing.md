# 测试

## 单元测试

```bash
cargo test --lib
```

## 集成测试

示例项目包含所有支持语言的测试客户端：

### 1. 启动示例服务器

```bash
cargo run -p example --bin example
```

这将启动：
- HTTP 服务器，端口 5001
- WebSocket 服务器，端口 3001
- TCP 服务器，端口 4001
- API 文档: http://localhost:5001/doc

### 2. 运行测试客户端

**Rust** (TCP 传输)：
```bash
cargo run -p example --bin test_client
```

**JavaScript** (Node.js — fetch + ws + nodetcp)：
```bash
cd client && node test_js.mjs
```

**TypeScript** (Bun — fetch + ws + buntcp)：
```bash
cd client && bun run test_ts.ts
```

**Kotlin** (Gradle — HTTP + WS + TCP)：
```bash
cd client/kt-test && gradle run
```

## Feature 组合

测试框架在各种 Feature 组合下能否正常编译：

```bash
cargo check
cargo check --features hook
cargo check --features rate-limit
cargo check --features ordinary-http
cargo check --features ordinary-ws
cargo check --features ordinary-sse
cargo check --features "ordinary-http,ordinary-ws,ordinary-sse,hook,rate-limit"
cargo check --all-features
```

## Clippy

```bash
cargo clippy --all-features
```

## 许可证

MIT
