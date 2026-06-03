# Interactive Documentation

With the `doc` feature, visit `http://host:port/doc` for interactive API docs:

```rust
let app = AFast::new()
    .service(svc)
    .document(afast::DocConfig::new()
        .title("My API Documentation")
        .output("./docs")  // Also write static HTML to disk
    )
    .http("0.0.0.0:5000");
```

- `GET /doc` — Index page listing all services
- `GET /doc/{service}` — Service docs with type definitions and online test panel
- Dark/light theme toggle
- Online test panel can send real requests to the server
