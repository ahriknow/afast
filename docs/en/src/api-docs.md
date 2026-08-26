# Interactive Documentation

With the `doc` feature, visit `http://host:port/doc` for interactive API docs.

## Setup

```rust
let app = AFast::new()
    .service(svc)
    .document(afast::DocConfig::with("My API", "./docs").basic_auth("username", "password"))
    .http("0.0.0.0:5001");
```

- `GET /doc` — Index page listing all services (services starting with `_` are hidden)
- `GET /doc/{service}` — Service docs with type definitions and online test panel
- Dark/light theme toggle
- Static HTML files written to `./docs` directory

## Features

### Binary Handler Testing

Each binary handler shows a form with:
- Input fields for `Custom`, `Data`, and `State` parameters
- Send button that serializes to the binary protocol
- Response panel showing deserialized result

### Ordinary HTTP Testing

REST endpoints (`GET`, `POST`, etc.) show:
- Path parameter inputs
- Query parameter inputs
- JSON body editor
- Response status and body display

### WebSocket Debugging

WS routes (`#[ws]`) show a debugging panel with:
- Path parameter inputs (auto-detected from route pattern)
- Query parameter input
- Connect/Disconnect buttons
- Message input with Send button (Enter shortcut)
- Real-time log panel showing sent/received messages

The panel connects to the HTTP port (ordinary WS routes use HTTP upgrade).

### SSE Debugging

SSE routes (`#[sse]`) show a debugging panel with:
- Path parameter inputs
- Query parameter input
- Connect/Disconnect buttons
- Real-time event log showing named events and data

### Configuration Panel

The top-right settings panel lets you configure:
- **Transport**: `ws` (binary), `fetch` (HTTP), `tcp`
- **Host**: server hostname (default: `localhost`)
- **Port**: auto-detected from server config
- **TLS**: enable secure connections

## Service Visibility

Services with names starting with `_` (underscore) are hidden from the doc index page but still accessible via direct URL (`/doc/_service`). Useful for internal-only endpoints.
