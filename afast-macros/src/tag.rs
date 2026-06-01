use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Fields, Ident, Lit, LitStr, Meta, Type};

/// Returns the identifier name as a string, stripping the raw-identifier prefix
/// `r#` when present.
fn ident_name(ident: &Ident) -> String {
    let s = ident.to_string();
    s.strip_prefix("r#").unwrap_or(&s).to_string()
}

/// A parsed validation rule extracted from a `#[afast(...)]` field attribute.
///
/// Each variant encodes one constraint and its error metadata:
/// - `Gt` / `Gte` / `Lt` / `Lte`: numeric comparison against a threshold.
/// - `Len`: string or collection length bounds.
/// - `Of`: set membership (the value must be one of the listed alternatives).
enum Validation {
    Gt {
        value: f64,
        code: i64,
        message: String,
    },
    Gte {
        value: f64,
        code: i64,
        message: String,
    },
    Lt {
        value: f64,
        code: i64,
        message: String,
    },
    Lte {
        value: f64,
        code: i64,
        message: String,
    },
    Len {
        min: i64,
        max: i64,
        code: i64,
        message: String,
    },
    Of {
        values: Vec<String>,
        code: i64,
        message: String,
    },
}

/// Parses all `#[afast(...)]` attributes on a field and returns the validation
/// rules they declare.
///
/// A single `#[afast(...)]` attribute can contain multiple comma-separated call
/// expressions:
///
/// ```no_run
/// #[afast(gte(0, 3002, "msg1"), lte(100, 3003, "msg2"))]
/// ```
///
/// Each call must match one of the recognized rule functions: `gt`, `gte`,
/// `lt`, `lte`, `len`, or `of`. Unrecognized calls are silently skipped,
/// allowing forward-compatible attribute content.
///
/// Also detects `#[afast(skip)]` and `#[afast(skip_with("marker"))]` which
/// are stored separately (not as validation rules) and returned via the
/// `skip` and `skip_with` out-parameters.
fn parse_validations(attrs: &[syn::Attribute]) -> (Vec<Validation>, bool, Option<String>) {
    let mut rules = Vec::new();
    let mut is_skip = false;
    let mut skip_with: Option<String> = None;
    for attr in attrs {
        if !attr.path().is_ident("afast") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            continue;
        };
        let punct: syn::punctuated::Punctuated<syn::Expr, syn::Token![,]> = list
            .parse_args_with(syn::punctuated::Punctuated::parse_terminated)
            .unwrap_or_default();
        let content: Vec<syn::Expr> = punct.into_iter().collect();
        // Each element in the content list represents one validation call
        // expression (e.g. `gte(0, 3002, "msg")`). Dispatch by function name
        // and parse the arguments accordingly.
        for expr in &content {
            let (name, args) = match expr {
                syn::Expr::Call(call) => {
                    let func_name = match call.func.as_ref() {
                        syn::Expr::Path(ep) => ep.path.segments.last().unwrap().ident.to_string(),
                        _ => continue,
                    };
                    let call_args: Vec<syn::Expr> = call.args.iter().cloned().collect();
                    (func_name, call_args)
                }
                syn::Expr::Path(ep) => {
                    let func_name = ep.path.segments.last().unwrap().ident.to_string();
                    (func_name, vec![])
                }
                _ => continue,
            };
            match name.as_str() {
                "skip" => {
                    is_skip = true;
                }
                "skip_with" => {
                    // Parse skip_with("marker") or skip_with("marker", "default_fn")
                    if let Some(first) = args.first() {
                        if let syn::Expr::Lit(syn::ExprLit {
                            lit: Lit::Str(s), ..
                        }) = first
                        {
                            skip_with = Some(s.value());
                        }
                    }
                }
                "gt" | "gte" | "lt" | "lte" => {
                    if let Some((val, code, msg)) = parse_range_args(&args) {
                        let rule = match name.as_str() {
                            "gt" => Validation::Gt {
                                value: val,
                                code,
                                message: msg,
                            },
                            "gte" => Validation::Gte {
                                value: val,
                                code,
                                message: msg,
                            },
                            "lt" => Validation::Lt {
                                value: val,
                                code,
                                message: msg,
                            },
                            "lte" => Validation::Lte {
                                value: val,
                                code,
                                message: msg,
                            },
                            _ => unreachable!(),
                        };
                        rules.push(rule);
                    }
                }
                "len" => {
                    if let Some((min, max, code, msg)) = parse_len_args(&args) {
                        rules.push(Validation::Len {
                            min,
                            max,
                            code,
                            message: msg,
                        });
                    }
                }
                "of" => {
                    if let Some((values, code, msg)) = parse_of_args(&args) {
                        rules.push(Validation::Of {
                            values,
                            code,
                            message: msg,
                        });
                    }
                }
                // Unrecognized rule names are skipped silently to allow
                // forward-compatible attribute extensions.
                _ => {}
            }
        }
    }
    (rules, is_skip, skip_with)
}

/// Parses the three arguments of a numeric comparison validation: the threshold
/// value, the error code, and the error message.
///
/// Returns `None` if any argument cannot be parsed into the expected literal
/// form.
fn parse_range_args(args: &[syn::Expr]) -> Option<(f64, i64, String)> {
    if args.len() != 3 {
        return None;
    }
    let value = match &args[0] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Float(f), ..
        }) => f.base10_parse::<f64>().ok()?,
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<f64>().ok()?,
        _ => return None,
    };
    let code = match &args[1] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<i64>().ok()?,
        _ => return None,
    };
    let message = match &args[2] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Str(s), ..
        }) => s.value(),
        _ => return None,
    };
    Some((value, code, message))
}

/// Parses the four arguments of a length validation: minimum, maximum, error
/// code, and error message.
///
/// Returns `None` if any argument cannot be parsed into the expected literal
/// form.
fn parse_len_args(args: &[syn::Expr]) -> Option<(i64, i64, i64, String)> {
    if args.len() != 4 {
        return None;
    }
    let min = match &args[0] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<i64>().ok()?,
        _ => return None,
    };
    let max = match &args[1] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<i64>().ok()?,
        _ => return None,
    };
    let code = match &args[2] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<i64>().ok()?,
        _ => return None,
    };
    let message = match &args[3] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Str(s), ..
        }) => s.value(),
        _ => return None,
    };
    Some((min, max, code, message))
}

/// Parses the three arguments of a set-membership validation: the allowed value
/// array, the error code, and the error message.
///
/// The values array can contain integers, floats, booleans, or strings. Each
/// element is converted to its string representation.
///
/// Returns `None` if any argument cannot be parsed into the expected literal
/// form.
fn parse_of_args(args: &[syn::Expr]) -> Option<(Vec<String>, i64, String)> {
    if args.len() != 3 {
        return None;
    }
    let values = match &args[0] {
        syn::Expr::Array(arr) => arr
            .elems
            .iter()
            .map(|e| match e {
                syn::Expr::Lit(syn::ExprLit {
                    lit: Lit::Int(i), ..
                }) => Ok(i.to_string()),
                syn::Expr::Lit(syn::ExprLit {
                    lit: Lit::Float(f), ..
                }) => Ok(f.to_string()),
                syn::Expr::Lit(syn::ExprLit {
                    lit: Lit::Bool(b), ..
                }) => Ok(b.value.to_string()),
                syn::Expr::Lit(syn::ExprLit {
                    lit: Lit::Str(s), ..
                }) => Ok(format!("\"{}\"", s.value())),
                _ => Err(()),
            })
            .collect::<Result<Vec<_>, _>>()
            .ok()?,
        _ => return None,
    };
    let code = match &args[1] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<i64>().ok()?,
        _ => return None,
    };
    let message = match &args[2] {
        syn::Expr::Lit(syn::ExprLit {
            lit: Lit::Str(s), ..
        }) => s.value(),
        _ => return None,
    };
    Some((values, code, message))
}

/// Determines whether a field type requires a `Structure` function pointer.
///
/// Primitive types (`i8` through `u64`, `usize`, `f32`, `f64`, `bool`,
/// `String`, `&str`) have no nested structure and return `false`. Container
/// types (`Option<T>`, `Vec<T>`) are unwrapped to their innermost type before
/// checking. All other user-defined types return `true`.
fn needs_structure(ty: &Type) -> bool {
    let s = type_to_string(ty);
    // Strip at most one Option<...> wrapper.
    let inner = s
        .strip_prefix("Option<")
        .and_then(|s| s.strip_suffix('>'))
        .map(|s| s.to_string())
        .unwrap_or(s.clone());
    // Strip at most one Vec<...> wrapper.
    let inner = inner
        .strip_prefix("Vec<")
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(&inner);
    !matches!(
        inner,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "f32"
            | "f64"
            | "bool"
            | "String"
            | "&str"
    )
}

/// Generates the `Structure` function pointer expression for a complex field
/// type.
///
/// For `Option<Address>`, `Vec<Address>`, or bare `Address`, this produces
/// `Some(|| <Address as afast::Structure>::structure())`. The outer wrapper
/// (`Option<...>` or `Vec<...>`) is stripped to reach the inner complex type.
fn structure_expr(ty: &Type) -> TokenStream {
    let s = type_to_string(ty);
    let inner = s
        .strip_prefix("Option<")
        .or_else(|| s.strip_prefix("Vec<"))
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(&s);
    let inner_ty: Type = syn::parse_str(inner).unwrap();
    quote! { Some(|| <#inner_ty as afast::Structure>::structure()) }
}

/// Generates a TokenStream expression for a slice of `ValidateRule` entries
/// from the parsed validation rules.
///
/// Returns `&[]` when no validations are present.
fn validations_expr(validations: &[Validation]) -> TokenStream {
    if validations.is_empty() {
        return quote! { &[] };
    }
    let entries: Vec<TokenStream> = validations.iter().map(|v| {
        match v {
            Validation::Gt { value, code, message } => quote! {
                afast::ValidateRule::Gt { value: #value, code: #code, message: #message }
            },
            Validation::Gte { value, code, message } => quote! {
                afast::ValidateRule::Gte { value: #value, code: #code, message: #message }
            },
            Validation::Lt { value, code, message } => quote! {
                afast::ValidateRule::Lt { value: #value, code: #code, message: #message }
            },
            Validation::Lte { value, code, message } => quote! {
                afast::ValidateRule::Lte { value: #value, code: #code, message: #message }
            },
            Validation::Len { min, max, code, message } => quote! {
                afast::ValidateRule::Len { min: #min, max: #max, code: #code, message: #message }
            },
            Validation::Of { values, code, message } => {
                let vals: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
                quote! {
                    afast::ValidateRule::Of { values: &[#( #vals ),*], code: #code, message: #message }
                }
            }
        }
    }).collect();
    quote! { &[#( #entries ),*] }
}

/// Entry point for the `#[derive(Tag)]` proc-macro.
///
/// Inspects the struct or enum definition and produces:
/// - A `TagMeta` const (`__TAG_<name>`) describing the type's fields/variants,
///   their types, descriptions, nested structure pointers, and validation rules.
/// - An implementation of `afast::Structure` whose `structure()` method returns
///   a reference to that const.
///
/// Structs must have named fields. Enums support unit, tuple, and named-field
/// variants. Unions are rejected.
pub fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input_fn: DeriveInput = syn::parse2(input)?;
    let struct_name = &input_fn.ident;
    let struct_name_str = ident_name(struct_name);

    // Extract the top-level description from `#[tag("...")]` on the type itself.
    let desc = input_fn
        .attrs
        .iter()
        .find_map(|attr| {
            if attr.path().is_ident("tag") {
                if let Meta::List(list) = &attr.meta {
                    if let Ok(lit) = list.parse_args::<LitStr>() {
                        return Some(lit.value());
                    }
                }
            }
            None
        })
        .unwrap_or_default();

    let (impl_generics, ty_generics, where_clause) = input_fn.generics.split_for_impl();

    let tag_kind = match &input_fn.data {
        Data::Struct(data) => {
            let fields = match &data.fields {
                Fields::Named(fields) => &fields.named,
                _ => {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        "Tag can only be derived for structs with named fields or enums",
                    ));
                }
            };

            let mut field_meta_entries = Vec::new();
            for field in fields {
                let field_name = ident_name(field.ident.as_ref().unwrap());
                let field_ty = type_to_string(&field.ty);
                let field_desc = field_tag_desc(&field.attrs);
                let structure = if needs_structure(&field.ty) {
                    structure_expr(&field.ty)
                } else {
                    quote! { None }
                };
                let (validations, skip, skip_with_val) = parse_validations(&field.attrs);
                let validations_expr = validations_expr(&validations);
                let skip_with_str = skip_with_val.as_deref().unwrap_or("");
                field_meta_entries.push(quote! {
                    afast::FieldMeta {
                        name: #field_name,
                        ty: #field_ty,
                        desc: #field_desc,
                        structure: #structure,
                        validations: #validations_expr,
                        skip: #skip,
                        skip_with: #skip_with_str,
                    }
                });
            }

            quote! { afast::TagKind::Struct(&[#( #field_meta_entries ),*]) }
        }
        Data::Enum(data) => {
            let mut variant_entries = Vec::new();
            for variant in &data.variants {
                let variant_name = ident_name(&variant.ident);
                let fields = match &variant.fields {
                    Fields::Named(named) => {
                        let mut field_metas = Vec::new();
                        for f in &named.named {
                            let fname = ident_name(f.ident.as_ref().unwrap());
                            let fty = type_to_string(&f.ty);
                            let fdesc = field_tag_desc(&f.attrs);
                            let structure = if needs_structure(&f.ty) {
                                structure_expr(&f.ty)
                            } else {
                                quote! { None }
                            };
                            let (validations, skip, skip_with_val) = parse_validations(&f.attrs);
                            let validations_expr = validations_expr(&validations);
                            let skip_with_str = skip_with_val.as_deref().unwrap_or("");
                            field_metas.push(quote! {
                                afast::FieldMeta {
                                    name: #fname,
                                    ty: #fty,
                                    desc: #fdesc,
                                    structure: #structure,
                                    validations: #validations_expr,
                                    skip: #skip,
                                    skip_with: #skip_with_str,
                                }
                            });
                        }
                        quote! { &[#( #field_metas ),*] }
                    }
                    Fields::Unnamed(unnamed) => {
                        let mut field_metas = Vec::new();
                        // Unnamed fields receive synthetic names (`__0`, `__1`,
                        // etc.) since the framework requires a name for each
                        // field in metadata.
                        for (i, f) in unnamed.unnamed.iter().enumerate() {
                            let fname = format!("__{}", i);
                            let fty = type_to_string(&f.ty);
                            let fdesc = field_tag_desc(&f.attrs);
                            let structure = if needs_structure(&f.ty) {
                                structure_expr(&f.ty)
                            } else {
                                quote! { None }
                            };
                            let (validations, skip, skip_with_val) = parse_validations(&f.attrs);
                            let validations_expr = validations_expr(&validations);
                            let skip_with_str = skip_with_val.as_deref().unwrap_or("");
                            field_metas.push(quote! {
                                afast::FieldMeta {
                                    name: #fname,
                                    ty: #fty,
                                    desc: #fdesc,
                                    structure: #structure,
                                    validations: #validations_expr,
                                    skip: #skip,
                                    skip_with: #skip_with_str,
                                }
                            });
                        }
                        quote! { &[#( #field_metas ),*] }
                    }
                    Fields::Unit => quote! { &[] },
                };

                variant_entries.push(quote! {
                    afast::EnumVariantMeta {
                        name: #variant_name,
                        fields: #fields,
                    }
                });
            }

            quote! { afast::TagKind::Enum(&[#( #variant_entries ),*]) }
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                Span::call_site(),
                "Tag cannot be derived for unions",
            ));
        }
    };

    let tag_name = format!("__TAG_{}", struct_name_str);
    let tag_ident = syn::Ident::new(&tag_name, Span::call_site());

    Ok(quote! {
        const #tag_ident: afast::TagMeta = afast::TagMeta {
            name: #struct_name_str,
            desc: #desc,
            kind: #tag_kind,
        };

        impl #impl_generics afast::Structure for #struct_name #ty_generics #where_clause {
            fn structure() -> &'static afast::TagMeta {
                &#tag_ident
            }
        }
    })
}

/// Extracts the description string from a `#[tag("...")]` attribute on a field.
///
/// Returns an empty string if the attribute is absent or unparseable.
fn field_tag_desc(attrs: &[Attribute]) -> String {
    attrs
        .iter()
        .find_map(|attr| {
            if attr.path().is_ident("tag") {
                if let Meta::List(list) = &attr.meta {
                    if let Ok(lit) = list.parse_args::<LitStr>() {
                        return Some(lit.value());
                    }
                }
            }
            None
        })
        .unwrap_or_default()
}

/// Converts a `syn::Type` to its string representation, with all whitespace
/// removed so that type comparisons are not affected by formatting differences.
fn type_to_string(ty: &Type) -> String {
    use quote::ToTokens;
    let mut tokens = proc_macro2::TokenStream::new();
    ty.to_tokens(&mut tokens);
    tokens.to_string().replace(' ', "")
}
