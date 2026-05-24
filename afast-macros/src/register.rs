use proc_macro2::{Span, TokenStream};
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
/// auto-generated function `__ordinary_entry_name()`, which returns an
/// `OrdinaryHandlerDef`.
///
/// The entry function is produced by `#[get]`, `#[post]`, and similar
/// HTTP-method macros, and includes both the handler entry and the
/// ordinary invoker reference.
pub fn expand_ordinary(input: TokenStream) -> syn::Result<TokenStream> {
    let ident: syn::Ident = syn::parse2(input)?;
    let fn_name = syn::Ident::new(&format!("__ordinary_entry_{}", ident), Span::call_site());

    Ok(quote! {
        #fn_name()
    })
}
