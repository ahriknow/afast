mod handler;
mod register;
mod tag;

use proc_macro::TokenStream;

/// Marks an async function as a binary-protocol handler.
///
/// Expands to produce the handler metadata (`__META_<name>`), invoker struct
/// (`__Invoker_<name>`), invoker const (`__INVOKER_<name>`), and a
/// registration entry function named after the handler.
///
/// `desc`: A required string literal describing the handler's purpose, used in documentation and metadata.
/// `name`: An optional string literal specifying the handler's name. If omitted, the function name is used.
/// `cache`: An optional integer literal specifying the cache duration in seconds.
///
/// ```no_run
/// use afast::handler;
///
/// #[handler(desc("Health check endpoint"), cache(60))]
/// async fn health() -> afast::Result<String> {
///     Ok("ok".to_string())
/// }
/// ```
#[proc_macro_attribute]
pub fn handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(attr.into(), item.into(), None)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as an HTTP GET handler.
///
/// Generates an ordinary invoker for HTTP request routing alongside a dummy
/// binary invoker that returns an error when called through the binary protocol.
///
/// ```no_run
/// use afast::get;
///
/// #[get(desc("Fetch user by ID"))]
/// async fn get_user() -> afast::HttpResult<String> {
///     Ok("user data".to_string())
/// }
/// ```
#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(attr.into(), item.into(), Some("GET"))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as an HTTP POST handler.
///
/// Generates an ordinary invoker for HTTP request routing alongside a dummy
/// binary invoker that returns an error when called through the binary protocol.
///
/// ```no_run
/// use afast::post;
///
/// #[post(desc("Create a resource"))]
/// async fn create() -> afast::HttpResult<String> {
///     Ok("created".to_string())
/// }
/// ```
#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(attr.into(), item.into(), Some("POST"))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as an HTTP PUT handler.
///
/// Generates an ordinary invoker for HTTP request routing alongside a dummy
/// binary invoker that returns an error when called through the binary protocol.
///
/// ```no_run
/// use afast::put;
///
/// #[put(desc("Replace a resource"))]
/// async fn replace() -> afast::HttpResult<String> {
///     Ok("replaced".to_string())
/// }
/// ```
#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(attr.into(), item.into(), Some("PUT"))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as an HTTP PATCH handler.
///
/// Generates an ordinary invoker for HTTP request routing alongside a dummy
/// binary invoker that returns an error when called through the binary protocol.
///
/// ```no_run
/// use afast::patch;
///
/// #[patch(desc("Partially update a resource"))]
/// async fn patch() -> afast::HttpResult<String> {
///     Ok("patched".to_string())
/// }
/// ```
#[proc_macro_attribute]
pub fn patch(attr: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(attr.into(), item.into(), Some("PATCH"))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as an HTTP DELETE handler.
///
/// Generates an ordinary invoker for HTTP request routing alongside a dummy
/// binary invoker that returns an error when called through the binary protocol.
///
/// ```no_run
/// use afast::delete;
///
/// #[delete(desc("Remove a resource"))]
/// async fn remove() -> afast::HttpResult<String> {
///     Ok("deleted".to_string())
/// }
/// ```
#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(attr.into(), item.into(), Some("DELETE"))
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks an async function as a WebSocket route handler.
///
/// Generates a `WsHandlerInvoker` implementation for path-based WebSocket
/// endpoints. The handler receives `WsQuery<T>`, `WsParam<T>`, and `WsStream`
/// extractors and returns `Result<()>`.
///
/// ```no_run
/// use afast::ws;
///
/// #[ws(desc("Chat handler"))]
/// async fn chat(stream: afast::WsStream) -> afast::Result<()> {
///     Ok(())
/// }
/// ```
#[proc_macro_attribute]
pub fn ws(attr: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand_ws(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Produces a `HandlerEntry` for the given handler function.
///
/// `register!(name)` expands to a call to the auto-generated entry function
/// `name()`, which returns a `HandlerEntry` containing the handler metadata and
/// invoker reference.
///
/// ```no_run
/// use afast::register;
///
/// let entry = register!(health);
/// ```
#[proc_macro]
pub fn register(input: TokenStream) -> TokenStream {
    register::expand(input.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Produces an `OrdinaryHandlerDef` for the given ordinary HTTP handler.
///
/// `register_ordinary!(name)` expands to a call to `__ordinary_entry_name()`,
/// which returns an `OrdinaryHandlerDef` containing both the handler entry and
/// the ordinary invoker reference.
///
/// ```no_run
/// use afast::register_ordinary;
///
/// let def = register_ordinary!(get_user);
/// ```
#[proc_macro]
pub fn register_ordinary(input: TokenStream) -> TokenStream {
    register::expand_ordinary(input.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Produces a WsHandlerDef for the given WebSocket handler.
///
/// `register_ws!(name)` expands to a call to the auto-generated entry function
/// `name()`, which returns a `(&'static dyn WsHandlerInvoker, &'static str)` tuple.
#[proc_macro]
pub fn register_ws(input: TokenStream) -> TokenStream {
    register::expand_ws(input.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Derives the `Structure` trait for a struct or enum.
///
/// Generates runtime type metadata (`TagMeta`) and implements `afast::Structure`,
/// which provides a `structure()` method returning a static reference to the
/// type descriptor. This metadata is consumed by the code generator and
/// validation layer.
///
/// ```no_run
/// use afast::Tag;
///
/// #[derive(Tag)]
/// #[tag("User profile data")]
/// struct User {
///     id: i64,
///     name: String,
/// }
/// ```
#[proc_macro_derive(Tag, attributes(tag, afast))]
pub fn derive_tag(input: TokenStream) -> TokenStream {
    tag::expand(input.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
