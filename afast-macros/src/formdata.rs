//! Derive macro for `FromFormData` trait.
//!
//! Automatically implements `FromFormData` for structs with named fields.
//! Each struct field name must match the corresponding form field name.
//!
//! Supported field types:
//! - `String` — text field value
//! - `i64`, `i32`, `i16`, `i8`, `u64`, `u32`, `u16`, `u8`, `f64`, `f32` — parsed from text
//! - `bool` — parsed from text ("true"/"false"/"1"/"0")
//! - `FileField` — file upload field
//! - `Option<T>` — optional field (defaults to `None` if missing)

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Type};

/// Entry point for `#[derive(FromFormData)]`.
pub fn expand_derive(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "FromFormData can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "FromFormData can only be derived for structs",
            ));
        }
    };

    // Collect all field names for matching
    let mut file_field_names: Vec<String> = Vec::new();
    for field in fields.iter() {
        if is_file_field(&field.ty) {
            file_field_names.push(field.ident.as_ref().unwrap().to_string());
        }
    }

    // Generate the extraction code
    let mut field_extractions = Vec::new();

    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        let field_type = &field.ty;

        let extraction = if is_file_field(field_type) {
            quote! {
                #field_name: __files.remove(#field_name_str).ok_or_else(|| afast::Error::Custom {
                    code: 400,
                    message: format!("required file field '{}' not found", #field_name_str),
                })?,
            }
        } else if is_option(field_type) {
            let inner_type = extract_option_inner(field_type).unwrap();
            let parse_expr = generate_parse_expr(&inner_type);
            quote! {
                #field_name: __text_fields.remove(#field_name_str).map(|__text| #parse_expr),
            }
        } else {
            let parse_expr = generate_parse_expr(field_type);
            quote! {
                #field_name: {
                    let __text = __text_fields.remove(#field_name_str).ok_or_else(|| afast::Error::Custom {
                        code: 400,
                        message: format!("required field '{}' not found", #field_name_str),
                    })?;
                    #parse_expr
                },
            }
        };

        field_extractions.push(extraction);
    }

    let expanded = quote! {
        impl afast::FromFormData for #name {
            fn from_multipart(
                mut __multipart: multer::Multipart<'static>,
            ) -> impl std::future::Future<Output = afast::Result<Self>> + Send {
                async move {
                    // First pass: read ALL fields from the multipart stream
                    let mut __text_fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                    let mut __files: std::collections::HashMap<String, afast::FileField> = std::collections::HashMap::new();

                    while let Some(__f) = __multipart.next_field().await.map_err(|e| {
                        afast::Error::Custom { code: 400, message: format!("multipart error: {}", e) }
                    })? {
                        let __fname = __f.name().unwrap_or("").to_string();

                        // Check if this field name matches a FileField
                        let __is_file = false #(|| __fname == #file_field_names)*;

                        if __is_file {
                            // This is a file field
                            let __filename = __f.file_name().map(|s| s.to_string());
                            let __content_type = __f.content_type().map(|m| m.to_string());
                            let __bytes = __f.bytes().await.map_err(|e| {
                                afast::Error::Custom { code: 400, message: format!("read file error: {}", e) }
                            })?;
                            __files.insert(__fname.clone(), afast::FileField {
                                name: __fname,
                                filename: __filename,
                                content_type: __content_type,
                                bytes: __bytes.to_vec(),
                            });
                        } else {
                            // This is a text field
                            let __text = __f.text().await.map_err(|e| {
                                afast::Error::Custom { code: 400, message: format!("read field error: {}", e) }
                            })?;
                            __text_fields.insert(__fname, __text);
                        }
                    }

                    Ok(#name {
                        #(#field_extractions)*
                    })
                }
            }
        }
    };

    Ok(expanded)
}

/// Check if the type is `FileField`
fn is_file_field(ty: &Type) -> bool {
    if let Type::Path(tp) = ty
        && let Some(seg) = tp.path.segments.last()
    {
        return seg.ident == "FileField";
    }
    false
}

/// Check if the type is `Option<T>`
fn is_option(ty: &Type) -> bool {
    if let Type::Path(tp) = ty
        && let Some(seg) = tp.path.segments.last()
    {
        return seg.ident == "Option";
    }
    false
}

/// Extract inner type from `Option<T>`
fn extract_option_inner(ty: &Type) -> Option<Type> {
    if let Type::Path(tp) = ty
        && let Some(seg) = tp.path.segments.last()
        && let syn::PathArguments::AngleBracketed(args) = &seg.arguments
        && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
    {
        return Some(inner.clone());
    }
    None
}

/// Generate parse expression for a type from string text
fn generate_parse_expr(ty: &Type) -> TokenStream {
    if let Type::Path(tp) = ty
        && let Some(seg) = tp.path.segments.last()
    {
        let ident = seg.ident.to_string();
        match ident.as_str() {
            "String" => return quote! { __text },
            "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" => {
                let ty_ident = &seg.ident;
                return quote! {
                    __text.parse::<#ty_ident>().map_err(|_| afast::Error::Custom {
                        code: 400,
                        message: format!("invalid {} value: '{}'", stringify!(#ty_ident), __text),
                    })?
                };
            }
            "bool" => {
                return quote! {
                    match __text.as_str() {
                        "true" | "1" | "yes" => true,
                        "false" | "0" | "no" => false,
                        _ => return Err(afast::Error::Custom {
                            code: 400,
                            message: format!("invalid bool value: '{}'", __text),
                        }),
                    }
                };
            }
            _ => {}
        }
    }
    // Fallback: try to parse as string
    quote! {
        __text.parse().map_err(|_| afast::Error::Custom {
            code: 400,
            message: format!("cannot parse '{}' to target type", __text),
        })?
    }
}
