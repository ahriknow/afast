//! Rust client-code generator.
//!
//! Produces a Rust library crate containing service-specific client modules
//! with full type annotations. The generated code communicates with the server
//! over the afast binary protocol via TCP (async with tokio or synchronous).

#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use super::buf::{CodeBuf, matches_wildcard};
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

// ─── Enum tag size ─────────────────────────────────────────────────

fn tag_write() -> &'static str {
    if cfg!(feature = "tag-u32") {
        "write_u32"
    } else if cfg!(feature = "tag-u16") {
        "write_u16"
    } else {
        "write_u8"
    }
}

fn tag_read() -> &'static str {
    if cfg!(feature = "tag-u32") {
        "read_u32"
    } else if cfg!(feature = "tag-u16") {
        "read_u16"
    } else {
        "read_u8"
    }
}

// ─── Naming helpers ────────────────────────────────────────────────

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

fn to_snake_case(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_lowercase().next().unwrap_or(ch));
    }
    result
}

fn prefixed_type(prefix: &str, type_name: &str) -> String {
    if type_name.starts_with(prefix) {
        return type_name.to_string();
    }
    for i in 1..prefix.len() {
        let suffix = &prefix[i..];
        if type_name.starts_with(suffix) {
            return format!("{}{}", &prefix[..i], type_name);
        }
    }
    format!("{}{}", prefix, type_name)
}

fn handler_prefix(path: &[&str]) -> String {
    path.iter()
        .map(|s| to_pascal_case(s.trim_start_matches(':')))
        .collect::<String>()
}

struct CustomInfo {
    ty: String,
}

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

fn custom_index(handlers: &[Handler], ty: &str) -> usize {
    let customs = collect_customs(handlers);
    customs.iter().position(|c| c.ty == ty).unwrap_or(0)
}

// ─── Type mapping ──────────────────────────────────────────────────

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

fn rust_type_to_rs(ty: &str) -> String {
    let ty = normalize_rust_type(ty);
    match ty.as_str() {
        "i8" => "i8".to_string(),
        "u8" => "u8".to_string(),
        "i16" => "i16".to_string(),
        "u16" => "u16".to_string(),
        "i32" => "i32".to_string(),
        "u32" => "u32".to_string(),
        "i64" => "i64".to_string(),
        "u64" => "u64".to_string(),
        "usize" => "u64".to_string(),
        "f32" => "f32".to_string(),
        "f64" => "f64".to_string(),
        "bool" => "bool".to_string(),
        "String" => "String".to_string(),
        "&str" => "String".to_string(),
        "Vec<u8>" => "Vec<u8>".to_string(),
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            format!("Option<{}>", rust_type_to_rs(inner))
        }
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            format!("Vec<{}>", rust_type_to_rs(inner))
        }
        other => other.to_string(),
    }
}

fn type_name_for(prefix: &str, meta: &TagMeta) -> String {
    prefixed_type(prefix, meta.name)
}

// ─── Common module generation ──────────────────────────────────────

#[allow(clippy::vec_init_then_push)]
fn generate_common_rs(services: &[Service], _is_async: bool) -> String {
    let mut cb = CodeBuf::new();
    cb.l("// Auto-generated by afast. DO NOT EDIT.");
    cb.l("#![allow(dead_code, clippy::type_complexity, clippy::needless_borrow)]");
    cb.b();

    // ─── BinaryWriter ─────────────────────────────────────────────
    cb.l("/// Little-endian binary serializer for the afast wire protocol.");
    cb.l("#[derive(Debug, Default)]");
    cb.l("pub struct BinaryWriter {");
    cb.l("    buf: Vec<u8>,");
    cb.l("}");
    cb.b();
    cb.l("impl BinaryWriter {");
    cb.l("    pub fn new() -> Self { Self { buf: Vec::new() } }");
    cb.l("    pub fn write_u8(&mut self, n: u8) { self.buf.push(n); }");
    cb.l("    pub fn write_i8(&mut self, n: i8) { self.buf.push(n as u8); }");
    cb.l(
        "    pub fn write_u16(&mut self, n: u16) { self.buf.extend_from_slice(&n.to_le_bytes()); }",
    );
    cb.l(
        "    pub fn write_i16(&mut self, n: i16) { self.buf.extend_from_slice(&n.to_le_bytes()); }",
    );
    cb.l(
        "    pub fn write_u32(&mut self, n: u32) { self.buf.extend_from_slice(&n.to_le_bytes()); }",
    );
    cb.l(
        "    pub fn write_i32(&mut self, n: i32) { self.buf.extend_from_slice(&n.to_le_bytes()); }",
    );
    cb.l(
        "    pub fn write_u64(&mut self, n: u64) { self.buf.extend_from_slice(&n.to_le_bytes()); }",
    );
    cb.l(
        "    pub fn write_i64(&mut self, n: i64) { self.buf.extend_from_slice(&n.to_le_bytes()); }",
    );
    cb.l(
        "    pub fn write_f32(&mut self, n: f32) { self.buf.extend_from_slice(&n.to_le_bytes()); }",
    );
    cb.l(
        "    pub fn write_f64(&mut self, n: f64) { self.buf.extend_from_slice(&n.to_le_bytes()); }",
    );
    cb.l("    pub fn write_bool(&mut self, b: bool) { self.buf.push(if b { 1 } else { 0 }); }");
    cb.l("    pub fn write_str(&mut self, s: &str) {");
    cb.l("        let bytes = s.as_bytes();");
    cb.l("        self.write_u32(bytes.len() as u32);");
    cb.l("        self.buf.extend_from_slice(bytes);");
    cb.l("    }");
    cb.l("    pub fn write_bytes(&mut self, data: &[u8]) {");
    cb.l("        self.write_u32(data.len() as u32);");
    cb.l("        self.buf.extend_from_slice(data);");
    cb.l("    }");
    cb.l("    pub fn write_raw(&mut self, data: &[u8]) {");
    cb.l("        self.buf.extend_from_slice(data);");
    cb.l("    }");
    cb.l("    pub fn into_bytes(self) -> Vec<u8> { self.buf }");
    cb.l("}");
    cb.b();

    // ─── BinaryReader ─────────────────────────────────────────────
    cb.l("/// Little-endian binary deserializer for the afast wire protocol.");
    cb.l("pub struct BinaryReader<'a> {");
    cb.l("    data: &'a [u8],");
    cb.l("    offset: usize,");
    cb.l("}");
    cb.b();
    cb.l("impl<'a> BinaryReader<'a> {");
    cb.l("    pub fn new(data: &'a [u8]) -> Self { Self { data, offset: 0 } }");
    cb.l("    pub fn read_u8(&mut self) -> u8 { let v = self.data[self.offset]; self.offset += 1; v }");
    cb.l("    pub fn read_i8(&mut self) -> i8 { self.read_u8() as i8 }");
    cb.l("    pub fn read_u16(&mut self) -> u16 {");
    cb.l(
        "        let v = u16::from_le_bytes([self.data[self.offset], self.data[self.offset + 1]]);",
    );
    cb.l("        self.offset += 2; v");
    cb.l("    }");
    cb.l("    pub fn read_i16(&mut self) -> i16 { self.read_u16() as i16 }");
    cb.l("    pub fn read_u32(&mut self) -> u32 {");
    cb.l("        let v = u32::from_le_bytes([");
    cb.l("            self.data[self.offset], self.data[self.offset + 1],");
    cb.l("            self.data[self.offset + 2], self.data[self.offset + 3],");
    cb.l("        ]);");
    cb.l("        self.offset += 4; v");
    cb.l("    }");
    cb.l("    pub fn read_i32(&mut self) -> i32 { self.read_u32() as i32 }");
    cb.l("    pub fn read_u64(&mut self) -> u64 {");
    cb.l("        let lo = self.read_u32() as u64;");
    cb.l("        let hi = self.read_u32() as u64;");
    cb.l("        (hi << 32) | lo");
    cb.l("    }");
    cb.l("    pub fn read_i64(&mut self) -> i64 { self.read_u64() as i64 }");
    cb.l("    pub fn read_f32(&mut self) -> f32 {");
    cb.l("        let mut bytes = [0u8; 4];");
    cb.l("        bytes.copy_from_slice(&self.data[self.offset..self.offset + 4]);");
    cb.l("        self.offset += 4;");
    cb.l("        f32::from_le_bytes(bytes)");
    cb.l("    }");
    cb.l("    pub fn read_f64(&mut self) -> f64 {");
    cb.l("        let mut bytes = [0u8; 8];");
    cb.l("        bytes.copy_from_slice(&self.data[self.offset..self.offset + 8]);");
    cb.l("        self.offset += 8;");
    cb.l("        f64::from_le_bytes(bytes)");
    cb.l("    }");
    cb.l("    pub fn read_bool(&mut self) -> bool { self.read_u8() == 1 }");
    cb.l("    pub fn read_str(&mut self) -> String {");
    cb.l("        let len = self.read_u32() as usize;");
    cb.l("        let s = String::from_utf8_lossy(&self.data[self.offset..self.offset + len]).into_owned();");
    cb.l("        self.offset += len;");
    cb.l("        s");
    cb.l("    }");
    cb.l("    pub fn read_bytes_len(&mut self, len: usize) -> Vec<u8> {");
    cb.l("        let v = self.data[self.offset..self.offset + len].to_vec();");
    cb.l("        self.offset += len;");
    cb.l("        v");
    cb.l("    }");
    cb.l("}");
    cb.b();

    // ─── AfastError ───────────────────────────────────────────────
    cb.l("/// Error type for afast client operations.");
    cb.l("#[derive(Debug)]");
    cb.l("pub enum AfastError {");
    cb.l("    Validation { code: i64, field: String, message: String },");
    cb.l("    Network(String),");
    cb.l("    Decode(String),");
    cb.l("}");
    cb.b();
    cb.l("impl std::fmt::Display for AfastError {");
    cb.l("    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {");
    cb.l("        match self {");
    cb.l("            AfastError::Validation { code, field, message } => {");
    cb.l("                write!(f, \"AfastError({}): {} on field '{}'\", code, message, field)");
    cb.l("            }");
    cb.l("            AfastError::Network(msg) => write!(f, \"Network error: {}\", msg),");
    cb.l("            AfastError::Decode(msg) => write!(f, \"Decode error: {}\", msg),");
    cb.l("        }");
    cb.l("    }");
    cb.l("}");
    cb.b();
    cb.l("impl std::error::Error for AfastError {}");
    cb.b();

    // ─── AfError (server error response) ──────────────────────────
    cb.l("/// Server-side error with code and message.");
    cb.l("#[derive(Debug)]");
    cb.l("pub struct AfError {");
    cb.l("    pub code: i64,");
    cb.l("    pub message: String,");
    cb.l("}");
    cb.b();
    cb.l("impl std::fmt::Display for AfError {");
    cb.l("    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {");
    cb.l("        write!(f, \"AfError({}): {}\", self.code, self.message)");
    cb.l("    }");
    cb.l("}");
    cb.b();
    cb.l("impl std::error::Error for AfError {}");
    cb.b();

    // Collect all Custom types across services and emit their type definitions
    let mut all_customs: Vec<String> = Vec::new();
    for svc in services {
        if svc.name.is_empty() {
            continue;
        }
        for ci in collect_customs(&svc.handlers) {
            if !all_customs.contains(&ci.ty) {
                all_customs.push(ci.ty);
            }
        }
    }

    if !all_customs.is_empty() {
        // Emit Custom type definitions (struct/enum) so they're in scope
        let mut emitted = Vec::new();
        for svc in services {
            if svc.name.is_empty() {
                continue;
            }
            collect_custom_type_exports_rs(&svc.handlers, &[], &mut emitted, &mut cb);
        }
        cb.b();
    }

    // ─── AfastSocket (placeholder for long-connection support) ────
    cb.l("/// Handle for a long-connection (WebSocket/TCP) session.");
    cb.l("#[derive(Debug)]");
    cb.l("pub struct AfastSocket {");
    cb.l("    pub conn_id: u32,");
    cb.l("}");
    cb.b();

    cb.build()
}

// ─── Type definition generation ────────────────────────────────────

fn rs_type_def(meta: &TagMeta) -> String {
    rs_type_def_named(meta.name, meta)
}

fn rs_type_def_named(type_name: &str, meta: &TagMeta) -> String {
    match meta.kind {
        TagKind::Struct(fields) => {
            let mut lines = CodeBuf::new();
            lines.push("#[derive(Debug, Clone)]".to_string());
            lines.push(format!("pub struct {} {{", type_name));
            for field in included(fields) {
                let rs_ty = rust_type_to_rs(field.ty);
                lines.push(format!("    pub {}: {},", to_snake_case(field.name), rs_ty));
            }
            lines.push("}".to_string());
            lines.build()
        }
        TagKind::Enum(variants) => {
            let mut lines = CodeBuf::new();
            lines.push("#[derive(Debug, Clone)]".to_string());
            lines.push(format!("pub enum {} {{", type_name));
            for variant in variants {
                if variant.fields.is_empty() {
                    lines.push(format!("    {},", variant.name));
                } else if variant.fields.len() == 1 && variant.fields[0].name.starts_with("__") {
                    let rs_ty = rust_type_to_rs(variant.fields[0].ty);
                    lines.push(format!("    {}({}),", variant.name, rs_ty));
                } else {
                    let mut field_entries = Vec::new();
                    for field in included(variant.fields) {
                        let rs_ty = rust_type_to_rs(field.ty);
                        field_entries.push(format!(
                            "        {}: {},",
                            to_snake_case(field.name),
                            rs_ty
                        ));
                    }
                    lines.push(format!("    {} {{", variant.name));
                    lines.extend_vec(field_entries);
                    lines.push("    },".to_string());
                }
            }
            lines.push("}".to_string());
            lines.build()
        }
    }
}

fn extract_nested_types_rs(meta: &'static TagMeta, lines: &mut CodeBuf, emitted: &mut Vec<String>) {
    if !emitted.contains(&meta.name.to_string()) {
        emitted.push(meta.name.to_string());
        lines.push(rs_type_def(meta));
    }
    match meta.kind {
        TagKind::Struct(fields) => {
            for field in included(fields) {
                if let Some(structure_fn) = field.structure {
                    extract_nested_types_rs(structure_fn(), lines, emitted);
                }
            }
        }
        TagKind::Enum(variants) => {
            for variant in variants {
                for field in included(variant.fields) {
                    if let Some(structure_fn) = field.structure {
                        extract_nested_types_rs(structure_fn(), lines, emitted);
                    }
                }
            }
        }
    }
}

fn handler_type_exports_rs(prefix: &str, meta: &HandlerMeta, emitted: &mut Vec<String>) -> String {
    let mut lines = CodeBuf::new();

    for param in meta.params {
        match param.extractor {
            "Custom" => {}
            "Data" | "Query" | "Param" | "Body" => {
                if let Some(structure_fn) = param.structure {
                    let structure = structure_fn();
                    let type_name = prefixed_type(prefix, structure.name);
                    if !emitted.contains(&type_name) {
                        lines.push(rs_type_def_named(&type_name, structure));
                        emitted.push(type_name.clone());
                        emitted.push(structure.name.to_string());
                        extract_nested_types_rs(structure, &mut lines, emitted);
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
        let type_name = prefixed_type(prefix, structure.name);
        if !emitted.contains(&type_name) {
            lines.push(rs_type_def_named(&type_name, structure));
            emitted.push(type_name.clone());
            emitted.push(structure.name.to_string());
            extract_nested_types_rs(structure, &mut lines, emitted);
        }
    }

    lines.build()
}

// ─── Request serialization ─────────────────────────────────────────

fn generate_request_serialize_rs(
    lines: &mut CodeBuf,
    var: &str,
    ty: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
) {
    match ty {
        "i8" => lines.push(format!("{}w.write_i8({});", indent, var)),
        "i16" => lines.push(format!("{}w.write_i16({});", indent, var)),
        "i32" => lines.push(format!("{}w.write_i32({});", indent, var)),
        "i64" => lines.push(format!("{}w.write_i64({});", indent, var)),
        "u8" => lines.push(format!("{}w.write_u8({});", indent, var)),
        "u16" => lines.push(format!("{}w.write_u16({});", indent, var)),
        "u32" => lines.push(format!("{}w.write_u32({});", indent, var)),
        "u64" => lines.push(format!("{}w.write_u64({});", indent, var)),
        "usize" => lines.push(format!("{}w.write_u64({} as u64);", indent, var)),
        "f32" => lines.push(format!("{}w.write_f32({});", indent, var)),
        "f64" => lines.push(format!("{}w.write_f64({});", indent, var)),
        "bool" => lines.push(format!("{}w.write_bool({});", indent, var)),
        "String" | "&str" => lines.push(format!("{}w.write_str(&{});", indent, var)),
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            lines.push(format!("{}w.write_u32({}.len() as u32);", indent, var));
            if inner == "u8" {
                lines.push(format!("{}w.write_raw(&{});", indent, var));
            } else {
                lines.push(format!("{}for _e in &{} {{", indent, var));
                generate_request_serialize_rs(
                    lines,
                    "_e",
                    inner,
                    &format!("{}    ", indent),
                    structure,
                );
                lines.push(format!("{}}}", indent));
            }
        }
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            lines.push(format!("{}if let Some(ref _v) = {} {{", indent, var));
            lines.push(format!("{}    w.write_u8(1);", indent));
            generate_request_serialize_rs(
                lines,
                "_v",
                inner,
                &format!("{}    ", indent),
                structure,
            );
            lines.push(format!("{}}} else {{", indent));
            lines.push(format!("{}    w.write_u8(0);", indent));
            lines.push(format!("{}}}", indent));
        }
        _ => {
            if let Some(s) = structure {
                let meta = s();
                match meta.kind {
                    TagKind::Struct(fields) => {
                        for field in included(fields) {
                            let field_var = format!("{}.{}", var, to_snake_case(field.name));
                            generate_request_serialize_rs(
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
                        lines.push(format!("{}match &{} {{", indent, var));
                        for (i, variant) in variants.iter().enumerate() {
                            if variant.fields.is_empty() {
                                lines.push(format!(
                                    "{}    {}::{} => w.{}({}),",
                                    deep_indent, meta.name, variant.name, tw, i
                                ));
                            } else if variant.fields.len() == 1
                                && variant.fields[0].name.starts_with("__")
                            {
                                lines.push(format!(
                                    "{}    {}::{}(_v) => {{",
                                    deep_indent, meta.name, variant.name
                                ));
                                lines.push(format!("{}        w.{}({});", deep_indent, tw, i));
                                generate_request_serialize_rs(
                                    lines,
                                    "_v",
                                    variant.fields[0].ty,
                                    &format!("{}        ", indent),
                                    variant.fields[0].structure,
                                );
                                lines.push(format!("{}    }}", deep_indent));
                            } else {
                                let field_names: Vec<String> = variant
                                    .fields
                                    .iter()
                                    .map(|f| to_snake_case(f.name))
                                    .collect();
                                let bindings = field_names
                                    .iter()
                                    .map(|n| format!("{}: ref _{}", n, n))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                lines.push(format!(
                                    "{}    {}::{} {{ {} }} => {{",
                                    deep_indent, meta.name, variant.name, bindings
                                ));
                                lines.push(format!("{}        w.{}({});", deep_indent, tw, i));
                                for field in included(variant.fields) {
                                    let field_var = format!("_{}", to_snake_case(field.name));
                                    generate_request_serialize_rs(
                                        lines,
                                        &field_var,
                                        field.ty,
                                        &format!("{}        ", indent),
                                        field.structure,
                                    );
                                }
                                lines.push(format!("{}    }}", deep_indent));
                            }
                        }
                        lines.push(format!("{}}}", indent));
                    }
                }
            }
        }
    }
}

// ─── Response deserialization ──────────────────────────────────────

fn response_expr_rs(
    reader: &str,
    ty: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
    override_name: Option<&str>,
) -> String {
    let ty = &normalize_rust_type(ty);
    match ty.as_str() {
        "i8" => format!("{}.read_i8()", reader),
        "i16" => format!("{}.read_i16()", reader),
        "i32" => format!("{}.read_i32()", reader),
        "i64" => format!("{}.read_i64()", reader),
        "u8" => format!("{}.read_u8()", reader),
        "u16" => format!("{}.read_u16()", reader),
        "u32" => format!("{}.read_u32()", reader),
        "u64" => format!("{}.read_u64()", reader),
        "usize" => format!("{}.read_u64()", reader),
        "f32" => format!("{}.read_f32()", reader),
        "f64" => format!("{}.read_f64()", reader),
        "bool" => format!("{}.read_bool()", reader),
        "String" => format!("{}.read_str()", reader),
        "&str" => format!("{}.read_str()", reader),
        "Vec<u8>" => {
            format!(
                "{{ let _len = {}.read_u32() as usize; {}.read_bytes_len(_len) }}",
                reader, reader
            )
        }
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            let elem = response_expr_rs(reader, inner, indent, structure, None);
            format!(
                "{{ let _count = {}.read_u32() as usize; (0.._count).map(|_| {}).collect::<Vec<_>>() }}",
                reader, elem
            )
        }
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            // Strip the Option<> wrapper from the override name to get the inner type name
            let inner_override_owned = override_name.and_then(|n| {
                let normalized = normalize_rust_type(n);
                if normalized.starts_with("Option<") {
                    Some(normalized[7..normalized.len() - 1].to_string())
                } else {
                    None
                }
            });
            let body = response_expr_rs(
                reader,
                inner,
                indent,
                structure,
                inner_override_owned.as_deref(),
            );
            format!(
                "if {}.read_u8() == 1 {{ Some({}) }} else {{ None }}",
                reader, body
            )
        }
        _ => {
            if let Some(s) = structure {
                let meta = s();
                let type_name = override_name.unwrap_or(meta.name);
                match meta.kind {
                    TagKind::Struct(fields) => {
                        let inner_indent = format!("{}    ", indent);
                        let field_lines: Vec<String> = included(fields)
                            .map(|f| {
                                let expr = response_expr_rs(
                                    reader,
                                    f.ty,
                                    &inner_indent,
                                    f.structure,
                                    None,
                                );
                                format!("{}    {}: {},", inner_indent, to_snake_case(f.name), expr)
                            })
                            .collect();
                        format!("{} {{\n{}\n{}}}", type_name, field_lines.join("\n"), indent)
                    }
                    TagKind::Enum(variants) => {
                        let tr = tag_read();
                        let inner_indent = format!("{}    ", indent);
                        let deep_indent = format!("{}        ", indent);
                        let mut arms = Vec::new();
                        for (i, variant) in variants.iter().enumerate() {
                            if variant.fields.is_empty() {
                                arms.push(format!(
                                    "{}    {} => {}::{},",
                                    deep_indent, i, type_name, variant.name
                                ));
                            } else if variant.fields.len() == 1
                                && variant.fields[0].name.starts_with("__")
                            {
                                let expr = response_expr_rs(
                                    reader,
                                    variant.fields[0].ty,
                                    &deep_indent,
                                    variant.fields[0].structure,
                                    None,
                                );
                                arms.push(format!(
                                    "{}    {} => {}::{}({}),",
                                    deep_indent, i, type_name, variant.name, expr
                                ));
                            } else {
                                let mut field_entries = Vec::new();
                                for field in included(variant.fields) {
                                    let expr = response_expr_rs(
                                        reader,
                                        field.ty,
                                        &format!("{}        ", inner_indent),
                                        field.structure,
                                        None,
                                    );
                                    field_entries.push(format!(
                                        "{}        {}: {},",
                                        deep_indent,
                                        to_snake_case(field.name),
                                        expr
                                    ));
                                }
                                arms.push(format!(
                                    "{}    {} => {}::{} {{\n{}\n{}    }},",
                                    deep_indent,
                                    i,
                                    type_name,
                                    variant.name,
                                    field_entries.join("\n"),
                                    deep_indent
                                ));
                            }
                        }
                        arms.push(format!(
                            "{}    v => panic!(\"unknown enum tag {{}} for {}\", v),",
                            deep_indent, type_name
                        ));
                        format!(
                            "match {}.{}() {{\n{}\n{}}}",
                            reader,
                            tr,
                            arms.join("\n"),
                            inner_indent
                        )
                    }
                }
            } else {
                format!(
                    "{{ let _len = {}.read_u32() as usize; {}.read_bytes_len(_len) }}",
                    reader, reader
                )
            }
        }
    }
}

#[allow(dead_code)]
fn generate_return_rs(
    lines: &mut CodeBuf,
    reader: &str,
    ty: &str,
    resp_type: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
) {
    if ty == "()" {
        lines.push(format!("{}Ok(())", indent));
        return;
    }
    let expr = response_expr_rs(reader, ty, indent, structure, Some(resp_type));
    lines.push(format!("{}Ok({})", indent, expr));
}

// ─── Validation code generation ────────────────────────────────────

fn generate_validation_rs(
    lines: &mut CodeBuf,
    var_prefix: &str,
    fields: &[crate::handler::FieldMeta],
    indent: &str,
) {
    for field in included(fields) {
        let is_option = field.ty.starts_with("Option<");
        let field_path = format!("{}.{}", var_prefix, to_snake_case(field.name));

        if is_option && !field.validations.is_empty() {
            lines.push(format!(
                "{indent}if let Some(ref _v) = {} {{",
                field_path,
                indent = indent
            ));
            let inner_indent = format!("{}    ", indent);
            emit_validation_checks_rs(lines, "_v", field.name, field.validations, &inner_indent);
            if let Some(structure_fn) = field.structure {
                let structure = structure_fn();
                if let crate::handler::TagKind::Struct(nested_fields) = structure.kind {
                    generate_validation_rs(lines, "_v", nested_fields, &inner_indent);
                }
            }
            lines.push(format!("{indent}}}", indent = indent));
        } else {
            if !field.validations.is_empty() {
                emit_validation_checks_rs(
                    lines,
                    &field_path,
                    field.name,
                    field.validations,
                    indent,
                );
            }
            if let Some(structure_fn) = field.structure {
                let structure = structure_fn();
                if let crate::handler::TagKind::Struct(nested_fields) = structure.kind {
                    generate_validation_rs(lines, &field_path, nested_fields, indent);
                }
            }
        }
    }
}

fn emit_validation_checks_rs(
    lines: &mut CodeBuf,
    field_path: &str,
    field_name: &str,
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
                    "{indent}if !(*{} > {}f64) {{ return Err(AfastError::Validation {{ code: {}, field: \"{}\".into(), message: \"{}\".into() }}); }}",
                    field_path, value, code, field_name, message
                ));
            }
            crate::handler::ValidateRule::Gte {
                value,
                code,
                message,
            } => {
                lines.push(format!(
                    "{indent}if !(*{} >= {}f64) {{ return Err(AfastError::Validation {{ code: {}, field: \"{}\".into(), message: \"{}\".into() }}); }}",
                    field_path, value, code, field_name, message
                ));
            }
            crate::handler::ValidateRule::Lt {
                value,
                code,
                message,
            } => {
                lines.push(format!(
                    "{indent}if !(*{} < {}f64) {{ return Err(AfastError::Validation {{ code: {}, field: \"{}\".into(), message: \"{}\".into() }}); }}",
                    field_path, value, code, field_name, message
                ));
            }
            crate::handler::ValidateRule::Lte {
                value,
                code,
                message,
            } => {
                lines.push(format!(
                    "{indent}if !(*{} <= {}f64) {{ return Err(AfastError::Validation {{ code: {}, field: \"{}\".into(), message: \"{}\".into() }}); }}",
                    field_path, value, code, field_name, message
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
                        "{indent}if {}.len() < {} || {}.len() > {} {{ return Err(AfastError::Validation {{ code: {}, field: \"{}\".into(), message: \"{}\".into() }}); }}",
                        field_path, min, field_path, max, code, field_name, message
                    ));
                } else if *min >= 0 {
                    lines.push(format!(
                        "{indent}if {}.len() < {} {{ return Err(AfastError::Validation {{ code: {}, field: \"{}\".into(), message: \"{}\".into() }}); }}",
                        field_path, min, code, field_name, message
                    ));
                } else if *max >= 0 {
                    lines.push(format!(
                        "{indent}if {}.len() > {} {{ return Err(AfastError::Validation {{ code: {}, field: \"{}\".into(), message: \"{}\".into() }}); }}",
                        field_path, max, code, field_name, message
                    ));
                }
            }
            crate::handler::ValidateRule::Of {
                values,
                code,
                message,
            } => {
                let list: Vec<String> = values.iter().map(|v| format!("\"{}\"", v)).collect();
                lines.push(format!(
                    "{indent}if ![{}].contains(&{}.as_str()) {{ return Err(AfastError::Validation {{ code: {}, field: \"{}\".into(), message: \"{}\".into() }}); }}",
                    list.join(", "), field_path, code, field_name, message
                ));
            }
        }
    }
}

// ─── Handler method generation ─────────────────────────────────────

fn handler_method_rs(
    handler: &Handler,
    all_handlers: &[Handler],
    prefix: &str,
    base_indent: &str,
    _svc_name: &str,
    _class_name: &str,
    is_async: bool,
    debug: bool,
) -> String {
    let meta = handler.meta;
    let func_name = if !meta.api_name.is_empty() {
        to_snake_case(handler.meta.api_name)
    } else {
        to_snake_case(handler.meta.name)
    };
    let id = handler.stable_id;
    let indent = format!("{}    ", base_indent);
    let ind = format!("{}    ", indent);

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

    // Build function parameters
    let mut fn_params = Vec::new();
    fn_params.push("&self".to_string());
    for &(ref var, param) in &data_params {
        if let Some(structure_fn) = param.structure {
            let structure = structure_fn();
            let rs_ty = type_name_for(prefix, structure);
            fn_params.push(format!("{}: &{}", var, rs_ty));
        }
    }

    let params_str = fn_params.join(", ");

    // Determine return type
    let return_type = if meta.long_connection {
        "AfastSocket".to_string()
    } else if let Some(structure_fn) = meta.return_structure {
        let structure = structure_fn();
        let base = type_name_for(prefix, structure);
        // Wrap with Option<> if the handler's return type is Option<T>
        let normalized_ret = normalize_rust_type(meta.return_type);
        if normalized_ret.starts_with("Option<") {
            format!("Option<{}>", base)
        } else {
            base
        }
    } else {
        "()".to_string()
    };

    let return_type_str = format!("Result<{}, AfastError>", return_type);

    // Build method body
    let mut body_lines = CodeBuf::new();

    // Validation
    for &(ref var, param) in &data_params {
        if let Some(structure_fn) = param.structure {
            let structure = structure_fn();
            if let TagKind::Struct(fields) = structure.kind {
                generate_validation_rs(&mut body_lines, var, fields, &ind);
            }
        }
    }

    // Custom providers - downcast from Box<dyn Any> to concrete type
    for &(ci, ty, _structure) in &custom_indices {
        let rust_ty = rust_type_to_rs(ty);
        body_lines.push(format!("{}let _c{}_any = (self.customs[{}])().map_err(|e| AfastError::Network(format!(\"Custom provider error: {{}}\", e)))?;", ind, ci, ci));
        body_lines.push(format!("{}let _c{} = _c{}_any.downcast::<{}>().map_err(|_| AfastError::Network(\"Custom provider type mismatch\".into()))?;", ind, ci, ci, rust_ty));
    }

    // Serialize
    body_lines.push(format!("{}let mut w = BinaryWriter::new();", ind));

    for &(ci, _ty, structure) in &custom_indices {
        let var = format!("_c{}", ci);
        if let Some(structure_fn) = structure {
            let meta = structure_fn();
            if let TagKind::Struct(fields) = meta.kind {
                for field in included(fields) {
                    let field_var = format!("{}.{}", var, to_snake_case(field.name));
                    generate_request_serialize_rs(
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

    for &(ref var, _param) in &data_params {
        generate_request_serialize_rs(&mut body_lines, var, _param.ty, &ind, _param.structure);
    }

    body_lines.push(format!("{}let data = w.into_bytes();", ind));

    if debug {
        body_lines.push(format!(
            "{}eprintln!(\"[afast:debug] → call handlerId={} payloadLen={{}}\", data.len());",
            ind, id
        ));
    }

    // Call transport via standalone function
    if is_async {
        body_lines.push(format!(
            "{}let resp = _call_tcp(&self.stream, {}, data).await?;",
            ind, id
        ));
    } else {
        body_lines.push(format!(
            "{}let resp = _call_tcp(&self.stream, {}, data)?;",
            ind, id
        ));
    }

    if debug {
        body_lines.push(format!(
            "{}eprintln!(\"[afast:debug] ← call handlerId={} respLen={{}}\", resp.len());",
            ind, id
        ));
    }

    // Deserialize
    body_lines.push(format!("{}let mut r = BinaryReader::new(&resp);", ind));
    if meta.long_connection {
        body_lines.push(format!("{}let conn_id = r.read_u32();", ind));
        body_lines.push(format!("{}Ok(AfastSocket {{ conn_id }})", ind));
    } else if meta.return_type == "()" {
        body_lines.push(format!("{}Ok(())", ind));
    } else {
        let ret_expr = response_expr_rs(
            "r",
            meta.return_type,
            &ind,
            meta.return_structure,
            Some(&return_type),
        );
        body_lines.push(format!("{}Ok({})", ind, ret_expr));
    }

    let body = body_lines.build();
    let async_kw = if is_async { "async " } else { "" };
    format!(
        "{indent}pub {async_kw}fn {func_name}({params_str}) -> {return_type_str} {{\n{body}\n{indent}}}",
        indent = indent,
        async_kw = async_kw,
        func_name = func_name,
        params_str = params_str,
        return_type_str = return_type_str,
        body = body,
    )
}

fn collect_type_exports_rs(
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
            lines.push(handler_type_exports_rs(&prefix_str, h.meta, emitted));
        }
        if !h.children.is_empty() {
            collect_type_exports_rs(&h.children, &child_path, emitted, lines);
        }
    }
}

fn collect_custom_type_exports_rs(
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
            for param in h.meta.params {
                if param.extractor == "Custom"
                    && let Some(structure_fn) = param.structure
                {
                    let structure = structure_fn();
                    if !emitted.contains(&structure.name.to_string()) {
                        lines.push(rs_type_def(structure));
                        emitted.push(structure.name.to_string());
                        extract_nested_types_rs(structure, lines, emitted);
                    }
                }
            }
        }
        if !h.children.is_empty() {
            collect_custom_type_exports_rs(&h.children, &child_path, emitted, lines);
        }
    }
}

// ─── Service code generation ───────────────────────────────────────

pub(crate) fn generate_service_rs(
    svc: &Service,
    calls: &[crate::RsCallType],
    debug: bool,
) -> String {
    let is_async = calls
        .iter()
        .any(|c| matches!(c, crate::RsCallType::TcpAsync));

    let mut lines = CodeBuf::new();
    lines.push("// Auto-generated by afast. DO NOT EDIT.".to_string());
    lines.push("#![allow(dead_code, unused_mut, clippy::needless_borrow)]".to_string());
    lines.b();

    if is_async {
        lines.push("use tokio::io::{AsyncReadExt, AsyncWriteExt};".to_string());
        lines.push("use tokio::net::TcpStream;".to_string());
        lines.push("use tokio::sync::Mutex;".to_string());
    } else {
        lines.push("use std::io::{Read, Write};".to_string());
        lines.push("use std::net::TcpStream;".to_string());
        lines.push("use std::sync::Mutex;".to_string());
    }
    lines.push("use std::sync::Arc;".to_string());
    lines.push("use super::common::*;".to_string());
    lines.b();

    let class_name = to_pascal_case(&svc.name);

    // Emit type definitions
    let mut emitted = Vec::new();
    collect_type_exports_rs(&svc.handlers, &[], &mut emitted, &mut lines);

    // Collect custom types for this service
    let customs = collect_customs(&svc.handlers);

    // Generate client struct
    lines.push(format!("/// Client for the `{}` service.", svc.name));
    if !svc.desc.is_empty() {
        lines.push(format!("/// {}", svc.desc));
    }
    lines.push(format!("pub struct {}Client {{", class_name));
    lines.push("    addr: String,".to_string());
    lines.push("    stream: Arc<Mutex<TcpStream>>,".to_string());
    if !customs.is_empty() {
        lines.push("    #[allow(clippy::type_complexity)]".to_string());
        lines.push("    pub customs: Vec<Box<dyn Fn() -> Result<Box<dyn std::any::Any>, Box<dyn std::error::Error>> + Send + Sync>>,".to_string());
    }
    lines.push("}".to_string());
    lines.b();

    // Generate impl block
    lines.push(format!("impl {}Client {{", class_name));

    // Constructor
    if is_async {
        lines.push(format!(
            "    /// Creates a new `{0}Client` connected to the given address.",
            svc.name
        ));
        lines.push("    pub async fn new(addr: &str) -> Result<Self, AfastError> {".to_string());
        lines.push("        let stream = TcpStream::connect(addr).await".to_string());
        lines.push("            .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("        Ok(Self {".to_string());
        lines.push("            addr: addr.to_string(),".to_string());
        lines.push("            stream: Arc::new(Mutex::new(stream)),".to_string());
        if !customs.is_empty() {
            lines.push("            customs: Vec::new(),".to_string());
        }
        lines.push("        })".to_string());
        lines.push("    }".to_string());
    } else {
        lines.push(format!(
            "    /// Creates a new `{0}Client` connected to the given address.",
            svc.name
        ));
        lines.push("    pub fn new(addr: &str) -> Result<Self, AfastError> {".to_string());
        lines.push("        let stream = TcpStream::connect(addr)".to_string());
        lines.push("            .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("        Ok(Self {".to_string());
        lines.push("            addr: addr.to_string(),".to_string());
        lines.push("            stream: Arc::new(Mutex::new(stream)),".to_string());
        if !customs.is_empty() {
            lines.push("            customs: Vec::new(),".to_string());
        }
        lines.push("        })".to_string());
        lines.push("    }".to_string());
    }

    lines.b();

    // Group accessor methods and direct handler methods go inside the impl block
    // (these are generated below after the impl block closes for the standalone fn)

    // Close the main client impl block temporarily to generate standalone fn
    lines.push("}".to_string());
    lines.b();

    // ─── Generate a standalone call_tcp function ──────────────────
    // All structs (main client and nested groups) share this function.
    if is_async {
        lines.push("async fn _call_tcp(stream: &Arc<Mutex<TcpStream>>, handler_id: u32, payload: Vec<u8>) -> Result<Vec<u8>, AfastError> {".to_string());
        lines.push("    let mut stream = stream.lock().await;".to_string());
        lines.push("    let req_id: i32 = 1;".to_string());
        lines.push("    let mut frame = BinaryWriter::new();".to_string());
        lines.push("    frame.write_i32(req_id);".to_string());
        lines.push("    frame.write_u32(handler_id);".to_string());
        lines.push("    frame.write_u32(payload.len() as u32);".to_string());
        lines.push("    frame.write_raw(&payload);".to_string());
        lines.push("    let frame_bytes = frame.into_bytes();".to_string());
        lines.push("    let mut envelope = BinaryWriter::new();".to_string());
        lines.push("    envelope.write_u32(frame_bytes.len() as u32);".to_string());
        lines.push("    envelope.write_raw(&frame_bytes);".to_string());
        lines.push("    let data = envelope.into_bytes();".to_string());
        lines.push("    stream.write_all(&data).await".to_string());
        lines.push("        .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("    stream.flush().await".to_string());
        lines.push("        .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("    let mut len_buf = [0u8; 4];".to_string());
        lines.push("    stream.read_exact(&mut len_buf).await".to_string());
        lines.push("        .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("    let resp_len = u32::from_le_bytes(len_buf) as usize;".to_string());
        lines.push("    let mut resp_buf = vec![0u8; resp_len];".to_string());
        lines.push("    stream.read_exact(&mut resp_buf).await".to_string());
        lines.push("        .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("    if resp_buf.len() < 4 + 4 + 1 + 8 {".to_string());
        lines.push(
            "        return Err(AfastError::Decode(\"response too short\".into()));".to_string(),
        );
        lines.push("    }".to_string());
        lines.push("    let status = resp_buf[8];".to_string());
        lines.push("    if status == 1 {".to_string());
        lines.push(
            "        let code = i64::from_le_bytes(resp_buf[9..17].try_into().expect(\"resp_buf len checked\"));"
                .to_string(),
        );
        lines.push(
            "        let msg = String::from_utf8_lossy(&resp_buf[17..]).into_owned();".to_string(),
        );
        lines.push(
            "        return Err(AfastError::Network(format!(\"AfError({}): {}\", code, msg)));"
                .to_string(),
        );
        lines.push("    }".to_string());
        lines.push("    Ok(resp_buf[17..].to_vec())".to_string());
        lines.push("}".to_string());
    } else {
        lines.push("fn _call_tcp(stream: &Arc<Mutex<TcpStream>>, handler_id: u32, payload: Vec<u8>) -> Result<Vec<u8>, AfastError> {".to_string());
        lines.push("    let mut stream = stream.lock().expect(\"mutex poisoned\");".to_string());
        lines.push("    let req_id: i32 = 1;".to_string());
        lines.push("    let mut frame = BinaryWriter::new();".to_string());
        lines.push("    frame.write_i32(req_id);".to_string());
        lines.push("    frame.write_u32(handler_id);".to_string());
        lines.push("    frame.write_u32(payload.len() as u32);".to_string());
        lines.push("    frame.write_raw(&payload);".to_string());
        lines.push("    let frame_bytes = frame.into_bytes();".to_string());
        lines.push("    let mut envelope = BinaryWriter::new();".to_string());
        lines.push("    envelope.write_u32(frame_bytes.len() as u32);".to_string());
        lines.push("    envelope.write_raw(&frame_bytes);".to_string());
        lines.push("    let data = envelope.into_bytes();".to_string());
        lines.push("    stream.write_all(&data)".to_string());
        lines.push("        .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("    stream.flush()".to_string());
        lines.push("        .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("    let mut len_buf = [0u8; 4];".to_string());
        lines.push("    stream.read_exact(&mut len_buf)".to_string());
        lines.push("        .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("    let resp_len = u32::from_le_bytes(len_buf) as usize;".to_string());
        lines.push("    let mut resp_buf = vec![0u8; resp_len];".to_string());
        lines.push("    stream.read_exact(&mut resp_buf)".to_string());
        lines.push("        .map_err(|e| AfastError::Network(e.to_string()))?;".to_string());
        lines.push("    if resp_buf.len() < 4 + 4 + 1 + 8 {".to_string());
        lines.push(
            "        return Err(AfastError::Decode(\"response too short\".into()));".to_string(),
        );
        lines.push("    }".to_string());
        lines.push("    let status = resp_buf[8];".to_string());
        lines.push("    if status == 1 {".to_string());
        lines.push(
            "        let code = i64::from_le_bytes(resp_buf[9..17].try_into().expect(\"resp_buf len checked\"));"
                .to_string(),
        );
        lines.push(
            "        let msg = String::from_utf8_lossy(&resp_buf[17..]).into_owned();".to_string(),
        );
        lines.push(
            "        return Err(AfastError::Network(format!(\"AfError({}): {}\", code, msg)));"
                .to_string(),
        );
        lines.push("    }".to_string());
        lines.push("    Ok(resp_buf[17..].to_vec())".to_string());
        lines.push("}".to_string());
    }
    lines.b();

    // Re-open impl block for accessor methods and handler methods
    lines.push(format!("impl {}Client {{", class_name));

    // ─── Generate group structs and their impl blocks ─────────────
    // Recursively generates struct definitions and impl blocks for
    // nested API groups. Each group gets its own struct with a `stream`
    // reference and handler methods.
    fn gen_group_struct(
        handlers: &[Handler],
        all: &[Handler],
        path: &[&str],
        struct_name: &str,
        _parent_name: &str,
        svc_name: &str,
        is_async: bool,
        debug: bool,
        customs: &[CustomInfo],
        lines: &mut CodeBuf,
    ) {
        // Struct definition
        lines.push(format!("/// Nested API group `{}`.", struct_name));
        lines.push(format!("pub struct {}<'a> {{", struct_name));
        lines.push("    stream: &'a Arc<Mutex<TcpStream>>,".to_string());
        if !customs.is_empty() {
            lines.push("    #[allow(clippy::type_complexity)]".to_string());
            lines.push("    customs: &'a Vec<Box<dyn Fn() -> Result<Box<dyn std::any::Any>, Box<dyn std::error::Error>> + Send + Sync>>,".to_string());
        }
        lines.push("}".to_string());
        lines.b();

        // Impl block
        lines.push(format!("impl<'a> {}<'a> {{", struct_name));

        for h in handlers {
            let child_path = {
                let mut p = path.to_vec();
                p.push(h.name);
                p
            };
            if h.meta.name.is_empty() {
                // Accessor method for nested group
                let clean_name = h.name.trim_start_matches(':');
                let sub_struct_name = format!("{}{}Api", struct_name, to_pascal_case(clean_name));
                let accessor_name = to_snake_case(clean_name);
                lines.push(format!(
                    "    pub fn {}(&self) -> {}<'a> {{",
                    accessor_name, sub_struct_name
                ));
                lines.push(format!(
                    "        {} {{ stream: self.stream,",
                    sub_struct_name
                ));
                if !customs.is_empty() {
                    lines.push("            customs: self.customs,".to_string());
                }
                lines.push("        }".to_string());
                lines.push("    }".to_string());
                lines.b();
            } else if !h.meta.is_ordinary {
                // Binary handler method
                let prefix_str = handler_prefix(&child_path);
                let method = handler_method_rs(
                    h,
                    all,
                    &prefix_str,
                    "    ",
                    svc_name,
                    struct_name,
                    is_async,
                    debug,
                );
                lines.push(method);
                lines.b();
            }
        }

        lines.push("}".to_string());
        lines.b();

        // Recursively generate sub-group structs AFTER this impl block
        for h in handlers {
            let child_path = {
                let mut p = path.to_vec();
                p.push(h.name);
                p
            };
            if h.meta.name.is_empty() {
                let clean_name = h.name.trim_start_matches(':');
                let sub_struct_name = format!("{}{}Api", struct_name, to_pascal_case(clean_name));
                gen_group_struct(
                    &h.children,
                    all,
                    &child_path,
                    &sub_struct_name,
                    struct_name,
                    svc_name,
                    is_async,
                    debug,
                    customs,
                    lines,
                );
            }
        }
    }

    // Main client: group accessor methods
    for h in &svc.handlers {
        let child_path = vec![h.name];
        if h.meta.name.is_empty() {
            let clean_name = h.name.trim_start_matches(':');
            let group_struct_name = format!("{}{}Api", class_name, to_pascal_case(clean_name));
            let accessor_name = to_snake_case(clean_name);
            lines.push(format!(
                "    pub fn {}(&self) -> {}<'_> {{",
                accessor_name, group_struct_name
            ));
            lines.push(format!(
                "        {} {{ stream: &self.stream,",
                group_struct_name
            ));
            if !customs.is_empty() {
                lines.push("            customs: &self.customs,".to_string());
            }
            lines.push("        }".to_string());
            lines.push("    }".to_string());
            lines.b();
        } else if !h.meta.is_ordinary {
            // Direct binary handler
            let prefix_str = handler_prefix(&child_path);
            lines.push(handler_method_rs(
                h,
                &svc.handlers,
                &prefix_str,
                "    ",
                &svc.name,
                &class_name,
                is_async,
                debug,
            ));
            lines.b();
        }
    }

    // ── Ordinary-ws routes (inside impl block) ──────────────
    #[cfg(feature = "ordinary-ws")]
    {
        for ws_route in &svc.ws_routes {
            let path = ws_route.path;
            let handler_name = ws_route.handler_name;

            let trimmed = path.trim_start_matches('/').trim_end_matches('/');
            let segments: Vec<&str> = trimmed.split('/').collect();

            let param_names: Vec<&str> = segments
                .iter()
                .filter_map(|s| s.strip_prefix(':'))
                .collect();

            let sig_params = if param_names.is_empty() {
                "&self, query: Option<&std::collections::HashMap<String, String>>".to_string()
            } else {
                let params_str = param_names
                    .iter()
                    .map(|p| format!("{}: &str", p))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "&self, {}, query: Option<&std::collections::HashMap<String, String>>",
                    params_str
                )
            };

            let path_fmt = segments
                .iter()
                .map(|s| {
                    if let Some(p) = s.strip_prefix(':') {
                        format!("{{{}}}", p)
                    } else {
                        s.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("/");

            let fn_name = to_snake_case(handler_name);
            lines.b();
            lines.push(format!("    /// WebSocket connect: {}", path));
            lines.push(format!(
                "    pub async fn {}({}) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Box<dyn std::error::Error + Send + Sync>> {{",
                fn_name, sig_params
            ));
            lines.push("        let scheme = if self.addr.starts_with(\"tls:\") || self.addr.starts_with(\"wss:\") { \"wss\" } else { \"ws\" };".to_string());
            lines.push("        let addr = self.addr.trim_start_matches(\"tls:\").trim_start_matches(\"wss:\").trim_start_matches(\"ws:\").trim_start_matches(\"tcp:\");".to_string());
            lines.push(format!(
                "        let mut url = format!(\"{{}}://{{}}/{}\", scheme, addr);",
                path_fmt
            ));
            lines.push("        if let Some(q) = query {".to_string());
            lines.push("            let qs: String = q.iter()".to_string());
            lines.push("                .map(|(k, v)| format!(\"{}={}\", k, v))".to_string());
            lines.push("                .collect::<Vec<_>>()".to_string());
            lines.push("                .join(\"&\");".to_string());
            lines.push(
                "            if !qs.is_empty() { url.push('?'); url.push_str(&qs); }".to_string(),
            );
            lines.push("        }".to_string());
            lines.push(
                "        let (ws, _) = tokio_tungstenite::connect_async(&url).await?;".to_string(),
            );
            lines.push("        Ok(ws)".to_string());
            lines.push("    }".to_string());
        }
    }

    lines.push("}".to_string());
    lines.b();

    // Generate nested group structs (outside the main impl block)
    for h in &svc.handlers {
        let child_path = vec![h.name];
        if h.meta.name.is_empty() {
            let clean_name = h.name.trim_start_matches(':');
            let group_struct_name = format!("{}{}Api", class_name, to_pascal_case(clean_name));
            gen_group_struct(
                &h.children,
                &svc.handlers,
                &child_path,
                &group_struct_name,
                &class_name,
                &svc.name,
                is_async,
                debug,
                &customs,
                &mut lines,
            );
        }
    }

    lines.build()
}

// ─── File output ───────────────────────────────────────────────────

fn write_service_rs(
    svc: &Service,
    dir: &Path,
    calls: &[crate::RsCallType],
    debug: bool,
) -> Result<(), Error> {
    use std::fs;

    let code = generate_service_rs(svc, calls, debug);
    let file_name = format!("{}.rs", svc.name);
    let file_path = dir.join(&file_name);
    fs::write(&file_path, &code).map_err(|e| Error::Io {
        message: e.to_string(),
    })?;

    Ok(())
}

impl AFast {
    /// Returns the complete generated Rust code as a single string.
    #[cfg(feature = "rs")]
    pub fn get_rs_code(&self) -> String {
        let is_async = true;
        let common = generate_common_rs(&self.services, is_async);
        let mut parts = vec![common];
        for svc in &self.services {
            if svc.name.is_empty() {
                continue;
            }
            parts.push(generate_service_rs(
                svc,
                &[crate::RsCallType::TcpAsync],
                false,
            ));
        }
        parts.join("\n\n")
    }

    /// Generates Rust client code for all registered services and writes
    /// the files to `dir`. Produces one `common.rs` with shared types and
    /// one `{service}.rs` per service.
    /// When `filter` is `Some`, only services whose names appear in the list
    /// are generated.
    #[cfg(feature = "rs")]
    pub fn generate_rs(
        &self,
        dir: &Path,
        calls: &[crate::RsCallType],
        debug: bool,
        filter: Option<&[String]>,
    ) -> Result<(), Error> {
        use std::fs;

        fs::create_dir_all(dir).map_err(|e| Error::Io {
            message: e.to_string(),
        })?;

        let is_async = calls
            .iter()
            .any(|c| matches!(c, crate::RsCallType::TcpAsync));

        let filtered_svcs: Vec<Service> = self
            .services
            .iter()
            .filter(|svc| {
                !svc.name.is_empty()
                    && filter
                        .map(|f| f.iter().any(|p| matches_wildcard(&svc.name, p)))
                        .unwrap_or(true)
            })
            .cloned()
            .collect();

        let common = generate_common_rs(&filtered_svcs, is_async);
        fs::write(dir.join("common.rs"), &common).map_err(|e| Error::Io {
            message: e.to_string(),
        })?;

        // Generate mod.rs
        let mut mod_lines = CodeBuf::new();
        mod_lines.push("// Auto-generated by afast. DO NOT EDIT.".to_string());
        mod_lines.b();
        mod_lines.push("pub mod common;".to_string());
        for svc in &filtered_svcs {
            mod_lines.push(format!("pub mod {};", svc.name));
        }
        mod_lines.b();
        fs::write(dir.join("mod.rs"), mod_lines.build()).map_err(|e| Error::Io {
            message: e.to_string(),
        })?;

        for svc in &self.services {
            if svc.name.is_empty() {
                continue;
            }
            if let Some(f) = filter
                && !f.iter().any(|p| matches_wildcard(&svc.name, p))
            {
                continue;
            }
            write_service_rs(svc, dir, calls, debug)?;
        }

        Ok(())
    }
}
