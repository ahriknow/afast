# Testing

## Unit Tests

```bash
cargo test --lib
```

## Integration Tests

The example project includes test clients for all supported languages:

### 1. Start the Example Server

```bash
cargo run -p example --bin example
```

This starts:
- HTTP server on port 5001
- WebSocket server on port 3001
- TCP server on port 4001
- API docs at http://localhost:5001/doc

### 2. Run Test Clients

**Rust** (TCP transport):
```bash
cargo run -p example --bin test_client
```

**JavaScript** (Node.js — fetch + ws + nodetcp):
```bash
cd client && node test_js.mjs
```

**TypeScript** (Bun — fetch + ws + buntcp):
```bash
cd client && bun run test_ts.ts
```

**Kotlin** (Gradle — HTTP + WS + TCP):
```bash
cd client/kt-test && gradle run
```

## Feature Combinations

Test that the framework compiles cleanly with various feature combinations:

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

## License

MIT
