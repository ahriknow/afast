# 核心概念

## Handler 注册

`#[handler]` 过程宏在编译时生成以下内容：

1. 原始函数保持不变
2. `HandlerMeta` — 名称、描述、参数列表、返回类型的元数据
3. `HandlerInvoker` trait 实现 — 类型擦除的调用器，反序列化参数并调用函数
4. 一个静态调用器实例 — 由 `register!` 宏引用

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

- `desc("...")` — 设置文档和 JSDoc 注释中使用的描述
- `name("...")` — 覆盖客户端方法名（默认为 Rust 函数名）
- `cache(seconds)` — 启用客户端缓存
- `rate_limit("policy")` — 将 handler 绑定到命名的速率限制策略
- 其他属性 — 作为自定义属性收集到 `HandlerMeta::attrs` 中

### 自定义属性

你可以在 handler 宏中添加任意属性。它们会作为 `Attr` 键值对收集到 `HandlerMeta::attrs` 中：

```rust
#[handler(desc("Create user"), tag("admin"), timeout(30), deprecated)]
async fn create_user(...) -> ... { ... }
```

在运行时，通过 `invoker.meta().unwrap().attrs` 读取：

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

值类型会自动推断：
- 字符串: `tag("admin")` → `AttrValue::Str("admin")`
- 整数: `timeout(30)` → `AttrValue::Int(30)`
- 布尔: `deprecated` → `AttrValue::Bool(true)`

支持两种语法：`tag("admin")` 和 `tag = "admin"`。

自定义属性也可以在钩子中通过 `RequestContext::attrs` 访问：

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

## 多 State 支持

AFast 支持注册**多个 State 类型**。`StateMap` 使用 `TypeId` 作为键，每个类型对应一个值。`State<T>` 持有 `&'static T` 引用 — 值在启动时通过 `Box::leak` 分配一次，不会在每次请求时克隆：

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

如果 handler 引用了未注册的 State 类型，运行时会返回 `CODE_STATE_NOT_FOUND` 错误。

### 内部可变性

由于 `State<T>` 提供的是共享的 `&'static T` 引用，修改需要使用内部可变性模式。将可变字段包装在 `Arc<Mutex<...>>` 或 `Arc<RwLock<...>>` 中：

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

`State<T>` 不再要求 `T: Clone` — 只需 `T: 'static`。

## 多 Data 参数

Handler 可以接受**多个 `Data<T>` 参数**，从二进制负载中按顺序反序列化：

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

生成的 TypeScript 客户端方法签名：

```typescript
async searchUsers(page: PageRequest, filter: FilterRequest): Promise<PageResponse>
```

## 自定义错误类型

`afast::Result<T>` 默认错误类型为 `Error`，但所有 handler 宏均支持自定义错误类型。实现 `AFastError` trait 即可：

```rust
use afast::AFastError;

enum AppError {
    NotFound { resource: String },
    Forbidden { reason: String },
}

impl AFastError for AppError {
    fn code(&self) -> i64 {
        match self {
            AppError::NotFound { .. } => 404,
            AppError::Forbidden { .. } => 403,
        }
    }

    fn message(&self) -> String {
        match self {
            AppError::NotFound { resource } => format!("{} not found", resource),
            AppError::Forbidden { reason } => reason.clone(),
        }
    }
}
```

然后在 handler 中使用 `afast::Result<T, AppError>`：

```rust
#[handler(desc("Get user"))]
async fn get_user(id: Data<UserId>) -> afast::Result<UserInfo, AppError> {
    let user = find_user(&id).ok_or(AppError::NotFound { resource: "user".into() })?;
    Ok(user)
}
```

适用于所有宏：`#[handler]`、`#[get]`/`#[post]`/`#[put]`/`#[delete]`、`#[ws]`、`#[sse]`。

使用 `afast::Result<T>`（不指定第二个参数）等价于 `Result<T, Error>`，完全向后兼容。

## 提取器类型

| 提取器 | 描述 | 协议 |
|--------|------|------|
| `State<T>` | 从 StateMap 按类型注入共享状态 (T: 'static, 零拷贝 &'static T) | 所有 |
| `Ctx<T>` | 注入钩子设置的请求上下文数据 (T: Clone) | 所有 |
| `Data<T>` | 从二进制负载反序列化请求体 | HTTP/WS/TCP |
| `Custom<T>` | 反序列化客户端自定义上下文（如认证令牌） | HTTP/WS/TCP |
| `Receiver` | 接收来自客户端的二进制消息（长连接） | WS/TCP |
| `Sender` | 向客户端发送二进制消息（长连接） | WS/TCP |
| `Query<T>` | 从 URL 查询字符串反序列化（需要 `ordinary-http`） | HTTP |
| `Param<T>` | 从路由路径参数 (`:id`) 反序列化（需要 `ordinary-http`） | HTTP |
| `Body<T>` | 从 HTTP JSON 请求体反序列化（需要 `ordinary-http`） | HTTP |
| `Header<T>` | 从 HTTP 请求头反序列化（需要 `ordinary-http`） | HTTP |

## 服务与嵌套

`service!` 宏通过 `group` 构建 handler 树，实现命名空间管理：

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
        h(chat),  // 使用 Receiver/Sender 的持久连接
    }),
});
```

客户端命名空间路径变为 `api.user.list_users`、`api.chat.chat` 等。

在 `group` 内可以混合使用二进制和 ordinary HTTP 路由：

```rust
group("user" => {
    h(get_user),                 // 二进制 handler
    get(":id", get_user_by_id),  // GET /user/:id
    post("", create_user),       // POST /user
    delete(":id", delete_user),  // DELETE /user/:id
}),
```

### 同名服务合并

注册多个同名服务时，后续的 handler 和路由会自动合并到第一个服务中：

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
    .service(user_extra_svc);  // 合并到 "api"
```

### 空名称服务

名称为空字符串 (`""`) 的服务注册的 handler 可通过二进制协议调用，但会从客户端代码生成和 API 文档中排除：

```rust
let internal_svc = service!("", "Internal" => {
    h(debug_info),
    get("ping", ping),
});
```

## 类型标签

`#[derive(Tag)]` 为结构体和枚举生成运行时类型元数据。代码生成器通过 `FieldMeta.structure` 函数指针递归发现嵌套类型：

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
    role: Role,           // 自动递归发现 Role 的字段
    tags: Vec<String>,    // Vec 元素类型自动展开
    avatar: Option<Vec<u8>>,
}
```

### 验证规则

| 规则 | 示例 | 描述 |
|------|------|------|
| `gt(value, code, "msg")` | `#[afast(gt(0, 400, "must > 0"))]` | 大于 |
| `gte(value, code, "msg")` | `#[afast(gte(1, 400, "must >= 1"))]` | 大于等于 |
| `lt(value, code, "msg")` | `#[afast(lt(100, 400, "must < 100"))]` | 小于 |
| `lte(value, code, "msg")` | `#[afast(lte(99, 400, "must <= 99"))]` | 小于等于 |
| `len(min, max, code, "msg")` | `#[afast(len(1, 20, 400, "len 1-20"))]` | 长度约束 |
| `of(["a","b"], code, "msg")` | `#[afast(of(["a","b"], 400, "a or b"))]` | 枚举值 |
