use proc_macro2::TokenStream;
use quote::quote;

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
