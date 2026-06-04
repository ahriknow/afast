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

/// Expands a `register_with_path!(name, "service_name")` invocation into code
/// that calls the auto-generated entry function `name()` and computes a stable
/// handler ID as `fnv1a_32(service_name + "/" + fn_name)`.
///
/// The stable ID is set on the returned `HandlerEntry` via `set_stable_id`.
pub fn expand_with_path(input: TokenStream) -> syn::Result<TokenStream> {
    let args: RegisterArgs = syn::parse2(input)?;
    let path = &args.path;
    let service_name = &args.service_name;
    let fn_name = path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
    let full_path = format!("{}/{}", service_name.value(), fn_name);
    let hash = fnv1a_32(full_path.as_bytes());
    Ok(quote! {{
        let mut __entry = #path();
        __entry.set_stable_id(#hash);
        __entry
    }})
}

struct RegisterArgs {
    path: syn::Path,
    service_name: syn::LitStr,
}

impl syn::parse::Parse for RegisterArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let path: syn::Path = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let service_name: syn::LitStr = input.parse()?;
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
