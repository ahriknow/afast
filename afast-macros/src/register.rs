use proc_macro2::TokenStream;
use quote::quote;

/// FNV-1a 32-bit hash — deterministic, zero-dependency, const-friendly.
const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

const fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

/// Expands a `register!(name)` invocation into a call to the auto-generated
/// entry function `name()`, which returns a `HandlerEntry`.
///
/// The caller is expected to provide a path (e.g. `health` or `api::health`)
/// that resolves to an entry function produced by the `#[handler]` macro.
pub fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let path: syn::Path = syn::parse2(input)?;
    Ok(quote! {
        #path()
    })
}

/// Expands a `register_with_path!(name, service_name)` invocation into code
/// that calls the auto-generated entry function `name()` and computes a stable
/// handler ID as `fnv1a_32(service_name + "/" + fn_name)`.
///
/// The stable ID is set on the returned `HandlerEntry` via `set_stable_id`.
///
/// `service_name` can be a string literal (compile-time hash) or any expression
/// that evaluates to `&str` / `String` (runtime hash via inline const fn).
pub fn expand_with_path(input: TokenStream) -> syn::Result<TokenStream> {
    let args: RegisterArgs = syn::parse2(input)?;
    let path = &args.path;
    let fn_name = path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();

    match &args.service_name {
        ServiceName::Lit(lit) => {
            // 字面量：编译期计算哈希（零运行时开销）
            let full_path = format!("{}/{}", lit.value(), fn_name);
            let hash = fnv1a_32(full_path.as_bytes());
            Ok(quote! {{
                let mut __entry = #path();
                __entry.set_stable_id(#hash);
                __entry
            }})
        }
        ServiceName::Expr(expr) => {
            // 表达式：内联 const fn 在运行时计算哈希
            let fn_name_str = fn_name.as_str();
            Ok(quote! {{
                const fn __afast_fnv1a_32(bytes: &[u8]) -> u32 {
                    let mut hash: u32 = 0x811c_9dc5u32;
                    let mut i = 0;
                    while i < bytes.len() {
                        hash ^= bytes[i] as u32;
                        hash = hash.wrapping_mul(0x0100_0193u32);
                        i += 1;
                    }
                    hash
                }
                let __svc_name: &str = &#expr;
                let __fn_name: &str = #fn_name_str;
                let mut __buf = ::std::vec::Vec::with_capacity(
                    __svc_name.len() + 1 + __fn_name.len()
                );
                __buf.extend_from_slice(__svc_name.as_bytes());
                __buf.push(b'/');
                __buf.extend_from_slice(__fn_name.as_bytes());
                let __hash = __afast_fnv1a_32(&__buf);
                let mut __entry = #path();
                __entry.set_stable_id(__hash);
                __entry
            }})
        }
    }
}

enum ServiceName {
    Lit(syn::LitStr),
    Expr(syn::Expr),
}

struct RegisterArgs {
    path: syn::Path,
    service_name: ServiceName,
}

impl syn::parse::Parse for RegisterArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let path: syn::Path = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        // 先尝试解析为字符串字面量（编译期哈希路径）
        let service_name = if let Ok(lit) = input.parse::<syn::LitStr>() {
            ServiceName::Lit(lit)
        } else {
            // 否则作为通用表达式（运行时哈希路径）
            ServiceName::Expr(input.parse::<syn::Expr>()?)
        };
        Ok(RegisterArgs { path, service_name })
    }
}

/// Expands a `register_ordinary!(name)` invocation into a call to the
/// auto-generated public entry function `name()`, which returns a
/// `HandlerEntry`.
///
/// The entry function is produced by `#[get]`, `#[post]`, and similar
/// HTTP-method macros. As of v0.1.1 the ordinary invoker is embedded
/// directly in the `HandlerEntry`, so the hidden `__ordinary_entry_*`
/// function is no longer needed.
pub fn expand_ordinary(input: TokenStream) -> syn::Result<TokenStream> {
    let path: syn::Path = syn::parse2(input)?;

    Ok(quote! {
        #path()
    })
}

/// Expands a `register_ws!(name)` invocation into a call to the
/// auto-generated entry function `name()`, which returns a
/// `(&'static dyn WsHandlerInvoker, &'static str)` tuple.
pub fn expand_ws(input: TokenStream) -> syn::Result<TokenStream> {
    let path: syn::Path = syn::parse2(input)?;

    Ok(quote! {
        #path()
    })
}

/// Expands a `register_sse!(name)` invocation into a call to the
/// auto-generated entry function `name()`, which returns a
/// `(&'static dyn SseHandlerInvoker, &'static str)` tuple.
pub fn expand_sse(input: TokenStream) -> syn::Result<TokenStream> {
    let path: syn::Path = syn::parse2(input)?;

    Ok(quote! {
        #path()
    })
}
