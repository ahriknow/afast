//! C# / .NET client-code generator.
//!
//! Produces C# source files for .NET 10+ projects containing service-specific
//! client classes with full type annotations.  The generated code communicates
//! with the server over the afast binary protocol via HTTP POST, WebSocket, or TCP.

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
        "WriteU32"
    } else if cfg!(feature = "tag-u16") {
        "WriteU16"
    } else {
        "WriteU8"
    }
}

fn tag_read() -> &'static str {
    if cfg!(feature = "tag-u32") {
        "ReadU32"
    } else if cfg!(feature = "tag-u16") {
        "ReadU16"
    } else {
        "ReadU8"
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

fn to_camel_case(s: &str) -> String {
    if s.starts_with("__") {
        return s.to_string();
    }
    let pascal = to_pascal_case(s);
    let mut c = pascal.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
    }
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
        .collect()
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

#[cfg(feature = "ordinary-http")]
fn collect_headers(handlers: &[Handler]) -> Vec<CustomInfo> {
    let mut headers: Vec<CustomInfo> = Vec::new();
    collect_headers_recursive(handlers, &mut headers);
    headers
}

#[cfg(feature = "ordinary-http")]
fn collect_headers_recursive(handlers: &[Handler], headers: &mut Vec<CustomInfo>) {
    for h in handlers {
        for param in h.meta.params {
            if param.extractor == "Header" {
                let ty = param.ty.to_string();
                if !headers.iter().any(|c| c.ty == ty) {
                    headers.push(CustomInfo { ty });
                }
            }
        }
        collect_headers_recursive(&h.children, headers);
    }
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

#[cfg(feature = "ordinary-http")]
fn unwrap_json_type(ty: &str) -> Option<String> {
    let ty = normalize_rust_type(ty);
    if ty.starts_with("Json<") && ty.ends_with('>') {
        let inner = &ty[5..ty.len() - 1].trim();
        Some(normalize_rust_type(inner))
    } else {
        None
    }
}

fn rust_type_to_cs(ty: &str) -> String {
    let ty = normalize_rust_type(ty);
    match ty.as_str() {
        "i8" => "sbyte".to_string(),
        "u8" => "byte".to_string(),
        "i16" => "short".to_string(),
        "u16" => "ushort".to_string(),
        "i32" => "int".to_string(),
        "u32" => "uint".to_string(),
        "i64" => "long".to_string(),
        "u64" => "ulong".to_string(),
        "usize" => "ulong".to_string(),
        "f32" => "float".to_string(),
        "f64" => "double".to_string(),
        "bool" => "bool".to_string(),
        "String" => "string".to_string(),
        "&str" => "string".to_string(),
        "Vec<u8>" => "byte[]".to_string(),
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            format!("{}?", rust_type_to_cs(inner))
        }
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            format!("List<{}>", rust_type_to_cs(inner))
        }
        other => other.to_string(),
    }
}

// ─── Common file generation ────────────────────────────────────────

#[allow(clippy::vec_init_then_push)]
fn generate_common_cs(services: &[Service], _seq64: bool, _has_ws: bool, _has_tcp: bool) -> String {
    let mut cb = CodeBuf::new();
    cb.l("// Auto-generated by afast. DO NOT EDIT.");
    cb.l("#pragma warning disable CS8618, CS8603, CS8604, CS0649");
    cb.l("using System;");
    cb.l("using System.Collections.Generic;");
    cb.l("using System.IO;");
    if _has_ws || _has_tcp {
        cb.l("using System.Net.Sockets;");
    }
    if _has_ws {
        cb.l("using System.Net.WebSockets;");
    }
    cb.l("using System.Text;");
    cb.l("using System.Text.Json;");
    cb.l("using System.Threading;");
    cb.l("using System.Threading.Tasks;");
    cb.b();
    cb.l("namespace Afast.Generated;");
    cb.b();

    // ─── BinaryWriter ─────────────────────────────────────────
    cb.l("internal class BinaryWriter");
    cb.l("{");
    cb.l("    private readonly List<byte> _buf = new();");
    cb.b();
    cb.l("    public void WriteU8(int n) => _buf.Add((byte)(n & 0xFF));");
    cb.l("    public void WriteI8(int n) => _buf.Add((byte)(n & 0xFF));");
    cb.l("    public void WriteU16(int n) { _buf.Add((byte)(n & 0xFF)); _buf.Add((byte)((n >> 8) & 0xFF)); }");
    cb.l("    public void WriteI16(int n) => WriteU16(n);");
    cb.l("    public void WriteU32(int n) { _buf.Add((byte)(n & 0xFF)); _buf.Add((byte)((n >> 8) & 0xFF)); _buf.Add((byte)((n >> 16) & 0xFF)); _buf.Add((byte)((n >> 24) & 0xFF)); }");
    cb.l("    public void WriteI32(int n) => WriteU32(n);");
    cb.l("    public void WriteU64(long n) { WriteU32((int)(n & 0xFFFFFFFF)); WriteU32((int)((n >> 32) & 0xFFFFFFFF)); }");
    cb.l("    public void WriteI64(long n) => WriteU64(n);");
    cb.l(
        "    public void WriteF32(float n) { var b = BitConverter.GetBytes(n); _buf.AddRange(b); }",
    );
    cb.l("    public void WriteF64(double n) { var b = BitConverter.GetBytes(n); _buf.AddRange(b); }");
    cb.l("    public void WriteBool(bool b) => _buf.Add(b ? (byte)1 : (byte)0);");
    cb.l("    public void WriteString(string s) { var u = Encoding.UTF8.GetBytes(s); WriteU32(u.Length); _buf.AddRange(u); }");
    cb.l("    public void WriteBytes(byte[] a) { WriteU32(a.Length); _buf.AddRange(a); }");
    cb.l("    public void WriteRaw(byte[] a) => _buf.AddRange(a);");
    cb.l("    public byte[] ToBytes() => _buf.ToArray();");
    cb.l("}");
    cb.b();

    // ─── BinaryReader ─────────────────────────────────────────
    cb.l("internal class BinaryReader");
    cb.l("{");
    cb.l("    private readonly byte[] _d;");
    cb.l("    private int _o;");
    cb.b();
    cb.l("    public BinaryReader(byte[] d) { _d = d; _o = 0; }");
    cb.b();
    cb.l("    public int ReadU8() => _d[_o++] & 0xFF;");
    cb.l("    public int ReadI8() => (sbyte)_d[_o++];");
    cb.l("    public int ReadU16() { int v = (_d[_o] & 0xFF) | ((_d[_o + 1] & 0xFF) << 8); _o += 2; return v; }");
    cb.l("    public int ReadI16() => (short)ReadU16();");
    cb.l("    public int ReadU32() { int v = (_d[_o] & 0xFF) | ((_d[_o + 1] & 0xFF) << 8) | ((_d[_o + 2] & 0xFF) << 16) | ((_d[_o + 3] & 0xFF) << 24); _o += 4; return v; }");
    cb.l("    public int ReadI32() => ReadU32();");
    cb.l("    public long ReadU64() { long lo = (uint)ReadU32(); long hi = (uint)ReadU32(); return (hi << 32) | lo; }");
    cb.l("    public long ReadI64() => ReadU64();");
    cb.l("    public float ReadF32() { float v = BitConverter.ToSingle(_d, _o); _o += 4; return v; }");
    cb.l("    public double ReadF64() { double v = BitConverter.ToDouble(_d, _o); _o += 8; return v; }");
    cb.l("    public bool ReadBool() => _d[_o++] == 1;");
    cb.l("    public string ReadString() { int len = ReadU32(); string s = Encoding.UTF8.GetString(_d, _o, len); _o += len; return s; }");
    cb.l("    public byte[] ReadBytes(int len) { var a = new byte[len]; Array.Copy(_d, _o, a, 0, len); _o += len; return a; }");
    cb.l("    public byte[] ReadBytesLen() { int len = ReadU32(); return ReadBytes(len); }");
    cb.l("    public bool Eof => _o >= _d.Length;");
    cb.l("}");
    cb.b();

    // ─── AFastValidationError ──────────────────────────────────
    cb.l("public class AFastValidationError : Exception");
    cb.l("{");
    cb.l("    public long Code { get; }");
    cb.l("    public string Field { get; }");
    cb.l("    public AFastValidationError(long code, string field, string message) : base(message) { Code = code; Field = field; }");
    cb.l("}");
    cb.b();

    // ─── AfastErrorResponse (ordinary-http) ────────────────────
    #[cfg(feature = "ordinary-http")]
    {
        cb.l("public class AfastErrorResponse");
        cb.l("{");
        cb.l("    public int Code { get; set; }");
        cb.l("    public string Message { get; set; } = \"\";");
        cb.l("}");
        cb.b();
    }

    // ─── AfastSocket ──────────────────────────────────────────
    cb.l("public class AfastSocket : IAsyncDisposable");
    cb.l("{");
    cb.l("    public int ConnId { get; }");
    cb.l("    public bool IsClosed { get; private set; }");
    cb.l("    private bool _closing;");
    cb.l("    private readonly Action<byte[]> _sendRaw;");
    cb.l("    private readonly Action<byte[], Action<byte[], Action<object>>> _callback;");
    cb.b();
    cb.l("    public AfastSocket(int connId, Action<byte[]> sendRaw, Action<byte[], Action<byte[], Action<object>>> callback)");
    cb.l("    {");
    cb.l("        ConnId = connId;");
    cb.l("        _sendRaw = sendRaw;");
    cb.l("        _callback = callback;");
    cb.l("    }");
    cb.b();
    cb.l("    public void Send(byte[] data)");
    cb.l("    {");
    cb.l(
        "        if (_closing || IsClosed) throw new InvalidOperationException(\"socket closed\");",
    );
    cb.l("        var w = new BinaryWriter();");
    cb.l("        w.WriteU32(0);");
    cb.l("        w.WriteU32(ConnId);");
    cb.l("        w.WriteU32(data.Length);");
    cb.l("        w.WriteRaw(data);");
    cb.l("        _sendRaw(w.ToBytes());");
    cb.l("    }");
    cb.b();
    cb.l("    public async ValueTask DisposeAsync()");
    cb.l("    {");
    cb.l("        if (IsClosed) return;");
    cb.l("        _closing = true;");
    cb.l("        var w = new BinaryWriter();");
    cb.l("        w.WriteU32(0);");
    cb.l("        w.WriteU32(ConnId);");
    cb.l("        w.WriteU32(0);");
    cb.l("        _sendRaw(w.ToBytes());");
    cb.l("        await Task.Delay(500);");
    cb.l("        IsClosed = true;");
    cb.l("    }");
    cb.b();
    cb.l("    internal void OnMessage(byte[] data)");
    cb.l("    {");
    cb.l(
        "        if (_closing && data.Length == 0) { IsClosed = true; _closing = false; return; }",
    );
    cb.l("        if (!IsClosed) _callback(data, (d, _) => Send(d));");
    cb.l("    }");
    cb.l("}");
    cb.b();

    // Gather every Custom extractor type from every service
    let mut all_customs = Vec::new();
    for svc in services {
        if svc.name.is_empty() {
            continue;
        }
        let customs = collect_customs(&svc.handlers);
        for ci in customs {
            if !all_customs.iter().any(|c: &CustomInfo| c.ty == ci.ty) {
                all_customs.push(ci);
            }
        }
    }

    if !all_customs.is_empty() {
        let mut emitted = Vec::new();
        for svc in services {
            if svc.name.is_empty() {
                continue;
            }
            collect_custom_type_exports_cs(&svc.handlers, &[], &mut emitted, &mut cb);
        }
        cb.b();
    }

    #[cfg(feature = "ordinary-http")]
    {
        let mut all_headers = Vec::new();
        for svc in services {
            if svc.name.is_empty() {
                continue;
            }
            let headers = collect_headers(&svc.handlers);
            for ci in headers {
                if !all_headers.iter().any(|c: &CustomInfo| c.ty == ci.ty) {
                    all_headers.push(ci);
                }
            }
        }

        if !all_headers.is_empty() {
            let mut emitted = Vec::new();
            for svc in services {
                if svc.name.is_empty() {
                    continue;
                }
                collect_header_type_exports_cs(&svc.handlers, &[], &mut emitted, &mut cb);
            }
            cb.b();
        }
    }

    cb.build()
}

fn collect_custom_type_exports_cs(
    handlers: &[Handler],
    _path: &[&str],
    emitted: &mut Vec<String>,
    cb: &mut CodeBuf,
) {
    for h in handlers {
        if !h.meta.name.is_empty() {
            for param in h.meta.params {
                if param.extractor == "Custom"
                    && let Some(structure_fn) = param.structure
                {
                    let structure = structure_fn();
                    if !emitted.contains(&structure.name.to_string()) {
                        cb.push(cs_type_def(structure));
                        emitted.push(structure.name.to_string());
                        extract_nested_types_cs(structure, cb, emitted);
                    }
                }
            }
        }
        if !h.children.is_empty() {
            collect_custom_type_exports_cs(&h.children, &[], emitted, cb);
        }
    }
}

#[cfg(feature = "ordinary-http")]
fn collect_header_type_exports_cs(
    handlers: &[Handler],
    _path: &[&str],
    emitted: &mut Vec<String>,
    cb: &mut CodeBuf,
) {
    for h in handlers {
        if !h.meta.name.is_empty() {
            for param in h.meta.params {
                if param.extractor == "Header"
                    && let Some(structure_fn) = param.structure
                {
                    let structure = structure_fn();
                    if !emitted.contains(&structure.name.to_string()) {
                        cb.push(cs_type_def(structure));
                        emitted.push(structure.name.to_string());
                        extract_nested_types_cs(structure, cb, emitted);
                    }
                }
            }
        }
        if !h.children.is_empty() {
            collect_header_type_exports_cs(&h.children, &[], emitted, cb);
        }
    }
}

// ─── Type definition generation ────────────────────────────────────

fn cs_type_def(meta: &TagMeta) -> String {
    match meta.kind {
        TagKind::Struct(fields) => {
            let mut cb = CodeBuf::new();
            if !meta.desc.is_empty() {
                cb.push(format!("/// <summary>{}</summary>", meta.desc));
            }
            cb.push(format!("public class {}", meta.name));
            cb.l("{");
            for field in included(fields) {
                let cs_ty = rust_type_to_cs(field.ty);
                let default = cs_default_value(&cs_ty);
                if !field.desc.is_empty() {
                    cb.push(format!("    /// <summary>{}</summary>", field.desc));
                }
                cb.push(format!(
                    "    public {} {} {{ get; set; }}{}",
                    cs_ty,
                    to_pascal_case(field.name),
                    default
                ));
            }
            cb.l("}");
            cb.build()
        }
        TagKind::Enum(variants) => {
            let mut cb = CodeBuf::new();
            // Abstract base record
            if !meta.desc.is_empty() {
                cb.push(format!("/// <summary>{}</summary>", meta.desc));
            }
            cb.push(format!("public abstract record {};", meta.name));
            cb.b();
            for variant in variants {
                if variant.fields.is_empty() {
                    cb.push(format!(
                        "public sealed record {} : {};",
                        variant.name, meta.name
                    ));
                } else if variant.fields.len() == 1 && variant.fields[0].name.starts_with("__") {
                    let cs_ty = rust_type_to_cs(variant.fields[0].ty);
                    cb.push(format!(
                        "public sealed record {}({} __0) : {};",
                        variant.name, cs_ty, meta.name
                    ));
                } else {
                    let mut field_entries = Vec::new();
                    for field in included(variant.fields) {
                        let cs_ty = rust_type_to_cs(field.ty);
                        field_entries.push(format!("{} {}", cs_ty, to_pascal_case(field.name)));
                    }
                    cb.push(format!(
                        "public sealed record {}({}) : {};",
                        variant.name,
                        field_entries.join(", "),
                        meta.name
                    ));
                }
            }
            cb.build()
        }
    }
}

fn cs_type_def_named(type_name: &str, meta: &TagMeta) -> String {
    match meta.kind {
        TagKind::Struct(fields) => {
            let mut cb = CodeBuf::new();
            if !meta.desc.is_empty() {
                cb.push(format!("/// <summary>{}</summary>", meta.desc));
            }
            cb.push(format!("public class {}", type_name));
            cb.l("{");
            for field in included(fields) {
                let cs_ty = rust_type_to_cs(field.ty);
                let default = cs_default_value(&cs_ty);
                if !field.desc.is_empty() {
                    cb.push(format!("    /// <summary>{}</summary>", field.desc));
                }
                cb.push(format!(
                    "    public {} {} {{ get; set; }}{}",
                    cs_ty,
                    to_pascal_case(field.name),
                    default
                ));
            }
            cb.l("}");
            cb.build()
        }
        TagKind::Enum(variants) => {
            let mut cb = CodeBuf::new();
            if !meta.desc.is_empty() {
                cb.push(format!("/// <summary>{}</summary>", meta.desc));
            }
            cb.push(format!("public abstract record {};", type_name));
            cb.b();
            for variant in variants {
                if variant.fields.is_empty() {
                    cb.push(format!(
                        "public sealed record {} : {};",
                        variant.name, type_name
                    ));
                } else if variant.fields.len() == 1 && variant.fields[0].name.starts_with("__") {
                    let cs_ty = rust_type_to_cs(variant.fields[0].ty);
                    cb.push(format!(
                        "public sealed record {}({} __0) : {};",
                        variant.name, cs_ty, type_name
                    ));
                } else {
                    let mut field_entries = Vec::new();
                    for field in included(variant.fields) {
                        let cs_ty = rust_type_to_cs(field.ty);
                        field_entries.push(format!("{} {}", cs_ty, to_pascal_case(field.name)));
                    }
                    cb.push(format!(
                        "public sealed record {}({}) : {};",
                        variant.name,
                        field_entries.join(", "),
                        type_name
                    ));
                }
            }
            cb.build()
        }
    }
}

/// Returns a default value suffix for a C# type (for class property initializers).
fn cs_default_value(ty: &str) -> &'static str {
    match ty {
        "string" => " = \"\";",
        "byte[]" => " = Array.Empty<byte>();",
        _ if ty.starts_with("List<") => " = new();",
        _ => "",
    }
}

// ─── Type exports (recursive) ──────────────────────────────────────

fn handler_type_exports_cs(prefix: &str, meta: &HandlerMeta, emitted: &mut Vec<String>) -> String {
    let mut cb = CodeBuf::new();

    let mut data_idx = 0;
    for param in meta.params {
        match param.extractor {
            "Custom" => {
                // Custom types are emitted in common.cs
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
                        cb.push(cs_type_def_named(&type_name, structure));
                        emitted.push(type_name.clone());
                        emitted.push(structure.name.to_string());
                        extract_nested_types_cs(structure, &mut cb, emitted);
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
            cb.push(cs_type_def_named(&type_name, structure));
            emitted.push(type_name.clone());
            emitted.push(structure.name.to_string());
            extract_nested_types_cs(structure, &mut cb, emitted);
        }
    }

    cb.build()
}

fn extract_nested_types_cs(meta: &'static TagMeta, cb: &mut CodeBuf, emitted: &mut Vec<String>) {
    if !emitted.contains(&meta.name.to_string()) {
        emitted.push(meta.name.to_string());
        cb.push(cs_type_def(meta));
    }
    match meta.kind {
        TagKind::Struct(fields) => {
            for field in included(fields) {
                if let Some(structure_fn) = field.structure {
                    extract_nested_types_cs(structure_fn(), cb, emitted);
                }
            }
        }
        TagKind::Enum(variants) => {
            for variant in variants {
                for field in included(variant.fields) {
                    if let Some(structure_fn) = field.structure {
                        extract_nested_types_cs(structure_fn(), cb, emitted);
                    }
                }
            }
        }
    }
}

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

// ─── Response deserialization ──────────────────────────────────────

fn response_expr_cs(
    reader: &str,
    ty: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
    override_name: Option<&str>,
) -> String {
    let ty = normalize_rust_type(ty);
    match ty.as_str() {
        "i8" => format!("(sbyte){}.ReadI8()", reader),
        "i16" => format!("(short){}.ReadI16()", reader),
        "i32" => format!("{}.ReadI32()", reader),
        "i64" => format!("{}.ReadI64()", reader),
        "u8" => format!("(byte){}.ReadU8()", reader),
        "u16" => format!("(ushort){}.ReadU16()", reader),
        "u32" => format!("{}.ReadU32()", reader),
        "u64" => format!("{}.ReadU64()", reader),
        "usize" => format!("{}.ReadU64()", reader),
        "f32" => format!("{}.ReadF32()", reader),
        "f64" => format!("{}.ReadF64()", reader),
        "bool" => format!("{}.ReadBool()", reader),
        "String" | "&str" => format!("{}.ReadString()", reader),
        "Vec<u8>" => format!("{}.ReadBytesLen()", reader),
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            let inner_override = override_name.map(|n| {
                n.trim_end_matches('?')
                    .trim_start_matches("List<")
                    .trim_end_matches('>')
            });
            let elem = response_expr_cs(reader, inner, indent, structure, inner_override);
            let _inner_cs = rust_type_to_cs(inner);
            format!(
                "Enumerable.Range(0, {}.ReadU32()).Select(_ => {}).ToList()",
                reader, elem
            )
        }
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            let inner_override = override_name.map(|n| n.trim_end_matches('?'));
            let body = response_expr_cs(reader, inner, indent, structure, inner_override);
            format!("{}.ReadU8() == 1 ? {} : default", reader, body)
        }
        _ => {
            if let Some(s) = structure {
                let meta = s();
                let type_name = override_name.unwrap_or(meta.name);
                match meta.kind {
                    TagKind::Struct(fields) => {
                        let field_indent = format!("{}    ", indent);
                        let mut assigns = Vec::new();
                        for f in included(fields) {
                            let expr =
                                response_expr_cs(reader, f.ty, &field_indent, f.structure, None);
                            assigns.push(format!(
                                "{}{} = {}",
                                field_indent,
                                to_pascal_case(f.name),
                                expr
                            ));
                        }
                        format!(
                            "new {} {{\n{}\n{}}}",
                            type_name,
                            assigns.join(",\n"),
                            indent
                        )
                    }
                    TagKind::Enum(variants) => {
                        let tr = tag_read();
                        let arm_indent = format!("{}    ", indent);
                        let mut arms = Vec::new();
                        for (i, variant) in variants.iter().enumerate() {
                            if variant.fields.is_empty() {
                                arms.push(format!(
                                    "{}{} => new {}(),",
                                    arm_indent, i, variant.name
                                ));
                            } else if variant.fields.len() == 1
                                && variant.fields[0].name.starts_with("__")
                            {
                                let expr = response_expr_cs(
                                    reader,
                                    variant.fields[0].ty,
                                    &arm_indent,
                                    variant.fields[0].structure,
                                    None,
                                );
                                arms.push(format!(
                                    "{}{} => new {}({}),",
                                    arm_indent, i, variant.name, expr
                                ));
                            } else {
                                let field_indent = format!("{}    ", arm_indent);
                                let mut field_inits = Vec::new();
                                for field in included(variant.fields) {
                                    let expr = response_expr_cs(
                                        reader,
                                        field.ty,
                                        &field_indent,
                                        field.structure,
                                        None,
                                    );
                                    field_inits.push(format!(
                                        "{}{} = {}",
                                        field_indent,
                                        to_pascal_case(field.name),
                                        expr
                                    ));
                                }
                                arms.push(format!(
                                    "{}{} => new {} {{\n{}\n{}}},",
                                    arm_indent,
                                    i,
                                    variant.name,
                                    field_inits.join(",\n"),
                                    arm_indent
                                ));
                            }
                        }
                        arms.push(format!(
                            "{}_ => throw new InvalidOperationException(\"unknown enum tag\")",
                            arm_indent
                        ));
                        format!(
                            "{}.Read{}() switch {{\n{}\n{}}}",
                            reader,
                            tr,
                            arms.join("\n"),
                            indent
                        )
                    }
                }
            } else {
                format!("{}.ReadBytesLen()", reader)
            }
        }
    }
}

fn generate_return_cs(
    cb: &mut CodeBuf,
    reader: &str,
    ty: &str,
    resp_type: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
) {
    if ty == "()" {
        cb.push(format!("{}return;", indent));
        return;
    }
    let expr = response_expr_cs(reader, ty, indent, structure, Some(resp_type));
    cb.push(format!("{}return {};", indent, expr));
}

// ─── Request serialization ─────────────────────────────────────────

fn generate_request_serialize_cs(
    cb: &mut CodeBuf,
    var: &str,
    ty: &str,
    indent: &str,
    structure: Option<fn() -> &'static TagMeta>,
) {
    let ty = normalize_rust_type(ty);
    match ty.as_str() {
        "i8" => cb.push(format!("{}w.WriteI8({});", indent, var)),
        "i16" => cb.push(format!("{}w.WriteI16({});", indent, var)),
        "i32" => cb.push(format!("{}w.WriteI32({});", indent, var)),
        "i64" => cb.push(format!("{}w.WriteI64({});", indent, var)),
        "u8" => cb.push(format!("{}w.WriteU8({});", indent, var)),
        "u16" => cb.push(format!("{}w.WriteU16({});", indent, var)),
        "u32" => cb.push(format!("{}w.WriteU32({});", indent, var)),
        "u64" => cb.push(format!("{}w.WriteU64({});", indent, var)),
        "usize" => cb.push(format!("{}w.WriteU64((long){});", indent, var)),
        "f32" => cb.push(format!("{}w.WriteF32({});", indent, var)),
        "f64" => cb.push(format!("{}w.WriteF64({});", indent, var)),
        "bool" => cb.push(format!("{}w.WriteBool({});", indent, var)),
        "String" | "&str" => cb.push(format!("{}w.WriteString({});", indent, var)),
        s if s.starts_with("Vec<") => {
            let inner = &s[4..s.len() - 1];
            cb.push(format!("{}w.WriteU32({}.Count);", indent, var));
            cb.push(format!("{}foreach (var _e in {})", indent, var));
            cb.push(format!("{}{{", indent));
            generate_request_serialize_cs(cb, "_e", inner, &format!("{}    ", indent), structure);
            cb.push(format!("{}}}", indent));
        }
        s if s.starts_with("Option<") => {
            let inner = &s[7..s.len() - 1];
            cb.push(format!(
                "{}if ({} == default) {{ w.WriteU8(0); }} else {{ w.WriteU8(1);",
                indent, var
            ));
            generate_request_serialize_cs(cb, var, inner, &format!("{}    ", indent), structure);
            cb.push(format!("{}}}", indent));
        }
        _ => {
            if let Some(s) = structure {
                let meta = s();
                match meta.kind {
                    TagKind::Struct(fields) => {
                        for field in included(fields) {
                            let field_var = format!("{}.{}", var, to_pascal_case(field.name));
                            generate_request_serialize_cs(
                                cb,
                                &field_var,
                                field.ty,
                                indent,
                                field.structure,
                            );
                        }
                    }
                    TagKind::Enum(variants) => {
                        let tw = tag_write();
                        cb.push(format!("{}switch ({})", indent, var));
                        cb.push(format!("{}{{", indent));
                        for (i, variant) in variants.iter().enumerate() {
                            if variant.fields.is_empty() {
                                cb.push(format!(
                                    "{}    case {}: w.{}({}); break;",
                                    indent, variant.name, tw, i
                                ));
                            } else if variant.fields.len() == 1
                                && variant.fields[0].name.starts_with("__")
                            {
                                cb.push(format!(
                                    "{}    case {} v: w.{}({});",
                                    indent, variant.name, tw, i
                                ));
                                let field_var =
                                    format!("v.{}", to_pascal_case(variant.fields[0].name));
                                generate_request_serialize_cs(
                                    cb,
                                    &field_var,
                                    variant.fields[0].ty,
                                    &format!("{}        ", indent),
                                    variant.fields[0].structure,
                                );
                                cb.push(format!("{}        break;", indent));
                            } else {
                                cb.push(format!(
                                    "{}    case {} v: w.{}({});",
                                    indent, variant.name, tw, i
                                ));
                                for field in included(variant.fields) {
                                    let field_var = format!("v.{}", to_pascal_case(field.name));
                                    generate_request_serialize_cs(
                                        cb,
                                        &field_var,
                                        field.ty,
                                        &format!("{}        ", indent),
                                        field.structure,
                                    );
                                }
                                cb.push(format!("{}        break;", indent));
                            }
                        }
                        cb.push(format!("{}}}", indent));
                    }
                }
            } else {
                cb.push(format!("{}w.WriteBytes({}.__Raw);", indent, var));
            }
        }
    }
}

// ─── Validation code generation ────────────────────────────────────

fn generate_validation_cs(
    cb: &mut CodeBuf,
    var_prefix: &str,
    fields: &[crate::handler::FieldMeta],
    indent: &str,
) {
    for field in included(fields) {
        let field_ty = normalize_rust_type(field.ty);
        let is_option = field_ty.starts_with("Option<");
        let field_path = format!("{}.{}", var_prefix, to_pascal_case(field.name));

        if is_option && !field.validations.is_empty() {
            cb.push(format!("{}if ({} != default) {{", indent, field_path));
            let inner_indent = format!("{}    ", indent);
            emit_validation_checks_cs(
                cb,
                &field_path,
                field.name,
                field.validations,
                &inner_indent,
            );
            if let Some(structure_fn) = field.structure {
                let structure = structure_fn();
                match structure.kind {
                    crate::handler::TagKind::Struct(nested_fields) => {
                        generate_validation_cs(cb, &field_path, nested_fields, &inner_indent);
                    }
                    crate::handler::TagKind::Enum(variants) => {
                        generate_enum_validation_cs(
                            cb,
                            &field_path,
                            structure.name,
                            variants,
                            &inner_indent,
                        );
                    }
                }
            }
            cb.push(format!("{}}}", indent));
        } else {
            if !field.validations.is_empty() {
                emit_validation_checks_cs(cb, &field_path, field.name, field.validations, indent);
            }
            if let Some(structure_fn) = field.structure {
                let structure = structure_fn();
                let prefix = if is_option {
                    cb.push(format!("{}if ({} != default) {{", indent, field_path));
                    field_path.clone()
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
                        generate_validation_cs(cb, &prefix, nested_fields, &inner_indent);
                    }
                    crate::handler::TagKind::Enum(variants) => {
                        generate_enum_validation_cs(
                            cb,
                            &prefix,
                            structure.name,
                            variants,
                            &inner_indent,
                        );
                    }
                }
                if is_option {
                    cb.push(format!("{}}}", indent));
                }
            }
        }
    }
}

fn emit_validation_checks_cs(
    cb: &mut CodeBuf,
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
                cb.push(format!(
                    "{}if (!({} > {}L)) throw new AFastValidationError({}L, \"{}\", \"{}\");",
                    indent, field_path, value, code, field_name, message
                ));
            }
            crate::handler::ValidateRule::Gte {
                value,
                code,
                message,
            } => {
                cb.push(format!(
                    "{}if (!({} >= {}L)) throw new AFastValidationError({}L, \"{}\", \"{}\");",
                    indent, field_path, value, code, field_name, message
                ));
            }
            crate::handler::ValidateRule::Lt {
                value,
                code,
                message,
            } => {
                cb.push(format!(
                    "{}if (!({} < {}L)) throw new AFastValidationError({}L, \"{}\", \"{}\");",
                    indent, field_path, value, code, field_name, message
                ));
            }
            crate::handler::ValidateRule::Lte {
                value,
                code,
                message,
            } => {
                cb.push(format!(
                    "{}if (!({} <= {}L)) throw new AFastValidationError({}L, \"{}\", \"{}\");",
                    indent, field_path, value, code, field_name, message
                ));
            }
            crate::handler::ValidateRule::Len {
                min,
                max,
                code,
                message,
            } => {
                if *min >= 0 && *max >= 0 {
                    cb.push(format!(
                        "{}if ({}.Length < {} || {}.Length > {}) throw new AFastValidationError({}L, \"{}\", \"{}\");",
                        indent, field_path, min, field_path, max, code, field_name, message
                    ));
                } else if *min >= 0 {
                    cb.push(format!(
                        "{}if ({}.Length < {}) throw new AFastValidationError({}L, \"{}\", \"{}\");",
                        indent, field_path, min, code, field_name, message
                    ));
                } else if *max >= 0 {
                    cb.push(format!(
                        "{}if ({}.Length > {}) throw new AFastValidationError({}L, \"{}\", \"{}\");",
                        indent, field_path, max, code, field_name, message
                    ));
                }
            }
            crate::handler::ValidateRule::Of {
                values,
                code,
                message,
            } => {
                let list: Vec<String> = values.iter().map(|v| format!("\"{}\"", v)).collect();
                cb.push(format!(
                    "{}if (!new[] {{ {} }}.Contains({})) throw new AFastValidationError({}L, \"{}\", \"{}\");",
                    indent, list.join(", "), field_path, code, field_name, message
                ));
            }
        }
    }
}

fn generate_enum_validation_cs(
    cb: &mut CodeBuf,
    var_prefix: &str,
    _enum_name: &str,
    variants: &[crate::handler::EnumVariantMeta],
    indent: &str,
) {
    for variant in variants {
        if variant.fields.is_empty() {
            continue;
        }
        if variant.fields.len() == 1 && variant.fields[0].name.starts_with("__") {
            let inner = &variant.fields[0];
            let inner_path = format!("{}.{}", var_prefix, to_pascal_case(inner.name));
            if !inner.validations.is_empty() {
                cb.push(format!(
                    "{}if ({} is {} v__) {{",
                    indent, var_prefix, variant.name
                ));
                let inner_indent = format!("{}    ", indent);
                emit_validation_checks_cs(
                    cb,
                    &inner_path,
                    inner.name,
                    inner.validations,
                    &inner_indent,
                );
                cb.push(format!("{}}}", indent));
            }
        } else {
            let has_work = variant
                .fields
                .iter()
                .any(|f| !f.validations.is_empty() || f.structure.is_some());
            if !has_work {
                continue;
            }
            cb.push(format!(
                "{}if ({} is {} v__) {{",
                indent, var_prefix, variant.name
            ));
            let inner_indent = format!("{}    ", indent);
            generate_validation_cs(cb, var_prefix, variant.fields, &inner_indent);
            cb.push(format!("{}}}", indent));
        }
    }
}

// ─── Handler method generation ─────────────────────────────────────

fn handler_method_cs(
    handler: &Handler,
    all_handlers: &[Handler],
    prefix: &str,
    base_indent: &str,
    _svc_name: &str,
    _class_name: &str,
    cache_key: &str,
) -> String {
    let meta = handler.meta;
    let func_name = if !meta.api_name.is_empty() {
        to_pascal_case(meta.api_name)
    } else {
        to_pascal_case(meta.name)
    };
    let cache_seconds = meta.cache_seconds;
    let id = handler.stable_id;
    let indent = base_indent;
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

    // Build function parameter list
    let mut fn_params = Vec::new();
    for &(ref var, param) in &data_params {
        if let Some(structure_fn) = param.structure {
            let structure = structure_fn();
            let cs_ty = prefixed_type(prefix, structure.name);
            fn_params.push(format!("{} {}", cs_ty, var));
        }
    }

    if meta.long_connection {
        fn_params.push("Action<byte[], Action<byte[], Action<object>>> callback".to_string());
    }

    if cache_seconds > 0 {
        fn_params.push("bool force = false".to_string());
    }

    let params_str = fn_params.join(", ");

    let is_option_return =
        meta.return_type.starts_with("Option<") || meta.return_type.starts_with("Option <");
    let return_type = if meta.long_connection {
        "AfastSocket".to_string()
    } else if let Some(structure_fn) = meta.return_structure {
        let structure = structure_fn();
        let base = prefixed_type(prefix, structure.name);
        if is_option_return {
            format!("{}?", base)
        } else {
            base
        }
    } else {
        "void".to_string()
    };

    let _async_prefix = "async Task<{}>".replace("{}", &return_type);
    let void_async = return_type == "void";

    let mut body = CodeBuf::new();

    // Cache check
    if cache_seconds > 0 {
        let cache_key_parts: Vec<String> = data_params.iter().map(|(v, _)| v.clone()).collect();
        let req_expr = if cache_key_parts.is_empty() {
            "\"[]\"".to_string()
        } else if cache_key_parts.len() == 1 {
            format!("{}.ToString()", cache_key_parts[0])
        } else {
            format!("string.Join(\",\", {})", cache_key_parts.join(", "))
        };
        body.push(format!("{}var cacheKey = \"{}\";", ind, cache_key));
        body.push(format!("{}var currentReq = {};", ind, req_expr));
        body.push(format!("{}if (!force)", ind));
        body.push(format!("{}{{", ind));
        body.push(format!("{}    if (_cache.TryGetValue(cacheKey, out var cached) && currentReq == cached.Request && DateTime.UtcNow.Ticks < cached.ExpiresAt)", ind));
        body.push(format!("{}    {{", ind));
        if void_async {
            body.push(format!("{}        return;", ind));
        } else {
            body.push(format!(
                "{}        return ({})cached.Value!;",
                ind, return_type
            ));
        }
        body.push(format!("{}    }}", ind));
        body.push(format!("{}}}", ind));
    }

    body.push(format!("{}var w = new BinaryWriter();", ind));

    // Execute Custom provider functions
    for &(_ci, ty, _structure) in &custom_indices {
        let var = format!("_c{}", to_pascal_case(ty));
        body.push(format!(
            "{}var {} = ({})(await Customs[\"{}\"]());",
            ind, ty, var, ty
        ));
    }

    // Validate Custom params
    for &(_ci, ty, structure) in &custom_indices {
        let var = format!("_c{}", to_pascal_case(ty));
        if let Some(structure_fn) = structure {
            let meta = structure_fn();
            if let TagKind::Struct(fields) = meta.kind {
                generate_validation_cs(&mut body, &var, fields, &ind);
            }
        }
    }

    // Validate Data params
    for &(ref var, param) in &data_params {
        if let Some(structure_fn) = param.structure {
            let structure = structure_fn();
            if let TagKind::Struct(fields) = structure.kind {
                generate_validation_cs(&mut body, var, fields, &ind);
            }
        }
    }

    // Serialize Custom fields
    for &(_ci, ty, structure) in &custom_indices {
        let var = format!("_c{}", to_pascal_case(ty));
        if let Some(structure_fn) = structure {
            let meta = structure_fn();
            if let TagKind::Struct(fields) = meta.kind {
                for field in included(fields) {
                    let field_var = format!("{}.{}", var, to_pascal_case(field.name));
                    generate_request_serialize_cs(
                        &mut body,
                        &field_var,
                        field.ty,
                        &ind,
                        field.structure,
                    );
                }
            }
        }
    }

    // Serialize Data fields
    for &(ref var, param) in &data_params {
        generate_request_serialize_cs(&mut body, var, param.ty, &ind, param.structure);
    }

    body.push(format!("{}var data = w.ToBytes();", ind));
    body.push(format!(
        "{}var resp = await _client.CallAsync({}, data);",
        ind, id
    ));

    if meta.long_connection {
        body.push(format!("{}var r = new BinaryReader(resp);", ind));
        body.push(format!("{}var connId = r.ReadU32();", ind));
        body.push(format!(
            "{}var socket = new AfastSocket(connId, d => _client._SendRaw(d), callback);",
            ind
        ));
        body.push(format!(
            "{}_client._pushHandlers[connId] = raw => socket.OnMessage(raw);",
            ind
        ));
        body.push(format!("{}return socket;", ind));
    } else {
        body.push(format!("{}var r = new BinaryReader(resp);", ind));
        if cache_seconds > 0 {
            if return_type == "void" {
                body.push(format!(
                    "{}{}._cache[cacheKey] = (currentReq, DateTime.UtcNow.Ticks + {}L, null);",
                    ind,
                    _class_name,
                    cache_seconds * 10_000_000
                ));
                body.push(format!("{}return;", ind));
            } else {
                let expr = response_expr_cs(
                    "r",
                    meta.return_type,
                    &ind,
                    meta.return_structure,
                    Some(&return_type),
                );
                body.push(format!("{}var result = {};", ind, expr));
                body.push(format!(
                    "{}{}._cache[cacheKey] = (currentReq, DateTime.UtcNow.Ticks + {}L, result);",
                    ind,
                    _class_name,
                    cache_seconds * 10_000_000
                ));
                body.push(format!("{}return result;", ind));
            }
        } else {
            generate_return_cs(
                &mut body,
                "r",
                meta.return_type,
                &return_type,
                &ind,
                meta.return_structure,
            );
        }
    }

    let body_str = body.build();
    // Build XML doc comment
    let mut doc = CodeBuf::new();
    if !meta.desc.is_empty() {
        doc.push(format!("{}/// <summary>", indent));
        doc.push(format!("{}/// {}", indent, meta.desc));
        doc.push(format!("{}/// </summary>", indent));
    }
    for &(ref var, param) in &data_params {
        if let Some(structure_fn) = param.structure {
            let structure = structure_fn();
            if !structure.desc.is_empty() {
                doc.push(format!(
                    "{}/// <param name=\"{}\">{}</param>",
                    indent, var, structure.desc
                ));
            }
        }
    }
    if meta.return_type != "()"
        && let Some(structure_fn) = meta.return_structure
    {
        let structure = structure_fn();
        if !structure.desc.is_empty() {
            doc.push(format!(
                "{}/// <returns>{}</returns>",
                indent, structure.desc
            ));
        }
    }
    let doc_str = doc.build();
    let doc_prefix = if doc_str.is_empty() {
        String::new()
    } else {
        format!("{}\n", doc_str)
    };
    if void_async {
        format!(
            "{doc_prefix}{indent}public async Task {func_name}({params_str})\n{indent}{{\n{body_str}\n{indent}}}",
            doc_prefix = doc_prefix,
            indent = indent,
            func_name = func_name,
            params_str = params_str,
            body_str = body_str
        )
    } else {
        format!(
            "{doc_prefix}{indent}public async Task<{return_type}> {func_name}({params_str})\n{indent}{{\n{body_str}\n{indent}}}",
            doc_prefix = doc_prefix,
            indent = indent,
            return_type = return_type,
            func_name = func_name,
            params_str = params_str,
            body_str = body_str
        )
    }
}

// ─── Ordinary HTTP handler method (CS) ─────────────────────────────

#[cfg(feature = "ordinary-http")]
fn ordinary_handler_method_cs(
    handler: &Handler,
    prefix: &str,
    group_path: &[&str],
    base_indent: &str,
    _svc_name: &str,
    debug: bool,
    _class_name: &str,
    cache_key: &str,
) -> String {
    let meta = handler.meta;
    let func_name = if !meta.api_name.is_empty() {
        to_pascal_case(meta.api_name)
    } else {
        to_pascal_case(meta.name)
    };
    let cache_seconds = meta.cache_seconds;
    let method = if meta.method.is_empty() {
        "GET"
    } else {
        meta.method
    };
    let indent = base_indent;
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

    let cs_type_of = |p: &ParamMeta, suffix: &str| -> String {
        if p.structure.is_some() {
            format!("{}{}", prefix, suffix)
        } else {
            rust_type_to_cs(p.ty)
        }
    };

    let mut fn_params: Vec<String> = Vec::new();
    if let Some(p) = param_param {
        fn_params.push(format!("{} @params", cs_type_of(p, "PathParams")));
    }
    if let Some(p) = query_param {
        fn_params.push(format!("{} query", cs_type_of(p, "Query")));
    }
    if let Some(p) = body_param {
        fn_params.push(format!("{} body", cs_type_of(p, "Body")));
    }
    if cache_seconds > 0 {
        fn_params.push("bool force = false".to_string());
    }
    let params_str = fn_params.join(", ");

    let cs_return = if meta.return_structure.is_some() {
        format!("{}Response", prefix)
    } else if let Some(inner) = unwrap_json_type(meta.return_type) {
        rust_type_to_cs(&inner)
    } else {
        let normalized = normalize_rust_type(meta.return_type);
        match normalized.as_str() {
            "()" => "void".to_string(),
            "Text" | "Html" => "string".to_string(),
            "File" => "byte[]".to_string(),
            "Status" | "Redirect" => "void".to_string(),
            _ if !normalized.is_empty() => rust_type_to_cs(&normalized),
            _ => "void".to_string(),
        }
    };

    let mut body = CodeBuf::new();
    let is_void = cs_return == "void";

    // Cache check
    if cache_seconds > 0 {
        let mut req_var_names: Vec<String> = Vec::new();
        if param_param.is_some() {
            req_var_names.push("params".to_string());
        }
        if query_param.is_some() {
            req_var_names.push("query".to_string());
        }
        if body_param.is_some() {
            req_var_names.push("body".to_string());
        }
        let req_expr = if req_var_names.is_empty() {
            "\"[]\"".to_string()
        } else if req_var_names.len() == 1 {
            format!("{}.ToString()", req_var_names[0])
        } else {
            format!("string.Join(\",\", {})", req_var_names.join(", "))
        };
        body.push(format!("{}var cacheKey = \"{}\";", ind, cache_key));
        body.push(format!("{}var currentReq = {};", ind, req_expr));
        body.push(format!("{}if (!force)", ind));
        body.push(format!("{}{{", ind));
        body.push(format!("{}    if (_cache.TryGetValue(cacheKey, out var cached) && currentReq == cached.Request && DateTime.UtcNow.Ticks < cached.ExpiresAt)", ind));
        body.push(format!("{}    {{", ind));
        if is_void {
            body.push(format!("{}        return;", ind));
        } else {
            body.push(format!(
                "{}        return ({})cached.Value!;",
                ind, cs_return
            ));
        }
        body.push(format!("{}    }}", ind));
        body.push(format!("{}}}", ind));
    }

    // Build URL
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

    body.push(format!(
        "{}var url = _client._baseUrl + \"{}\";",
        ind, full_path
    ));

    // Path param substitution
    if let Some(p) = param_param
        && let Some(s) = p.structure
    {
        let structure = s();
        if let TagKind::Struct(fields) = structure.kind {
            for field in included(fields) {
                let placeholder = format!(":{}", field.name);
                body.push(format!(
                    "{}url = url.Replace(\"{}\", Uri.EscapeDataString(@params.{}.ToString()));",
                    ind,
                    placeholder,
                    to_pascal_case(field.name)
                ));
            }
        }
    }

    // Query string
    if query_param.is_some() {
        body.push(format!("{}{{", ind));
        body.push(format!("{}    var qs = new List<string>();", ind));
        if let Some(p) = query_param
            && let Some(s) = p.structure
        {
            let structure = s();
            if let TagKind::Struct(fields) = structure.kind {
                for field in included(fields) {
                    body.push(format!(
                        "{}    if (query.{} != default) qs.Add(Uri.EscapeDataString(\"{}=\") + Uri.EscapeDataString(query.{}.ToString()!));",
                        ind, to_pascal_case(field.name), field.name, to_pascal_case(field.name)
                    ));
                }
            }
        }
        body.push(format!(
            "{}    if (qs.Count > 0) url += \"?\" + string.Join(\"&\", qs);",
            ind
        ));
        body.push(format!("{}}}", ind));
    }

    // HTTP request
    body.push(format!(
        "{}var request = new HttpRequestMessage(new HttpMethod(\"{}\"), url);",
        ind, method
    ));
    body.push(format!(
        "{}request.Headers.Add(\"Accept\", \"application/json\");",
        ind
    ));

    if body_param.is_some() {
        body.push(format!("{}request.Content = new StringContent(JsonSerializer.Serialize(body), Encoding.UTF8, \"application/json\");", ind));
    }

    if debug {
        body.push(format!(
            "{}if (_client._debug) Console.WriteLine(\"[afast:debug] → {} {{url}}\");",
            ind, method
        ));
    }

    body.push(format!(
        "{}var response = await _client._httpClient.SendAsync(request);",
        ind
    ));
    body.push(format!("{}response.EnsureSuccessStatusCode();", ind));
    body.push(format!(
        "{}var respBytes = await response.Content.ReadAsByteArrayAsync();",
        ind
    ));

    if debug {
        body.push(format!("{}if (_client._debug) Console.WriteLine(\"[afast:debug] ← {} {{url}} status={{(int)response.StatusCode}}\");", ind, method));
    }

    if is_void {
        if cache_seconds > 0 {
            body.push(format!(
                "{}{}._cache[cacheKey] = (currentReq, DateTime.UtcNow.Ticks + {}L, null);",
                ind,
                _class_name,
                cache_seconds * 10_000_000
            ));
        }
        body.push(format!("{}return;", ind));
    } else {
        body.push(format!(
            "{}var result = JsonSerializer.Deserialize<{}>(respBytes)!;",
            ind, cs_return
        ));
        if cache_seconds > 0 {
            body.push(format!(
                "{}{}._cache[cacheKey] = (currentReq, DateTime.UtcNow.Ticks + {}L, result);",
                ind,
                _class_name,
                cache_seconds * 10_000_000
            ));
        }
        body.push(format!("{}return result;", ind));
    }

    let body_str = body.build();
    let async_ret = if is_void {
        "async Task"
    } else {
        &format!("async Task<{}>", cs_return)
    };
    // Build XML doc comment
    let mut doc = CodeBuf::new();
    if !meta.desc.is_empty() {
        doc.push(format!("{}/// <summary>", indent));
        doc.push(format!("{}/// {}", indent, meta.desc));
        doc.push(format!("{}/// </summary>", indent));
    }
    if let Some(p) = param_param
        && let Some(s) = p.structure
    {
        let structure = s();
        if !structure.desc.is_empty() {
            doc.push(format!(
                "{}/// <param name=\"params\">{}</param>",
                indent, structure.desc
            ));
        }
    }
    if let Some(p) = query_param
        && let Some(s) = p.structure
    {
        let structure = s();
        if !structure.desc.is_empty() {
            doc.push(format!(
                "{}/// <param name=\"query\">{}</param>",
                indent, structure.desc
            ));
        }
    }
    if let Some(p) = body_param
        && let Some(s) = p.structure
    {
        let structure = s();
        if !structure.desc.is_empty() {
            doc.push(format!(
                "{}/// <param name=\"body\">{}</param>",
                indent, structure.desc
            ));
        }
    }
    if meta.return_type != "()"
        && let Some(structure_fn) = meta.return_structure
    {
        let structure = structure_fn();
        if !structure.desc.is_empty() {
            doc.push(format!(
                "{}/// <returns>{}</returns>",
                indent, structure.desc
            ));
        }
    }
    let doc_str = doc.build();
    let doc_prefix = if doc_str.is_empty() {
        String::new()
    } else {
        format!("{}\n", doc_str)
    };
    format!(
        "{doc_prefix}{indent}public {async_ret} {func_name}({params_str})\n{indent}{{\n{body_str}\n{indent}}}",
        doc_prefix = doc_prefix,
        indent = indent,
        async_ret = async_ret,
        func_name = func_name,
        params_str = params_str,
        body_str = body_str
    )
}

// ─── Service-level code generation ─────────────────────────────────

pub(crate) fn generate_service_cs(
    svc: &Service,
    calls: &[crate::NetCallType],
    debug: bool,
) -> String {
    let has_http = calls.contains(&crate::NetCallType::Http);
    let has_ws = calls.contains(&crate::NetCallType::Ws);
    let has_tcp = calls.contains(&crate::NetCallType::Tcp);

    let mut cb = CodeBuf::new();
    cb.l("// Auto-generated by afast. DO NOT EDIT.");
    cb.l("#pragma warning disable CS8618, CS8603, CS8604, CS0649");
    cb.l("using System;");
    cb.l("using System.Collections.Generic;");
    cb.l("using System.IO;");
    cb.l("using System.Linq;");
    // HttpClient is needed for binary HTTP, ordinary HTTP routes, and SSE routes
    cb.l("using System.Net.Http;");
    if has_tcp {
        cb.l("using System.Net.Sockets;");
    }
    if has_ws {
        cb.l("using System.Net.WebSockets;");
    }
    cb.l("using System.Text;");
    cb.l("using System.Text.Json;");
    cb.l("using System.Threading;");
    cb.l("using System.Threading.Tasks;");
    cb.b();
    cb.push(format!(
        "namespace Afast.Generated.{};",
        to_pascal_case(&svc.name)
    ));
    cb.b();

    // Type exports
    let mut emitted = Vec::new();
    collect_type_exports_cs(&svc.handlers, &[], &mut emitted, &mut cb);
    cb.b();

    let _customs = collect_customs(&svc.handlers);
    let class_name = to_pascal_case(&svc.name);

    #[cfg(feature = "ordinary-http")]
    let has_cache = svc
        .handlers
        .iter()
        .any(|_h| has_cache_handlers_cs(&svc.handlers))
        || svc
            .ordinary_routes
            .iter()
            .any(|r| r.handler_entry.meta.cache_seconds > 0);
    #[cfg(not(feature = "ordinary-http"))]
    let has_cache = has_cache_handlers_cs(&svc.handlers);

    // Client class
    cb.push(format!(
        "public class {}Client : IAsyncDisposable",
        class_name
    ));
    cb.l("{");

    // Transport enum — only include requested transports
    {
        let mut variants = Vec::new();
        if has_http {
            variants.push("Http");
        }
        if has_ws {
            variants.push("Ws");
        }
        if has_tcp {
            variants.push("Tcp");
        }
        if variants.is_empty() {
            variants.push("Http");
        } // fallback
        cb.push(format!(
            "    public enum Transport {{ {} }}",
            variants.join(", ")
        ));
    }
    cb.b();

    // Fields
    cb.l("    private readonly string _host;");
    cb.l("    private readonly int _port;");
    cb.l("    private readonly bool _tls;");
    cb.l("    private readonly Transport _transport;");
    if debug {
        cb.l("    private readonly bool _debug = true;");
    }
    cb.l("    private readonly HttpClient _httpClient;");
    if has_ws || has_tcp {
        cb.l(
            "    private readonly Dictionary<int, TaskCompletionSource<byte[]>> _pending = new();",
        );
        cb.l("    private readonly Dictionary<int, Action<byte[]>> _pushHandlers = new();");
        cb.l("    private int _nextId = 1;");
        cb.l("    private readonly object _lock = new();");
    }
    if has_tcp {
        cb.l("    private TcpClient? _tcpClient;");
        cb.l("    private NetworkStream? _tcpStream;");
        cb.l("    private BinaryReader? _tcpReader;");
    }
    if has_ws {
        cb.l("    private ClientWebSocket? _ws;");
    }
    if has_cache {
        cb.l("    private static readonly Dictionary<string, (string Request, long ExpiresAt, object? Value)> _cache = new();");
    }
    cb.l("    private readonly string _baseUrl;");
    cb.l("    public Dictionary<string, Func<Task<object>>> Customs { get; } = new();");
    cb.b();

    // Constructor — default transport is the first one available
    let default_transport = if has_http {
        "Http"
    } else if has_ws {
        "Ws"
    } else {
        "Tcp"
    };
    cb.push(format!("    public {}Client(string host, int port, bool tls = false, Transport transport = Transport.{})", class_name, default_transport));
    cb.l("    {");
    cb.l("        _host = host;");
    cb.l("        _port = port;");
    cb.l("        _tls = tls;");
    cb.l("        _transport = transport;");
    cb.l("        _httpClient = new HttpClient();");
    cb.l("        _baseUrl = $\"{(tls ? \"https\" : \"http\")}://{host}:{port}\";");
    if has_tcp {
        cb.l("        if (transport == Transport.Tcp) InitTcp();");
    }
    if has_ws {
        cb.l("        if (transport == Transport.Ws) InitWs().Wait();");
    }
    cb.push(format!("        Apis = new {}Apis(this);", class_name));
    cb.l("    }");
    cb.b();

    // TCP init
    if has_tcp {
        cb.l("    private void InitTcp()");
        cb.l("    {");
        cb.l("        _tcpClient = new TcpClient(_host, _port);");
        cb.l("        _tcpStream = _tcpClient.GetStream();");
        cb.l("        _ = Task.Run(TcpReaderLoop);");
        cb.l("    }");
        cb.b();
        cb.l("    private async Task TcpReaderLoop()");
        cb.l("    {");
        cb.l("        try");
        cb.l("        {");
        cb.l("            while (true)");
        cb.l("            {");
        if cfg!(feature = "len64") {
            cb.l("                var lenBuf = new byte[8];");
            cb.l("                await _tcpStream!.ReadExactlyAsync(lenBuf);");
            cb.l("                var frameLen = (int)BitConverter.ToInt64(lenBuf);");
        } else {
            cb.l("                var lenBuf = new byte[4];");
            cb.l("                await _tcpStream!.ReadExactlyAsync(lenBuf);");
            cb.l("                var frameLen = BitConverter.ToInt32(lenBuf);");
        }
        cb.l("                if (frameLen == 0) continue;");
        cb.l("                var frame = new byte[frameLen];");
        cb.l("                await _tcpStream.ReadExactlyAsync(frame);");
        cb.l("                HandleMessage(frame);");
        cb.l("            }");
        cb.l("        }");
        cb.l("        catch { /* connection closed */ }");
        cb.l("    }");
        cb.b();
    }

    // WS init
    if has_ws {
        cb.l("    private async Task InitWs()");
        cb.l("    {");
        cb.l("        _ws = new ClientWebSocket();");
        cb.l("        var wsUrl = $\"{(_tls ? \"wss\" : \"ws\")}://{_host}:{_port}/_ws\";");
        cb.l("        await _ws.ConnectAsync(new Uri(wsUrl), CancellationToken.None);");
        cb.l("        _ = Task.Run(WsReaderLoop);");
        cb.l("    }");
        cb.b();
        cb.l("    private async Task WsReaderLoop()");
        cb.l("    {");
        cb.l("        var buf = new byte[65536];");
        cb.l("        try");
        cb.l("        {");
        cb.l("            while (_ws!.State == WebSocketState.Open)");
        cb.l("            {");
        cb.l("                var ms = new MemoryStream();");
        cb.l("                WebSocketReceiveResult result;");
        cb.l("                do");
        cb.l("                {");
        cb.l("                    result = await _ws.ReceiveAsync(new ArraySegment<byte>(buf), CancellationToken.None);");
        cb.l("                    ms.Write(buf, 0, result.Count);");
        cb.l("                } while (!result.EndOfMessage);");
        cb.l("                if (result.MessageType == WebSocketMessageType.Binary)");
        cb.l("                    HandleMessage(ms.ToArray());");
        cb.l("            }");
        cb.l("        }");
        cb.l("        catch { /* connection closed */ }");
        cb.l("    }");
        cb.b();
    }

    // HandleMessage
    if has_ws || has_tcp {
        let seq64 = cfg!(feature = "seq64");
        let sid: usize = if seq64 { 8 } else { 4 };
        let len_bytes: usize = if cfg!(feature = "len64") { 8 } else { 4 };
        let status_off = sid + len_bytes;
        let code_off = status_off + 1;
        let payload_off = code_off + 8;

        cb.l("    private void HandleMessage(byte[] raw)");
        cb.l("    {");
        if seq64 {
            cb.l("        if (raw.Length < 8) return;");
            cb.l("        long reqId = BitConverter.ToInt64(raw, 0);");
        } else {
            cb.l("        if (raw.Length < 4) return;");
            cb.l("        long reqId = BitConverter.ToUInt32(raw, 0);");
        }
        cb.l("        if (reqId == 0)");
        cb.l("        {");
        let cid = sid;
        let lid = sid + 4;
        let push_hdr = sid + 4 + len_bytes;
        cb.push(format!(
            "            int connId = BitConverter.ToInt32(raw, {});",
            cid
        ));
        cb.push(format!(
            "            int len = (int)BitConverter.ToUInt32(raw, {});",
            lid
        ));
        cb.l("            var payload = new byte[len];");
        cb.push(format!(
            "            Array.Copy(raw, {}, payload, 0, len);",
            push_hdr
        ));
        cb.l("            lock (_lock) { if (_pushHandlers.TryGetValue(connId, out var h)) h(payload); }");
        cb.l("            return;");
        cb.l("        }");
        cb.l("        TaskCompletionSource<byte[]> tcs;");
        cb.l("        lock (_lock) { if (!_pending.Remove((int)reqId, out tcs!)) return; }");
        cb.push(format!("        if (raw.Length < {}) {{ tcs.SetException(new InvalidOperationException(\"response too short\")); return; }}", payload_off));
        cb.push(format!("        int status = raw[{}];", status_off));
        cb.l("        if (status == 1)");
        cb.l("        {");
        cb.push(format!(
            "            long code = BitConverter.ToInt64(raw, {});",
            code_off
        ));
        cb.push(format!(
            "            string msg = Encoding.UTF8.GetString(raw, {}, raw.Length - {});",
            payload_off, payload_off
        ));
        cb.l("            tcs.SetException(new InvalidOperationException($\"AfError({code}): {msg}\"));");
        cb.l("        }");
        cb.l("        else");
        cb.l("        {");
        cb.push(format!(
            "            var data = new byte[raw.Length - {}];",
            payload_off
        ));
        cb.push(format!(
            "            Array.Copy(raw, {}, data, 0, data.Length);",
            payload_off
        ));
        cb.l("            tcs.SetResult(data);");
        cb.l("        }");
        cb.l("    }");
        cb.b();
    }

    // CallAsync
    cb.l("    private async Task<byte[]> CallAsync(long handlerId, byte[] payload)");
    cb.l("    {");
    cb.l("        return _transport switch");
    cb.l("        {");
    if has_http {
        cb.l("            Transport.Http => await CallFetchAsync(handlerId, payload),");
    }
    if has_ws {
        cb.l("            Transport.Ws => await CallWsAsync(handlerId, payload),");
    }
    if has_tcp {
        cb.l("            Transport.Tcp => await CallTcpAsync(handlerId, payload),");
    }
    cb.l("            _ => throw new InvalidOperationException($\"transport not enabled: {_transport}\")");
    cb.l("        };");
    cb.l("    }");
    cb.b();

    // HTTP call
    if has_http {
        cb.l("    private async Task<byte[]> CallFetchAsync(long handlerId, byte[] payload)");
        cb.l("    {");
        cb.l("        var w = new BinaryWriter();");
        cb.l("        w.WriteU32((int)handlerId);");
        cb.l("        w.WriteRaw(payload);");
        cb.l("        var body = w.ToBytes();");
        cb.l(
            "        var request = new HttpRequestMessage(HttpMethod.Post, $\"{_baseUrl}/_api\");",
        );
        cb.l("        request.Content = new ByteArrayContent(body);");
        cb.l("        request.Content.Headers.ContentType = new System.Net.Http.Headers.MediaTypeHeaderValue(\"application/octet-stream\");");
        cb.l("        var response = await _httpClient.SendAsync(request);");
        cb.l("        response.EnsureSuccessStatusCode();");
        cb.l("        var respBytes = await response.Content.ReadAsByteArrayAsync();");
        cb.l("        if (respBytes.Length < 9) throw new InvalidOperationException(\"response too short\");");
        cb.l("        int status = respBytes[0];");
        cb.l("        if (status == 1)");
        cb.l("        {");
        cb.l("            long code = BitConverter.ToInt64(respBytes, 1);");
        cb.l(
            "            string msg = Encoding.UTF8.GetString(respBytes, 9, respBytes.Length - 9);",
        );
        cb.l("            throw new InvalidOperationException($\"AfError({code}): {msg}\");");
        cb.l("        }");
        cb.l("        var result = new byte[respBytes.Length - 9];");
        cb.l("        Array.Copy(respBytes, 9, result, 0, result.Length);");
        cb.l("        return result;");
        cb.l("    }");
        cb.b();
    }

    // WS call
    if has_ws {
        cb.l("    private async Task<byte[]> CallWsAsync(long handlerId, byte[] payload)");
        cb.l("    {");
        cb.l("        int id;");
        cb.l("        var tcs = new TaskCompletionSource<byte[]>();");
        cb.l("        lock (_lock) { id = _nextId++; _pending[id] = tcs; }");
        cb.l("        _ = Task.Run(async () => { await Task.Delay(5000); lock (_lock) { if (_pending.Remove(id)) tcs.SetException(new TimeoutException($\"timeout id={id}\")); } });");
        cb.l("        var w = new BinaryWriter();");
        cb.l("        w.WriteU32(id);");
        cb.l("        w.WriteU32((int)handlerId);");
        if cfg!(feature = "len64") {
            cb.l("        w.WriteU64(payload.Length);");
        } else {
            cb.l("        w.WriteU32(payload.Length);");
        }
        cb.l("        w.WriteRaw(payload);");
        cb.l("        var data = w.ToBytes();");
        cb.l("        await _ws!.SendAsync(new ArraySegment<byte>(data), WebSocketMessageType.Binary, true, CancellationToken.None);");
        cb.l("        return await tcs.Task;");
        cb.l("    }");
        cb.b();
    }

    // TCP call
    if has_tcp {
        cb.l("    private async Task<byte[]> CallTcpAsync(long handlerId, byte[] payload)");
        cb.l("    {");
        cb.l("        int id;");
        cb.l("        var tcs = new TaskCompletionSource<byte[]>();");
        cb.l("        lock (_lock) { id = _nextId++; _pending[id] = tcs; }");
        cb.l("        _ = Task.Run(async () => { await Task.Delay(5000); lock (_lock) { if (_pending.Remove(id)) tcs.SetException(new TimeoutException($\"timeout id={id}\")); } });");
        cb.l("        var inner = new BinaryWriter();");
        cb.l("        inner.WriteU32(id);");
        cb.l("        inner.WriteU32((int)handlerId);");
        if cfg!(feature = "len64") {
            cb.l("        inner.WriteU64(payload.Length);");
        } else {
            cb.l("        inner.WriteU32(payload.Length);");
        }
        cb.l("        inner.WriteRaw(payload);");
        cb.l("        var innerBytes = inner.ToBytes();");
        cb.l("        var envelope = new BinaryWriter();");
        if cfg!(feature = "len64") {
            cb.l("        envelope.WriteU64(innerBytes.Length);");
        } else {
            cb.l("        envelope.WriteU32(innerBytes.Length);");
        }
        cb.l("        envelope.WriteRaw(innerBytes);");
        cb.l("        var data = envelope.ToBytes();");
        cb.l("        await _tcpStream!.WriteAsync(data);");
        cb.l("        await _tcpStream.FlushAsync();");
        cb.l("        return await tcs.Task;");
        cb.l("    }");
        cb.b();
    }

    // _SendRaw
    if has_ws || has_tcp {
        cb.l("    private void _SendRaw(byte[] data)");
        cb.l("    {");
        if has_ws && has_tcp {
            cb.l("        if (_transport == Transport.Ws)");
            cb.l("        {");
            cb.l("            _ws!.SendAsync(new ArraySegment<byte>(data), WebSocketMessageType.Binary, true, CancellationToken.None).Wait();");
            cb.l("        }");
            cb.l("        else");
            cb.l("        {");
            cb.l("            var w = new BinaryWriter();");
            if cfg!(feature = "len64") {
                cb.l("            w.WriteU64(data.Length);");
            } else {
                cb.l("            w.WriteU32(data.Length);");
            }
            cb.l("            w.WriteRaw(data);");
            cb.l("            _tcpStream!.Write(w.ToBytes());");
            cb.l("            _tcpStream.Flush();");
            cb.l("        }");
        } else if has_ws {
            cb.l("        _ws!.SendAsync(new ArraySegment<byte>(data), WebSocketMessageType.Binary, true, CancellationToken.None).Wait();");
        } else {
            cb.l("        var w = new BinaryWriter();");
            if cfg!(feature = "len64") {
                cb.l("        w.WriteU64(data.Length);");
            } else {
                cb.l("        w.WriteU32(data.Length);");
            }
            cb.l("        w.WriteRaw(data);");
            cb.l("        _tcpStream!.Write(w.ToBytes());");
            cb.l("        _tcpStream.Flush();");
        }
        cb.l("    }");
        cb.b();
    }

    // Handler methods — nested class structure like Kotlin
    let apis_class_name = format!("{}Apis", class_name);
    cb.push(format!("    public class {}", apis_class_name));
    cb.l("    {");
    cb.push(format!(
        "        private readonly {}Client _client;",
        class_name
    ));
    cb.b();
    // Collect group names for constructor initialization
    let mut group_names: Vec<String> = Vec::new();
    collect_group_names_cs(&svc.handlers, &mut group_names);
    // Generate nested classes and properties
    gen_handler_object_cs(
        &svc.handlers,
        &svc.handlers,
        &[],
        "    ",
        &class_name,
        debug,
        &mut cb,
    );
    // Constructor with nested class initialization
    cb.push(format!(
        "        public {}Apis({}Client client)",
        class_name, class_name
    ));
    cb.l("        {");
    cb.l("            _client = client;");
    for gn in &group_names {
        let prop_name = to_camel_case(gn.strip_suffix("Api").unwrap_or(gn));
        cb.push(format!(
            "            this.{} = new {}(client);",
            prop_name, gn
        ));
    }
    cb.l("        }");
    cb.l("    }");
    cb.b();
    cb.push(format!("    public {} Apis {{ get; }}", apis_class_name));

    // ── Ordinary-ws routes ────────────────────────────────────
    #[cfg(feature = "ordinary-ws")]
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
            "Dictionary<string, string>? query = null".to_string()
        } else {
            let params_str: Vec<String> = param_names
                .iter()
                .map(|p| format!("string {}", p))
                .collect();
            format!(
                "{}, Dictionary<string, string>? query = null",
                params_str.join(", ")
            )
        };

        let mut path_expr = String::new();
        for (i, seg) in segments.iter().enumerate() {
            if i > 0 {
                path_expr.push('/');
            }
            if let Some(param) = seg.strip_prefix(':') {
                path_expr.push_str(&format!("{{{}}}", param));
            } else {
                path_expr.push_str(seg);
            }
        }

        let ind = "        ";
        let camel_name = to_camel_case(handler_name);
        cb.push(format!("    /// <summary>WebSocket: {}</summary>", path));
        cb.push(format!(
            "    public async Task<ClientWebSocket> {}({})",
            camel_name, sig_params
        ));
        cb.l("    {");
        cb.push(format!(
            "        {}var scheme = _tls ? \"wss\" : \"ws\";",
            ind
        ));
        cb.push(format!(
            "        {}var url = $\"{{scheme}}://{{_host}}:{{_port}}/{}\";",
            ind, path_expr
        ));
        cb.push(format!("        {}if (query != null) {{", ind));
        cb.push(format!("            {}var qs = string.Join(\"&\", query.Select(kv => $\"{{Uri.EscapeDataString(kv.Key)}}={{Uri.EscapeDataString(kv.Value)}}\"));", ind));
        cb.push(format!(
            "            {}if (qs.Length > 0) url += \"?\" + qs;",
            ind
        ));
        cb.push(format!("        {}}}", ind));
        cb.push(format!("        {}var ws = new ClientWebSocket();", ind));
        cb.push(format!(
            "        {}await ws.ConnectAsync(new Uri(url), CancellationToken.None);",
            ind
        ));
        cb.push(format!("        {}return ws;", ind));
        cb.l("    }");
        cb.b();
    }

    // ── Ordinary-sse routes ────────────────────────────────────
    #[cfg(feature = "ordinary-sse")]
    for sse_route in &svc.sse_routes {
        let path = sse_route.path;
        let handler_name = sse_route.handler_name;
        let trimmed = path.trim_start_matches('/').trim_end_matches('/');
        let segments: Vec<&str> = trimmed.split('/').collect();
        let param_names: Vec<&str> = segments
            .iter()
            .filter_map(|s| s.strip_prefix(':'))
            .collect();

        let sig_params = if param_names.is_empty() {
            "Dictionary<string, string>? query = null".to_string()
        } else {
            let params_str: Vec<String> = param_names
                .iter()
                .map(|p| format!("string {}", p))
                .collect();
            format!(
                "{}, Dictionary<string, string>? query = null",
                params_str.join(", ")
            )
        };

        let mut path_expr = String::new();
        for (i, seg) in segments.iter().enumerate() {
            if i > 0 {
                path_expr.push('/');
            }
            if let Some(param) = seg.strip_prefix(':') {
                path_expr.push_str(&format!("{{{}}}", param));
            } else {
                path_expr.push_str(seg);
            }
        }

        let ind = "        ";
        let camel_name = to_camel_case(handler_name);
        cb.push(format!("    /// <summary>SSE: {}</summary>", path));
        cb.push(format!(
            "    public async Task<HttpResponseMessage> {}({})",
            camel_name, sig_params
        ));
        cb.l("    {");
        cb.push(format!(
            "        {}var scheme = _tls ? \"https\" : \"http\";",
            ind
        ));
        cb.push(format!(
            "        {}var url = $\"{{scheme}}://{{_host}}:{{_port}}/{}\";",
            ind, path_expr
        ));
        cb.push(format!("        {}if (query != null) {{", ind));
        cb.push(format!("            {}var qs = string.Join(\"&\", query.Select(kv => $\"{{Uri.EscapeDataString(kv.Key)}}={{Uri.EscapeDataString(kv.Value)}}\"));", ind));
        cb.push(format!(
            "            {}if (qs.Length > 0) url += \"?\" + qs;",
            ind
        ));
        cb.push(format!("        {}}}", ind));
        cb.push(format!(
            "        {}var request = new HttpRequestMessage(HttpMethod.Get, url);",
            ind
        ));
        cb.push(format!(
            "        {}request.Headers.Add(\"Accept\", \"text/event-stream\");",
            ind
        ));
        cb.push(format!("        {}return await _httpClient.SendAsync(request, HttpCompletionOption.ResponseHeadersRead);", ind));
        cb.l("    }");
        cb.b();
    }

    // DisposeAsync
    cb.l("    public async ValueTask DisposeAsync()");
    cb.l("    {");
    if has_ws {
        cb.l("        if (_ws != null && _ws.State == WebSocketState.Open)");
        cb.l("            await _ws.CloseAsync(WebSocketCloseStatus.NormalClosure, \"dispose\", CancellationToken.None);");
        cb.l("        _ws?.Dispose();");
    }
    if has_tcp {
        cb.l("        _tcpStream?.Close();");
        cb.l("        _tcpClient?.Close();");
    }
    cb.l("        _httpClient.Dispose();");
    cb.l("    }");

    cb.l("}");
    cb.build()
}

fn has_cache_handlers_cs(handlers: &[Handler]) -> bool {
    for h in handlers {
        if h.meta.cache_seconds > 0 {
            return true;
        }
        if has_cache_handlers_cs(&h.children) {
            return true;
        }
    }
    false
}

fn collect_group_names_cs(handlers: &[Handler], names: &mut Vec<String>) {
    for h in handlers {
        if h.meta.name.is_empty() {
            let clean_name = h.name.trim_start_matches(':');
            let group_class_name = format!("{}Api", to_pascal_case(clean_name));
            names.push(group_class_name);
            collect_group_names_cs(&h.children, names);
        }
    }
}

#[allow(clippy::only_used_in_recursion)]
fn gen_handler_object_cs(
    handlers: &[Handler],
    all: &[Handler],
    path: &[&str],
    indent: &str,
    svc_name: &str,
    debug: bool,
    cb: &mut CodeBuf,
) {
    let inner_indent = format!("{}    ", indent);
    for h in handlers {
        let child_path = {
            let mut p = path.to_vec();
            p.push(h.name);
            p
        };
        if h.meta.name.is_empty() {
            // Group node — emit a nested class like Kotlin's inner class
            let clean_name = h.name.trim_start_matches(':');
            let group_class_name = format!("{}Api", to_pascal_case(clean_name));
            cb.push(format!("{}public class {}", inner_indent, group_class_name));
            cb.push(format!("{}{{", inner_indent));
            cb.push(format!(
                "{}    private readonly {}Client _client;",
                inner_indent, svc_name
            ));
            cb.push(format!(
                "{}    public {}({}Client client) {{ _client = client; }}",
                inner_indent, group_class_name, svc_name
            ));
            cb.b();
            gen_handler_object_cs(
                &h.children,
                all,
                &child_path,
                &inner_indent,
                svc_name,
                debug,
                cb,
            );
            cb.push(format!("{}}}", inner_indent));
            cb.b();
            cb.push(format!(
                "{}public {} {} {{ get; }}",
                inner_indent,
                group_class_name,
                to_camel_case(clean_name)
            ));
            cb.b();
        } else {
            let prefix_str = handler_prefix(&child_path);
            let cache_key_parts: Vec<&str> = path
                .iter()
                .chain(std::iter::once(&h.name))
                .copied()
                .collect();
            let cache_key = cache_key_parts.join(".");
            if h.meta.is_ordinary {
                #[cfg(feature = "ordinary-http")]
                {
                    cb.push(ordinary_handler_method_cs(
                        h,
                        &prefix_str,
                        path,
                        &inner_indent,
                        svc_name,
                        debug,
                        svc_name,
                        &cache_key,
                    ));
                }
                #[cfg(not(feature = "ordinary-http"))]
                {
                    let fn_name = to_pascal_case(if !h.meta.api_name.is_empty() {
                        h.meta.api_name
                    } else {
                        h.meta.name
                    });
                    cb.push(format!("{}public Task {}() => throw new InvalidOperationException(\"ordinary-http not enabled\");", inner_indent, fn_name));
                }
            } else {
                cb.push(handler_method_cs(
                    h,
                    all,
                    &prefix_str,
                    &inner_indent,
                    svc_name,
                    svc_name,
                    &cache_key,
                ));
            }
        }
    }
}

fn collect_type_exports_cs(
    handlers: &[Handler],
    path: &[&str],
    emitted: &mut Vec<String>,
    cb: &mut CodeBuf,
) {
    for h in handlers {
        let child_path = {
            let mut p = path.to_vec();
            p.push(h.name);
            p
        };
        if !h.meta.name.is_empty() {
            let prefix_str = handler_prefix(&child_path);
            cb.push(handler_type_exports_cs(&prefix_str, h.meta, emitted));
        }
        if !h.children.is_empty() {
            collect_type_exports_cs(&h.children, &child_path, emitted, cb);
        }
    }
}

// ─── File output ───────────────────────────────────────────────────

#[cfg(feature = "cs")]
fn write_service_cs(
    svc: &Service,
    dir: &Path,
    calls: &[crate::NetCallType],
    debug: bool,
) -> Result<(), Error> {
    use std::fs;

    let code = generate_service_cs(svc, calls, debug);
    let svc_dir = dir.join(&svc.name);
    fs::create_dir_all(&svc_dir).map_err(|e| Error::Io {
        message: e.to_string(),
    })?;
    let file_name = format!("{}.cs", svc.name);
    let file_path = svc_dir.join(&file_name);
    fs::write(&file_path, &code).map_err(|e| Error::Io {
        message: e.to_string(),
    })?;

    Ok(())
}

impl AFast {
    /// Returns the complete generated C# code as a single string.
    #[cfg(feature = "cs")]
    pub fn get_cs_code(&self) -> String {
        let seq64 = cfg!(feature = "seq64");
        let common = generate_common_cs(&self.services, seq64, true, true);
        let mut parts = vec![common];
        for svc in &self.services {
            if svc.name.is_empty() {
                continue;
            }
            parts.push(generate_service_cs(
                svc,
                &[
                    crate::NetCallType::Http,
                    crate::NetCallType::Ws,
                    crate::NetCallType::Tcp,
                ],
                false,
            ));
        }
        parts.join("\n\n")
    }

    /// Generates C# client code for all registered services and writes
    /// the files to `dir`.  Produces one `Common.cs` with shared types
    /// and one `{service}.cs` per service.
    #[cfg(feature = "cs")]
    pub fn generate_cs(
        &self,
        dir: &Path,
        calls: &[crate::NetCallType],
        debug: bool,
        filter: Option<&[String]>,
    ) -> Result<(), Error> {
        use std::fs;

        fs::create_dir_all(dir).map_err(|e| Error::Io {
            message: e.to_string(),
        })?;

        let seq64 = cfg!(feature = "seq64");
        let has_ws = calls.iter().any(|c| matches!(c, crate::NetCallType::Ws));
        let has_tcp = calls.iter().any(|c| matches!(c, crate::NetCallType::Tcp));

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

        let common = generate_common_cs(&filtered_svcs, seq64, has_ws, has_tcp);
        fs::write(dir.join("Common.cs"), &common).map_err(|e| Error::Io {
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
            write_service_cs(svc, dir, calls, debug)?;
        }

        Ok(())
    }
}
