# Core Concepts

## Handler Registration

The `#[handler]` proc macro generates the following at compile time:

1. The original function unchanged
2. `HandlerMeta` — name, description, parameter list, return type metadata
3. `HandlerInvoker` trait impl — type-erased invoker, deserializes params, calls the function
4. A static invoker instance — referenced by the `register!` macro

```rust
#[handler(desc("Get user"), name("get_user"))]
async fn get_user_handler(
    state: State<AppState>,
    auth: Custom<Auth>,
    req: Data<UserIdRequest>,
) -> Result<UserInfo> {
    // ...
}
```

- `desc("...")` — Sets description used in docs and JSDoc comments
- `name("...")` — Overrides the client-side method name (defaults to the Rust function name)
- `cache(seconds)` — Enables client-side caching
- `rate_limit("policy")` — Binds the handler to a named rate-limit policy
- Any other attribute — Collected as custom attributes in `HandlerMeta::attrs`

### Custom Attributes

You can add arbitrary attributes to handler macros. They are collected into `HandlerMeta::attrs` as `Attr` key-value pairs:

```rust
#[handler(desc("Create user"), tag("admin"), timeout(30), deprecated)]
async fn create_user(...) -> ... { ... }
```

At runtime, read them via `invoker.meta().unwrap().attrs`:

```rust
if let Some(meta) = invoker.meta() {
    for attr in meta.attrs {
        match attr.value {
            AttrValue::Str(v) => println!("{} = {}", attr.key, v),
            AttrValue::Int(v) => println!("{} = {}", attr.key, v),
            AttrValue::Bool(v) => println!("{} = {}", attr.key, v),
        }
    }
}
```

Value type is inferred automatically:
- String: `tag("admin")` → `AttrValue::Str("admin")`
- Integer: `timeout(30)` → `AttrValue::Int(30)`
- Boolean: `deprecated` → `AttrValue::Bool(true)`

Both syntaxes are supported: `tag("admin")` and `tag = "admin"`.

Custom attributes are also available in hooks via `RequestContext::attrs`:

```rust
impl Hook for MyHook {
    fn before_request(&self, ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        for attr in ctx.attrs {
            if attr.key == "deprecated" {
                eprintln!("WARNING: {} is deprecated", ctx.handler_name);
            }
        }
        None
    }
}
```

## Multiple States

AFast supports registering **multiple State types**. `StateMap` uses `TypeId` as keys, with one value per type. `State<T>` holds a `&'static T` reference — the value is allocated once at startup via `Box::leak` and never cloned per-request:

```rust
struct DbConfig { url: String }
struct RedisConfig { url: String }
struct AppConfig { name: String }

let app = AFast::new()
    .state(DbConfig { url: "postgres://...".into() })
    .state(RedisConfig { url: "redis://...".into() })
    .state(AppConfig { name: "my-app".into() });

#[handler(desc("Multiple State example"))]
async fn my_handler(
    db: State<DbConfig>,
    redis: State<RedisConfig>,
    config: State<AppConfig>,
) -> Result<()> {
    println!("DB: {}, Redis: {}, App: {}", db.url, redis.url, config.name);
    Ok(())
}
```

If a handler references a State type that was not registered, it returns a `CODE_STATE_NOT_FOUND` error at runtime.

### Interior Mutability

Since `State<T>` provides a shared `&'static T` reference, mutations require interior mutability patterns. Wrap mutable fields in `Arc<Mutex<...>>` or `Arc<RwLock<...>>`:

```rust
use std::sync::{Arc, Mutex};

struct AppState {
    db: Arc<Mutex<Database>>,
    counter: Arc<Mutex<u64>>,
}

#[handler(desc("Increment counter"))]
async fn increment(state: State<AppState>) -> Result<()> {
    let mut count = state.counter.lock().unwrap();
    *count += 1;
    Ok(())
}
```

`State<T>` no longer requires `T: Clone` — only `T: 'static`.

## Multiple Data Params

A handler can accept **multiple `Data<T>` parameters**, deserialized sequentially from the binary payload:

```rust
#[derive(AFastDeserialize, Tag)]
#[tag("Pagination")]
struct PageRequest { page: i64, size: i64 }

#[derive(AFastDeserialize, Tag)]
#[tag("Filter")]
struct FilterRequest { keyword: String, status: i32 }

#[handler(desc("Search users"))]
async fn search_users(
    page: Data<PageRequest>,
    filter: Data<FilterRequest>,
) -> Result<PageResponse> {
    // ...
}
```

Generated TypeScript client method signature:

```typescript
async searchUsers(page: PageRequest, filter: FilterRequest): Promise<PageResponse>
```

## Extractor Types

| Extractor | Description | Protocols |
|-----------|-------------|-----------|
| `State<T>` | Injects shared state by type from StateMap (T: 'static, zero-copy &'static T) | All |
| `Ctx<T>` | Injects per-request context data set by hooks (T: Clone) | All |
| `Data<T>` | Deserializes request body from binary payload | HTTP/WS/TCP |
| `Custom<T>` | Deserializes client-side custom context (e.g., auth token) | HTTP/WS/TCP |
| `Receiver` | Receives binary messages from the client (long connection) | WS/TCP |
| `Sender` | Sends binary messages to the client (long connection) | WS/TCP |
| `Query<T>` | Deserializes from URL query string (requires `ordinary-http`) | HTTP |
| `Param<T>` | Deserializes from route path params (`:id`) (requires `ordinary-http`) | HTTP |
| `Body<T>` | Deserializes from HTTP JSON body (requires `ordinary-http`) | HTTP |
| `Header<T>` | Deserializes from HTTP request headers (requires `ordinary-http`) | HTTP |

## Services and Nesting

The `service!` macro builds handler trees with `group` for namespacing:

```rust
let api_svc = service!("api", "User API" => {
    h(health),
    group("user" => {
        h(list_users),
        h(get_user),
        group("posts" => {
            h(list_posts),
        }),
    }),
    group("chat" => {
        h(chat),  // Persistent connection using Receiver/Sender
    }),
});
```

Client namespace paths become `api.user.list_users`, `api.chat.chat`, etc.

Binary and ordinary HTTP routes can be mixed within a `group`:

```rust
group("user" => {
    h(get_user),                 // binary handler
    get(":id", get_user_by_id),  // GET /user/:id
    post("", create_user),       // POST /user
    delete(":id", delete_user),  // DELETE /user/:id
}),
```

### Service Merge on Duplicate Name

Registering multiple services with the same name automatically merges the later handlers and routes into the first service:

```rust
let user_svc = service!("api", "User API" => {
    h(list_users),
    h(create_user),
});

let user_extra_svc = service!("api" => {
    h(delete_user),
    get(":id", get_user_http),
});

let app = AFast::new()
    .service(user_svc)
    .service(user_extra_svc);  // merge into "api"
```

### Empty-Name Service

A service with an empty string name (`""`) registers handlers callable via binary protocol, but excluded from client code generation and API documentation:

```rust
let internal_svc = service!("", "Internal" => {
    h(debug_info),
    get("ping", ping),
});
```

## Type Tags

`#[derive(Tag)]` generates runtime type metadata for structs and enums. The code generator recursively discovers nested types through `FieldMeta.structure` function pointers:

```rust
use afast::Tag;

#[derive(Tag)]
#[tag("User role")]
enum Role {
    Admin,
    User { level: i32 },
    Guest { expires_at: i64 },
    Custom(String),
}

#[derive(Tag)]
#[tag("User info")]
struct User {
    name: String,
    role: Role,           // Auto-discovers Role fields recursively
    tags: Vec<String>,    // Vec element type auto-expanded
    avatar: Option<Vec<u8>>,
}
```

### Validation Rules

| Rule | Example | Description |
|------|---------|-------------|
| `gt(value, code, "msg")` | `#[afast(gt(0, 400, "must > 0"))]` | Greater than |
| `gte(value, code, "msg")` | `#[afast(gte(1, 400, "must >= 1"))]` | Greater or equal |
| `lt(value, code, "msg")` | `#[afast(lt(100, 400, "must < 100"))]` | Less than |
| `lte(value, code, "msg")` | `#[afast(lte(99, 400, "must <= 99"))]` | Less or equal |
| `len(min, max, code, "msg")` | `#[afast(len(1, 20, 400, "len 1-20"))]` | Length constraint |
| `of(["a","b"], code, "msg")` | `#[afast(of(["a","b"], 400, "a or b"))]` | Enum of values |
