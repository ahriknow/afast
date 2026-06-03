# Project Structure

```
afast/           — Main framework crate (core types, State, transports, code generation)
afast-macros/    — Proc macros (#[handler], register!, #[derive(Tag)])
example/         — Example project (full usage including HTTP, WS, TCP, docs)
```

## Dependencies

- `afast` → `afast-macros`, `afastdata`, `tokio`
- `afast-macros` → `syn`, `quote`, `proc-macro2`
- User crates indirectly depend on `afastdata-core` (referenced by `#[derive(Tag)]` expanded code)
