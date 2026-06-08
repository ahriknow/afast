# Conditional Serialization (Marker)

When the `marker` feature is enabled, `AFast::marker()` sets a global marker string (default `"afast"`) passed to afastdata's `to_bytes_with` / `from_bytes_with`. Fields annotated with `#[afast(skip_with("marker"))]` are conditionally skipped during serialization/deserialization based on the active marker.

## Skip Modes

- **`#[afast(skip)]`** — Field is always skipped, never serialized/deserialized. Must have a `Default` impl or initialization function.
- **`#[afast(skip_with("marker"))]`** — Skipped when the marker matches; serialized normally otherwise.

The marker propagates recursively into nested types (`Vec<T>`, `Option<T>`, etc.).

Generated client code (TS/JS/KT/RS) and API docs automatically exclude skipped fields.

## Example

```rust
#[derive(AFastSerialize, AFastDeserialize, Tag)]
#[tag("User info")]
struct User {
    name: String,
    #[afast(skip)]
    internal_secret: String,        // always skipped
    #[afast(skip_with("afast"))]
    internal_note: String,          // skipped when marker is "afast"
}

let app = AFast::new()
    .marker("afast")  // set marker; default is already "afast"
    .service(svc)
    .http("0.0.0.0:5000");
```

Without the `marker` feature, `serialize` / `deserialize` use plain `to_bytes` / `from_bytes` and all fields are always included. However, `#[afast(skip)]` fields are still excluded from generated client code.
