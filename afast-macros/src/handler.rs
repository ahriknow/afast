use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::{Expr, ExprLit};
use syn::{FnArg, ItemFn, Lit, LitInt, LitStr, Meta, Pat, Type};

/// Describes a single handler parameter discovered during signature analysis.
struct ParamInfo {
    /// The parameter name as it appears in the function signature.
    name: String,
    /// The extractor kind: `State`, `Custom`, `Data`, `Receiver`, `Sender`,
    /// or for ordinary handlers: `Query`, `Param`, `Body`, `Header`.
    extractor: String,
    /// String representation of the inner type (e.g. `"User"` for `Custom<User>`).
    ty_str: String,
    /// The inner type parsed from the extractor's generic argument.
    inner_type: Type,
    /// The full type as written in the function signature (e.g. `Custom<User>`).
    full_type: Type,
}

/// Entry point for the `#[handler]` and `#[get]`/`#[post]`/etc. proc-macro
/// attributes.
///
/// When `method` is `Some`, an ordinary HTTP handler is generated (with an
/// `OrdinaryHandlerInvoker` implementation). When `None`, a binary-protocol
/// handler is generated.
///
/// The expansion produces:
/// - The renamed implementation function (`__impl_<name>`).
/// - A `HandlerMeta` const describing the handler's interface.
/// - An invoker struct implementing `HandlerInvoker` (and optionally
///   `OrdinaryHandlerInvoker`).
/// - Entry functions for registration (`<name>()` and optionally
///   `__ordinary_entry_<name>()`).
pub fn expand(
    attr: TokenStream,
    item: TokenStream,
    method: Option<&str>,
) -> syn::Result<TokenStream> {
    let input_fn: ItemFn = syn::parse2(item)?;
    let fn_name = &input_fn.sig.ident;
    let fn_name_str = fn_name.to_string();

    let (desc, api_name, cache_seconds, rate_limit_policy) =
        parse_handler_attrs_from_tokens(&attr)?;
    let params = parse_handler_params(&input_fn, method)?;

    // Detect whether the handler mixes binary and ordinary extractors, which is
    // unsupported because the two transport layers use incompatible calling
    // conventions and dispatch paths.
    let has_ordinary = params
        .iter()
        .any(|p| matches!(p.extractor.as_str(), "Query" | "Param" | "Body" | "Header"));

    let has_binary = params.iter().any(|p| {
        matches!(
            p.extractor.as_str(),
            "Custom" | "Data" | "Receiver" | "Sender"
        )
    });

    if has_ordinary && has_binary {
        return Err(syn::Error::new(
            Span::call_site(),
            "cannot mix ordinary extractors (Query/Param/Body/Header) with binary extractors (Custom/Data/Receiver/Sender)",
        ));
    }

    // Long-connection handlers use channel-based communication (Receiver/Sender)
    // and are only meaningful over WebSocket/TCP transports.
    let long_connection = params
        .iter()
        .any(|p| p.extractor == "Receiver" || p.extractor == "Sender");

    if long_connection && method.is_some() {
        return Err(syn::Error::new(
            Span::call_site(),
            "long-connection handlers (Receiver/Sender) are not supported for ordinary HTTP routes",
        ));
    }

    // Receiver and Sender form a matched pair for bidirectional streaming;
    // exactly one of each must appear.
    let receiver_count = params.iter().filter(|p| p.extractor == "Receiver").count();
    let sender_count = params.iter().filter(|p| p.extractor == "Sender").count();
    if receiver_count != sender_count {
        return Err(syn::Error::new(
            Span::call_site(),
            "Receiver and Sender must appear together and at most once each",
        ));
    }

    // Enforce mutual exclusion: ordinary extractors only on HTTP-method macros,
    // binary extractors only on #[handler].
    if method.is_none() && has_ordinary {
        return Err(syn::Error::new(
            Span::call_site(),
            "ordinary extractors (Query/Param/Body/Header) are only allowed with #[get], #[post], etc., not #[handler]",
        ));
    }
    if method.is_some() && has_binary {
        return Err(syn::Error::new(
            Span::call_site(),
            "binary extractors (Custom/Data/Receiver/Sender) are not allowed with #[get], #[post], etc.",
        ));
    }

    let return_type_str = extract_return_type(&input_fn)?;
    let return_type_syn = extract_return_type_syn(&input_fn);

    // Determine the return type's `Structure` function pointer so the code
    // generator can recursively discover nested complex types.
    let return_structure = return_type_syn.as_ref().and_then(|ty| {
        let ident = extract_outermost_ident(ty);
        let is_primitive = matches!(
            ident.as_deref(),
            Some(
                "i8" | "i16"
                    | "i32"
                    | "i64"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "f32"
                    | "f64"
                    | "bool"
                    | "String"
            )
        );
        let is_response_wrapper = matches!(
            ident.as_deref(),
            Some("Json" | "Text" | "Html" | "File" | "Status" | "Redirect")
        );
        let is_container = matches!(ident.as_deref(), Some("Vec" | "Option"));
        let ty_str = type_to_string(ty);
        let is_unit = ty_str.trim() == "()";
        if is_primitive || is_unit {
            None
        } else if is_container {
            // Unwrap the first level of Vec<T> or Option<T>. If the inner type
            // is complex (non-primitive), emit a structure pointer to it.
            if let Ok(inner_ty) = extract_generic_inner(ty) {
                let inner_ident = extract_outermost_ident(&inner_ty);
                let inner_is_primitive = matches!(
                    inner_ident.as_deref(),
                    Some(
                        "i8" | "i16"
                            | "i32"
                            | "i64"
                            | "u8"
                            | "u16"
                            | "u32"
                            | "u64"
                            | "f32"
                            | "f64"
                            | "bool"
                            | "String"
                    )
                );
                if !inner_is_primitive {
                    return Some(quote! { || <#inner_ty as afast::Structure>::structure() });
                }
                // Double-wrapped containers (e.g. Vec<Option<User>>,
                // Option<Vec<User>>) need one more level of unwrapping.
                if (inner_ident.as_deref() == Some("Vec")
                    || inner_ident.as_deref() == Some("Option"))
                    && let Ok(nested) = extract_generic_inner(&inner_ty)
                {
                    let nested_ident = extract_outermost_ident(&nested);
                    let nested_is_primitive = matches!(
                        nested_ident.as_deref(),
                        Some(
                            "i8" | "i16"
                                | "i32"
                                | "i64"
                                | "u8"
                                | "u16"
                                | "u32"
                                | "u64"
                                | "f32"
                                | "f64"
                                | "bool"
                                | "String"
                        )
                    );
                    if !nested_is_primitive {
                        return Some(quote! { || <#nested as afast::Structure>::structure() });
                    }
                }
            }
            None
        } else if is_response_wrapper {
            // For response wrappers like Json<T>, extract the inner type's
            // structure. Only Json<T> carries user-defined types; Text, Html,
            // File, Status, and Redirect are opaque wrappers.
            if ident.as_deref() == Some("Json")
                && let Ok(inner_ty) = extract_generic_inner(ty)
            {
                let inner_ident = extract_outermost_ident(&inner_ty);
                let inner_is_primitive = matches!(
                    inner_ident.as_deref(),
                    Some(
                        "i8" | "i16"
                            | "i32"
                            | "i64"
                            | "u8"
                            | "u16"
                            | "u32"
                            | "u64"
                            | "f32"
                            | "f64"
                            | "bool"
                            | "String"
                            | "Vec"
                            | "Option"
                    )
                );
                if !inner_is_primitive {
                    return Some(quote! { || <#inner_ty as afast::Structure>::structure() });
                }
                // Handle double-wrapped Json<Vec<User>> and Json<Option<User>>.
                if (inner_ident.as_deref() == Some("Vec")
                    || inner_ident.as_deref() == Some("Option"))
                    && let Ok(nested) = extract_generic_inner(&inner_ty)
                {
                    let nested_ident = extract_outermost_ident(&nested);
                    let nested_is_primitive = matches!(
                        nested_ident.as_deref(),
                        Some(
                            "i8" | "i16"
                                | "i32"
                                | "i64"
                                | "u8"
                                | "u16"
                                | "u32"
                                | "u64"
                                | "f32"
                                | "f64"
                                | "bool"
                                | "String"
                                | "Vec"
                                | "Option"
                        )
                    );
                    if !nested_is_primitive {
                        return Some(quote! { || <#nested as afast::Structure>::structure() });
                    }
                }
            }
            None
        } else {
            // Bare complex type; emit a structure pointer directly.
            Some(quote! { || <#ty as afast::Structure>::structure() })
        }
    });

    let param_meta_entries = build_param_meta_entries(&params);
    let meta_tokens = build_handler_meta(&HandlerMetaParams {
        fn_name_str: &fn_name_str,
        desc: &desc,
        api_name: &api_name,
        return_type_str: &return_type_str,
        return_structure,
        long_connection,
        method: method.unwrap_or(""),
        is_ordinary: method.is_some(),
        param_meta_entries: &param_meta_entries,
        cache_seconds,
        rate_limit_policy: &rate_limit_policy,
    });

    let invoker_ident = syn::Ident::new(&format!("__Invoker_{}", fn_name_str), Span::call_site());
    let invoker_const = syn::Ident::new(&format!("__INVOKER_{}", fn_name_str), Span::call_site());
    let meta_ident = syn::Ident::new(&format!("__META_{}", fn_name_str), Span::call_site());
    let impl_fn_name = syn::Ident::new(&format!("__impl_{}", fn_name_str), Span::call_site());

    // Rename the original user function to `__impl_<name>` so the generated
    // entry function can claim the original name for registration.
    let mut impl_fn = input_fn.clone();
    impl_fn.sig.ident = impl_fn_name.clone();

    // Emit a const type assertion so the compiler verifies the return type at
    // compile time, even though the invoker uses type-erased serialization.
    let return_type_ref = return_type_syn.as_ref().map(|ty| {
        quote! { const _: fn() -> #ty = || unreachable!(); }
    });

    if method.is_some() {
        // Ordinary HTTP handler path: generates both an OrdinaryHandlerInvoker
        // (for HTTP routing) and a dummy HandlerInvoker (to satisfy the
        // HandlerEntry type, which always requires a binary invoker).
        //
        // The ordinary invoker is embedded directly in the HandlerEntry so
        // that `register!` (or `register_ordinary!`) produces a single public
        // entry function call — no hidden `__ordinary_entry_*` symbols are
        // needed at the call site.

        let ordinary_invoker_ident = syn::Ident::new(
            &format!("__OrdinaryInvoker_{}", fn_name_str),
            Span::call_site(),
        );
        let ordinary_invoker_const = syn::Ident::new(
            &format!("__ORDINARY_INVOKER_{}", fn_name_str),
            Span::call_site(),
        );

        let ordinary_invoker_impl =
            build_ordinary_invoker_impl(&ordinary_invoker_ident, &impl_fn_name, &params);

        let dummy_invoker_impl = build_dummy_invoker_impl(&invoker_ident);

        Ok(quote! {
            #impl_fn

            #meta_tokens

            #ordinary_invoker_impl

            const #ordinary_invoker_const: #ordinary_invoker_ident = #ordinary_invoker_ident;

            #dummy_invoker_impl

            const #invoker_const: #invoker_ident = #invoker_ident;

            #return_type_ref

            pub fn #fn_name() -> afast::HandlerEntry {
                afast::HandlerEntry::with_ordinary(
                    stringify!(#fn_name),
                    &#invoker_const,
                    &#meta_ident,
                    &#ordinary_invoker_const,
                )
            }
        })
    } else {
        // Binary-protocol handler path: generates the standard HandlerInvoker
        // implementation that deserializes payload bytes and calls the user
        // function.

        let invoker_impl = build_invoker_impl(
            &invoker_ident,
            &impl_fn_name,
            &params,
            return_type_str.starts_with("Result"),
            &meta_ident,
        );

        Ok(quote! {
            #impl_fn

            #meta_tokens

            #invoker_impl

            const #invoker_const: #invoker_ident = #invoker_ident;

            #return_type_ref

            pub fn #fn_name() -> afast::HandlerEntry {
                afast::HandlerEntry::new(
                    stringify!(#fn_name),
                    &#invoker_const,
                    &#meta_ident,
                )
            }
        })
    }
}

/// Parses the `desc(...)`, `name(...)`, and `cache(...)` attributes from the
/// handler's attribute tokens.
///
/// Returns `(description, api_name, cache_seconds)`. `cache_seconds` defaults
/// to 0 (no caching) when the attribute is not provided.
fn parse_handler_attrs_from_tokens(
    attr_tokens: &TokenStream,
) -> syn::Result<(String, String, u64, String)> {
    let mut desc = String::new();
    let mut api_name = String::new();
    let mut cache_seconds: u64 = 0;
    let mut rate_limit_policy = String::new();

    if attr_tokens.is_empty() {
        return Ok((desc, api_name, cache_seconds, rate_limit_policy));
    }

    let parser = |input: syn::parse::ParseStream| -> syn::Result<Vec<Meta>> {
        let mut items = Vec::new();
        while !input.is_empty() {
            items.push(input.parse::<Meta>()?);
            if !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
            }
        }
        Ok(items)
    };
    let items: Vec<Meta> = parser.parse2(attr_tokens.clone())?;

    for item in items {
        match item {
            Meta::NameValue(nv) => {
                if nv.path.is_ident("desc") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = nv.value
                    {
                        desc = s.value();
                    }
                } else if nv.path.is_ident("name") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = nv.value
                    {
                        api_name = s.value();
                    }
                } else if nv.path.is_ident("cache")
                    && let Expr::Lit(ExprLit {
                        lit: Lit::Int(n), ..
                    }) = nv.value
                {
                    cache_seconds = n.base10_parse()?;
                } else if nv.path.is_ident("rate_limit")
                    && let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = nv.value
                {
                    rate_limit_policy = s.value();
                }
            }
            Meta::List(list) => {
                if list.path.is_ident("desc") {
                    let lit: LitStr = list.parse_args()?;
                    desc = lit.value();
                } else if list.path.is_ident("name") {
                    let lit: LitStr = list.parse_args()?;
                    api_name = lit.value();
                } else if list.path.is_ident("cache") {
                    let lit: LitInt = list.parse_args()?;
                    cache_seconds = lit.base10_parse()?;
                } else if list.path.is_ident("rate_limit") {
                    let lit: LitStr = list.parse_args()?;
                    rate_limit_policy = lit.value();
                }
            }
            _ => {}
        }
    }

    Ok((desc, api_name, cache_seconds, rate_limit_policy))
}

/// Analyzes each parameter in the handler's function signature and classifies
/// it by extractor type.
///
/// For binary handlers, recognized extractors are `State<T>`, `Custom<T>`,
/// `Data<T>`, `Receiver`, and `Sender`. For ordinary HTTP handlers, `Query<T>`,
/// `Param<T>`, `Body<T>`, and `Header<T>` are also accepted.
///
/// Parameters that do not match any recognized pattern produce a compile error
/// pointing at the offending type.
fn parse_handler_params(input_fn: &ItemFn, method: Option<&str>) -> syn::Result<Vec<ParamInfo>> {
    let mut params = Vec::new();
    let is_ordinary = method.is_some();

    for input in &input_fn.sig.inputs {
        match input {
            FnArg::Typed(pat_type) => {
                let name = match pat_type.pat.as_ref() {
                    Pat::Ident(pat_ident) => {
                        let s = pat_ident.ident.to_string();
                        s.strip_prefix("r#").unwrap_or(&s).to_string()
                    }
                    Pat::TupleStruct(ts) => {
                        if let Some(inner) = ts.elems.first() {
                            match inner {
                                Pat::Ident(pi) => {
                                    let s = pi.ident.to_string();
                                    s.strip_prefix("r#").unwrap_or(&s).to_string()
                                }
                                _ => "unknown".to_string(),
                            }
                        } else {
                            "unknown".to_string()
                        }
                    }
                    _ => "unknown".to_string(),
                };

                let ty_str = type_to_string(&pat_type.ty);

                let (extractor, ty_str_inner, inner_type) = if is_state_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("State".to_string(), extract_ident_name(&inner), inner)
                } else if is_custom_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("Custom".to_string(), extract_ident_name(&inner), inner)
                } else if is_data_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("Data".to_string(), extract_ident_name(&inner), inner)
                } else if is_ordinary && is_ws_sender_type(&pat_type.ty) {
                    let ws_type: Type = syn::parse_quote!(WsSender);
                    ("WsSender".to_string(), "WsSender".to_string(), ws_type)
                } else if is_ordinary && is_ws_receiver_type(&pat_type.ty) {
                    let ws_type: Type = syn::parse_quote!(WsReceiver);
                    ("WsReceiver".to_string(), "WsReceiver".to_string(), ws_type)
                } else if is_ordinary && is_sse_sender_type(&pat_type.ty) {
                    let sse_type: Type = syn::parse_quote!(SseSender);
                    ("SseSender".to_string(), "SseSender".to_string(), sse_type)
                } else if is_receiver_type(&pat_type.ty) {
                    let receiver_type: Type = syn::parse_quote!(Receiver);
                    (
                        "Receiver".to_string(),
                        "Receiver".to_string(),
                        receiver_type,
                    )
                } else if is_sender_type(&pat_type.ty) {
                    let sender_type: Type = syn::parse_quote!(Sender);
                    ("Sender".to_string(), "Sender".to_string(), sender_type)
                } else if is_ordinary && is_query_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("Query".to_string(), extract_ident_name(&inner), inner)
                } else if is_ordinary && is_param_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("Param".to_string(), extract_ident_name(&inner), inner)
                } else if is_ordinary && is_body_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("Body".to_string(), extract_ident_name(&inner), inner)
                } else if is_ordinary && is_header_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("Header".to_string(), extract_ident_name(&inner), inner)
                } else if is_ordinary && is_ws_query_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("WsQuery".to_string(), extract_ident_name(&inner), inner)
                } else if is_ordinary && is_ws_param_type(&pat_type.ty) {
                    let inner = extract_generic_inner(&pat_type.ty)?;
                    ("WsParam".to_string(), extract_ident_name(&inner), inner)
                } else {
                    return Err(syn::Error::new(
                        pat_type.ty.span(),
                        format!(
                            "unsupported parameter type: expected State<T>, Custom<T>, Data<T>, Receiver, Sender{} got: {}",
                            if is_ordinary {
                                ", Query<T>, Param<T>, Body<T>, Header<T>"
                            } else {
                                ""
                            },
                            ty_str
                        ),
                    ));
                };

                params.push(ParamInfo {
                    name,
                    extractor,
                    ty_str: ty_str_inner,
                    inner_type,
                    full_type: (*pat_type.ty).clone(),
                });
            }
            FnArg::Receiver(_) => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "handler functions must not have `self` parameter",
                ));
            }
        }
    }

    Ok(params)
}

/// Returns true when the outermost identifier of the type is `State`.
fn is_state_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "State")
}

/// Returns true when the outermost identifier of the type is `Custom`.
fn is_custom_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "Custom")
}

/// Returns true when the outermost identifier of the type is `Data`.
fn is_data_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "Data")
}

/// Returns true when the type string contains `Receiver`, matching either
/// `Receiver` (untyped) or a hypothetical `Receiver<T>` form.
fn is_receiver_type(ty: &Type) -> bool {
    type_to_string(ty).contains("Receiver")
}

/// Returns true when the type string contains `Sender`, matching either
/// `Sender` (untyped) or a hypothetical `Sender<T>` form.
fn is_sender_type(ty: &Type) -> bool {
    type_to_string(ty).contains("Sender")
}

/// Returns true when the outermost identifier of the type is `Query`.
fn is_query_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "Query")
}

/// Returns true when the outermost identifier of the type is `Param`.
fn is_param_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "Param")
}

/// Returns true when the outermost identifier of the type is `Body`.
fn is_body_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "Body")
}

/// Returns true when the outermost identifier of the type is `Header`.
fn is_header_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "Header")
}

/// Returns true when the outermost identifier of the type is `WsQuery`.
fn is_ws_query_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "WsQuery")
}

/// Returns true when the outermost identifier of the type is `WsParam`.
fn is_ws_param_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "WsParam")
}

/// Returns true when the outermost identifier of the type is `WsSender`.
fn is_ws_sender_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "WsSender")
}

/// Returns true when the outermost identifier of the type is `WsReceiver`.
fn is_ws_receiver_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "WsReceiver")
}

/// Returns true when the outermost identifier of the type is `SseSender`.
fn is_sse_sender_type(ty: &Type) -> bool {
    extract_outermost_ident(ty).is_some_and(|s| s == "SseSender")
}

/// Extracts the last path segment identifier from a `Type::Path`, if any.
///
/// Returns `None` for types that are not simple path expressions (e.g.
/// references, tuples, trait objects).
fn extract_outermost_ident(ty: &Type) -> Option<String> {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return Some(segment.ident.to_string());
    }
    None
}

/// Returns the last path segment identifier as a string, falling back to the
/// full type string for non-path types.
fn extract_ident_name(ty: &Type) -> String {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return segment.ident.to_string();
    }
    type_to_string(ty)
}

/// Extracts the first generic type argument from a type path.
///
/// For `Custom<User>`, returns `User`. Errors if the type is not a generic path
/// with angle-bracketed arguments.
fn extract_generic_inner(ty: &Type) -> syn::Result<Type> {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
        && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
        && let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first()
    {
        return Ok(inner_ty.clone());
    }
    Err(syn::Error::new(
        Span::call_site(),
        "expected a generic type like State<T>",
    ))
}

/// Returns true when the identifier is `Result` or `HttpResult`, the two
/// return-type wrappers recognized by the framework.
fn is_result_ident(ident: &syn::Ident) -> bool {
    ident == "Result" || ident == "HttpResult"
}

/// Extracts the success type from a handler's return type, unwrapping
/// `Result<T, E>` and `HttpResult<T>` to `T`, or returning the raw type string
/// otherwise.
fn extract_return_type(input_fn: &ItemFn) -> syn::Result<String> {
    match &input_fn.sig.output {
        syn::ReturnType::Default => Ok("()".to_string()),
        syn::ReturnType::Type(_, ty) => {
            if let Type::Path(type_path) = ty.as_ref()
                && let Some(segment) = type_path.path.segments.last()
                && is_result_ident(&segment.ident)
                && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
                && let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first()
            {
                return Ok(type_to_string(inner_ty));
            }
            Ok(type_to_string(ty))
        }
    }
}

/// Extracts the return type as a `syn::Type`, unwrapping `Result<T, E>` and
/// `HttpResult<T>` to `T`.
///
/// Returns `None` for functions without an explicit return type annotation
/// (i.e. unit return).
fn extract_return_type_syn(input_fn: &ItemFn) -> Option<Type> {
    match &input_fn.sig.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, ty) => {
            if let Type::Path(type_path) = ty.as_ref()
                && let Some(segment) = type_path.path.segments.last()
                && is_result_ident(&segment.ident)
                && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
                && let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first()
            {
                return Some(inner_ty.clone());
            }
            Some(ty.as_ref().clone())
        }
    }
}

/// Builds the `ParamMeta` entries for the handler's parameter list.
///
/// Each entry includes the parameter name, type, extractor kind, and a
/// structure pointer for complex inner types so the code generator can
/// recursively discover user-defined types.
fn build_param_meta_entries(params: &[ParamInfo]) -> Vec<TokenStream> {
    params
        .iter()
        .map(|p| {
            let name = &p.name;
            let ty = &p.ty_str;
            let extractor = &p.extractor;
            let structure = match extractor.as_str() {
                "Custom" | "Data" | "Query" | "Param" | "Body" | "Header" => {
                    let inner = &p.inner_type;
                    quote! { Some(|| <#inner as afast::Structure>::structure()) }
                }
                _ => quote! { afast::no_structure() },
            };
            quote! {
                afast::ParamMeta {
                    name: #name,
                    ty: #ty,
                    extractor: #extractor,
                    structure: #structure,
                }
            }
        })
        .collect()
}

/// Builds the `HandlerMeta` const describing the handler's name, description,
/// API name, parameter list, return type, transport characteristics, and
/// client-side cache duration.
/// Parameters for [`build_handler_meta`].
struct HandlerMetaParams<'a> {
    fn_name_str: &'a str,
    desc: &'a str,
    api_name: &'a str,
    return_type_str: &'a str,
    return_structure: Option<TokenStream>,
    long_connection: bool,
    method: &'a str,
    is_ordinary: bool,
    param_meta_entries: &'a [TokenStream],
    cache_seconds: u64,
    rate_limit_policy: &'a str,
}

fn build_handler_meta(p: &HandlerMetaParams<'_>) -> TokenStream {
    let meta_ident = syn::Ident::new(&format!("__META_{}", p.fn_name_str), Span::call_site());
    let ret_struct = match &p.return_structure {
        Some(expr) => quote! { Some(#expr) },
        None => quote! { afast::no_structure() },
    };
    let fn_name_str = p.fn_name_str;
    let desc = p.desc;
    let api_name = p.api_name;
    let return_type_str = p.return_type_str;
    let long_connection = p.long_connection;
    let method = p.method;
    let is_ordinary = p.is_ordinary;
    let param_meta_entries = p.param_meta_entries;
    let cache_seconds = p.cache_seconds;
    let rate_limit_policy = p.rate_limit_policy;
    quote! {
        pub const #meta_ident: afast::HandlerMeta = afast::HandlerMeta {
            name: #fn_name_str,
            desc: #desc,
            api_name: #api_name,
            params: &[#( #param_meta_entries ),*],
            return_type: #return_type_str,
            return_structure: #ret_struct,
            long_connection: #long_connection,
            is_ordinary: #is_ordinary,
            method: #method,
            path: "",
            cache_seconds: #cache_seconds,
            rate_limit_policy: #rate_limit_policy,
        };
    }
}

/// Builds the `HandlerInvoker` implementation for a handler, dispatching to
/// either a request-response invoker or a streaming (long-connection) invoker
/// based on whether the handler uses Receiver/Sender.
fn build_invoker_impl(
    invoker_ident: &syn::Ident,
    fn_name: &syn::Ident,
    params: &[ParamInfo],
    returns_result: bool,
    meta_ident: &syn::Ident,
) -> TokenStream {
    let long_connection = params
        .iter()
        .any(|p| p.extractor == "Receiver" || p.extractor == "Sender");

    if long_connection {
        return build_stream_invoker_impl(
            invoker_ident,
            fn_name,
            params,
            returns_result,
            meta_ident,
        );
    }

    build_call_invoker_impl(invoker_ident, fn_name, params, meta_ident)
}

/// Builds the request-response `HandlerInvoker` implementation.
///
/// The generated `call` method synchronously extracts `State`, `Custom`, and
/// `Data` parameters from the byte payload (advancing an offset cursor through
/// the buffer), then calls the user function asynchronously and serializes the
/// result.
fn build_call_invoker_impl(
    invoker_ident: &syn::Ident,
    fn_name: &syn::Ident,
    params: &[ParamInfo],
    meta_ident: &syn::Ident,
) -> TokenStream {
    let mut sync_extractions: Vec<TokenStream> = Vec::new();
    let mut has_payload_extractor = false;

    for p in params {
        let var_name = syn::Ident::new(&p.name, Span::call_site());
        let full_type = &p.full_type;
        let inner = &p.inner_type;
        let err_var = syn::Ident::new(&format!("__err_{}", p.name), Span::call_site());

        match p.extractor.as_str() {
            "State" => {
                sync_extractions.push(quote! {
                    let #var_name: #full_type = match state.get::<#inner>() {
                        Some(v) => afast::State(v.clone()),
                        None => {
                            let #err_var = afast::Error::StateNotFound { message: stringify!(#inner).to_string() };
                            return Box::pin(async move { Err(#err_var) });
                        }
                    };
                });
            }
            "Custom" => {
                has_payload_extractor = true;
                sync_extractions.push(quote! {
                    let #var_name: #full_type = match afast::marker::deserialize(&payload[_off..], &__marker) {
                        Ok((__custom_val, __consumed)) => {
                            _off += __consumed;
                            afast::Custom(__custom_val)
                        }
                        Err(e) => {
                            return Box::pin(async move { Err(e.into()) });
                        }
                    };
                });
            }
            "Data" => {
                has_payload_extractor = true;
                sync_extractions.push(quote! {
                    let #var_name: #full_type = match afast::marker::deserialize(&payload[_off..], &__marker) {
                        Ok((__data_val, __consumed)) => {
                            _off += __consumed;
                            afast::Data(__data_val)
                        }
                        Err(e) => {
                            return Box::pin(async move { Err(e.into()) });
                        }
                    };
                });
            }
            _ => {}
        }
    }

    let call_args: Vec<TokenStream> = params
        .iter()
        .map(|p| {
            let var_name = syn::Ident::new(&p.name, Span::call_site());
            quote! { #var_name }
        })
        .collect();

    // The offset cursor is only needed when Custom/Data extractors are present,
    // since they consume payload bytes sequentially.
    let offset_init = if has_payload_extractor {
        quote! { let mut _off: usize = 0; }
    } else {
        quote! {}
    };

    quote! {
        pub struct #invoker_ident;

        impl afast::HandlerInvoker for #invoker_ident {
            fn call(
                &self,
                state: &afast::StateMap,
                payload: &[u8],
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Vec<u8>, afast::Error>> + Send + '_>
            > {
                let __marker = state.get::<std::sync::Arc<String>>().cloned()
                    .unwrap_or_else(|| std::sync::Arc::new("afast".to_string()));
                #offset_init
                #( #sync_extractions )*
                Box::pin(async move {
                    let result = #fn_name( #( #call_args ),* ).await?;
                    Ok(afast::marker::serialize(&result, &__marker))
                })
            }

            fn meta(&self) -> Option<&'static afast::HandlerMeta> {
                Some(&#meta_ident)
            }
        }
    }
}

/// Builds the streaming (long-connection) `HandlerInvoker` implementation for
/// handlers that use `Receiver`/`Sender`.
///
/// The generated `call_stream` method sets up a bidirectional channel,
/// constructs `Receiver` and `Sender` from the channel halves, spawns the user
/// function on a Tokio task, and returns immediately with an empty success
/// payload. The initial `call` method is wired to return an error directing
/// callers to use `call_stream` instead.
fn build_stream_invoker_impl(
    invoker_ident: &syn::Ident,
    fn_name: &syn::Ident,
    params: &[ParamInfo],
    returns_result: bool,
    meta_ident: &syn::Ident,
) -> TokenStream {
    let mut sync_extractions: Vec<TokenStream> = Vec::new();
    let mut has_payload_extractor = false;
    let mut recv_var: Option<syn::Ident> = None;
    let mut send_var: Option<syn::Ident> = None;

    for p in params {
        let var_name = syn::Ident::new(&p.name, Span::call_site());
        let full_type = &p.full_type;
        let inner = &p.inner_type;
        let err_var = syn::Ident::new(&format!("__err_{}", p.name), Span::call_site());

        match p.extractor.as_str() {
            "State" => {
                sync_extractions.push(quote! {
                    let #var_name: #full_type = match state.get::<#inner>() {
                        Some(v) => afast::State(v.clone()),
                        None => {
                            let #err_var = afast::Error::StateNotFound { message: stringify!(#inner).to_string() };
                            return Box::pin(async move { Err(#err_var) });
                        }
                    };
                });
            }
            "Custom" => {
                has_payload_extractor = true;
                sync_extractions.push(quote! {
                    let #var_name: #full_type = match afast::marker::deserialize(&payload[_off..], &__marker) {
                        Ok((__custom_val, __consumed)) => {
                            _off += __consumed;
                            afast::Custom(__custom_val)
                        }
                        Err(e) => {
                            return Box::pin(async move { Err(e.into()) });
                        }
                    };
                });
            }
            "Data" => {
                has_payload_extractor = true;
                sync_extractions.push(quote! {
                    let #var_name: #full_type = match afast::marker::deserialize(&payload[_off..], &__marker) {
                        Ok((__data_val, __consumed)) => {
                            _off += __consumed;
                            afast::Data(__data_val)
                        }
                        Err(e) => {
                            return Box::pin(async move { Err(e.into()) });
                        }
                    };
                });
            }
            "Receiver" => {
                recv_var = Some(var_name);
            }
            "Sender" => {
                send_var = Some(var_name);
            }
            _ => {}
        }
    }

    let call_args: Vec<TokenStream> = params
        .iter()
        .map(|p| {
            let var_name = syn::Ident::new(&p.name, Span::call_site());
            quote! { #var_name }
        })
        .collect();

    // The offset cursor is only needed when Custom/Data extractors appear in
    // the parameter list.
    let offset_init = if has_payload_extractor {
        quote! { let mut _off: usize = 0; }
    } else {
        quote! {}
    };

    let recv_name = recv_var.expect("persistent handler must have Receiver");
    let send_name = send_var.expect("persistent handler must have Sender");

    let spawn_body = if returns_result {
        quote! {
            if let Err(e) = #fn_name( #( #call_args ),* ).await {
                eprintln!("afast: long-connection handler error: {}", e);
            }
        }
    } else {
        quote! {
            #fn_name( #( #call_args ),* ).await;
        }
    };

    quote! {
        pub struct #invoker_ident;

        impl afast::HandlerInvoker for #invoker_ident {
            fn call(
                &self,
                _state: &afast::StateMap,
                _payload: &[u8],
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Vec<u8>, afast::Error>> + Send + '_>
            > {
                Box::pin(async {
                    Err(afast::Error::Handler {
                        message: "persistent handler must be called via call_stream".into(),
                    })
                })
            }

            fn is_long_connection(&self) -> bool {
                true
            }

            fn call_stream<'a>(
                &'a self,
                state: &'a afast::StateMap,
                payload: &'a [u8],
                server_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
                server_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Vec<u8>, afast::Error>> + Send + 'a>
            > {
                let __marker = state.get::<std::sync::Arc<String>>().cloned()
                    .unwrap_or_else(|| std::sync::Arc::new("afast".to_string()));
                #offset_init
                #( #sync_extractions )*

                let #recv_name: afast::Receiver = afast::Receiver::new(server_rx);
                let #send_name: afast::Sender = afast::Sender::new(server_tx);

                Box::pin(async move {
                    tokio::spawn(async move {
                        #spawn_body
                    });
                    Ok(Vec::new())
                })
            }

            fn meta(&self) -> Option<&'static afast::HandlerMeta> {
                Some(&#meta_ident)
            }
        }
    }
}

/// Builds a dummy `HandlerInvoker` implementation that always returns an error.
///
/// Used for ordinary HTTP handlers to satisfy the `HandlerEntry` type, which
/// requires a binary invoker even when the handler is only reachable via HTTP
/// routing.
fn build_dummy_invoker_impl(invoker_ident: &syn::Ident) -> TokenStream {
    quote! {
        pub struct #invoker_ident;

        impl afast::HandlerInvoker for #invoker_ident {
            fn call(
                &self,
                _state: &afast::StateMap,
                _payload: &[u8],
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<Vec<u8>, afast::Error>> + Send + '_>
            > {
                Box::pin(async {
                    Err(afast::Error::Handler {
                        message: "ordinary handler must be called via HTTP routing".into(),
                    })
                })
            }
        }
    }
}

/// Builds the `OrdinaryHandlerInvoker` implementation for HTTP handlers.
///
/// The generated `call_ordinary` method receives the raw `hyper::Request`,
/// parsed path parameters, query string, and state map. It synchronously
/// extracts `State` parameters, then asynchronously extracts `Query`, `Param`,
/// `Body`, and `Header` parameters (which require parsing the HTTP request),
/// calls the user function, and converts the return value into an HTTP response
/// via `IntoResponse`.
fn build_ordinary_invoker_impl(
    invoker_ident: &syn::Ident,
    fn_name: &syn::Ident,
    params: &[ParamInfo],
) -> TokenStream {
    let mut state_extractions: Vec<TokenStream> = Vec::new();
    let mut has_query = false;
    let mut has_param = false;
    let mut has_body = false;
    let mut has_header = false;
    let mut query_var: Option<syn::Ident> = None;
    let mut param_var: Option<syn::Ident> = None;
    let mut body_var: Option<syn::Ident> = None;
    let mut header_var: Option<syn::Ident> = None;
    let mut query_inner: Option<Type> = None;
    let mut param_inner: Option<Type> = None;
    let mut body_inner: Option<Type> = None;
    let mut header_inner: Option<Type> = None;

    let mut async_extractions: Vec<TokenStream> = Vec::new();

    for p in params {
        let var_name = syn::Ident::new(&p.name, Span::call_site());
        let full_type = &p.full_type;
        let inner = &p.inner_type;
        let err_var = syn::Ident::new(&format!("__err_{}", p.name), Span::call_site());

        match p.extractor.as_str() {
            "State" => {
                state_extractions.push(quote! {
                    let #var_name: #full_type = match state.get::<#inner>() {
                        Some(v) => afast::State(v.clone()),
                        None => {
                            let #err_var = afast::Error::StateNotFound { message: stringify!(#inner).to_string() };
                            return Box::pin(async move { Err(#err_var) });
                        }
                    };
                });
            }
            "Query" => {
                has_query = true;
                query_var = Some(var_name);
                query_inner = Some(inner.clone());
            }
            "Param" => {
                has_param = true;
                param_var = Some(var_name);
                param_inner = Some(inner.clone());
            }
            "Body" => {
                has_body = true;
                body_var = Some(var_name);
                body_inner = Some(inner.clone());
            }
            "Header" => {
                has_header = true;
                header_var = Some(var_name);
                header_inner = Some(inner.clone());
            }
            _ => {}
        }
    }

    // Query parameters are extracted by serializing the parsed query string to
    // JSON and deserializing into the target type with lenient coercion (e.g.
    // string-to-int conversion).
    if has_query {
        let var_name = query_var.as_ref().unwrap();
        let inner = query_inner.as_ref().unwrap();
        async_extractions.push(quote! {
            let #var_name: afast::Query<#inner> = afast::Query::from_query(&query_string)?;
        });
    }

    // Path parameters (e.g. `/user/:id`) are extracted similarly: serialized to
    // JSON and deserialized via lenient coercion.
    if has_param {
        let var_name = param_var.as_ref().unwrap();
        let inner = param_inner.as_ref().unwrap();
        async_extractions.push(quote! {
            let #var_name: afast::Param<#inner> = afast::Param::from_params(&path_params)?;
        });
    }

    // Header extraction reads all request headers into a JSON object and fills
    // default values for fields declared in the target type's Structure metadata.
    // This must execute before Body extraction because Body consumes the request.
    if has_header {
        let var_name = header_var.as_ref().unwrap();
        let inner = header_inner.as_ref().unwrap();
        let err_var = syn::Ident::new(&format!("__err_{}", var_name), Span::call_site());
        async_extractions.push(quote! {
            let mut __header_json = afast::req_headers_to_json(req.headers());
            afast::fill_standard_header_defaults(&mut __header_json, || <#inner as afast::Structure>::structure());
            let #var_name: afast::Header<#inner> = match serde_json::from_value(__header_json) {
                Ok(v) => afast::Header(v),
                Err(e) => {
                    let #err_var = afast::Error::Custom { code: 400, message: format!("header parse error: {}", e) };
                    return Err(#err_var);
                }
            };
        });
    }

    // Body extraction reads the full request body as bytes and deserializes
    // from JSON. This must be the last extraction step because it consumes the
    // request body stream.
    if has_body {
        let var_name = body_var.as_ref().unwrap();
        let inner = body_inner.as_ref().unwrap();
        let err_var = syn::Ident::new(&format!("__err_{}", var_name), Span::call_site());
        async_extractions.push(quote! {
            let __body_bytes = afast::read_body_bytes(req).await?;
            let #var_name: afast::Body<#inner> = match serde_json::from_slice(&__body_bytes) {
                Ok(v) => afast::Body(v),
                Err(e) => {
                    let #err_var = afast::Error::Custom { code: 400, message: format!("body parse error: {}", e) };
                    return Err(#err_var);
                }
            };
        });
    }

    let call_args: Vec<TokenStream> = params
        .iter()
        .map(|p| {
            let var_name = syn::Ident::new(&p.name, Span::call_site());
            quote! { #var_name }
        })
        .collect();

    // Move the borrowed query string and path params into owned values so they
    // survive into the async block.
    let owned_query = if has_query || has_param || has_header {
        vec![
            quote! { let query_string = query_string.to_string(); },
            quote! { let path_params = path_params.clone(); },
        ]
    } else {
        vec![]
    };

    quote! {
        pub struct #invoker_ident;

        impl afast::OrdinaryHandlerInvoker for #invoker_ident {
            fn call_ordinary(
                &self,
                mut req: hyper::Request<hyper::body::Incoming>,
                path_params: &std::collections::HashMap<String, String>,
                query_string: &str,
                state: &afast::StateMap,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                        Output = Result<
                            hyper::Response<http_body_util::Full<hyper::body::Bytes>>,
                            afast::Error,
                        >,
                    > + Send + '_,
                >,
            > {
                #( #state_extractions )*
                #( #owned_query )*
                Box::pin(async move {
                    use afast::IntoResponse;
                    #( #async_extractions )*
                    let __result = #fn_name( #( #call_args ),* ).await;
                    Ok(__result.into_response())
                })
            }
        }
    }
}

/// Converts a `syn::Type` to its string representation using `quote!`.
fn type_to_string(ty: &Type) -> String {
    quote! { #ty }.to_string()
}

/// Entry point for the `#[ws]` proc-macro attribute.
///
/// Generates a `WsHandlerInvoker` implementation for WebSocket route handlers.
/// The handler function receives `WsQuery<T>`, `WsParam<T>`, and
/// `WsSender`/`WsReceiver` extractors, and returns `Result<()>`.
///
/// ```no_run
/// use afast::ws;
///
/// #[ws(desc("Chat handler"))]
/// async fn chat(sender: afast::WsSender, mut receiver: afast::WsReceiver) -> afast::Result<()> {
///     Ok(())
/// }
/// ```
pub fn expand_ws(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let input_fn: ItemFn = syn::parse2(item)?;
    let fn_name = &input_fn.sig.ident;
    let fn_name_str = fn_name.to_string();

    let (desc, api_name, _cache_seconds, _rate_limit_policy) =
        parse_handler_attrs_from_tokens(&attr)?;
    let params = parse_handler_params(&input_fn, Some("WS"))?;

    // Validate: ws handlers cannot use Body/Custom/Data/Receiver/Sender
    for p in &params {
        match p.extractor.as_str() {
            "State" | "WsQuery" | "WsParam" | "Query" | "Param" | "Header" | "WsSender"
            | "WsReceiver" => {}
            other => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "extractor '{}' is not supported in #[ws] handlers. \
                         Allowed: State, Query, Param, Header, WsSender, WsReceiver",
                        other
                    ),
                ));
            }
        }
    }

    // Build the implementation function (renamed to __impl_<name>).
    let impl_fn_name = syn::Ident::new(&format!("__impl_{}", fn_name_str), Span::call_site());
    let mut impl_sig = input_fn.sig.clone();
    impl_sig.ident = impl_fn_name.clone();
    let impl_fn = {
        let block = &input_fn.block;
        let attrs: Vec<_> = input_fn
            .attrs
            .iter()
            .filter(|a| !a.path().is_ident("ws"))
            .collect();
        quote! {
            #(#attrs)*
            #impl_sig #block
        }
    };

    let invoker_ident = syn::Ident::new(&format!("__WsInvoker_{}", fn_name_str), Span::call_site());
    let invoker_const =
        syn::Ident::new(&format!("__WS_INVOKER_{}", fn_name_str), Span::call_site());
    let meta_ident = syn::Ident::new(&format!("__META_{}", fn_name_str), Span::call_site());

    let desc_str = desc;
    let name_str = if api_name.is_empty() {
        fn_name_str.clone()
    } else {
        api_name
    };

    let ws_invoker_impl = build_ws_invoker_impl(&invoker_ident, &impl_fn_name, &params);

    Ok(quote! {
        #impl_fn

        const #meta_ident: afast::HandlerMeta = afast::HandlerMeta {
            name: #name_str,
            desc: #desc_str,
            api_name: "",
            params: &[],
            return_type: "()",
            return_structure: None,
            long_connection: false,
            is_ordinary: true,
            method: "WS",
            path: "",
            cache_seconds: 0,
            rate_limit_policy: "",
        };

        #ws_invoker_impl

        const #invoker_const: #invoker_ident = #invoker_ident;

        pub fn #fn_name() -> (&'static dyn afast::app::ordinary_ws::WsHandlerInvoker, &'static str) {
            (&#invoker_const, #name_str)
        }
    })
}

/// Builds the `WsHandlerInvoker` implementation for a WebSocket handler.
fn build_ws_invoker_impl(
    invoker_ident: &syn::Ident,
    fn_name: &syn::Ident,
    params: &[ParamInfo],
) -> TokenStream {
    let mut state_extractions: Vec<TokenStream> = Vec::new();
    let mut has_query = false;
    let mut has_param = false;
    let mut has_header = false;
    let mut has_sender = false;
    let mut has_receiver = false;
    let mut query_var: Option<syn::Ident> = None;
    let mut param_var: Option<syn::Ident> = None;
    let mut header_var: Option<syn::Ident> = None;
    let mut query_inner: Option<Type> = None;
    let mut param_inner: Option<Type> = None;
    let mut header_inner: Option<Type> = None;

    for p in params {
        let var_name = syn::Ident::new(&p.name, Span::call_site());
        let full_type = &p.full_type;
        let inner = &p.inner_type;

        match p.extractor.as_str() {
            "State" => {
                state_extractions.push(quote! {
                    let #var_name: #full_type = match state.get::<#inner>() {
                        Some(v) => afast::State(v.clone()),
                        None => {
                            return Err(afast::Error::StateNotFound {
                                message: stringify!(#inner).to_string()
                            });
                        }
                    };
                });
            }
            "WsQuery" | "Query" => {
                has_query = true;
                query_var = Some(var_name);
                query_inner = Some(inner.clone());
            }
            "WsParam" | "Param" => {
                has_param = true;
                param_var = Some(var_name);
                param_inner = Some(inner.clone());
            }
            "Header" => {
                has_header = true;
                header_var = Some(var_name);
                header_inner = Some(inner.clone());
            }
            "WsSender" => {
                has_sender = true;
            }
            "WsReceiver" => {
                has_receiver = true;
            }
            _ => {}
        }
    }

    let mut extractions: Vec<TokenStream> = Vec::new();

    if has_query {
        let var_name = query_var.as_ref().unwrap();
        let inner = query_inner.as_ref().unwrap();
        extractions.push(quote! {
            let #var_name: afast::Query<#inner> = afast::Query::from_query(&query_owned)?;
        });
    }

    if has_param {
        let var_name = param_var.as_ref().unwrap();
        let inner = param_inner.as_ref().unwrap();
        extractions.push(quote! {
            let #var_name: afast::Param<#inner> = afast::Param::from_params(&path_params_owned)?;
        });
    }

    if has_header {
        let var_name = header_var.as_ref().unwrap();
        let inner = header_inner.as_ref().unwrap();
        extractions.push(quote! {
            let mut __header_json = headers_owned.clone();
            afast::fill_standard_header_defaults(&mut __header_json, || <#inner as afast::Structure>::structure());
            let #var_name: afast::Header<#inner> = match serde_json::from_value(__header_json) {
                Ok(v) => afast::Header(v),
                Err(e) => return Err(afast::Error::Custom { code: 400, message: format!("header parse error: {}", e) }),
            };
        });
    }

    // WsSender and WsReceiver are passed directly as function arguments.

    // Determine which args need to be passed from the call_ws signature.
    let mut extra_args = Vec::new();
    if has_sender {
        extra_args.push(quote! { sender });
    }
    if has_receiver {
        extra_args.push(quote! { receiver });
    }

    // Build the call args: extracted vars first, then sender/receiver
    let all_call_args: Vec<TokenStream> = {
        let mut args = Vec::new();
        // Add extracted params (State, WsQuery, WsParam) in order
        for p in params {
            let var_name = syn::Ident::new(&p.name, Span::call_site());
            match p.extractor.as_str() {
                "WsSender" | "WsReceiver" => {} // added via extra_args
                _ => args.push(quote! { #var_name }),
            }
        }
        // Add sender/receiver at the end
        args.extend(extra_args);
        args
    };

    quote! {
        pub struct #invoker_ident;

        impl afast::app::ordinary_ws::WsHandlerInvoker for #invoker_ident {
            fn call_ws(
                &'static self,
                query: &str,
                path_params: &std::collections::HashMap<String, String>,
                headers: serde_json::Value,
                sender: afast::WsSender,
                receiver: afast::WsReceiver,
                state: std::sync::Arc<afast::StateMap>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                        Output = Result<(), afast::Error>,
                    > + Send + 'static,
                >,
            > {
                let query_owned = query.to_string();
                let path_params_owned = path_params.clone();
                let headers_owned = headers;

                Box::pin(async move {
                    #( #state_extractions )*
                    #( #extractions )*
                    #fn_name( #( #all_call_args ),* ).await
                })
            }
        }
    }
}

/// Entry point for the `#[sse]` proc-macro attribute.
///
/// Generates an `SseHandlerInvoker` implementation for SSE route handlers.
/// The handler receives `SseSender` to push events and optionally
/// `WsQuery<T>` / `WsParam<T>` for HTTP parameters.
pub fn expand_sse(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let input_fn: ItemFn = syn::parse2(item)?;
    let fn_name = &input_fn.sig.ident;
    let fn_name_str = fn_name.to_string();

    let (desc, api_name, _cache_seconds, _rate_limit_policy) =
        parse_handler_attrs_from_tokens(&attr)?;
    let params = parse_handler_params(&input_fn, Some("SSE"))?;

    // Validate: sse handlers support State, SseSender, Query, Param, WsQuery, WsParam
    for p in &params {
        match p.extractor.as_str() {
            "State" | "SseSender" | "WsQuery" | "WsParam" | "Query" | "Param" | "Header" => {}
            other => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "extractor '{}' is not supported in #[sse] handlers. \
                         Allowed: State, SseSender, Query, Param, Header, WsQuery, WsParam",
                        other
                    ),
                ));
            }
        }
    }

    // Build the implementation function (renamed to __impl_<name>).
    let impl_fn_name = syn::Ident::new(&format!("__impl_{}", fn_name_str), Span::call_site());
    let mut impl_sig = input_fn.sig.clone();
    impl_sig.ident = impl_fn_name.clone();
    let impl_fn = {
        let block = &input_fn.block;
        let attrs: Vec<_> = input_fn
            .attrs
            .iter()
            .filter(|a| !a.path().is_ident("sse"))
            .collect();
        quote! {
            #(#attrs)*
            #impl_sig #block
        }
    };

    let invoker_ident =
        syn::Ident::new(&format!("__SseInvoker_{}", fn_name_str), Span::call_site());
    let invoker_const =
        syn::Ident::new(&format!("__SSE_INVOKER_{}", fn_name_str), Span::call_site());
    let meta_ident = syn::Ident::new(&format!("__META_{}", fn_name_str), Span::call_site());

    let desc_str = desc;
    let name_str = if api_name.is_empty() {
        fn_name_str.clone()
    } else {
        api_name
    };

    let sse_invoker_impl = build_sse_invoker_impl(&invoker_ident, &impl_fn_name, &params);

    Ok(quote! {
        #impl_fn

        const #meta_ident: afast::HandlerMeta = afast::HandlerMeta {
            name: #name_str,
            desc: #desc_str,
            api_name: "",
            params: &[],
            return_type: "()",
            return_structure: None,
            long_connection: false,
            is_ordinary: true,
            method: "SSE",
            path: "",
            cache_seconds: 0,
            rate_limit_policy: "",
        };

        #sse_invoker_impl

        const #invoker_const: #invoker_ident = #invoker_ident;

        pub fn #fn_name() -> (&'static dyn afast::app::ordinary_sse::SseHandlerInvoker, &'static str) {
            (&#invoker_const, #name_str)
        }
    })
}

/// Builds the `SseHandlerInvoker` implementation for an SSE handler.
fn build_sse_invoker_impl(
    invoker_ident: &syn::Ident,
    fn_name: &syn::Ident,
    params: &[ParamInfo],
) -> TokenStream {
    let mut state_extractions: Vec<TokenStream> = Vec::new();
    let mut has_query = false;
    let mut has_param = false;
    let mut has_header = false;
    let mut _has_sender = false;
    let mut query_var: Option<syn::Ident> = None;
    let mut param_var: Option<syn::Ident> = None;
    let mut header_var: Option<syn::Ident> = None;
    let mut query_inner: Option<Type> = None;
    let mut param_inner: Option<Type> = None;
    let mut header_inner: Option<Type> = None;

    for p in params {
        let var_name = syn::Ident::new(&p.name, Span::call_site());
        let full_type = &p.full_type;
        let inner = &p.inner_type;

        match p.extractor.as_str() {
            "State" => {
                state_extractions.push(quote! {
                    let #var_name: #full_type = match state.get::<#inner>() {
                        Some(v) => afast::State(v.clone()),
                        None => {
                            return Err(afast::Error::StateNotFound {
                                message: stringify!(#inner).to_string()
                            });
                        }
                    };
                });
            }
            "SseSender" => {
                _has_sender = true;
            }
            "WsQuery" | "Query" => {
                has_query = true;
                query_var = Some(var_name);
                query_inner = Some(inner.clone());
            }
            "WsParam" | "Param" => {
                has_param = true;
                param_var = Some(var_name);
                param_inner = Some(inner.clone());
            }
            "Header" => {
                has_header = true;
                header_var = Some(var_name);
                header_inner = Some(inner.clone());
            }
            _ => {}
        }
    }

    let mut extractions: Vec<TokenStream> = Vec::new();

    if has_query {
        let var_name = query_var.as_ref().unwrap();
        let inner = query_inner.as_ref().unwrap();
        extractions.push(quote! {
            let #var_name: afast::Query<#inner> = afast::Query::from_query(&query_owned)?;
        });
    }

    if has_param {
        let var_name = param_var.as_ref().unwrap();
        let inner = param_inner.as_ref().unwrap();
        extractions.push(quote! {
            let #var_name: afast::Param<#inner> = afast::Param::from_params(&path_params_owned)?;
        });
    }

    if has_header {
        let var_name = header_var.as_ref().unwrap();
        let inner = header_inner.as_ref().unwrap();
        extractions.push(quote! {
            let mut __header_json = headers_owned.clone();
            afast::fill_standard_header_defaults(&mut __header_json, || <#inner as afast::Structure>::structure());
            let #var_name: afast::Header<#inner> = match serde_json::from_value(__header_json) {
                Ok(v) => afast::Header(v),
                Err(e) => return Err(afast::Error::Custom { code: 400, message: format!("header parse error: {}", e) }),
            };
        });
    }

    // Build call args: extracted vars + sender
    let all_call_args: Vec<TokenStream> = {
        let mut args = Vec::new();
        for p in params {
            let var_name = syn::Ident::new(&p.name, Span::call_site());
            match p.extractor.as_str() {
                "SseSender" => args.push(quote! { sender }),
                _ => args.push(quote! { #var_name }),
            }
        }
        args
    };

    quote! {
        pub struct #invoker_ident;

        impl afast::app::ordinary_sse::SseHandlerInvoker for #invoker_ident {
            fn call_sse(
                &'static self,
                query: &str,
                path_params: &std::collections::HashMap<String, String>,
                headers: serde_json::Value,
                sender: afast::SseSender,
                state: std::sync::Arc<afast::StateMap>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                        Output = Result<(), afast::Error>,
                    > + Send + 'static,
                >,
            > {
                let query_owned = query.to_string();
                let path_params_owned = path_params.clone();
                let headers_owned = headers;

                Box::pin(async move {
                    #( #state_extractions )*
                    #( #extractions )*
                    #fn_name( #( #all_call_args ),* ).await
                })
            }
        }
    }
}
