//! JavaScript client-code generator.
//!
//! Produces an ESM module containing a service-specific `Client` class.
//! The generated code communicates with the server over the afast binary
//! protocol (via WebSocket, TCP, or HTTP POST to `/api`) with optional
//! support for ordinary JSON-over-HTTP handlers. All Rust types referenced
//! by handlers are emitted as JSDoc `@typedef` blocks so that editors can
//! provide type hints without a TypeScript compilation step.

#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::only_used_in_recursion
)]

use super::buf::CodeBuf;
use crate::{AFast, Error, Handler, HandlerMeta, ParamMeta, Service, TagKind, TagMeta};
use std::path::Path;

/// Returns an iterator over fields that should be included in generated code,
/// filtering out `skip` fields and `skip_with` fields whose marker matches.
fn included(
    fields: &[crate::handler::FieldMeta],
) -> impl Iterator<Item = &crate::handler::FieldMeta> {
    fields
        .iter()
        .filter(|f| crate::marker::should_include_field(f))
}

// ─── Enum tag size (must match afastdata feature) ─────────────

/// Returns the JavaScript writer method name for the enum tag integer,
/// selected at compile time by the `tag-u32` / `tag-u16` feature flags.
fn tag_write() -> &'static str {
    if cfg!(feature = "tag-u32") {
        "wU32"
    } else if cfg!(feature = "tag-u16") {
        "wU16"
    } else {
        "wU8"
    }
}

/// Returns the JavaScript reader method name for the enum tag integer,
/// mirroring the feature-gated choice in `tag_write`.
fn tag_read() -> &'static str {
    if cfg!(feature = "tag-u32") {
        "rU32"
    } else if cfg!(feature = "tag-u16") {
        "rU16"
    } else {
        "rU8"
    }
}

// ─── Type helpers ───────────────────────────────────────────────

/// Strips `afast::` path prefixes and normalises whitespace in generic type
/// parameters so that type string comparisons are consistent regardless of
/// how the Rust compiler formats the type.
fn normalize_rust_type(ty: &str) -> String {
    let ty = ty.trim();
    let ty = ty.strip_prefix("afast :: ").unwrap_or(ty);
    let ty = ty.strip_prefix("afast::").unwrap_or(ty);
    ty.replace(" <", "<")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace("> ", ">")
        .replace(" ,", ",")
        .replace(", ", ",")
}

/// If the given Rust type is `Json<T>`, returns the inner type `T`.
/// `Json<T>` is deserialised as JSON in the generated client rather than
/// through the binary reader, so the inner type must be extracted for
/// code generation.
fn unwrap_json_type(ty: &str) -> Option<String> {
    let ty = normalize_rust_type(ty);
    if ty.starts_with("Json<") && ty.ends_with('>') {
        let inner = &ty[5..ty.len() - 1].trim();
        Some(normalize_rust_type(inner))
    } else {
        None
    }
}

/// Maps a Rust type name to the corresponding JavaScript type for use
/// in JSDoc annotations. Handles primitives, `Option<T>`, `Vec<T>`,
/// and `Vec<u8>` (which maps to `Uint8Array`).
fn rust_type_to_js(ty: &str) -> String {
    let ty = normalize_rust_type(ty);
    match ty.as_str() {
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "f32" | "f64" => {
            "number".to_string()
        }
        "bool" => "boolean".to_string(),
        "String" | "&str" => "string".to_string(),
        "Vec<u8>" => "Uint8Array".to_string(),
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            format!("{}|null", rust_type_to_js(inner))
        }
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            let js_inner = rust_type_to_js(inner);
            format!("{}[]", js_inner)
        }
        other => other.to_string(),
    }
}

// ─── Helpers ──────────────────────────────────────────────────

/// Converts a snake_case string to PascalCase.
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut c = word.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

/// Combines a handler path prefix with a type name, detecting and eliminating
/// overlapping suffix/prefix segments to avoid duplicate name components.
fn prefixed_type(prefix: &str, type_name: &str) -> String {
    if type_name.starts_with(prefix) {
        return type_name.to_string();
    }
    // Detect suffix-prefix overlap to avoid doubling e.g. "TestNewFeaturesTestNewFeatures".
    for i in 1..prefix.len() {
        let suffix = &prefix[i..];
        if type_name.starts_with(suffix) {
            return format!("{}{}", &prefix[..i], type_name);
        }
    }
    format!("{}{}", prefix, type_name)
}

/// Produces the conventional suffix appended to handler-prefixed type names
/// for ordinary (non-binary) HTTP handlers. The first `Data` extractor
/// becomes `Request`, subsequent ones become `Request2`, `Request3`, etc.
fn ordinary_suffix(extractor: &str, data_idx: usize) -> String {
    match extractor {
        "Data" if data_idx == 0 => "Request".to_string(),
        "Data" => format!("Request{}", data_idx + 1),
        "Query" => "Query".to_string(),
        "Param" => "PathParams".to_string(),
        "Body" => "Body".to_string(),
        _ => "Response".to_string(),
    }
}

fn handler_prefix(path: &[&str]) -> String {
    path.iter()
        .map(|s| to_pascal_case(s.trim_start_matches(':')))
        .collect::<String>()
}

/// Metadata for a single collected custom/header type.
struct CustomInfo {
    ty: String,
}

/// Collects all unique Custom extractor types from the handler tree depth-first.
fn collect_customs(handlers: &[Handler]) -> Vec<CustomInfo> {
    let mut customs: Vec<CustomInfo> = Vec::new();
    collect_customs_recursive(handlers, &mut customs);
    customs
}

fn collect_customs_recursive(handlers: &[Handler], customs: &mut Vec<CustomInfo>) {
    for h in handlers {
        for param in h.meta.params {
            if param.extractor == "Custom" {
                let ty = param.ty.to_string();
                if !customs.iter().any(|c| c.ty == ty) {
                    customs.push(CustomInfo { ty });
                }
            }
        }
        collect_customs_recursive(&h.children, customs);
    }
}

/// Returns the index of a Custom type within the handler tree's collected customs,
/// used to bind the correct `_customs[i]` call in generated client code.
fn custom_index(handlers: &[Handler], ty: &str) -> usize {
    let customs = collect_customs(handlers);
    customs.iter().position(|c| c.ty == ty).unwrap_or(0)
}

/// Collects all unique Header extractor types from the handler tree,
/// excluding headers with only standard fields (content-type, authorization, etc.).
fn collect_headers(handlers: &[Handler]) -> Vec<CustomInfo> {
    let mut headers: Vec<CustomInfo> = Vec::new();
    collect_headers_recursive(handlers, &mut headers);
    headers
}

fn collect_headers_recursive(handlers: &[Handler], headers: &mut Vec<CustomInfo>) {
    for h in handlers {
        for param in h.meta.params {
            if param.extractor == "Header" {
                let ty = param.ty.to_string();
                if !headers.iter().any(|c| c.ty == ty) {
                    // Skip types whose fields are all standard headers (content-type, etc.),
                    // since those are already handled by the generated code.
                    let has_non_standard = param
                        .structure
                        .map(|s| {
                            let structure = s();
                            match structure.kind {
                                crate::handler::TagKind::Struct(fields) => fields
                                    .iter()
                                    .any(|f| !crate::is_standard_header(&f.name.replace('_', "-"))),
                                _ => true,
                            }
                        })
                        .unwrap_or(true);
                    if has_non_standard {
                        headers.push(CustomInfo { ty });
                    }
                }
            }
        }
        collect_headers_recursive(&h.children, headers);
    }
}

/// Recursively checks whether any handler in the tree is marked as an
/// ordinary (non-binary, JSON-over-HTTP) handler. This determines whether
/// the generated client class needs `_request` methods and field definitions.
fn has_ordinary_handlers(handlers: &[Handler]) -> bool {
    for h in handlers {
        if h.meta.is_ordinary {
            return true;
        }
        if has_ordinary_handlers(&h.children) {
            return true;
        }
    }
    false
}

/// Recursively checks whether any handler in the tree has `cache_seconds > 0`.
fn has_cache_handlers_js(handlers: &[Handler]) -> bool {
    for h in handlers {
        if h.meta.cache_seconds > 0 {
            return true;
        }
        if has_cache_handlers_js(&h.children) {
            return true;
        }
    }
    false
}

// ─── JSDoc typedef generation ─────────────────────────────────

/// Emits a JSDoc `@typedef` block for a Header struct. Fields that
/// correspond to standard HTTP headers (content-type, authorization, etc.)
/// are omitted from the typedef because the generated runtime already
/// handles them automatically.
fn jsdoc_typedef_header(meta: &TagMeta) -> String {
    match meta.kind {
        TagKind::Struct(fields) => {
            let mut lines = CodeBuf::new();
            lines.push("/**".to_string());
            if !meta.desc.is_empty() {
                lines.push(format!(" * @description {}", meta.desc));
            }
            lines.push(format!(" * @typedef {{Object}} {}", meta.name));
            for field in included(fields) {
                if crate::is_standard_header(&field.name.replace('_', "-")) {
                    continue;
                }
                let js_ty = rust_type_to_js(field.ty);
                lines.push(format!(" * @property {{{}}} {}", js_ty, field.name));
            }
            lines.push(" */".to_string());
            lines.build()
        }
        _ => jsdoc_typedef(meta),
    }
}

/// Emits JSDoc `@typedef` blocks for a struct or enum type using its
/// original Rust name. Structs produce a single object typedef. Enums
/// produce one typedef per variant (e.g. `Foo_VariantA`) plus a union
/// typedef that joins all variant names.
fn jsdoc_typedef(meta: &TagMeta) -> String {
    match meta.kind {
        TagKind::Struct(fields) => {
            let mut lines = CodeBuf::new();
            lines.push("/**".to_string());
            let desc = if meta.desc.is_empty() {
                meta.name.to_string()
            } else {
                meta.desc.to_string()
            };
            lines.push(format!(" * @typedef {{Object}} {}", meta.name));
            if !desc.is_empty() && desc != meta.name {
                lines.push(format!(" * @description {}", desc));
            }
            for field in included(fields) {
                let js_ty = rust_type_to_js(field.ty);
                lines.push(format!(" * @property {{{}}} {}", js_ty, field.name));
            }
            lines.push(" */".to_string());
            lines.build()
        }
        TagKind::Enum(variants) => {
            let mut lines = CodeBuf::new();
            for variant in variants {
                let variant_name = format!("{}_{}", meta.name, variant.name);
                lines.push("/**".to_string());
                lines.push(format!(" * @typedef {{Object}} {}", variant_name));
                lines.push(format!(" * @property {{'{}'}} tag", variant.name));
                if variant.fields.is_empty() {
                    lines.push(" * @property {null} data".to_string());
                } else if variant.fields.len() == 1 && variant.fields[0].name.starts_with("__") {
                    let js_ty = rust_type_to_js(variant.fields[0].ty);
                    lines.push(format!(" * @property {{{}}} data", js_ty));
                } else {
                    lines.push(" * @property {Object} data".to_string());
                    for field in included(variant.fields) {
                        let js_ty = rust_type_to_js(field.ty);
                        lines.push(format!(" * @property {{{}}} data.{}", js_ty, field.name));
                    }
                }
                lines.push(" */".to_string());
                lines.b();
            }
            // Union type
            let variant_names: Vec<String> = variants
                .iter()
                .map(|v| format!("{}_{}", meta.name, v.name))
                .collect();
            lines.push("/**".to_string());
            lines.push(format!(
                " * @typedef {{{}}} {}",
                variant_names.join(" | "),
                meta.name
            ));
            if !meta.desc.is_empty() && meta.desc != meta.name {
                lines.push(format!(" * @description {}", meta.desc));
            }
            lines.push(" */".to_string());
            lines.build()
        }
    }
}

/// Like `jsdoc_typedef` but emits the typedef under an explicit name
/// provided by the caller. This is necessary when the generated type name
/// differs from the original Rust type name (e.g. when a handler prefix
/// is prepended or a suffix such as `Request`/`Response` is appended).
fn jsdoc_typedef_named(type_name: &str, meta: &TagMeta) -> String {
    match meta.kind {
        TagKind::Struct(fields) => {
            let mut lines = CodeBuf::new();
            lines.push("/**".to_string());
            lines.push(format!(" * @typedef {{Object}} {}", type_name));
            if !meta.desc.is_empty() {
                lines.push(format!(" * @description {}", meta.desc));
            }
            for field in included(fields) {
                let js_ty = rust_type_to_js(field.ty);
                lines.push(format!(" * @property {{{}}} {}", js_ty, field.name));
            }
            lines.push(" */".to_string());
            lines.build()
        }
        TagKind::Enum(variants) => {
            let mut lines = CodeBuf::new();
            for variant in variants {
                let variant_name = format!("{}_{}", type_name, variant.name);
                lines.push("/**".to_string());
                lines.push(format!(" * @typedef {{Object}} {}", variant_name));
                lines.push(format!(" * @property {{'{}'}} tag", variant.name));
                if variant.fields.is_empty() {
                    lines.push(" * @property {null} data".to_string());
                } else if variant.fields.len() == 1 && variant.fields[0].name.starts_with("__") {
                    let js_ty = rust_type_to_js(variant.fields[0].ty);
                    lines.push(format!(" * @property {{{}}} data", js_ty));
                } else {
                    lines.push(" * @property {Object} data".to_string());
                    for field in included(variant.fields) {
                        let js_ty = rust_type_to_js(field.ty);
                        lines.push(format!(" * @property {{{}}} data.{}", js_ty, field.name));
                    }
                }
                lines.push(" */".to_string());
                lines.b();
            }
            let variant_names: Vec<String> = variants
                .iter()
                .map(|v| format!("{}_{}", type_name, v.name))
                .collect();
            lines.push("/**".to_string());
            lines.push(format!(
                " * @typedef {{{}}} {}",
                variant_names.join(" | "),
                type_name
            ));
            if !meta.desc.is_empty() {
                lines.push(format!(" * @description {}", meta.desc));
            }
            lines.push(" */".to_string());
            lines.build()
        }
    }
}

// ─── Type exports (recursive) ─────────────────────────────────

/// Generates JSDoc typedefs for every type that a single handler references:
/// custom extractor types, header types, data/query/param/body types, and the
/// return type. Each type is emitted only once (tracked by the `emitted` set).
/// Nested structures discovered through `FieldMeta.structure` pointers are
/// recursively expanded by `extract_nested_types_js`.
fn handler_type_exports_js(prefix: &str, meta: &HandlerMeta, emitted: &mut Vec<String>) -> String {
    let mut lines = CodeBuf::new();

    let mut data_idx = 0;
    for param in meta.params {
        match param.extractor {
            "Custom" => {
                if let Some(structure_fn) = param.structure {
                    let structure = structure_fn();
                    if !emitted.contains(&structure.name.to_string()) {
                        lines.push(jsdoc_typedef(structure));
                        emitted.push(structure.name.to_string());
                        extract_nested_types_js(structure, &mut lines, emitted);
                    }
                }
            }
            "Header" => {
                if let Some(structure_fn) = param.structure {
                    let structure = structure_fn();
                    if !emitted.contains(&structure.name.to_string()) {
                        lines.push(jsdoc_typedef_header(structure));
                        emitted.push(structure.name.to_string());
                        extract_nested_types_js(structure, &mut lines, emitted);
                    }
                }
            }
            "Data" | "Query" | "Param" | "Body" => {
                if let Some(structure_fn) = param.structure {
                    let structure = structure_fn();
                    let type_name = if meta.is_ordinary {
                        let suffix = ordinary_suffix(param.extractor, data_idx);
                        if param.extractor == "Data" {
                            data_idx += 1;
                        }
                        format!("{}{}", prefix, suffix)
                    } else {
                        prefixed_type(prefix, structure.name)
                    };
                    if !emitted.contains(&type_name) {
                        lines.push(jsdoc_typedef_named(&type_name, structure));
                        emitted.push(type_name.clone());
                        emitted.push(structure.name.to_string());
                        extract_nested_types_js(structure, &mut lines, emitted);
                    }
                }
            }
            _ => {}
        }
    }

    if meta.return_type != "()"
        && let Some(structure_fn) = meta.return_structure
    {
        let structure = structure_fn();
        let type_name = if meta.is_ordinary {
            format!("{}Response", prefix)
        } else {
            prefixed_type(prefix, structure.name)
        };
        if !emitted.contains(&type_name) {
            lines.push(jsdoc_typedef_named(&type_name, structure));
            emitted.push(type_name.clone());
            emitted.push(structure.name.to_string());
            extract_nested_types_js(structure, &mut lines, emitted);
        }
    }

    lines.build()
}

/// Depth-first traversal of nested struct/enum types reachable from a
/// type's fields. For each type not already in `emitted`, emits a JSDoc
/// typedef and recurses into its children. This ensures that every
/// referenced complex type appears in the generated output exactly once.
fn extract_nested_types_js(meta: &'static TagMeta, lines: &mut CodeBuf, emitted: &mut Vec<String>) {
    if !emitted.contains(&meta.name.to_string()) {
        emitted.push(meta.name.to_string());
        lines.push(jsdoc_typedef(meta));
    }
    match meta.kind {
        TagKind::Struct(fields) => {
            for field in included(fields) {
                if let Some(structure_fn) = field.structure {
                    extract_nested_types_js(structure_fn(), lines, emitted);
                }
            }
        }
        TagKind::Enum(variants) => {
            for variant in variants {
                for field in included(variant.fields) {
                    if let Some(structure_fn) = field.structure {
                        extract_nested_types_js(structure_fn(), lines, emitted);
                    }
                }
            }
        }
    }
}

// ─── Request serialization ────────────────────────────────────

/// Emits JavaScript statements that write a value into the afast binary
/// writer (`w`). Recursively handles primitives, `Vec<T>`, `Option<T>`,
/// structs (via field iteration), and enums (via a `switch` on the `tag`
/// property). When no `structure` metadata is available the value is
/// serialised as raw bytes via `wBytes`.
fn generate_request_serialize_js(
    lines: &mut CodeBuf,
    var: &str,
    ty: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
) {
    let ty = normalize_rust_type(ty);
    match ty.as_str() {
        "i8" => lines.push(format!("{}w.wI8({});", indent, var)),
        "i16" => lines.push(format!("{}w.wI16({});", indent, var)),
        "i32" => lines.push(format!("{}w.wI32({});", indent, var)),
        "i64" => lines.push(format!("{}w.wI64({});", indent, var)),
        "u8" => lines.push(format!("{}w.wU8({});", indent, var)),
        "u16" => lines.push(format!("{}w.wU16({});", indent, var)),
        "u32" => lines.push(format!("{}w.wU32({});", indent, var)),
        "u64" => lines.push(format!("{}w.wU64({});", indent, var)),
        "usize" => lines.push(format!("{}w.wU64({});", indent, var)),
        "f32" => lines.push(format!("{}w.wF32({});", indent, var)),
        "f64" => lines.push(format!("{}w.wF64({});", indent, var)),
        "bool" => lines.push(format!("{}w.wB({});", indent, var)),
        "String" | "&str" => lines.push(format!("{}w.wS({});", indent, var)),
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            lines.push(format!("{}w.wU32({}.length);", indent, var));
            lines.push(format!("{}for(const _e of {}){{", indent, var));
            generate_request_serialize_js(
                lines,
                "_e",
                inner,
                &format!("{}    ", indent),
                structure,
            );
            lines.push(format!("{}}}", indent));
        }
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            lines.push(format!(
                "{}if({}===null){{w.wU8(0);}}else{{w.wU8(1);",
                indent, var
            ));
            generate_request_serialize_js(lines, var, inner, &format!("{}    ", indent), structure);
            lines.push(format!("{}}}", indent));
        }
        _ => {
            if let Some(s) = structure {
                let meta = s();
                match meta.kind {
                    TagKind::Struct(fields) => {
                        for field in included(fields) {
                            let field_var = format!("{}.{}", var, field.name);
                            generate_request_serialize_js(
                                lines,
                                &field_var,
                                field.ty,
                                indent,
                                field.structure,
                            );
                        }
                    }
                    TagKind::Enum(variants) => {
                        let tw = tag_write();
                        let deep_indent = format!("{}        ", indent);
                        lines.push(format!("{}switch({}.tag){{", indent, var));
                        for (i, variant) in variants.iter().enumerate() {
                            if variant.fields.is_empty() {
                                lines.push(format!(
                                    "{}case '{}': w.{}({}); break;",
                                    deep_indent, variant.name, tw, i
                                ));
                            } else if variant.fields.len() == 1
                                && variant.fields[0].name.starts_with("__")
                            {
                                lines.push(format!("{}case '{}': {{", deep_indent, variant.name));
                                lines.push(format!("{}w.{}({});", deep_indent, tw, i));
                                let field_var = format!("{}.data", var);
                                generate_request_serialize_js(
                                    lines,
                                    &field_var,
                                    variant.fields[0].ty,
                                    &deep_indent,
                                    variant.fields[0].structure,
                                );
                                lines.push(format!("{}break;", deep_indent));
                                lines.push(format!("{}}}", deep_indent));
                            } else {
                                lines.push(format!("{}case '{}': {{", deep_indent, variant.name));
                                lines.push(format!("{}w.{}({});", deep_indent, tw, i));
                                for field in included(variant.fields) {
                                    let field_var = format!("{}.data.{}", var, field.name);
                                    generate_request_serialize_js(
                                        lines,
                                        &field_var,
                                        field.ty,
                                        &deep_indent,
                                        field.structure,
                                    );
                                }
                                lines.push(format!("{}break;", deep_indent));
                                lines.push(format!("{}}}", deep_indent));
                            }
                        }
                        lines.push(format!("{}}}", indent));
                    }
                }
            } else {
                lines.push(format!("{}w.wBytes({}._raw);", indent, var));
            }
        }
    }
}

// ─── Response deserialization ─────────────────────────────────

/// Returns a JavaScript expression that deserialises a single value of
/// the given Rust type from the afast binary reader (`r`). The returned
/// string is a self-contained expression (not a statement). Recurses into
/// `Vec<T>` (wrapped in `Array.from`), `Option<T>` (guarded by a `rU8`
/// check), structs (inline object literal), and enums (IIFE with `switch`).
fn response_expr_js(
    reader: &str,
    ty: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
) -> String {
    let ty = normalize_rust_type(ty);
    match ty.as_str() {
        "i8" => format!("{}.rI8()", reader),
        "i16" => format!("{}.rI16()", reader),
        "i32" => format!("{}.rI32()", reader),
        "i64" => format!("{}.rI64()", reader),
        "u8" => format!("{}.rU8()", reader),
        "u16" => format!("{}.rU16()", reader),
        "u32" => format!("{}.rU32()", reader),
        "u64" => format!("{}.rU64()", reader),
        "usize" => format!("{}.rU64()", reader),
        "f32" => format!("{}.rF32()", reader),
        "f64" => format!("{}.rF64()", reader),
        "bool" => format!("{}.rB()", reader),
        "String" | "&str" => format!("{}.rS()", reader),
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            let elem = response_expr_js(reader, inner, indent, structure);
            format!("Array.from({{length:{}.rU32()}},()=>({}))", reader, elem)
        }
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            let body = response_expr_js(reader, inner, indent, structure);
            format!(
                "{}.rU8()===1?(()=>{{ const _t={}; return _t; }})():null",
                reader, body
            )
        }
        _ => {
            if let Some(s) = structure {
                let meta = s();
                match meta.kind {
                    TagKind::Struct(fields) => {
                        let inner_indent = format!("{}    ", indent);
                        let field_lines: Vec<String> = included(fields)
                            .map(|f| {
                                let expr =
                                    response_expr_js(reader, f.ty, &inner_indent, f.structure);
                                format!("{}{}: {},", inner_indent, f.name, expr)
                            })
                            .collect();
                        format!("{{\n{}\n{}}}", field_lines.join("\n"), indent)
                    }
                    TagKind::Enum(variants) => {
                        let tr = tag_read();
                        let inner_indent = format!("{}    ", indent);
                        let deep_indent = format!("{}        ", indent);
                        let mut arms = Vec::new();
                        for (i, variant) in variants.iter().enumerate() {
                            if variant.fields.is_empty() {
                                arms.push(format!(
                                    "{}case {}: return {{ tag: '{}', data: null }};",
                                    deep_indent, i, variant.name
                                ));
                            } else if variant.fields.len() == 1
                                && variant.fields[0].name.starts_with("__")
                            {
                                let expr = response_expr_js(
                                    reader,
                                    variant.fields[0].ty,
                                    &deep_indent,
                                    variant.fields[0].structure,
                                );
                                arms.push(format!(
                                    "{}case {}: return {{ tag: '{}', data: {} }};",
                                    deep_indent, i, variant.name, expr
                                ));
                            } else {
                                let mut field_entries = Vec::new();
                                for field in included(variant.fields) {
                                    let expr = response_expr_js(
                                        reader,
                                        field.ty,
                                        &format!("{}        ", inner_indent),
                                        field.structure,
                                    );
                                    field_entries.push(format!(
                                        "{}    {}: {},",
                                        deep_indent, field.name, expr
                                    ));
                                }
                                arms.push(format!(
                                    "{}case {}: return {{ tag: '{}', data: {{\n{}\n{}    }} }};",
                                    deep_indent,
                                    i,
                                    variant.name,
                                    field_entries.join("\n"),
                                    deep_indent
                                ));
                            }
                        }
                        format!(
                            "(()=>{{ const _t={}.{}(); switch(_t) {{\n{}\n{}}}}})()",
                            reader,
                            tr,
                            arms.join("\n"),
                            inner_indent
                        )
                    }
                }
            } else {
                format!("{}.rBytes({}.rU32())", reader, reader)
            }
        }
    }
}

/// Appends JavaScript statements to `lines` that deserialise the handler
/// return value from the binary reader. When `debug` and `emit_return` are
/// both true, the result is first assigned to `_result`, logged via
/// `console.log`, then returned. Otherwise the expression is returned
/// directly. Unit type `()` produces an empty object literal.
fn generate_return_js(
    lines: &mut CodeBuf,
    reader: &str,
    ty: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
    emit_return: bool,
    debug: bool,
    func_name: &str,
) {
    let ret = if emit_return {
        "return "
    } else {
        "const _result = "
    };
    if ty == "()" {
        lines.push(format!("{}{}{{}};", indent, ret));
        return;
    }
    let expr = response_expr_js(reader, ty, indent, structure);
    if debug && emit_return {
        lines.push(format!("{}const _result = {};", indent, expr));
        lines.push(format!(
            "{}if (this._debug) console.log('[afast:debug] ← {}', JSON.stringify(_result));",
            indent, func_name
        ));
        lines.push(format!("{}return _result;", indent));
    } else {
        lines.push(format!("{}{}{};", indent, ret, expr));
    }
}

// ─── Handler method generation ────────────────────────────────

/// Generates a JavaScript async arrow-function method for a binary-protocol
/// handler. The generated code: (1) collects custom extractor values,
/// (2) validates all arguments against `#[validate]` rules, (3) serialises
/// the payload with `_writer()`, (4) dispatches through `this._call(handlerId, data)`.
/// Long-connection handlers additionally create a `Socket` and register a
/// push handler before returning.
fn handler_method_js(
    handler: &Handler,
    all_handlers: &[Handler],
    _prefix: &str,
    base_indent: &str,
    debug: bool,
    class_name: &str,
    cache_key: &str,
) -> String {
    let meta = handler.meta;
    let func_name = if !meta.api_name.is_empty() {
        meta.api_name
    } else {
        meta.name
    };
    let cache_seconds = meta.cache_seconds;
    let id = handler.stable_id;
    let indent = format!("{}    ", base_indent);

    let mut custom_indices: Vec<(usize, &str, Option<fn() -> &'static TagMeta>)> = Vec::new();
    let mut data_params: Vec<(String, &ParamMeta)> = Vec::new();

    for param in meta.params {
        match param.extractor {
            "State" | "Receiver" | "Sender" => continue,
            "Custom" => {
                let idx = custom_index(all_handlers, param.ty);
                custom_indices.push((idx, param.ty, param.structure));
            }
            "Data" => {
                let idx = data_params.len();
                let var = if idx == 0 {
                    "request".to_string()
                } else {
                    format!("request{}", idx + 1)
                };
                data_params.push((var, param));
            }
            _ => {}
        }
    }

    // Function params (only Data params, no types)
    let mut fn_params = Vec::new();
    for &(ref var, _param) in &data_params {
        fn_params.push(var.clone());
    }

    // For long connections, add callback parameter
    if meta.long_connection {
        fn_params.push("callback".to_string());
    }

    // Cache: add force param
    if cache_seconds > 0 {
        fn_params.push("force = false".to_string());
    }

    let params_str = fn_params.join(", ");

    // Build body
    let ind = format!("{}    ", indent);
    let mut body_lines = CodeBuf::new();

    // Cache check (before serialization, so we skip work on cache hit)
    if cache_seconds > 0 {
        let req_vars: Vec<String> = data_params.iter().map(|(v, _)| v.clone()).collect();
        let req_expr = if req_vars.is_empty() {
            "\"[]\"".to_string()
        } else {
            format!("JSON.stringify([{}])", req_vars.join(", "))
        };
        body_lines.push(format!("{}const _cacheKey = \"{}\";", ind, cache_key));
        body_lines.push(format!("{}const _currentReq = {};", ind, req_expr));
        body_lines.push(format!("{}if (!force) {{", ind));
        body_lines.push(format!(
            "{}    const _cached = {client}Client._cache.get(_cacheKey);",
            ind,
            client = class_name
        ));
        body_lines.push(format!(
            "{}    if (_cached && Date.now() < _cached.expiry && _currentReq === _cached.request) return _cached.response;",
            ind
        ));
        body_lines.push(format!("{}}}", ind));
    }

    body_lines.push(format!("{}const w=this._writer();", ind));

    // Debug: log request
    if debug {
        if data_params.is_empty() {
            body_lines.push(format!(
                "{}if (this._debug) console.log('[afast:debug] → {}');",
                ind, func_name
            ));
        } else if data_params.len() == 1 {
            body_lines.push(format!(
                "{}if (this._debug) console.log('[afast:debug] → {}', JSON.stringify({}));",
                ind, func_name, data_params[0].0
            ));
        } else {
            let vars: Vec<String> = data_params.iter().map(|(v, _)| v.clone()).collect();
            body_lines.push(format!(
                "{}if (this._debug) console.log('[afast:debug] → {}', JSON.stringify([{}]));",
                ind,
                func_name,
                vars.join(", ")
            ));
        }
    }

    // Call custom functions first
    for &(ci, _ty, _structure) in &custom_indices {
        let var = format!("_c{}", ci);
        body_lines.push(format!(
            "{}const {}=await this._customs[{}]();",
            ind, var, ci
        ));
    }

    // Validate custom params
    for &(ci, _ty, structure) in &custom_indices {
        let var = format!("_c{}", ci);
        if let Some(structure_fn) = structure {
            let meta = structure_fn();
            if let TagKind::Struct(fields) = meta.kind {
                generate_validation_js(&mut body_lines, &var, fields, &ind);
            }
        }
    }

    // Validate data params
    for &(ref var, param) in &data_params {
        if let Some(structure_fn) = param.structure {
            let structure = structure_fn();
            if let TagKind::Struct(fields) = structure.kind {
                generate_validation_js(&mut body_lines, var, fields, &ind);
            }
        }
    }

    // Serialize customs
    for &(ci, _ty, structure) in &custom_indices {
        let var = format!("_c{}", ci);
        if let Some(structure_fn) = structure {
            let meta = structure_fn();
            if let TagKind::Struct(fields) = meta.kind {
                for field in included(fields) {
                    let field_var = format!("{}.{}", var, field.name);
                    generate_request_serialize_js(
                        &mut body_lines,
                        &field_var,
                        field.ty,
                        &ind,
                        field.structure,
                    );
                }
            }
        }
    }

    // Serialize data params
    for &(ref var, param) in &data_params {
        generate_request_serialize_js(&mut body_lines, var, param.ty, &ind, param.structure);
    }

    body_lines.push(format!("{}const _data=w.toBytes();", ind));
    body_lines.push(format!(
        "{}const _resp=await this._call({}, _data);",
        ind, id
    ));

    if meta.long_connection {
        body_lines.push(format!("{}const r=this._reader(_resp);", ind));
        body_lines.push(format!("{}const connId=r.rU32();", ind));
        body_lines.push(format!(
            "{}const socket=new Socket(connId,this,callback);",
            ind
        ));
        body_lines.push(format!(
            "{}this._pushHandlers.set(connId,(raw)=>socket._onMessage(raw));",
            ind
        ));
        body_lines.push(format!("{}return socket;", ind));
    } else {
        body_lines.push(format!("{}const r=this._reader(_resp);", ind));
        if cache_seconds > 0 {
            generate_return_js(
                &mut body_lines,
                "r",
                meta.return_type,
                &ind,
                meta.return_structure,
                false,
                debug,
                func_name,
            );
            body_lines.push(format!(
                "{ind}{client}Client._cache.set(_cacheKey, {{ request: _currentReq, response: _result, expiry: Date.now() + {cache_ms} }});",
                ind = ind,
                client = class_name,
                cache_ms = cache_seconds * 1000,
            ));
            if debug {
                body_lines.push(format!(
                    "{}if (this._debug) console.log('[afast:debug] ← {}', JSON.stringify(_result));",
                    ind, func_name
                ));
            }
            body_lines.push(format!("{}return _result;", ind));
        } else {
            generate_return_js(
                &mut body_lines,
                "r",
                meta.return_type,
                &ind,
                meta.return_structure,
                true,
                debug,
                func_name,
            );
        }
    }

    let body = body_lines.build();

    format!(
        "{indent}{func_name}:async({params_str})=>{{\n{body}\n{indent}}},\n",
        indent = indent,
        func_name = func_name,
        params_str = params_str,
        body = body,
    )
}

// ─── Nested handler object generation ─────────────────────────

/// Generates a JavaScript async method for an ordinary (non-binary, JSON-over-HTTP)
/// handler. The generated code builds a URL with path-parameter substitution and
/// query-string assembly, merges global header functions, and dispatches through
/// `this._request(url, opts)`. The response is returned directly as parsed JSON.
fn ordinary_handler_method_js(
    handler: &Handler,
    prefix: &str,
    group_path: &[&str],
    base_indent: &str,
    header_count: usize,
    debug: bool,
    class_name: &str,
    cache_key: &str,
) -> String {
    let meta = handler.meta;
    let func_name = if !meta.api_name.is_empty() {
        meta.api_name
    } else {
        meta.name
    };
    let cache_seconds = meta.cache_seconds;
    let method = if meta.method.is_empty() {
        "GET"
    } else {
        meta.method
    };
    let indent = format!("{}    ", base_indent);
    let ind = format!("{}    ", indent);

    let mut param_param: Option<&ParamMeta> = None;
    let mut query_param: Option<&ParamMeta> = None;
    let mut body_param: Option<&ParamMeta> = None;
    for param in meta.params {
        match param.extractor {
            "Param" => param_param = Some(param),
            "Query" => query_param = Some(param),
            "Body" => body_param = Some(param),
            _ => {}
        }
    }

    // Build data fields for the function parameter
    let mut data_fields: Vec<String> = Vec::new();
    if let Some(p) = param_param {
        if p.structure.is_some() {
            data_fields.push(format!("params: {}{}", prefix, "PathParams"));
        } else {
            data_fields.push(format!("params: {}", rust_type_to_js(p.ty)));
        }
    }
    if let Some(p) = query_param {
        if p.structure.is_some() {
            data_fields.push(format!("queries: {}{}", prefix, "Query"));
        } else {
            data_fields.push(format!("queries: {}", rust_type_to_js(p.ty)));
        }
    }
    if let Some(p) = body_param {
        if p.structure.is_some() {
            data_fields.push(format!("body: {}{}", prefix, "Body"));
        } else {
            data_fields.push(format!("body: {}", rust_type_to_js(p.ty)));
        }
    }

    let fn_sig = if cache_seconds > 0 {
        if data_fields.is_empty() {
            format!("{}:async(force = false)=>", func_name)
        } else {
            format!("{}:async(request, force = false)=>", func_name)
        }
    } else {
        if data_fields.is_empty() {
            format!("{}:async()=>", func_name)
        } else {
            format!("{}:async(request)=>", func_name)
        }
    };

    // Build method body
    let mut body_lines = CodeBuf::new();

    let group_prefix = group_path.join("/");
    let normalized_path = if !handler.path.is_empty() && !handler.path.starts_with('/') {
        format!("/{}", handler.path)
    } else {
        handler.path.to_string()
    };
    let full_path = if group_prefix.is_empty() {
        normalized_path
    } else {
        format!("/{}{}", group_prefix, normalized_path)
    };

    body_lines.push(format!("{}let url = this._url + '{}';", ind, full_path));

    // Substitute path params
    if let Some(p) = param_param
        && let Some(s) = p.structure
    {
        let structure = s();
        match structure.kind {
            TagKind::Struct(fields) => {
                for field in included(fields) {
                    body_lines.push(format!(
                            "{}url = url.replace(':' + '{}', encodeURIComponent(String(request.params.{})));",
                            ind, field.name, field.name
                        ));
                }
            }
            _ => {
                body_lines.push(format!(
                        "{}url = url.replace(/:([^/]+)/, () => encodeURIComponent(String(request.params)));",
                        ind
                    ));
            }
        }
    }

    // Build query string
    if let Some(p) = query_param {
        body_lines.push(format!("{}const qs = new URLSearchParams();", ind));
        if let Some(s) = p.structure {
            let structure = s();
            match structure.kind {
                TagKind::Struct(fields) => {
                    for field in included(fields) {
                        body_lines.push(format!(
                            "{}if (request.queries.{} != null) qs.append('{}', String(request.queries.{}));",
                            ind, field.name, field.name, field.name
                        ));
                    }
                }
                _ => {
                    body_lines.push(format!(
                        "{}if (request.queries != null) qs.append('{}', String(request.queries));",
                        ind, p.name
                    ));
                }
            }
        }
        body_lines.push(format!(
            "{}if (qs.toString()) url += '?' + qs.toString();",
            ind
        ));
    }

    let needs_body = body_param.is_some()
        || method == "POST"
        || method == "PUT"
        || method == "PATCH"
        || method == "DELETE";
    let needs_opts = needs_body || header_count > 0 || method != "GET";

    // Cache check (after URL/query construction, before fetch)
    if cache_seconds > 0 {
        body_lines.push(format!("{}const _cacheKey = \"{}\";", ind, cache_key));
        if body_param.is_some() {
            body_lines.push(format!(
                "{}const _currentReq = url + ':' + JSON.stringify(request.body);",
                ind
            ));
        } else {
            body_lines.push(format!("{}const _currentReq = url;", ind));
        }
        body_lines.push(format!("{}if (!force) {{", ind));
        body_lines.push(format!(
            "{}    const _cached = {client}Client._cache.get(_cacheKey);",
            ind,
            client = class_name
        ));
        body_lines.push(format!(
            "{}    if (_cached && Date.now() < _cached.expiry && _currentReq === _cached.request) return _cached.response;",
            ind
        ));
        body_lines.push(format!("{}}}", ind));
    }

    // Debug: log request
    if debug {
        if data_fields.is_empty() {
            body_lines.push(format!(
                "{}if (this._debug) console.log('[afast:debug] → {}');",
                ind, func_name
            ));
        } else {
            body_lines.push(format!(
                "{}if (this._debug) console.log('[afast:debug] → {}', JSON.stringify(request));",
                ind, func_name
            ));
        }
    }

    if needs_opts {
        body_lines.push(format!("{}const opts = {{ method: '{}' }};", ind, method));
        // Merge global _headers
        body_lines.push(format!("{}const hdrs = {{}};", ind));
        body_lines.push(format!("{}for (const fn of this._headers) {{", ind));
        body_lines.push(format!("{}    const val = await fn();", ind));
        body_lines.push(format!("{}    if (val && typeof val === 'object') {{", ind));
        body_lines.push(format!(
            "{}        for (const [k, v] of Object.entries(val)) {{",
            ind
        ));
        body_lines.push(format!(
            "{}            if (v != null) hdrs[k] = String(v);",
            ind
        ));
        body_lines.push(format!("{}        }}", ind));
        body_lines.push(format!("{}    }}", ind));
        body_lines.push(format!("{}}}", ind));
        if body_param.is_some() {
            body_lines.push(format!("{}hdrs['Content-Type'] = 'application/json';", ind));
        }
        body_lines.push(format!("{}opts.headers = hdrs;", ind));
        if body_param.is_some() {
            body_lines.push(format!("{}opts.body = JSON.stringify(request.body);", ind));
        }
        body_lines.push(format!(
            "{}const data = await this._request(url, opts);",
            ind
        ));
    } else {
        body_lines.push(format!(
            "{}const data = await this._request(url, {{ method: '{}' }});",
            ind, method
        ));
    }

    // Cache store (before debug log)
    if cache_seconds > 0 {
        body_lines.push(format!(
            "{ind}{client}Client._cache.set(_cacheKey, {{ request: _currentReq, response: data, expiry: Date.now() + {cache_ms} }});",
            ind = ind,
            client = class_name,
            cache_ms = cache_seconds * 1000,
        ));
    }

    // Debug: log response
    if debug {
        body_lines.push(format!(
            "{}if (this._debug) console.log('[afast:debug] ← {}', JSON.stringify(data));",
            ind, func_name
        ));
    }

    if meta.return_type != "()" {
        body_lines.push(format!("{}return data;", ind));
    }

    let body = body_lines.build();
    format!(
        "{}{} {{
{}
{}}},",
        indent, fn_sig, body, indent
    )
}

/// Recursively builds the nested JavaScript object literal that becomes the
/// `apis` getter body. Group handlers (empty `meta.name`) produce nested
/// objects. Leaf handlers produce JSDoc-annotated async methods, dispatching
/// to either `handler_method_js` (binary) or `ordinary_handler_method_js`
/// (JSON-over-HTTP) depending on the `is_ordinary` flag.
fn generate_handler_object_js(
    handlers: &[Handler],
    all_handlers: &[Handler],
    path: &[&str],
    indent: &str,
    emitted: &mut Vec<String>,
    seq64: bool,
    header_count: usize,
    debug: bool,
    class_name: &str,
) -> String {
    let mut lines = CodeBuf::new();
    let inner_indent = format!("{}    ", indent);

    for h in handlers {
        let child_path = {
            let mut p = path.to_vec();
            p.push(h.name);
            p
        };

        if h.meta.name.is_empty() {
            // Group handler — generate nested object
            let child_obj = generate_handler_object_js(
                &h.children,
                all_handlers,
                &child_path,
                &inner_indent,
                emitted,
                seq64,
                header_count,
                debug,
                class_name,
            );
            lines.push(format!(
                "{}{}: {{",
                inner_indent,
                h.name.trim_start_matches(':')
            ));
            lines.push(child_obj);
            lines.push(format!("{}}},", inner_indent));
        } else {
            // Leaf handler — generate JSDoc + method
            let prefix_str = handler_prefix(&child_path);
            let cache_key_parts: Vec<&str> = path
                .iter()
                .chain(std::iter::once(&h.name))
                .copied()
                .collect();
            let cache_key = cache_key_parts.join(".");
            if h.meta.is_ordinary {
                // Ordinary HTTP handler
                lines.push(format!("{}/**", inner_indent));
                if !h.meta.desc.is_empty() {
                    lines.push(format!("{} * {}", inner_indent, h.meta.desc));
                    lines.push(format!("{} *", inner_indent));
                }
                for param in h.meta.params {
                    match param.extractor {
                        "Query" | "Param" | "Body" => {
                            let desc = param.structure.map(|s| s().desc).unwrap_or("");
                            let label = match param.extractor {
                                "Query" => "queries",
                                "Param" => "params",
                                _ => "body",
                            };
                            if param.structure.is_some() {
                                let suffix = ordinary_suffix(param.extractor, 0);
                                let type_name = format!("{}{}", &prefix_str, suffix);
                                let jsdoc_type = match param.extractor {
                                    "Query" => format!("{{queries: {}}}", type_name),
                                    "Param" => format!("{{params: {}}}", type_name),
                                    _ => format!("{{body: {}}}", type_name),
                                };
                                if !desc.is_empty() {
                                    lines.push(format!(
                                        "{} * @param {{{}}} request.{} - {}",
                                        inner_indent, jsdoc_type, label, desc
                                    ));
                                } else {
                                    lines.push(format!(
                                        "{} * @param {{{}}} request.{}",
                                        inner_indent, jsdoc_type, label
                                    ));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if h.meta.return_type != "()" {
                    if let Some(structure_fn) = h.meta.return_structure {
                        let structure = structure_fn();
                        let mut type_name = format!("{}Response", &prefix_str);
                        let normalized = normalize_rust_type(h.meta.return_type);
                        if normalized.starts_with("Option<") {
                            type_name = format!("{} | null", type_name);
                        } else if normalized.starts_with("Vec<") {
                            type_name = format!("{}[]", type_name);
                        }
                        if !structure.desc.is_empty() {
                            lines.push(format!(
                                "{} * @returns {{Promise<{}>}} {}",
                                inner_indent, type_name, structure.desc
                            ));
                        } else {
                            lines.push(format!(
                                "{} * @returns {{Promise<{}>}}",
                                inner_indent, type_name
                            ));
                        }
                    } else if let Some(inner) = unwrap_json_type(h.meta.return_type) {
                        let js_type = rust_type_to_js(&inner);
                        lines.push(format!(
                            "{} * @returns {{Promise<{}>}}",
                            inner_indent, js_type
                        ));
                    }
                }
                lines.push(format!("{} */", inner_indent));

                lines.push(ordinary_handler_method_js(
                    h,
                    &prefix_str,
                    path,
                    indent,
                    header_count,
                    debug,
                    class_name,
                    &cache_key,
                ));
            } else {
                // Binary handler
                lines.push(format!("{}/**", inner_indent));
                if !h.meta.desc.is_empty() {
                    lines.push(format!("{} * {}", inner_indent, h.meta.desc));
                }
                lines.push(format!("{} *", inner_indent));

                let mut data_idx = 0;
                for param in h.meta.params {
                    match param.extractor {
                        "Custom" => {}
                        "Data" => {
                            let desc = param.structure.map(|s| s().desc).unwrap_or("");
                            let var_name = if data_idx == 0 {
                                "request".to_string()
                            } else {
                                format!("request{}", data_idx + 1)
                            };
                            if let Some(structure_fn) = param.structure {
                                let structure = structure_fn();
                                let type_name = prefixed_type(&prefix_str, structure.name);
                                if !desc.is_empty() {
                                    lines.push(format!(
                                        "{} * @param {{{}}} {} - {}",
                                        inner_indent, type_name, var_name, desc
                                    ));
                                } else {
                                    lines.push(format!(
                                        "{} * @param {{{}}} {}",
                                        inner_indent, type_name, var_name
                                    ));
                                }
                            }
                            data_idx += 1;
                        }
                        _ => {}
                    }
                }

                if h.meta.return_type != "()" {
                    if let Some(structure_fn) = h.meta.return_structure {
                        let structure = structure_fn();
                        let mut type_name = prefixed_type(&prefix_str, structure.name);
                        let normalized = normalize_rust_type(h.meta.return_type);
                        if normalized.starts_with("Option<") {
                            type_name = format!("{} | null", type_name);
                        } else if normalized.starts_with("Vec<") {
                            type_name = format!("{}[]", type_name);
                        }
                        if !structure.desc.is_empty() {
                            lines.push(format!(
                                "{} * @returns {{Promise<{}>}} {}",
                                inner_indent, type_name, structure.desc
                            ));
                        } else {
                            lines.push(format!(
                                "{} * @returns {{Promise<{}>}}",
                                inner_indent, type_name
                            ));
                        }
                    } else {
                        lines.push(format!("{} * @returns {{Promise<Object>}}", inner_indent));
                    }
                } else if h.meta.long_connection {
                    lines.push(format!("{} * @returns {{Promise<Socket>}}", inner_indent));
                }

                lines.push(format!("{} */", inner_indent));

                lines.push(handler_method_js(
                    h,
                    all_handlers,
                    &prefix_str,
                    indent,
                    debug,
                    class_name,
                    &cache_key,
                ));
            }
        }
    }

    lines.build()
}

// ─── Service-level code generation ────────────────────────────

/// Recursively walks the handler tree depth-first and collects JSDoc typedefs
/// for all leaf handlers into `lines`, tracking already-emitted types in
/// `emitted` to avoid duplicates. The `path` parameter accumulates handler
/// path segments used for computing type-name prefixes.
fn collect_type_exports_js(
    handlers: &[Handler],
    path: &[&str],
    emitted: &mut Vec<String>,
    lines: &mut CodeBuf,
) {
    for h in handlers {
        let child_path = {
            let mut p = path.to_vec();
            p.push(h.name);
            p
        };

        if !h.meta.name.is_empty() {
            let prefix_str = handler_prefix(&child_path);
            lines.push(handler_type_exports_js(&prefix_str, h.meta, emitted));
        }

        if !h.children.is_empty() {
            collect_type_exports_js(&h.children, &child_path, emitted, lines);
        }
    }
}

/// Top-level entry point for JavaScript client code generation. Produces a
/// complete ESM module as a single `String` containing: (1) the Socket class
/// (if WS or TCP transports are requested), (2) JSDoc typedefs for all
/// referenced types, (3) the AFastError class, (4) the service client class
/// with transport-specific `_call*` / `_request*` methods, binary reader/writer,
/// heartbeat logic, and (5) the `apis` getter that exposes all handler methods
/// as a nested object tree.
pub(crate) fn generate_service_js(
    svc: &Service,
    calls: &[crate::JsTsCallType],
    debug: bool,
) -> String {
    let mut lines = CodeBuf::new();

    lines.push("// Auto-generated by afast. DO NOT EDIT.".to_string());
    lines.b();

    let seq64 = cfg!(feature = "seq64");
    let len64 = cfg!(feature = "len64");
    let seq_bytes: usize = if seq64 { 8 } else { 4 };
    let len_bytes: usize = if len64 { 8 } else { 4 };

    let has_fetch = calls.contains(&crate::JsTsCallType::Fetch);
    let has_ws = calls.contains(&crate::JsTsCallType::Ws);
    let has_nodetcp = calls.contains(&crate::JsTsCallType::NodeTcp);
    let has_buntcp = calls.contains(&crate::JsTsCallType::BunTcp);
    let has_tcp = has_nodetcp || has_buntcp;
    let has_uni_req = calls.contains(&crate::JsTsCallType::UniRequest);
    let has_uni_ws = calls.contains(&crate::JsTsCallType::UniWs);
    let has_wxrequest = calls.contains(&crate::JsTsCallType::WxRequest);
    let has_wxws = calls.contains(&crate::JsTsCallType::WxWs);
    let has_any_ws = has_ws || has_uni_ws || has_wxws;
    #[cfg(feature = "ordinary-http")]
    let has_ordinary_routes = !svc.ordinary_routes.is_empty();
    #[cfg(not(feature = "ordinary-http"))]
    let has_ordinary_routes = false;
    let has_ordinary = has_ordinary_handlers(&svc.handlers) || has_ordinary_routes;

    let mut transport_types: Vec<&str> = Vec::new();
    if has_fetch {
        transport_types.push("'fetch'");
    }
    if has_ws {
        transport_types.push("'ws'");
    }
    if has_nodetcp {
        transport_types.push("'nodetcp'");
    }
    if has_buntcp {
        transport_types.push("'buntcp'");
    }
    if has_uni_req {
        transport_types.push("'unirequest'");
    }
    if has_uni_ws {
        transport_types.push("'uniws'");
    }
    if has_wxrequest {
        transport_types.push("'wxrequest'");
    }
    if has_wxws {
        transport_types.push("'wxws'");
    }
    if transport_types.is_empty() {
        transport_types.push("'fetch'");
    }
    let transport_union = transport_types.join("|");

    let default_transport = if has_fetch {
        "'fetch'"
    } else if has_ws {
        "'ws'"
    } else if has_uni_req {
        "'unirequest'"
    } else if has_uni_ws {
        "'uniws'"
    } else if has_wxrequest {
        "'wxrequest'"
    } else if has_wxws {
        "'wxws'"
    } else if has_nodetcp {
        "'nodetcp'"
    } else {
        "'buntcp'"
    };

    // Node.js TCP import
    if has_nodetcp {
        lines.push("import { createConnection } from 'node:net';".to_string());
        lines.b();
    }

    // Socket class — WS, UniApp WS, or TCP
    if has_any_ws || has_tcp {
        let push_hdr = seq_bytes + 4 + len_bytes;
        lines.push("export class Socket {".into());
        lines.push("    /** @type {number} */".into());
        lines.push("    _connId;".into());
        lines.push("    /** @type {any} */".into());
        lines.push("    _client;".into());
        lines.push("    _closing = false;".into());
        lines.push("    _closed = false;".into());
        lines.push("    /** @type {(() => void)|undefined} */".into());
        lines.push("    _closeResolve;".into());
        lines.push("    /** @type {((data: Uint8Array, send: (data: string | Uint8Array | any) => void) => void)|undefined} */".into());
        lines.push("    _callback;".into());
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} connId".into());
        lines.push("     * @param {any} client".into());
        lines.push("     * @param {(data: Uint8Array, send: (data: string | Uint8Array | any) => void) => void} callback".into());
        lines.push("     */".into());
        lines.push("    constructor(connId, client, callback) {".into());
        lines.push("        this._connId = connId;".into());
        lines.push("        this._client = client;".into());
        lines.push("        this._callback = callback;".into());
        lines.push("    }".into());
        lines.push("".into());
        lines.push("    /** @returns {number} */".into());
        lines.push("    get connId() { return this._connId; }".into());
        lines.push("    /** @returns {boolean} */".into());
        lines.push("    get isClosed() { return this._closed; }".into());
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {string|Uint8Array|any} data".into());
        lines.push("     */".into());
        lines.push("    send(data) {".into());
        lines.push(
            "        if (this._closing || this._closed) throw new Error('socket closed');".into(),
        );
        lines.push("        let payload;".into());
        lines.push("        if (typeof data === 'string') {".into());
        lines.push("            payload = new TextEncoder().encode(data);".into());
        lines.push("        } else if (data instanceof Uint8Array) {".into());
        lines.push("            payload = data;".into());
        lines.push("        } else {".into());
        lines.push("            payload = new TextEncoder().encode(JSON.stringify(data));".into());
        lines.push("        }".into());
        lines.push(format!(
            "        const buf = new ArrayBuffer({push_hdr} + payload.length);"
        ));
        lines.push("        const v = new DataView(buf);".into());
        if seq64 {
            lines.push("        v.setUint32(0, 0, true);".into());
            lines.push("        v.setUint32(4, 0, true);".into());
            lines.push(format!(
                "        v.setUint32({seq_bytes}, this._connId, true);"
            ));
            lines.push(format!(
                "        v.setUint32({} + 4, payload.length & 0xFFFFFFFF, true);",
                seq_bytes
            ));
            lines.push(format!("        v.setUint32({} + 8, Math.floor(payload.length / 0x100000000) & 0xFFFFFFFF, true);", seq_bytes));
        } else {
            lines.push("        v.setUint32(0, 0, true);".into());
            lines.push(format!(
                "        v.setUint32({seq_bytes}, this._connId, true);"
            ));
            lines.push(format!(
                "        v.setUint32({} + 4, payload.length, true);",
                seq_bytes
            ));
        }
        lines.push(format!(
            "        new Uint8Array(buf).set(payload, {push_hdr});"
        ));
        lines.push("        this._client._sendRaw(new Uint8Array(buf));".into());
        lines.push("    }".into());
        lines.push("".into());
        lines.push("    /** @returns {Promise<void>} */".into());
        lines.push("    async close() {".into());
        lines.push("        if (this._closed) return;".into());
        lines.push("        this._closing = true;".into());
        lines.push(format!("        const buf = new ArrayBuffer({push_hdr});"));
        lines.push("        const v = new DataView(buf);".into());
        if seq64 {
            lines.push("        v.setUint32(0, 0, true);".into());
            lines.push("        v.setUint32(4, 0, true);".into());
            lines.push(format!(
                "        v.setUint32({seq_bytes}, this._connId, true);"
            ));
            lines.push(format!("        v.setUint32({} + 4, 0, true);", seq_bytes));
            lines.push(format!("        v.setUint32({} + 8, 0, true);", seq_bytes));
        } else {
            lines.push("        v.setUint32(0, 0, true);".into());
            lines.push(format!(
                "        v.setUint32({seq_bytes}, this._connId, true);"
            ));
            lines.push(format!("        v.setUint32({} + 4, 0, true);", seq_bytes));
        }
        lines.push("        this._client._sendRaw(new Uint8Array(buf));".into());
        lines.push(
            "        return new Promise(resolve => { this._closeResolve = resolve; });".into(),
        );
        lines.push("    }".into());
        lines.push("".into());
        lines.push("    /** @param {Uint8Array} data */".into());
        lines.push("    _onMessage(data) {".into());
        lines.push("        if (this._closing && data.length === 0) {".into());
        lines.push("            this._closed = true;".into());
        lines.push("            this._closing = false;".into());
        lines.push("            this._closeResolve?.();".into());
        lines.push("            return;".into());
        lines.push("        }".into());
        lines.push("        if (!this._closed) {".into());
        lines.push("            this._callback?.(data, this.send.bind(this));".into());
        lines.push("        }".into());
        lines.push("    }".into());
        lines.push("}".into());
    }

    // Type exports
    let mut emitted = Vec::new();
    collect_type_exports_js(&svc.handlers, &[], &mut emitted, &mut lines);

    // Custom function types
    let customs = collect_customs(&svc.handlers);
    #[allow(unused_mut)]
    let mut headers_cl = collect_headers(&svc.handlers);
    // Also collect headers from top-level ordinary routes
    #[cfg(feature = "ordinary-http")]
    for route in &svc.ordinary_routes {
        for param in route.handler_entry.meta.params {
            if param.extractor == "Header" {
                let ty = param.ty.to_string();
                if !headers_cl.iter().any(|c| c.ty == ty) {
                    headers_cl.push(CustomInfo { ty });
                }
            }
        }
    }
    if !customs.is_empty() {
        lines.b();
        for ci in &customs {
            lines.push("/**".to_string());
            lines.push(format!(" * @typedef {{Function}} CustomFn_{}", ci.ty));
            lines.push(format!(" * @returns {{Promise<{}>}}", ci.ty));
            lines.push(" */".to_string());
        }
    }
    if !headers_cl.is_empty() {
        lines.b();
        for hi in &headers_cl {
            lines.push("/**".to_string());
            lines.push(format!(" * @typedef {{Function}} HeaderFn_{}", hi.ty));
            lines.push(format!(" * @returns {{Promise<{}>}}", hi.ty));
            lines.push(" */".to_string());
        }
    }
    lines.b();

    // AFastError class
    lines.push("export class AFastError extends Error {".to_string());
    lines.push("    /** @param {number} code @param {string} message */".to_string());
    lines.push("    constructor(code, message) {".to_string());
    lines.push("        super(message);".to_string());
    lines.push("        this.name = 'AFastError';".to_string());
    lines.push("        this.code = code;".to_string());
    lines.push("    }".to_string());
    lines.push("}".to_string());
    lines.b();

    // Client class
    let class_name = to_pascal_case(&svc.name);
    #[cfg(feature = "ordinary-http")]
    let has_cache = has_cache_handlers_js(&svc.handlers)
        || svc
            .ordinary_routes
            .iter()
            .any(|r| r.handler_entry.meta.cache_seconds > 0);
    #[cfg(not(feature = "ordinary-http"))]
    let has_cache = has_cache_handlers_js(&svc.handlers);
    lines.push(format!("export class {}Client {{", class_name));

    // Static cache (shared across all instances)
    if has_cache {
        lines.push(
            "    /** @type {Map<string, { request: string, response: any, expiry: number }>} */"
                .to_string(),
        );
        lines.push("    static _cache = new Map();".to_string());
    }

    lines.push(format!("    /** @type {{{}}} */", transport_union));
    lines.push("    _transport;".into());
    lines.push("    /** @type {string} */".into());
    lines.push("    _url;".into());
    lines.push("    /** @type {string} */".into());
    lines.push("    _host;".into());
    lines.push("    /** @type {number} */".into());
    lines.push("    _port;".into());
    lines.push("    /** @type {boolean} */".into());
    lines.push("    _tls;".into());
    if has_tcp {
        lines.push("    /** @type {number} */".into());
        lines.push("    /** @type {any} */".into());
        lines.push("    _tcpSocket;".into());
        lines.push("    /** @type {Uint8Array} */".into());
        lines.push("    _tcpBuf = new Uint8Array(0);".into());
    }
    if has_ws {
        lines.push("    /** @type {WebSocket} */".into());
        lines.push("    _ws;".into());
    }
    if has_uni_ws {
        lines.push("    /** @type {UniApp.SocketTask} */".into());
        lines.push("    _wsTask;".into());
    }
    if has_wxws {
        lines.push("    /** @type {WechatMiniprogram.SocketTask} */".into());
        lines.push("    _wxSocketTask;".into());
    }
    if has_any_ws || has_tcp {
        lines.push("    _nextId = 1;".into());
        lines.push("    /** @type {Map<number, {resolve: (d: Uint8Array) => void, reject: (e: Error) => void, timer: ReturnType<typeof setTimeout>}>} */".into());
        lines.push("    _pending = new Map();".into());
        lines.push("    /** @type {Map<number, (data: Uint8Array) => void>} */".into());
        lines.push("    _pushHandlers = new Map();".into());
        lines.push("    /** @type {ReturnType<typeof setInterval>|undefined} */".into());
        lines.push("    _heartbeatTimer;".into());
    }
    lines.push("    /** @type {Promise<void>} */".into());
    lines.push("    _ready;".into());
    lines.push(
        "    /** @type {(handlerId: number, payload: Uint8Array) => Promise<Uint8Array>} */".into(),
    );
    lines.push("    _call;".into());
    if has_ordinary && (has_fetch || has_uni_req || has_wxrequest) {
        lines.push(
            "    /** @type {(url: string, opts: Record<string, any>) => Promise<any>} */".into(),
        );
        lines.push("    _request;".into());
    }

    // _customs type
    if customs.is_empty() {
        lines.push("    /** @type {[]} */".into());
        lines.push("    _customs;".into());
    } else {
        let tuple: Vec<String> = customs
            .iter()
            .map(|c| format!("CustomFn_{}", c.ty))
            .collect();
        lines.push(format!("    /** @type {{[{}]}} */", tuple.join(", ")));
        lines.push("    _customs;".into());
    }
    // _headers type
    if headers_cl.is_empty() {
        lines.push("    /** @type {[]} */".into());
        lines.push("    _headers;".into());
    } else {
        let htuple: Vec<String> = headers_cl
            .iter()
            .map(|h| format!("HeaderFn_{}", h.ty))
            .collect();
        lines.push(format!("    /** @type {{[{}]}} */", htuple.join(", ")));
        lines.push("    _headers;".into());
    }
    lines.push("    /** @type {((code: number, message: string) => void) | undefined} */".into());
    lines.push("    _onError;".into());
    if debug {
        lines.push("    /** @type {boolean} */".to_string());
        lines.push("    _debug;".to_string());
    }

    // Constructor — single options object
    lines.push("".into());
    {
        let mut ctor_docs: Vec<String> = vec![
            "     * @param {Object} options".into(),
            "     * @param {string} options.host".into(),
            "     * @param {number} options.port".into(),
            "     * @param {boolean} options.tls".into(),
        ];
        let mut ctor_body: Vec<String> = vec![
            "        this._host = options.host;".into(),
            "        this._port = options.port;".into(),
            "        this._tls = options.tls;".into(),
        ];
        if !customs.is_empty() {
            ctor_docs.push("     * @param {Object} [options.customs]".into());
            for ci in &customs {
                ctor_docs.push(format!(
                    "     * @param {{CustomFn_{}}} [options.customs.{}]",
                    ci.ty, ci.ty
                ));
            }
            ctor_body.push(format!(
                "        this._customs = [{}];",
                customs
                    .iter()
                    .map(|c| format!("options.customs.{}", c.ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            ctor_body.push("        this._customs = [];".into());
        }
        if !headers_cl.is_empty() {
            ctor_docs.push("     * @param {Object} [options.headers]".into());
            for hi in &headers_cl {
                ctor_docs.push(format!(
                    "     * @param {{HeaderFn_{}}} [options.headers.{}]",
                    hi.ty, hi.ty
                ));
            }
            ctor_body.push(format!(
                "        this._headers = [{}];",
                headers_cl
                    .iter()
                    .map(|h| format!("options.headers.{}", h.ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            ctor_body.push("        this._headers = [];".into());
        }
        ctor_docs.push(format!(
            "     * @param {{{transport_union}}} [options.transport='{default_transport}']",
            transport_union = transport_union,
            default_transport = default_transport.trim_matches('\'')
        ));
        ctor_docs.push(
            "     * @param {Function} [options.onError] - Called with (code, message) on error"
                .into(),
        );
        if debug {
            ctor_docs.push("     * @param {boolean} [options.debug=true]".into());
        }
        ctor_body.push(format!(
            "        this._transport = options.transport || {default_transport};"
        ));
        {
            let mut http_parts: Vec<&str> = Vec::new();
            if has_fetch {
                http_parts.push("this._transport === 'fetch'");
            }
            if has_uni_req {
                http_parts.push("this._transport === 'unirequest'");
            }
            if has_wxrequest {
                http_parts.push("this._transport === 'wxrequest'");
            }
            let http_check = if http_parts.is_empty() {
                "false".to_string()
            } else {
                http_parts.join(" || ")
            };
            ctor_body.push(format!("        const scheme = options.tls ? ({} ? 'https' : 'wss') : ({} ? 'http' : 'ws');", http_check, http_check));
        }
        ctor_body
            .push("        this._url = scheme + '://' + options.host + ':' + options.port;".into());
        ctor_body.push("        this._onError = options.onError;".into());
        if debug {
            ctor_body.push(
                "        this._debug = options.debug !== undefined ? options.debug : true;".into(),
            );
        }

        lines.push("    /**".into());
        for doc in &ctor_docs {
            lines.push(doc.clone());
        }
        lines.push("     */".into());
        lines.push("    constructor(options) {".into());
        for body_line in &ctor_body {
            lines.push(body_line.clone());
        }
    }

    // Initialize transport
    let mut init_parts: Vec<String> = Vec::new();

    if has_ws {
        init_parts.push("        if (this._transport === 'ws') {".into());
        init_parts.push("            this._ready = new Promise((resolve, reject) => {".into());
        init_parts.push("                this._ws = new WebSocket(this._url + '/_ws');".into());
        init_parts.push("                const failTimer = setTimeout(() => reject(new Error('WebSocket connection timeout (10s)')), 10000);".into());
        init_parts.push("                this._ws.binaryType = 'arraybuffer';".into());
        init_parts.push("                this._ws.addEventListener('open', () => { clearTimeout(failTimer); resolve(); });".into());
        init_parts.push("                this._ws.addEventListener('error', () => { clearTimeout(failTimer); reject(new Error('WebSocket connection failed')); });".into());
        init_parts.push("                this._ws.addEventListener('close', (ev) => { clearTimeout(failTimer); reject(new Error(`WebSocket closed before open (code=${ev.code})`)); });".into());
        init_parts.push("                this._ws.addEventListener('message', (ev) => this._handleMessage(new Uint8Array(ev.data)));".into());
        init_parts.push("            });".into());
        init_parts.push("            this._ready.then(() => { if (this._heartbeatTimer) clearInterval(this._heartbeatTimer); this._heartbeatTimer = setInterval(() => this._sendHeartbeat(), 180_000); });".into());
        init_parts.push("            this._call = this._callWs;".into());
    }
    if has_uni_ws {
        let kw = if init_parts.is_empty() {
            "if"
        } else {
            "} else if"
        };
        init_parts.push(format!("        {} (this._transport === 'uniws') {{", kw));
        init_parts.push("            this._ready = new Promise((resolve, reject) => {".into());
        init_parts.push("                this._wsTask = uni.connectSocket({ url: this._url + '/_ws', complete: () => {} });".into());
        init_parts.push("                const failTimer = setTimeout(() => reject(new Error('UniApp WS connection timeout (10s)')), 10000);".into());
        init_parts.push(
            "                this._wsTask.onOpen(() => { clearTimeout(failTimer); resolve(); });"
                .into(),
        );
        init_parts.push("                this._wsTask.onError(() => { clearTimeout(failTimer); reject(new Error('UniApp WS connection failed')); });".into());
        init_parts.push("                this._wsTask.onClose(() => { clearTimeout(failTimer); reject(new Error('UniApp WS closed before open')); });".into());
        init_parts.push("                this._wsTask.onMessage((res) => {".into());
        init_parts
            .push("                    const raw = typeof res.data === 'string' ? new TextEncoder().encode(res.data) : new Uint8Array(res.data);".into());
        init_parts.push("                    this._handleMessage(raw);".into());
        init_parts.push("                });".into());
        init_parts.push("            });".into());
        init_parts.push("            this._ready.then(() => { if (this._heartbeatTimer) clearInterval(this._heartbeatTimer); this._heartbeatTimer = setInterval(() => this._sendHeartbeat(), 180_000); });".into());
        init_parts.push("            this._call = this._callUniWs;".into());
    }
    if has_wxws {
        let kw = if init_parts.is_empty() {
            "if"
        } else {
            "} else if"
        };
        init_parts.push(format!("        {} (this._transport === 'wxws') {{", kw));
        init_parts.push("            this._ready = new Promise((resolve, reject) => {".into());
        init_parts.push(
            "                this._wxSocketTask = wx.connectSocket({ url: this._url + '/_ws' });"
                .into(),
        );
        init_parts.push("                const failTimer = setTimeout(() => reject(new Error('Wx WS connection timeout (10s)')), 10000);".into());
        init_parts.push("                this._wxSocketTask.onOpen(() => { clearTimeout(failTimer); resolve(); });".into());
        init_parts.push("                this._wxSocketTask.onError(() => { clearTimeout(failTimer); reject(new Error('Wx WS connection failed')); });".into());
        init_parts.push("                this._wxSocketTask.onClose(() => { clearTimeout(failTimer); reject(new Error('Wx WS closed before open')); });".into());
        init_parts.push("                this._wxSocketTask.onMessage((res) => {".into());
        init_parts
            .push("                    const raw = typeof res.data === 'string' ? new TextEncoder().encode(res.data) : new Uint8Array(res.data);".into());
        init_parts.push("                    this._handleMessage(raw);".into());
        init_parts.push("                });".into());
        init_parts.push("            });".into());
        init_parts.push("            this._ready.then(() => { if (this._heartbeatTimer) clearInterval(this._heartbeatTimer); this._heartbeatTimer = setInterval(() => this._sendHeartbeat(), 180_000); });".into());
        init_parts.push("            this._call = this._callWxWs;".into());
    }
    if has_wxrequest {
        let kw = if init_parts.is_empty() {
            "if"
        } else {
            "} else if"
        };
        init_parts.push(format!(
            "        {} (this._transport === 'wxrequest') {{",
            kw
        ));
        init_parts.push("            this._ready = Promise.resolve();".into());
        init_parts.push("            this._call = this._callWxReq;".into());
        if has_ordinary {
            init_parts.push("            this._request = this._requestWxReq;".into());
        }
    }
    if has_fetch {
        let kw = if init_parts.is_empty() {
            "if"
        } else {
            "} else if"
        };
        init_parts.push(format!("        {} (this._transport === 'fetch') {{", kw));
        init_parts.push("            this._ready = Promise.resolve();".into());
        init_parts.push("            this._call = this._callFetch;".into());
        if has_ordinary {
            init_parts.push("            this._request = this._requestFetch;".into());
        }
    }
    if has_uni_req {
        let kw = if init_parts.is_empty() {
            "if"
        } else {
            "} else if"
        };
        init_parts.push(format!(
            "        {} (this._transport === 'unirequest') {{",
            kw
        ));
        init_parts.push("            this._ready = Promise.resolve();".into());
        init_parts.push("            this._call = this._callUniReq;".into());
        if has_ordinary {
            init_parts.push("            this._request = this._requestUniReq;".into());
        }
    }
    if has_nodetcp {
        let kw = if init_parts.is_empty() {
            "if"
        } else {
            "} else if"
        };
        init_parts.push(format!("        {} (this._transport === 'nodetcp') {{", kw));
        init_parts.push("            this._initNodeTcp();".into());
    }
    if has_buntcp {
        let kw = if init_parts.is_empty() {
            "if"
        } else {
            "} else if"
        };
        init_parts.push(format!("        {} (this._transport === 'buntcp') {{", kw));
        init_parts.push("            this._initBunTcp();".into());
    }

    if init_parts.is_empty() {
        init_parts.push("        this._ready = Promise.resolve();".into());
    } else {
        init_parts.push("        }".into());
    }

    for line in init_parts {
        lines.push(line);
    }
    lines.push("    }".into());

    let sid = seq_bytes;

    // _handleMessage — WS, UniApp WS, or TCP
    if has_any_ws || has_tcp {
        lines.push("".into());
        lines.push("    /** @param {Uint8Array} raw */".into());
        lines.push("    _handleMessage(raw) {".into());
        if seq64 {
            lines.push(format!("        if (raw.length < {sid}) return;"));
            lines.push(
                "        const lo = raw[0] | (raw[1] << 8) | (raw[2] << 16) | (raw[3] << 24);"
                    .into(),
            );
            lines.push(
                "        const hi = raw[4] | (raw[5] << 8) | (raw[6] << 16) | (raw[7] << 24);"
                    .into(),
            );
            lines.push("        const reqId = hi * 0x100000000 + (lo >>> 0);".into());
        } else {
            lines.push("        if (raw.length < 4) return;".into());
            lines.push(
                "        const reqId = raw[0] | (raw[1] << 8) | (raw[2] << 16) | (raw[3] << 24);"
                    .into(),
            );
        }
        lines.push("        if (reqId === 0) {".into());
        let push_hdr = sid + 4 + len_bytes;
        lines.push(format!("            if (raw.length < {push_hdr}) return;"));
        lines.push(format!(
            "            const connId = raw[{sid}] | (raw[{}]<<8) | (raw[{}]<<16) | (raw[{}]<<24);",
            sid + 1,
            sid + 2,
            sid + 3
        ));
        let len_off = sid + 4;
        if len64 {
            lines.push(format!("            const lenLo = raw[{len_off}]|(raw[{}]<<8)|(raw[{}]<<16)|(raw[{}]<<24);", len_off+1, len_off+2, len_off+3));
            lines.push(format!(
                "            const lenHi = raw[{}]|(raw[{}]<<8)|(raw[{}]<<16)|(raw[{}]<<24);",
                len_off + 4,
                len_off + 5,
                len_off + 6,
                len_off + 7
            ));
            lines.push("            const len = lenHi * 0x100000000 + (lenLo >>> 0);".into());
        } else {
            lines.push(format!(
                "            const len = raw[{len_off}]|(raw[{}]<<8)|(raw[{}]<<16)|(raw[{}]<<24);",
                len_off + 1,
                len_off + 2,
                len_off + 3
            ));
        }
        lines.push(format!(
            "            const payload = raw.slice({push_hdr}, {push_hdr} + len);"
        ));
        lines.push("            this._pushHandlers.get(connId)?.(payload);".into());
        lines.push("            return;".into());
        lines.push("        }".into());
        lines.push("        const p = this._pending.get(reqId);".into());
        lines.push("        if (!p) return;".into());
        lines.push("        clearTimeout(p.timer);".into());
        lines.push("        this._pending.delete(reqId);".into());
        let status_off = sid + len_bytes;
        let code_off = status_off + 1;
        let payload_off = code_off + 8;
        let resp_min = payload_off;
        lines.push(format!("        if (raw.length < {resp_min}) {{ if(this._onError) this._onError(-1, 'response too short'); p.reject(new AFastError(-1, 'response too short')); return; }}"));
        lines.push(format!("        const status = raw[{status_off}];"));
        lines.push("        if (status === 1) {".into());
        lines.push(format!(
            "            const codeLo = raw[{code_off}]|(raw[{}]<<8)|(raw[{}]<<16)|(raw[{}]<<24);",
            code_off + 1,
            code_off + 2,
            code_off + 3
        ));
        lines.push(format!(
            "            const codeHi = raw[{}]|(raw[{}]<<8)|(raw[{}]<<16)|(raw[{}]<<24);",
            code_off + 4,
            code_off + 5,
            code_off + 6,
            code_off + 7
        ));
        lines.push("            const codeVal = codeHi * 0x100000000 + (codeLo >>> 0);".into());
        lines.push(format!(
            "            const msg = new TextDecoder().decode(raw.slice({payload_off}));"
        ));
        lines.push("            if(this._onError) this._onError(codeVal, msg);".into());
        lines.push("            p.reject(new AFastError(codeVal, msg));".into());
        lines.push("        } else {".into());
        lines.push(format!("            p.resolve(raw.slice({payload_off}));"));
        lines.push("        }".into());
        lines.push("    }".into());
    }

    // _requestFetch — ordinary HTTP via fetch()
    if has_ordinary && has_fetch {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {string} url".into());
        lines.push("     * @param {RequestInit} opts".into());
        lines.push("     * @returns {Promise<any>}".into());
        lines.push("     */".into());
        lines.push("    async _requestFetch(url, opts) {".into());
        lines.push("        const resp = await fetch(url, opts);".into());
        lines.push("        if (!resp.ok) {".into());
        lines.push("            let err;".into());
        lines.push("            try { err = await resp.json(); } catch { err = { code: -1, message: resp.statusText }; }".into());
        lines.push("            if (this._onError) this._onError(err.code || -1, err.message || String(resp.status));".into());
        lines.push(
            "            throw new AFastError(err.code || -1, err.message || String(resp.status));"
                .into(),
        );
        lines.push("        }".into());
        lines.push("        try { return await resp.json(); } catch { return null; }".into());
        lines.push("    }".into());
    }

    // _requestUniReq — ordinary HTTP via uni.request
    if has_ordinary && has_uni_req {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {string} url".into());
        lines.push("     * @param {Record<string, any>} opts".into());
        lines.push("     * @returns {Promise<any>}".into());
        lines.push("     */".into());
        lines.push("    async _requestUniReq(url, opts) {".into());
        lines.push("        const res = await uni.request({".into());
        lines.push("            url,".into());
        lines.push("            method: opts.method || 'GET',".into());
        lines.push("            header: opts.headers,".into());
        lines.push("            data: opts.body,".into());
        lines.push("        });".into());
        lines.push("        if (res.statusCode < 200 || res.statusCode >= 300) {".into());
        lines.push("            const err = typeof res.data === 'object' ? res.data : { code: -1, message: String(res.statusCode) };".into());
        lines.push("            if (this._onError) this._onError(err.code || -1, err.message || String(res.statusCode));".into());
        lines.push("            throw new AFastError(err.code || -1, err.message || String(res.statusCode));".into());
        lines.push("        }".into());
        lines.push("        return typeof res.data === 'object' ? res.data : null;".into());
        lines.push("    }".into());
    }

    // _requestWxReq — ordinary HTTP via wx.request
    if has_ordinary && has_wxrequest {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {string} url".into());
        lines.push("     * @param {Record<string, any>} opts".into());
        lines.push("     * @returns {Promise<any>}".into());
        lines.push("     */".into());
        lines.push("    async _requestWxReq(url, opts) {".into());
        lines.push("        const res = await new Promise((resolve, reject) => {".into());
        lines.push("            wx.request({".into());
        lines.push("                url,".into());
        lines.push("                method: opts.method || 'GET',".into());
        lines.push("                header: opts.headers,".into());
        lines.push("                data: opts.body,".into());
        lines.push("                success: resolve,".into());
        lines.push("                fail: reject,".into());
        lines.push("            });".into());
        lines.push("        });".into());
        lines.push("        if (res.statusCode < 200 || res.statusCode >= 300) {".into());
        lines.push("            const err = typeof res.data === 'object' ? res.data : { code: -1, message: String(res.statusCode) };".into());
        lines.push("            if (this._onError) this._onError(err.code || -1, err.message || String(res.statusCode));".into());
        lines.push("            throw new AFastError(err.code || -1, err.message || String(res.statusCode));".into());
        lines.push("        }".into());
        lines.push("        return typeof res.data === 'object' ? res.data : null;".into());
        lines.push("    }".into());
    }

    // _callFetch
    if has_fetch {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} handlerId".into());
        lines.push("     * @param {Uint8Array} payload".into());
        lines.push("     * @returns {Promise<Uint8Array>}".into());
        lines.push("     */".into());
        lines.push("    async _callFetch(handlerId, payload) {".into());
        lines.push("        await this._ready;".into());
        lines.push("        const body = new ArrayBuffer(4 + payload.length);".into());
        lines.push("        const bv = new DataView(body);".into());
        lines.push("        bv.setUint32(0, handlerId, true);".into());
        lines.push("        new Uint8Array(body).set(payload, 4);".into());
        lines.push(
            "        const resp = await fetch(`${this._url}/_api`, { method: 'POST', body });"
                .into(),
        );
        lines.push("        const buf = new Uint8Array(await resp.arrayBuffer());".into());
        lines.push("        if (buf.length < 9) { if(this._onError) this._onError(-1, 'response too short'); throw new AFastError(-1, 'response too short'); }".into());
        lines.push("        const status = buf[0];".into());
        lines.push("        if (status === 1) {".into());
        lines.push(
            "            const codeLo = buf[1]|(buf[2]<<8)|(buf[3]<<16)|(buf[4]<<24);".into(),
        );
        lines.push(
            "            const codeHi = buf[5]|(buf[6]<<8)|(buf[7]<<16)|(buf[8]<<24);".into(),
        );
        lines.push("            const codeVal = codeHi * 0x100000000 + (codeLo >>> 0);".into());
        lines.push("            const msg = new TextDecoder().decode(buf.slice(9));".into());
        lines.push("            if(this._onError) this._onError(codeVal, msg);".into());
        lines.push("            throw new AFastError(codeVal, msg);".into());
        lines.push("        }".into());
        lines.push("        return buf.slice(9);".into());
        lines.push("    }".into());
    }

    // _callUniReq: POST /api via uni.request
    if has_uni_req {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} handlerId".into());
        lines.push("     * @param {Uint8Array} payload".into());
        lines.push("     * @returns {Promise<Uint8Array>}".into());
        lines.push("     */".into());
        lines.push("    async _callUniReq(handlerId, payload) {".into());
        lines.push("        await this._ready;".into());
        lines.push("        const body = new ArrayBuffer(4 + payload.length);".into());
        lines.push("        const bv = new DataView(body);".into());
        lines.push("        bv.setUint32(0, handlerId, true);".into());
        lines.push("        new Uint8Array(body).set(payload, 4);".into());
        lines.push("        const res = await uni.request({".into());
        lines.push("            url: `${this._url}/_api`,".into());
        lines.push("            method: 'POST',".into());
        lines.push("            data: body,".into());
        lines.push("            responseType: 'arraybuffer',".into());
        lines.push("        });".into());
        lines.push("        const buf = new Uint8Array(res.data);".into());
        lines.push("        if (buf.length < 9) { if(this._onError) this._onError(-1, 'response too short'); throw new AFastError(-1, 'response too short'); }".into());
        lines.push("        const status = buf[0];".into());
        lines.push("        if (status === 1) {".into());
        lines.push(
            "            const codeLo = buf[1]|(buf[2]<<8)|(buf[3]<<16)|(buf[4]<<24);".into(),
        );
        lines.push(
            "            const codeHi = buf[5]|(buf[6]<<8)|(buf[7]<<16)|(buf[8]<<24);".into(),
        );
        lines.push("            const codeVal = codeHi * 0x100000000 + (codeLo >>> 0);".into());
        lines.push("            const msg = new TextDecoder().decode(buf.slice(9));".into());
        lines.push("            if(this._onError) this._onError(codeVal, msg);".into());
        lines.push("            throw new AFastError(codeVal, msg);".into());
        lines.push("        }".into());
        lines.push("        return buf.slice(9);".into());
        lines.push("    }".into());
    }

    // _callWxReq: POST /api via wx.request
    if has_wxrequest {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} handlerId".into());
        lines.push("     * @param {Uint8Array} payload".into());
        lines.push("     * @returns {Promise<Uint8Array>}".into());
        lines.push("     */".into());
        lines.push("    async _callWxReq(handlerId, payload) {".into());
        lines.push("        await this._ready;".into());
        lines.push("        const body = new ArrayBuffer(4 + payload.length);".into());
        lines.push("        const bv = new DataView(body);".into());
        lines.push("        bv.setUint32(0, handlerId, true);".into());
        lines.push("        new Uint8Array(body).set(payload, 4);".into());
        lines.push("        const res = await new Promise((resolve, reject) => {".into());
        lines.push("            wx.request({".into());
        lines.push("                url: `${this._url}/_api`,".into());
        lines.push("                method: 'POST',".into());
        lines.push("                data: body,".into());
        lines.push("                responseType: 'arraybuffer',".into());
        lines.push("                success: resolve,".into());
        lines.push("                fail: reject,".into());
        lines.push("            });".into());
        lines.push("        });".into());
        lines.push("        const buf = new Uint8Array(res.data);".into());
        lines.push("        if (buf.length < 9) { if(this._onError) this._onError(-1, 'response too short'); throw new AFastError(-1, 'response too short'); }".into());
        lines.push("        const status = buf[0];".into());
        lines.push("        if (status === 1) {".into());
        lines.push(
            "            const codeLo = buf[1]|(buf[2]<<8)|(buf[3]<<16)|(buf[4]<<24);".into(),
        );
        lines.push(
            "            const codeHi = buf[5]|(buf[6]<<8)|(buf[7]<<16)|(buf[8]<<24);".into(),
        );
        lines.push("            const codeVal = codeHi * 0x100000000 + (codeLo >>> 0);".into());
        lines.push("            const msg = new TextDecoder().decode(buf.slice(9));".into());
        lines.push("            if(this._onError) this._onError(codeVal, msg);".into());
        lines.push("            throw new AFastError(codeVal, msg);".into());
        lines.push("        }".into());
        lines.push("        return buf.slice(9);".into());
        lines.push("    }".into());
    }

    // _callWs
    if has_ws {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} handlerId".into());
        lines.push("     * @param {Uint8Array} payload".into());
        lines.push("     * @returns {Promise<Uint8Array>}".into());
        lines.push("     */".into());
        lines.push("    async _callWs(handlerId, payload) {".into());
        lines.push("        await this._ready;".into());
        lines.push("        return new Promise((resolve, reject) => {".into());
        lines.push("            const id = this._nextId;".into());
        lines.push("            this._nextId = this._nextId >= Number.MAX_SAFE_INTEGER ? 1 : this._nextId + 1;".into());
        lines.push("            const timer = setTimeout(() => { this._pending.delete(id); reject(new Error(`timeout id=${id}`)); }, 5000);".into());
        lines.push("            this._pending.set(id, { resolve, reject, timer });".into());
        lines.push(format!(
            "            const frame = new ArrayBuffer({sid} + 4 + {len_bytes} + payload.length);"
        ));
        lines.push("            const fv = new DataView(frame);".into());
        if seq64 {
            lines.push("            fv.setUint32(0, id & 0xFFFFFFFF, true);".into());
            lines.push(
                "            fv.setUint32(4, Math.floor(id / 0x100000000) & 0xFFFFFFFF, true);"
                    .into(),
            );
        } else {
            lines.push("            fv.setUint32(0, id, true);".into());
        }
        lines.push(format!("            fv.setUint32({sid}, handlerId, true);"));
        if len64 {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length & 0xFFFFFFFF, true);"
            ));
            lines.push(format!("            fv.setUint32({sid} + 8, Math.floor(payload.length / 0x100000000) & 0xFFFFFFFF, true);"));
        } else {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length, true);"
            ));
        }
        lines.push(format!(
            "            new Uint8Array(frame).set(payload, {sid} + 4 + {len_bytes});"
        ));
        lines.push("            this._ws.send(frame);".into());
        lines.push("        });".into());
        lines.push("    }".into());
    }

    // _callUniWs: send WS frame via uni.connectSocket, wait for response by req_id
    if has_uni_ws {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} handlerId".into());
        lines.push("     * @param {Uint8Array} payload".into());
        lines.push("     * @returns {Promise<Uint8Array>}".into());
        lines.push("     */".into());
        lines.push("    async _callUniWs(handlerId, payload) {".into());
        lines.push("        await this._ready;".into());
        lines.push("        return new Promise((resolve, reject) => {".into());
        lines.push("            const id = this._nextId;".into());
        lines.push("            this._nextId = this._nextId >= Number.MAX_SAFE_INTEGER ? 1 : this._nextId + 1;".into());
        lines.push("            const timer = setTimeout(() => { this._pending.delete(id); reject(new Error(`timeout id=${id}`)); }, 5000);".into());
        lines.push("            this._pending.set(id, { resolve, reject, timer });".into());
        lines.push(format!(
            "            const frame = new ArrayBuffer({sid} + 4 + {len_bytes} + payload.length);"
        ));
        lines.push("            const fv = new DataView(frame);".into());
        if seq64 {
            lines.push("            fv.setUint32(0, id & 0xFFFFFFFF, true);".into());
            lines.push(
                "            fv.setUint32(4, Math.floor(id / 0x100000000) & 0xFFFFFFFF, true);"
                    .into(),
            );
        } else {
            lines.push("            fv.setUint32(0, id, true);".into());
        }
        lines.push(format!("            fv.setUint32({sid}, handlerId, true);"));
        if len64 {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length & 0xFFFFFFFF, true);"
            ));
            lines.push(format!("            fv.setUint32({sid} + 8, Math.floor(payload.length / 0x100000000) & 0xFFFFFFFF, true);"));
        } else {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length, true);"
            ));
        }
        lines.push(format!(
            "            new Uint8Array(frame).set(payload, {sid} + 4 + {len_bytes});"
        ));
        lines.push("            this._wsTask.send({ data: frame });".into());
        lines.push("        });".into());
        lines.push("    }".into());
    }

    // _callWxWs: send WS frame via wx.connectSocket SocketTask
    if has_wxws {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} handlerId".into());
        lines.push("     * @param {Uint8Array} payload".into());
        lines.push("     * @returns {Promise<Uint8Array>}".into());
        lines.push("     */".into());
        lines.push("    async _callWxWs(handlerId, payload) {".into());
        lines.push("        await this._ready;".into());
        lines.push("        return new Promise((resolve, reject) => {".into());
        lines.push("            const id = this._nextId;".into());
        lines.push("            this._nextId = this._nextId >= Number.MAX_SAFE_INTEGER ? 1 : this._nextId + 1;".into());
        lines.push("            const timer = setTimeout(() => { this._pending.delete(id); reject(new Error(`timeout id=${id}`)); }, 5000);".into());
        lines.push("            this._pending.set(id, { resolve, reject, timer });".into());
        lines.push(format!(
            "            const frame = new ArrayBuffer({sid} + 4 + {len_bytes} + payload.length);"
        ));
        lines.push("            const fv = new DataView(frame);".into());
        if seq64 {
            lines.push("            fv.setUint32(0, id & 0xFFFFFFFF, true);".into());
            lines.push(
                "            fv.setUint32(4, Math.floor(id / 0x100000000) & 0xFFFFFFFF, true);"
                    .into(),
            );
        } else {
            lines.push("            fv.setUint32(0, id, true);".into());
        }
        lines.push(format!("            fv.setUint32({sid}, handlerId, true);"));
        if len64 {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length & 0xFFFFFFFF, true);"
            ));
            lines.push(format!("            fv.setUint32({sid} + 8, Math.floor(payload.length / 0x100000000) & 0xFFFFFFFF, true);"));
        } else {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length, true);"
            ));
        }
        lines.push(format!(
            "            new Uint8Array(frame).set(payload, {sid} + 4 + {len_bytes});"
        ));
        lines.push("            this._wxSocketTask.send({ data: frame });".into());
        lines.push("        });".into());
        lines.push("    }".into());
    }

    // _sendRaw — shared by WS, UniApp WS, and TCP
    if has_any_ws || has_tcp {
        lines.push("".into());
        lines.push("    /** @param {Uint8Array} data */".into());

        let send_raw_count = [has_ws, has_uni_ws, has_wxws, has_tcp]
            .iter()
            .filter(|&&x| x)
            .count();
        if send_raw_count > 1 {
            lines.push("    _sendRaw(data) {".into());
            let mut first = true;
            if has_ws {
                lines.push("        if (this._transport === 'ws') {".into());
                lines.push("            this._ws.send(data);".into());
                first = false;
            }
            if has_uni_ws {
                if first {
                    lines.push("        if (this._transport === 'uniws') {".into());
                } else {
                    lines.push("        } else if (this._transport === 'uniws') {".into());
                }
                lines.push("            this._wsTask.send({ data });".into());
                first = false;
            }
            if has_wxws {
                if first {
                    lines.push("        if (this._transport === 'wxws') {".into());
                } else {
                    lines.push("        } else if (this._transport === 'wxws') {".into());
                }
                lines.push("            this._wxSocketTask.send({ data });".into());
            }
            if has_tcp {
                lines.push("        } else {".into());
                lines.push(format!(
                    "            const buf = new ArrayBuffer({len_bytes} + data.length);"
                ));
                lines.push("            const bv = new DataView(buf);".into());
                if len64 {
                    lines.push(
                        "            bv.setUint32(0, data.length & 0xFFFFFFFF, true);".into(),
                    );
                    lines.push("            bv.setUint32(4, Math.floor(data.length / 0x100000000) & 0xFFFFFFFF, true);".into());
                } else {
                    lines.push("            bv.setUint32(0, data.length, true);".into());
                }
                lines.push(
                    "            new Uint8Array(buf).set(data, ".to_string()
                        + &len_bytes.to_string()
                        + ");",
                );
                lines.push("            this._tcpSocket.write(new Uint8Array(buf));".into());
            }
            lines.push("        }".into());
            lines.push("    }".into());
        } else if has_ws {
            lines.push("    _sendRaw(data) { this._ws.send(data); }".into());
        } else if has_uni_ws {
            lines.push("    _sendRaw(data) { this._wsTask.send({ data }); }".into());
        } else if has_wxws {
            lines.push("    _sendRaw(data) { this._wxSocketTask.send({ data }); }".into());
        } else {
            // TCP only
            lines.push("    _sendRaw(data) {".into());
            lines.push(format!(
                "        const buf = new ArrayBuffer({len_bytes} + data.length);"
            ));
            lines.push("        const bv = new DataView(buf);".into());
            if len64 {
                lines.push("        bv.setUint32(0, data.length & 0xFFFFFFFF, true);".into());
                lines.push("        bv.setUint32(4, Math.floor(data.length / 0x100000000) & 0xFFFFFFFF, true);".into());
            } else {
                lines.push("        bv.setUint32(0, data.length, true);".into());
            }
            lines.push(
                "        new Uint8Array(buf).set(data, ".to_string()
                    + &len_bytes.to_string()
                    + ");",
            );
            lines.push("        this._tcpSocket.write(new Uint8Array(buf));".into());
            lines.push("    }".into());
        }
    }

    // _sendHeartbeat — shared by WS, UniApp WS, and TCP
    if has_any_ws || has_tcp {
        let hb_hdr = sid + len_bytes;
        lines.push("    /** @private */".into());
        lines.push("    _sendHeartbeat() {".into());
        lines.push("        const ids = Array.from(this._pushHandlers.keys());".into());
        lines.push(format!(
            "        const buf = new ArrayBuffer({hb_hdr} + ids.length * 4);"
        ));
        lines.push("        const v = new DataView(buf);".into());
        if seq64 {
            lines.push("        v.setUint32(0, 0xFFFFFFFF, true);".into());
            lines.push("        v.setUint32(4, 0xFFFFFFFF, true);".into());
        } else {
            lines.push("        v.setUint32(0, 0xFFFFFFFF, true);".into());
        }
        if len64 {
            lines.push("        const len = ids.length * 4;".to_string());
            lines.push(format!(
                "        v.setUint32({sid}, len & 0xFFFFFFFF, true);"
            ));
            lines.push(format!(
                "        v.setUint32({sid} + 4, Math.floor(len / 0x100000000) & 0xFFFFFFFF, true);"
            ));
            lines.push(format!(
                "        ids.forEach((id, i) => v.setUint32({sid} + 8 + i * 4, id, true));"
            ));
        } else {
            lines.push(format!("        v.setUint32({sid}, ids.length * 4, true);"));
            lines.push(format!(
                "        ids.forEach((id, i) => v.setUint32({sid} + 4 + i * 4, id, true));"
            ));
        }
        if has_ws && (has_tcp || has_uni_ws || has_wxws) {
            lines.push("        if (this._transport === 'ws') { this._ws.send(buf); } else { this._sendRaw(new Uint8Array(buf)); }".into());
        } else if has_ws {
            lines.push("        this._ws.send(buf);".into());
        } else {
            lines.push("        this._sendRaw(new Uint8Array(buf));".into());
        }
        lines.push("    }".into());
    }

    // _initNodeTcp — create persistent Node.js TCP connection
    if has_nodetcp {
        lines.push("".into());
        lines.push("    /** @private */".into());
        lines.push("    _initNodeTcp() {".into());
        lines.push(
            "        this._tcpSocket = createConnection({ host: this._host, port: this._port });"
                .into(),
        );
        lines.push("        this._ready = new Promise((resolve) => {".into());
        lines.push("            this._tcpSocket.on('connect', () => resolve());".into());
        lines.push("        });".into());
        lines.push("        this._tcpSocket.on('data', (chunk) => this._handleTcpData(new Uint8Array(chunk)));".into());
        lines.push("        this._tcpSocket.on('error', (e) => {".into());
        lines.push("            for (const [id, p] of this._pending) { p.reject(e); this._pending.delete(id); }".into());
        lines.push("        });".into());
        lines.push("        this._ready.then(() => { if (this._heartbeatTimer) clearInterval(this._heartbeatTimer); this._heartbeatTimer = setInterval(() => this._sendHeartbeat(), 180_000); });".into());
        lines.push("        this._call = this._callNodeTcp;".into());
        lines.push("    }".into());
    }

    // _initBunTcp — create persistent Bun TCP connection
    if has_buntcp {
        lines.push("".into());
        lines.push("    /** @private */".into());
        lines.push("    _initBunTcp() {".into());
        lines.push("        this._ready = new Promise((resolve, reject) => {".into());
        lines.push("            Bun.connect({".into());
        lines.push("                hostname: this._host,".into());
        lines.push("                port: this._port,".into());
        lines.push("                socket: {".into());
        lines.push("                    open: (sock) => {".into());
        lines.push("                        this._tcpSocket = sock;".into());
        lines.push("                        resolve();".into());
        lines.push("                    },".into());
        lines.push(
            "                    data: (_sock, data) => this._handleTcpData(new Uint8Array(data)),"
                .into(),
        );
        lines.push("                    error: (_sock, e) => {".into());
        lines.push("                        for (const [id, p] of this._pending) { p.reject(e); this._pending.delete(id); }".into());
        lines.push("                        reject(e);".into());
        lines.push("                    },".into());
        lines.push("                },".into());
        lines.push("            }).catch(reject);".into());
        lines.push("        });".into());
        lines.push("        this._ready.then(() => { if (this._heartbeatTimer) clearInterval(this._heartbeatTimer); this._heartbeatTimer = setInterval(() => this._sendHeartbeat(), 180_000); });".into());
        lines.push("        this._call = this._callBunTcp;".into());
        lines.push("    }".into());
    }

    // _handleTcpData — accumulate TCP frames and dispatch to _handleMessage
    if has_tcp {
        lines.push("".into());
        lines.push("    /** @private @param {Uint8Array} chunk */".into());
        lines.push("    _handleTcpData(chunk) {".into());
        lines.push(
            "        const combined = new Uint8Array(this._tcpBuf.length + chunk.length);".into(),
        );
        lines.push("        combined.set(this._tcpBuf, 0);".into());
        lines.push("        combined.set(chunk, this._tcpBuf.length);".into());
        lines.push("        this._tcpBuf = combined;".into());
        lines.push(format!(
            "        while (this._tcpBuf.length >= {len_bytes}) {{"
        ));
        lines.push("            let frameLen;".into());
        if len64 {
            lines.push("            frameLen = this._tcpBuf[0] | (this._tcpBuf[1] << 8) | (this._tcpBuf[2] << 16) | (this._tcpBuf[3] << 24);".into());
            lines.push("            frameLen += (this._tcpBuf[4] | (this._tcpBuf[5] << 8) | (this._tcpBuf[6] << 16) | (this._tcpBuf[7] << 24)) * 0x100000000;".into());
        } else {
            lines.push("            frameLen = this._tcpBuf[0] | (this._tcpBuf[1] << 8) | (this._tcpBuf[2] << 16) | (this._tcpBuf[3] << 24);".into());
        }
        lines.push(format!(
            "            if (this._tcpBuf.length < {len_bytes} + frameLen) return;"
        ));
        lines.push(format!(
            "            const inner = this._tcpBuf.slice({len_bytes}, {len_bytes} + frameLen);"
        ));
        lines.push(format!(
            "            this._tcpBuf = this._tcpBuf.slice({len_bytes} + frameLen);"
        ));
        lines.push("            this._handleMessage(inner);".into());
        lines.push("        }".into());
        lines.push("    }".into());
    }

    // _callNodeTcp — send request via persistent TCP, wait for response by req_id
    if has_nodetcp {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} handlerId".into());
        lines.push("     * @param {Uint8Array} payload".into());
        lines.push("     * @returns {Promise<Uint8Array>}".into());
        lines.push("     */".into());
        lines.push("    async _callNodeTcp(handlerId, payload) {".into());
        lines.push("        await this._ready;".into());
        lines.push("        return new Promise((resolve, reject) => {".into());
        lines.push("            const id = this._nextId;".into());
        lines.push("            this._nextId = this._nextId >= Number.MAX_SAFE_INTEGER ? 1 : this._nextId + 1;".into());
        lines.push("            const timer = setTimeout(() => { this._pending.delete(id); reject(new Error(`timeout id=${id}`)); }, 5000);".into());
        lines.push("            this._pending.set(id, { resolve, reject, timer });".into());
        lines.push(format!(
            "            const frame = new ArrayBuffer({sid} + 4 + {len_bytes} + payload.length);"
        ));
        lines.push("            const fv = new DataView(frame);".into());
        if seq64 {
            lines.push("            fv.setUint32(0, id & 0xFFFFFFFF, true);".into());
            lines.push(
                "            fv.setUint32(4, Math.floor(id / 0x100000000) & 0xFFFFFFFF, true);"
                    .into(),
            );
        } else {
            lines.push("            fv.setUint32(0, id, true);".into());
        }
        lines.push(format!("            fv.setUint32({sid}, handlerId, true);"));
        if len64 {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length & 0xFFFFFFFF, true);"
            ));
            lines.push(format!("            fv.setUint32({sid} + 8, Math.floor(payload.length / 0x100000000) & 0xFFFFFFFF, true);"));
        } else {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length, true);"
            ));
        }
        lines.push(format!(
            "            new Uint8Array(frame).set(payload, {sid} + 4 + {len_bytes});"
        ));
        lines.push(format!("            const buf = new ArrayBuffer({len_bytes} + {sid} + 4 + {len_bytes} + payload.length);"));
        lines.push("            const bv = new DataView(buf);".into());
        if len64 {
            lines.push(format!(
                "            const innerLen = {sid} + 4 + {len_bytes} + payload.length;"
            ));
            lines.push("            bv.setUint32(0, innerLen & 0xFFFFFFFF, true);".into());
            lines.push("            bv.setUint32(4, Math.floor(innerLen / 0x100000000) & 0xFFFFFFFF, true);".into());
        } else {
            lines.push(format!(
                "            bv.setUint32(0, {sid} + 4 + {len_bytes} + payload.length, true);"
            ));
        }
        lines.push(format!(
            "            new Uint8Array(buf).set(new Uint8Array(frame), {len_bytes});"
        ));
        lines.push("            this._tcpSocket.write(new Uint8Array(buf));".into());
        lines.push("        });".into());
        lines.push("    }".into());
    }

    // _callBunTcp — send request via persistent TCP, wait for response by req_id
    if has_buntcp {
        lines.push("".into());
        lines.push("    /**".into());
        lines.push("     * @param {number} handlerId".into());
        lines.push("     * @param {Uint8Array} payload".into());
        lines.push("     * @returns {Promise<Uint8Array>}".into());
        lines.push("     */".into());
        lines.push("    async _callBunTcp(handlerId, payload) {".into());
        lines.push("        await this._ready;".into());
        lines.push("        return new Promise((resolve, reject) => {".into());
        lines.push("            const id = this._nextId;".into());
        lines.push("            this._nextId = this._nextId >= Number.MAX_SAFE_INTEGER ? 1 : this._nextId + 1;".into());
        lines.push("            const timer = setTimeout(() => { this._pending.delete(id); reject(new Error(`timeout id=${id}`)); }, 5000);".into());
        lines.push("            this._pending.set(id, { resolve, reject, timer });".into());
        lines.push(format!(
            "            const frame = new ArrayBuffer({sid} + 4 + {len_bytes} + payload.length);"
        ));
        lines.push("            const fv = new DataView(frame);".into());
        if seq64 {
            lines.push("            fv.setUint32(0, id & 0xFFFFFFFF, true);".into());
            lines.push(
                "            fv.setUint32(4, Math.floor(id / 0x100000000) & 0xFFFFFFFF, true);"
                    .into(),
            );
        } else {
            lines.push("            fv.setUint32(0, id, true);".into());
        }
        lines.push(format!("            fv.setUint32({sid}, handlerId, true);"));
        if len64 {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length & 0xFFFFFFFF, true);"
            ));
            lines.push(format!("            fv.setUint32({sid} + 8, Math.floor(payload.length / 0x100000000) & 0xFFFFFFFF, true);"));
        } else {
            lines.push(format!(
                "            fv.setUint32({sid} + 4, payload.length, true);"
            ));
        }
        lines.push(format!(
            "            new Uint8Array(frame).set(payload, {sid} + 4 + {len_bytes});"
        ));
        lines.push(format!("            const buf = new ArrayBuffer({len_bytes} + {sid} + 4 + {len_bytes} + payload.length);"));
        lines.push("            const bv = new DataView(buf);".into());
        if len64 {
            lines.push(format!(
                "            const innerLen = {sid} + 4 + {len_bytes} + payload.length;"
            ));
            lines.push("            bv.setUint32(0, innerLen & 0xFFFFFFFF, true);".into());
            lines.push("            bv.setUint32(4, Math.floor(innerLen / 0x100000000) & 0xFFFFFFFF, true);".into());
        } else {
            lines.push(format!(
                "            bv.setUint32(0, {sid} + 4 + {len_bytes} + payload.length, true);"
            ));
        }
        lines.push(format!(
            "            new Uint8Array(buf).set(new Uint8Array(frame), {len_bytes});"
        ));
        lines.push("            this._tcpSocket.write(new Uint8Array(buf));".into());
        lines.push("        });".into());
        lines.push("    }".into());
    }

    // _writer
    lines.push("".into());
    lines.push("    /** @private */".into());
    lines.push("    _writer() {".into());
    lines.push("        const b = []; const v = new DataView(new ArrayBuffer(8));".into());
    lines.push("        return {".into());
    lines.push("            wU8(n) { b.push(n & 0xFF); },".into());
    lines.push("            wI8(n) { b.push(n & 0xFF); },".into());
    lines.push(
        "            wU16(n) { v.setUint16(0, n, true); b.push(v.getUint8(0), v.getUint8(1)); },"
            .into(),
    );
    lines.push(
        "            wI16(n) { v.setInt16(0, n, true); b.push(v.getUint8(0), v.getUint8(1)); },"
            .into(),
    );
    lines.push("            wU32(n) { v.setUint32(0, n, true); for (let i = 0; i < 4; i++) b.push(v.getUint8(i)); },".into());
    lines.push("            wI32(n) { v.setInt32(0, n, true); for (let i = 0; i < 4; i++) b.push(v.getUint8(i)); },".into());
    lines.push("            wU64(n) { this.wU32(n & 0xFFFFFFFF); this.wU32(Math.floor(n / 0x100000000) & 0xFFFFFFFF); },".into());
    lines.push("            wI64(n) { this.wU64(n); },".into());
    lines.push("            wF32(n) { v.setFloat32(0, n, true); for (let i = 0; 4 > i; i++) b.push(v.getUint8(i)); },".into());
    lines.push("            wF64(n) { v.setFloat64(0, n, true); for (let i = 0; 8 > i; i++) b.push(v.getUint8(i)); },".into());
    lines.push("            wB(b_) { b.push(b_ ? 1 : 0); },".into());
    lines.push("            wS(s) { const u = new TextEncoder().encode(s); this.wU32(u.length); b.push(...u); },".into());
    lines.push("            wBytes(a) { this.wU32(a.length); b.push(...a); },".into());
    lines.push("            wRaw(a) { b.push(...a); },".into());
    lines.push("            toBytes() { return new Uint8Array(b); },".into());
    lines.push("        };".into());
    lines.push("    }".into());

    // _reader
    lines.push("".into());
    lines.push("    /** @private */".into());
    lines.push("    _reader(data) {".into());
    lines.push("        const d = Array.from(data); let o = 0;".into());
    lines.push("        return {".into());
    lines.push("            rU8() { return d[o++]; },".into());
    lines.push("            rI8() { return d[o++]; },".into());
    lines.push("            rU16() { const v = d[o] | (d[o+1] << 8); o += 2; return v; },".into());
    lines.push(
        "            rI16() { const v = (d[o] | (d[o+1] << 8)) << 16 >> 16; o += 2; return v; },"
            .into(),
    );
    lines.push("            rU32() { const v = d[o] | (d[o+1] << 8) | (d[o+2] << 16) | (d[o+3] << 24); o += 4; return v >>> 0; },".into());
    lines.push("            rI32() { const v = d[o] | (d[o+1] << 8) | (d[o+2] << 16) | (d[o+3] << 24); o += 4; return v; },".into());
    lines.push("            rU64() { const lo = this.rU32(); const hi = this.rU32(); return hi * 0x100000000 + lo; },".into());
    lines.push("            rI64() { return this.rU64(); },".into());
    lines.push("            rF32() { const buf = new ArrayBuffer(4); const u = new Uint8Array(buf); u[0] = d[o]; u[1] = d[o+1]; u[2] = d[o+2]; u[3] = d[o+3]; o += 4; return new DataView(buf).getFloat32(0, true); },".into());
    lines.push("            rF64() { const buf = new ArrayBuffer(8); const u = new Uint8Array(buf); for (let i = 0; i < 8; i++) u[i] = d[o+i]; o += 8; return new DataView(buf).getFloat64(0, true); },".into());
    lines.push("            rB() { return d[o++] === 1; },".into());
    lines.push("            rS() { const len = this.rU32(); const u = new Uint8Array(d.slice(o, o + len)); o += len; return new TextDecoder().decode(u); },".into());
    lines.push("            rBytes(len) { const u = new Uint8Array(d.slice(o, o + len)); o += len; return u; },".into());
    lines.push("            eof() { return o >= d.length; },".into());
    lines.push("        };".into());
    lines.push("    }".into());

    // apis
    lines.push("".into());
    lines.push("    get apis() {".into());
    lines.push("        return {".into());

    let handler_obj = generate_handler_object_js(
        &svc.handlers,
        &svc.handlers,
        &[],
        "        ",
        &mut emitted,
        seq64,
        headers_cl.len(),
        debug,
        &class_name,
    );
    lines.push(handler_obj);

    // ── Ordinary-ws routes ────────────────────────────────────
    #[cfg(feature = "ordinary-ws")]
    {
        for ws_route in &svc.ws_routes {
            let path = ws_route.path;
            let handler_name = ws_route.handler_name;

            let trimmed = path.trim_start_matches('/').trim_end_matches('/');
            let segments: Vec<&str> = trimmed.split('/').collect();
            let has_params = segments.iter().any(|s| s.starts_with(':'));

            let param_names: Vec<&str> = segments
                .iter()
                .filter_map(|s| s.strip_prefix(':'))
                .collect();

            let sig_params = if param_names.is_empty() {
                "query".to_string()
            } else {
                let params_str = param_names.join(", ");
                format!("{}, query", params_str)
            };

            let mut path_expr = String::new();
            for (i, seg) in segments.iter().enumerate() {
                if i > 0 {
                    path_expr.push('/');
                }
                if let Some(param) = seg.strip_prefix(':') {
                    path_expr.push_str(&format!("${{encodeURIComponent({})}}", param));
                } else {
                    path_expr.push_str(seg);
                }
            }

            let ind = "            ";
            let mut body = String::new();
            body.push_str(&format!(
                "{}const scheme = this._tls ? 'wss' : 'ws';\n",
                ind
            ));
            if has_params {
                body.push_str(&format!(
                    "{}let url = `${{scheme}}://${{this._host}}:${{this._port}}/{}`;\n",
                    ind, path_expr
                ));
            } else {
                body.push_str(&format!(
                    "{}let url = `${{scheme}}://${{this._host}}:${{this._port}}/{}`;\n",
                    ind, trimmed
                ));
            }
            body.push_str(&format!("{}if (query) {{\n", ind));
            body.push_str(&format!(
                "    {}const qs = new URLSearchParams(query);\n",
                ind
            ));
            body.push_str(&format!(
                "    {}if (qs.toString()) url += '?' + qs.toString();\n",
                ind
            ));
            body.push_str(&format!("{}}}\n", ind));
            body.push_str(&format!("{}if (this._transport === 'uniws') {{\n", ind));
            body.push_str(&format!(
                "    {}return uni.connectSocket({{ url, complete: () => {{}} }});\n",
                ind
            ));
            body.push_str(&format!(
                "{}}} else if (this._transport === 'wxws') {{\n",
                ind
            ));
            body.push_str(&format!("    {}return wx.connectSocket({{ url }});\n", ind));
            body.push_str(&format!("{}}}\n", ind));
            body.push_str(&format!("{}return new WebSocket(url);\n", ind));

            lines.push(format!("        /** WebSocket: {} */", path));
            lines.push(format!(
                "        {}: ({}) => {{\n{}        }},",
                handler_name, sig_params, body
            ));
        }
    }

    // ── Ordinary-sse routes ────────────────────────────────────
    #[cfg(feature = "ordinary-sse")]
    {
        for sse_route in &svc.sse_routes {
            let path = sse_route.path;
            let handler_name = sse_route.handler_name;

            let trimmed = path.trim_start_matches('/').trim_end_matches('/');
            let segments: Vec<&str> = trimmed.split('/').collect();
            let has_params = segments.iter().any(|s| s.starts_with(':'));

            let param_names: Vec<&str> = segments
                .iter()
                .filter_map(|s| s.strip_prefix(':'))
                .collect();

            let sig_params = if param_names.is_empty() {
                "query".to_string()
            } else {
                let params_str = param_names
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}, query", params_str)
            };

            let mut path_expr = String::new();
            for (i, seg) in segments.iter().enumerate() {
                if i > 0 {
                    path_expr.push('/');
                }
                if let Some(param) = seg.strip_prefix(':') {
                    path_expr.push_str(&format!("${{encodeURIComponent({})}}", param));
                } else {
                    path_expr.push_str(seg);
                }
            }

            let ind = "        ";
            let mut body = String::new();
            body.push_str(&format!(
                "{}const scheme = this._tls ? 'https' : 'http';\n",
                ind
            ));
            if has_params {
                body.push_str(&format!(
                    "{}let url = `${{scheme}}://${{this._host}}:${{this._port}}/{}`;\n",
                    ind, path_expr
                ));
            } else {
                body.push_str(&format!(
                    "{}let url = `${{scheme}}://${{this._host}}:${{this._port}}/{}`;\n",
                    ind, trimmed
                ));
            }
            body.push_str(&format!("{}if (query) {{\n", ind));
            body.push_str(&format!(
                "    {}const qs = new URLSearchParams(query);\n",
                ind
            ));
            body.push_str(&format!(
                "    {}if (qs.toString()) url += '?' + qs.toString();\n",
                ind
            ));
            body.push_str(&format!("{}}}\n", ind));
            body.push_str(&format!("{}const es = new EventSource(url);\n", ind));
            body.push_str(&format!("{}return es;\n", ind));

            lines.push(format!("        /** SSE: {} */", path));
            lines.push(format!(
                "        {}: ({}) => {{\n{}        }},",
                handler_name, sig_params, body
            ));
        }
    }

    lines.push("        };".into());
    lines.push("    }".into());
    lines.push("}".into());

    lines.build()
}

/// Emits JavaScript validation checks for each field in a struct.
/// `Option<T>` fields are guarded by a null check before validation.
/// Nested structs and enums are recursed into. All validation failures
/// throw `AFastError` with the rule's code and message.
fn generate_validation_js(
    lines: &mut CodeBuf,
    var_prefix: &str,
    fields: &[crate::handler::FieldMeta],
    indent: &str,
) {
    for field in included(fields) {
        let is_option = field.ty.starts_with("Option<");
        let field_path = format!("{}.{}", var_prefix, field.name);

        if is_option && !field.validations.is_empty() {
            let tmp = format!("_tmp{}", field.name);
            lines.push(format!(
                "{indent}if({field_path}!=null){{const {tmp}={field_path};",
                indent = indent,
                field_path = field_path,
                tmp = tmp,
            ));
            let inner_indent = format!("{}    ", indent);
            emit_validation_checks_js(lines, &tmp, field.validations, &inner_indent);
            if let Some(structure_fn) = field.structure {
                let structure = structure_fn();
                match structure.kind {
                    crate::handler::TagKind::Struct(nested_fields) => {
                        generate_validation_js(lines, &tmp, nested_fields, &inner_indent);
                    }
                    crate::handler::TagKind::Enum(variants) => {
                        generate_enum_validation_js(lines, &tmp, variants, &inner_indent);
                    }
                }
            }
            lines.push(format!("{indent}}}", indent = indent));
        } else {
            if !field.validations.is_empty() {
                emit_validation_checks_js(lines, &field_path, field.validations, indent);
            }
            if let Some(structure_fn) = field.structure {
                let structure = structure_fn();
                let prefix = if is_option {
                    let tmp = format!("_tmp{}", field.name);
                    lines.push(format!(
                        "{indent}if({field_path}!=null){{const {tmp}={field_path};",
                        indent = indent,
                        field_path = field_path,
                        tmp = tmp,
                    ));
                    tmp
                } else {
                    field_path.clone()
                };
                let inner_indent = if is_option {
                    format!("{}    ", indent)
                } else {
                    indent.to_string()
                };
                match structure.kind {
                    crate::handler::TagKind::Struct(nested_fields) => {
                        generate_validation_js(lines, &prefix, nested_fields, &inner_indent);
                    }
                    crate::handler::TagKind::Enum(variants) => {
                        generate_enum_validation_js(lines, &prefix, variants, &inner_indent);
                    }
                }
                if is_option {
                    lines.push(format!("{indent}}}", indent = indent));
                }
            }
        }
    }
}

/// Emits individual validation rule checks (gt, gte, lt, lte, len, of)
/// for a single field as JavaScript `if` statements. Each check calls
/// `this._onError` (if set) and throws `AFastError` on failure.
fn emit_validation_checks_js(
    lines: &mut CodeBuf,
    field_path: &str,
    validations: &[crate::handler::ValidateRule],
    indent: &str,
) {
    for rule in validations {
        match rule {
            crate::handler::ValidateRule::Gt {
                value,
                code,
                message,
            } => {
                lines.push(format!(
                    "{indent}if(!({field_path}>{value})){{if(this._onError){{this._onError({code},'{message}');}}throw new AFastError({code},'{message}');}}",
                    indent = indent, field_path = field_path, value = value,
                    code = code, message = message,
                ));
            }
            crate::handler::ValidateRule::Gte {
                value,
                code,
                message,
            } => {
                lines.push(format!(
                    "{indent}if(!({field_path}>={value})){{if(this._onError){{this._onError({code},'{message}');}}throw new AFastError({code},'{message}');}}",
                    indent = indent, field_path = field_path, value = value,
                    code = code, message = message,
                ));
            }
            crate::handler::ValidateRule::Lt {
                value,
                code,
                message,
            } => {
                lines.push(format!(
                    "{indent}if(!({field_path}<{value})){{if(this._onError){{this._onError({code},'{message}');}}throw new AFastError({code},'{message}');}}",
                    indent = indent, field_path = field_path, value = value,
                    code = code, message = message,
                ));
            }
            crate::handler::ValidateRule::Lte {
                value,
                code,
                message,
            } => {
                lines.push(format!(
                    "{indent}if(!({field_path}<={value})){{if(this._onError){{this._onError({code},'{message}');}}throw new AFastError({code},'{message}');}}",
                    indent = indent, field_path = field_path, value = value,
                    code = code, message = message,
                ));
            }
            crate::handler::ValidateRule::Len {
                min,
                max,
                code,
                message,
            } => {
                if *min >= 0 && *max >= 0 {
                    lines.push(format!(
                        "{indent}if({field_path}.length<{min}||{field_path}.length>{max}){{if(this._onError){{this._onError({code},'{message}');}}throw new AFastError({code},'{message}');}}",
                        indent = indent, field_path = field_path, min = min, max = max,
                        code = code, message = message,
                    ));
                } else if *min >= 0 {
                    lines.push(format!(
                        "{indent}if({field_path}.length<{min}){{if(this._onError){{this._onError({code},'{message}');}}throw new AFastError({code},'{message}');}}",
                        indent = indent, field_path = field_path, min = min,
                        code = code, message = message,
                    ));
                } else if *max >= 0 {
                    lines.push(format!(
                        "{indent}if({field_path}.length>{max}){{if(this._onError){{this._onError({code},'{message}');}}throw new AFastError({code},'{message}');}}",
                        indent = indent, field_path = field_path, max = max,
                        code = code, message = message,
                    ));
                }
            }
            crate::handler::ValidateRule::Of {
                values,
                code,
                message,
            } => {
                let list = values.join(",");
                lines.push(format!(
                    "{indent}if(![{list}].includes({field_path})){{if(this._onError){{this._onError({code},'{message}');}}throw new AFastError({code},'{message}');}}",
                    indent = indent, list = list, field_path = field_path,
                    code = code, message = message,
                ));
            }
        }
    }
}

/// Emits JavaScript validation checks for an enum value. Each variant with
/// validation rules is guarded by a `tag === 'VariantName'` condition.
/// Tuple variants (single unnamed field) access data directly; named
/// variants access fields through `data.fieldName`.
fn generate_enum_validation_js(
    lines: &mut CodeBuf,
    var_prefix: &str,
    variants: &[crate::handler::EnumVariantMeta],
    indent: &str,
) {
    for variant in variants {
        if variant.fields.is_empty() {
            continue;
        }
        let variant_tag = &variant.name;
        if variant.fields.len() == 1 && variant.fields[0].name.starts_with("__") {
            let data_path = format!("{}.data", var_prefix);
            let inner = &variant.fields[0];
            if !inner.validations.is_empty() {
                lines.push(format!(
                    "{indent}if({var_prefix}.tag==='{variant_tag}'){{",
                    indent = indent,
                    var_prefix = var_prefix,
                    variant_tag = variant_tag,
                ));
                let inner_indent = format!("{}    ", indent);
                emit_validation_checks_js(lines, &data_path, inner.validations, &inner_indent);
                lines.push(format!("{indent}}}"));
            }
            if let Some(structure_fn) = inner.structure {
                let structure = structure_fn();
                let nested_prefix = format!("{}.data", var_prefix);
                match structure.kind {
                    crate::handler::TagKind::Struct(nested_fields) => {
                        lines.push(format!(
                            "{indent}if({var_prefix}.tag==='{variant_tag}'){{",
                            indent = indent,
                            var_prefix = var_prefix,
                            variant_tag = variant_tag,
                        ));
                        let inner_indent = format!("{}    ", indent);
                        generate_validation_js(lines, &nested_prefix, nested_fields, &inner_indent);
                        lines.push(format!("{indent}}}"));
                    }
                    crate::handler::TagKind::Enum(nested_variants) => {
                        lines.push(format!(
                            "{indent}if({var_prefix}.tag==='{variant_tag}'){{",
                            indent = indent,
                            var_prefix = var_prefix,
                            variant_tag = variant_tag,
                        ));
                        let inner_indent = format!("{}    ", indent);
                        generate_enum_validation_js(
                            lines,
                            &nested_prefix,
                            nested_variants,
                            &inner_indent,
                        );
                        lines.push(format!("{indent}}}"));
                    }
                }
            }
        } else {
            let data_prefix = format!("{}.data", var_prefix);
            let has_work = variant
                .fields
                .iter()
                .any(|f| !f.validations.is_empty() || f.structure.is_some());
            if !has_work {
                continue;
            }
            lines.push(format!(
                "{indent}if({var_prefix}.tag==='{variant_tag}'){{",
                indent = indent,
                var_prefix = var_prefix,
                variant_tag = variant_tag,
            ));
            let inner_indent = format!("{}    ", indent);
            generate_validation_js(lines, &data_prefix, variant.fields, &inner_indent);
            lines.push(format!("{indent}}}"));
        }
    }
}

// ─── File output ──────────────────────────────────────────────

/// Writes the generated JavaScript code for a single service to a `.js` file
/// in the target directory. The filename is derived from the service name.
#[cfg(feature = "js")]
fn write_service_js(
    svc: &Service,
    dir: &Path,
    calls: &[crate::JsTsCallType],
    debug: bool,
) -> Result<(), Error> {
    use std::fs;

    let code = generate_service_js(svc, calls, debug);
    let file_name = format!("{}.js", svc.name);
    let file_path = dir.join(&file_name);
    fs::write(&file_path, &code).map_err(|e| Error::Io {
        message: e.to_string(),
    })?;

    Ok(())
}

impl AFast {
    /// Returns the generated JavaScript client code for all registered services
    /// concatenated with double newlines. Uses `Fetch` and `Ws` call types with
    /// debug logging disabled by default.
    #[cfg(feature = "js")]
    pub fn get_js_code(&self) -> String {
        let mut parts = Vec::new();
        for svc in &self.services {
            if svc.name.is_empty() {
                continue;
            }
            parts.push(generate_service_js(
                svc,
                &[crate::JsTsCallType::Fetch, crate::JsTsCallType::Ws],
                false,
            ));
        }
        parts.join("\n\n")
    }

    /// Generates JavaScript client code files for all registered services
    /// and writes them into `dir`. Each service produces one `.js` file.
    /// `calls` selects which transport backends to include (Fetch, Ws, etc.).
    /// When `debug` is true the generated methods log requests and responses.
    #[cfg(feature = "js")]
    pub fn generate_js(
        &self,
        dir: &Path,
        calls: &[crate::JsTsCallType],
        debug: bool,
    ) -> Result<(), Error> {
        use std::fs;

        fs::create_dir_all(dir).map_err(|e| Error::Io {
            message: e.to_string(),
        })?;

        for svc in &self.services {
            if svc.name.is_empty() {
                continue;
            }
            write_service_js(svc, dir, calls, debug)?;
        }

        Ok(())
    }
}
