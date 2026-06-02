use crate::{Handler, Service, TagKind};
use std::collections::HashMap;

/// Returns an iterator over fields that should be included in generated code,
/// filtering out `skip` fields and `skip_with` fields whose marker matches.
fn included(
    fields: &[crate::handler::FieldMeta],
) -> impl Iterator<Item = &crate::handler::FieldMeta> {
    fields
        .iter()
        .filter(|f| crate::marker::should_include_field(f))
}

// ─── JsonBuilder ──────────────────────────────────────────────────
// Minimal JSON serializer used to build the schema data embedded in
// the generated HTML page.  Avoids pulling in a full JSON library
// for this single-purpose use case.

struct JsonBuilder {
    buf: String,
}

impl JsonBuilder {
    fn new() -> Self {
        Self {
            buf: String::with_capacity(4096),
        }
    }
    fn object_start(&mut self) {
        self.buf.push('{');
    }
    fn object_end(&mut self) {
        self.buf.push('}');
    }
    fn array_start(&mut self) {
        self.buf.push('[');
    }
    fn array_end(&mut self) {
        self.buf.push(']');
    }
    fn key(&mut self, k: &str) {
        self.buf.push('"');
        self.buf.push_str(k);
        self.buf.push_str("\":");
    }
    fn string(&mut self, v: &str) {
        self.buf.push('"');
        for c in v.chars() {
            match c {
                '"' => self.buf.push_str("\\\""),
                '\\' => self.buf.push_str("\\\\"),
                '\n' => self.buf.push_str("\\n"),
                '\r' => self.buf.push_str("\\r"),
                '\t' => self.buf.push_str("\\t"),
                '<' => self.buf.push_str("\\u003c"),
                '>' => self.buf.push_str("\\u003e"),
                '&' => self.buf.push_str("\\u0026"),
                _ => self.buf.push(c),
            }
        }
        self.buf.push('"');
    }
    fn number(&mut self, v: i64) {
        self.buf.push_str(&v.to_string());
    }
    fn i64(&mut self, v: i64) {
        self.buf.push_str(&v.to_string());
    }
    fn f64(&mut self, v: f64) {
        self.buf.push_str(&v.to_string());
    }
    fn bool(&mut self, v: bool) {
        self.buf.push_str(if v { "true" } else { "false" });
    }
    fn null(&mut self) {
        self.buf.push_str("null");
    }
    fn comma(&mut self) {
        self.buf.push(',');
    }
    fn raw(&mut self, s: &str) {
        self.buf.push_str(s);
    }
    fn build(self) -> String {
        self.buf
    }
}

// ─── Type helpers ─────────────────────────────────────────────────

fn is_primitive(ty: &str) -> bool {
    matches!(
        ty,
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

// ─── Schema collection ────────────────────────────────────────────
// Collects handler metadata, type definitions, Custom/Header info,
// and validation rules into a JSON schema that the frontend JavaScript
// consumes to render the interactive documentation UI.

struct CustomInfo {
    ty: String,
    desc: String,
}

fn collect_all_headers(handlers: &[Handler]) -> Vec<CustomInfo> {
    let mut headers = Vec::new();
    collect_headers_recursive(handlers, &mut headers);
    headers
}

fn collect_headers_recursive(handlers: &[Handler], headers: &mut Vec<CustomInfo>) {
    for h in handlers {
        for param in h.meta.params {
            if param.extractor == "Header" {
                let ty = param.ty.to_string();
                if !headers.iter().any(|c| c.ty == ty) {
                    let desc = param
                        .structure
                        .map(|s| s().desc.to_string())
                        .unwrap_or_default();
                    // Omit header types whose fields are all standard HTTP headers
                    // (e.g. content-type, accept).  These are set by the browser
                    // automatically and do not need user-facing input fields.
                    let has_non_standard = param
                        .structure
                        .map(|s| {
                            let structure = s();
                            match structure.kind {
                                crate::handler::TagKind::Struct(fields) => fields
                                    .iter()
                                    .any(|f| !crate::is_standard_header(&to_header_name(f.name))),
                                _ => true,
                            }
                        })
                        .unwrap_or(true);
                    if has_non_standard {
                        headers.push(CustomInfo { ty, desc });
                    }
                }
            }
        }
        collect_headers_recursive(&h.children, headers);
    }
}

fn to_header_name(field_name: &str) -> String {
    field_name.replace('_', "-")
}

/// Collects all unique Custom extractor types across all handlers in a
/// service tree.  Used by the frontend to render global custom-parameter
/// input panels.
fn collect_all_customs(handlers: &[Handler]) -> Vec<CustomInfo> {
    let mut customs = Vec::new();
    collect_customs_recursive(handlers, &mut customs);
    customs
}

fn collect_customs_recursive(handlers: &[Handler], customs: &mut Vec<CustomInfo>) {
    for h in handlers {
        for param in h.meta.params {
            if param.extractor == "Custom" {
                let ty = param.ty.to_string();
                if !customs.iter().any(|c| c.ty == ty) {
                    let desc = param
                        .structure
                        .map(|s| s().desc.to_string())
                        .unwrap_or_default();
                    customs.push(CustomInfo { ty, desc });
                }
            }
        }
        collect_customs_recursive(&h.children, customs);
    }
}

/// Recursively collects all structured type definitions referenced by
/// handler parameters and return types into a HashMap keyed by type name.
fn collect_all_types(handlers: &[Handler], types: &mut HashMap<String, String>) {
    for h in handlers {
        for param in h.meta.params {
            if let Some(structure_fn) = param.structure {
                emit_type_to_map(structure_fn(), types);
            }
        }
        if h.meta.return_type != "()"
            && let Some(structure_fn) = h.meta.return_structure
        {
            emit_type_to_map(structure_fn(), types);
        }
        collect_all_types(&h.children, types);
    }
}

fn emit_validations(jb: &mut JsonBuilder, validations: &'static [crate::handler::ValidateRule]) {
    jb.key("validations");
    jb.array_start();
    for (ri, rule) in validations.iter().enumerate() {
        if ri > 0 {
            jb.comma();
        }
        match rule {
            crate::handler::ValidateRule::Gt {
                value,
                code,
                message,
            } => {
                jb.object_start();
                jb.key("rule");
                jb.string("gt");
                jb.comma();
                jb.key("value");
                jb.f64(*value);
                jb.comma();
                jb.key("code");
                jb.i64(*code);
                jb.comma();
                jb.key("message");
                jb.string(message);
                jb.object_end();
            }
            crate::handler::ValidateRule::Gte {
                value,
                code,
                message,
            } => {
                jb.object_start();
                jb.key("rule");
                jb.string("gte");
                jb.comma();
                jb.key("value");
                jb.f64(*value);
                jb.comma();
                jb.key("code");
                jb.i64(*code);
                jb.comma();
                jb.key("message");
                jb.string(message);
                jb.object_end();
            }
            crate::handler::ValidateRule::Lt {
                value,
                code,
                message,
            } => {
                jb.object_start();
                jb.key("rule");
                jb.string("lt");
                jb.comma();
                jb.key("value");
                jb.f64(*value);
                jb.comma();
                jb.key("code");
                jb.i64(*code);
                jb.comma();
                jb.key("message");
                jb.string(message);
                jb.object_end();
            }
            crate::handler::ValidateRule::Lte {
                value,
                code,
                message,
            } => {
                jb.object_start();
                jb.key("rule");
                jb.string("lte");
                jb.comma();
                jb.key("value");
                jb.f64(*value);
                jb.comma();
                jb.key("code");
                jb.i64(*code);
                jb.comma();
                jb.key("message");
                jb.string(message);
                jb.object_end();
            }
            crate::handler::ValidateRule::Len {
                min,
                max,
                code,
                message,
            } => {
                jb.object_start();
                jb.key("rule");
                jb.string("len");
                jb.comma();
                jb.key("min");
                jb.i64(*min);
                jb.comma();
                jb.key("max");
                jb.i64(*max);
                jb.comma();
                jb.key("code");
                jb.i64(*code);
                jb.comma();
                jb.key("message");
                jb.string(message);
                jb.object_end();
            }
            crate::handler::ValidateRule::Of {
                values,
                code,
                message,
            } => {
                jb.object_start();
                jb.key("rule");
                jb.string("of");
                jb.comma();
                jb.key("values");
                jb.array_start();
                for (vi, v) in values.iter().enumerate() {
                    if vi > 0 {
                        jb.comma();
                    }
                    jb.string(v);
                }
                jb.array_end();
                jb.comma();
                jb.key("code");
                jb.i64(*code);
                jb.comma();
                jb.key("message");
                jb.string(message);
                jb.object_end();
            }
        }
    }
    jb.array_end();
}

fn emit_type_to_map(meta: &'static crate::TagMeta, types: &mut HashMap<String, String>) {
    if types.contains_key(meta.name) {
        return;
    }

    let mut jb = JsonBuilder::new();
    jb.object_start();
    jb.key("name");
    jb.string(meta.name);
    jb.comma();
    jb.key("desc");
    jb.string(meta.desc);
    jb.comma();

    match meta.kind {
        TagKind::Struct(fields) => {
            jb.key("kind");
            jb.string("struct");
            jb.comma();
            jb.key("fields");
            jb.array_start();
            for (i, f) in included(fields).enumerate() {
                if i > 0 {
                    jb.comma();
                }
                jb.object_start();
                jb.key("name");
                jb.string(f.name);
                jb.comma();
                jb.key("ty");
                jb.string(f.ty);
                jb.comma();
                jb.key("desc");
                jb.string(f.desc);
                jb.comma();
                jb.key("structure");
                match f.structure {
                    Some(f_fn) => jb.string(f_fn().name),
                    None => jb.null(),
                }
                jb.comma();
                jb.key("primitive");
                jb.bool(is_primitive(f.ty));
                jb.comma();
                emit_validations(&mut jb, f.validations);
                jb.object_end();
                if let Some(f_fn) = f.structure {
                    emit_type_to_map(f_fn(), types);
                }
            }
            jb.array_end();
        }
        TagKind::Enum(variants) => {
            jb.key("kind");
            jb.string("enum");
            jb.comma();
            jb.key("variants");
            jb.array_start();
            for (i, v) in variants.iter().enumerate() {
                if i > 0 {
                    jb.comma();
                }
                jb.object_start();
                jb.key("name");
                jb.string(v.name);
                jb.comma();
                jb.key("fields");
                jb.array_start();
                for (j, f) in included(v.fields).enumerate() {
                    if j > 0 {
                        jb.comma();
                    }
                    jb.object_start();
                    jb.key("name");
                    jb.string(f.name);
                    jb.comma();
                    jb.key("ty");
                    jb.string(f.ty);
                    jb.comma();
                    jb.key("desc");
                    jb.string(f.desc);
                    jb.comma();
                    jb.key("structure");
                    match f.structure {
                        Some(f_fn) => jb.string(f_fn().name),
                        None => jb.null(),
                    }
                    jb.comma();
                    jb.key("primitive");
                    jb.bool(is_primitive(f.ty));
                    jb.comma();
                    emit_validations(&mut jb, f.validations);
                    jb.object_end();
                    if let Some(f_fn) = f.structure {
                        emit_type_to_map(f_fn(), types);
                    }
                }
                jb.array_end();
                jb.object_end();
            }
            jb.array_end();
        }
    }

    jb.object_end();
    types.insert(meta.name.to_string(), jb.build());
}

fn build_handler_tree(handlers: &[Handler], path_prefix: &[&str]) -> Vec<String> {
    let mut result = Vec::new();
    for h in handlers {
        let mut full_path = path_prefix.to_vec();
        full_path.push(h.name);

        // Construct the public API path: uses apiName for the last
        // segment when set, falling back to the Rust function name.
        let mut api_path = path_prefix.to_vec();
        let display_name = if !h.meta.api_name.is_empty() {
            h.meta.api_name
        } else {
            h.name
        };
        api_path.push(display_name);

        let mut jb = JsonBuilder::new();
        jb.object_start();

        jb.key("path");
        jb.string(&full_path.join("."));
        jb.comma();
        jb.key("apiPath");
        jb.string(&api_path.join("."));
        jb.comma();
        jb.key("name");
        jb.string(h.name);
        jb.comma();
        jb.key("description");
        jb.string(h.meta.desc);
        jb.comma();
        jb.key("apiName");
        jb.string(h.meta.api_name);
        jb.comma();

        jb.key("offset");
        if h.meta.name.is_empty() {
            jb.number(-1);
        } else {
            jb.number(h.offset as i64);
        }
        jb.comma();

        jb.key("longConnection");
        jb.bool(h.meta.long_connection);
        jb.comma();

        // Fields specific to ordinary (REST-style) HTTP handlers.
        jb.key("isOrdinary");
        jb.bool(h.meta.is_ordinary);
        jb.comma();
        jb.key("method");
        jb.string(h.meta.method);
        jb.comma();
        jb.key("httpPath");
        jb.string(h.path);
        jb.comma();

        // Flag indicating this node is a group (no handler function).
        jb.key("isGroup");
        jb.bool(h.meta.name.is_empty());
        jb.comma();

        // Serialise each parameter's metadata for the frontend.
        jb.key("params");
        jb.array_start();
        for (i, p) in h.meta.params.iter().enumerate() {
            if i > 0 {
                jb.comma();
            }
            jb.object_start();
            jb.key("name");
            jb.string(p.name);
            jb.comma();
            jb.key("extractor");
            jb.string(p.extractor);
            jb.comma();
            jb.key("ty");
            jb.string(p.ty);
            jb.comma();
            jb.key("tag");
            match p.structure {
                Some(f) => jb.string(f().desc),
                None => jb.string(""),
            }
            jb.comma();
            jb.key("structure");
            match p.structure {
                Some(f) => jb.string(f().name),
                None => jb.null(),
            }
            jb.comma();
            jb.key("primitive");
            jb.bool(is_primitive(p.ty));
            jb.object_end();
        }
        jb.array_end();
        jb.comma();

        jb.key("returnType");
        jb.string(h.meta.return_type);
        jb.comma();
        jb.key("returnStructure");
        match h.meta.return_structure {
            Some(f) => jb.string(f().name),
            None => jb.null(),
        }
        jb.comma();

        // Recursively serialise child handlers/groups.
        jb.key("children");
        let children = build_handler_tree(&h.children, &full_path);
        jb.array_start();
        for (i, child) in children.iter().enumerate() {
            if i > 0 {
                jb.comma();
            }
            jb.raw(child);
        }
        jb.array_end();

        jb.object_end();
        result.push(jb.build());
    }
    result
}

fn build_schema(svc: &Service) -> String {
    let customs = collect_all_customs(&svc.handlers);
    #[allow(unused_mut)]
    let mut headers = collect_all_headers(&svc.handlers);
    // Top-level ordinary routes may define their own Header extractors
    // that are not reachable from the nested handler tree.  Collect them
    // here, applying the same non-standard-header filter.
    #[cfg(feature = "ordinary-http")]
    for route in &svc.ordinary_routes {
        for param in route.handler_entry.meta.params {
            if param.extractor == "Header" {
                let ty = param.ty.to_string();
                if !headers.iter().any(|c| c.ty == ty) {
                    let desc = param
                        .structure
                        .map(|s| s().desc.to_string())
                        .unwrap_or_default();
                    let has_non_standard = param
                        .structure
                        .map(|s| {
                            let structure = s();
                            match structure.kind {
                                crate::handler::TagKind::Struct(fields) => fields
                                    .iter()
                                    .any(|f| !crate::is_standard_header(&to_header_name(f.name))),
                                _ => true,
                            }
                        })
                        .unwrap_or(true);
                    if has_non_standard {
                        headers.push(CustomInfo { ty, desc });
                    }
                }
            }
        }
    }
    let mut types_map = HashMap::new();
    collect_all_types(&svc.handlers, &mut types_map);

    let handler_tree = build_handler_tree(&svc.handlers, &[]);

    let mut jb = JsonBuilder::new();
    jb.object_start();

    // Top-level service metadata.
    jb.key("serviceName");
    jb.string(&svc.name);
    jb.comma();
    jb.key("serviceDesc");
    jb.string(&svc.desc);
    jb.comma();

    // Global custom parameter types with their descriptions.
    jb.key("customs");
    jb.array_start();
    for (i, c) in customs.iter().enumerate() {
        if i > 0 {
            jb.comma();
        }
        jb.object_start();
        jb.key("typeName");
        jb.string(&c.ty);
        jb.comma();
        jb.key("description");
        jb.string(&c.desc);
        jb.object_end();
    }
    jb.array_end();
    jb.comma();

    // Global header types for ordinary HTTP requests.
    jb.key("headers");
    jb.array_start();
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            jb.comma();
        }
        jb.object_start();
        jb.key("typeName");
        jb.string(&h.ty);
        jb.comma();
        jb.key("description");
        jb.string(&h.desc);
        jb.object_end();
    }
    jb.array_end();
    jb.comma();

    // All structured type definitions referenced by this service.
    jb.key("types");
    jb.object_start();
    for (i, (name, entry)) in types_map.iter().enumerate() {
        if i > 0 {
            jb.comma();
        }
        jb.key(name);
        jb.raw(entry);
    }
    jb.object_end();
    jb.comma();

    // The handler tree, serialised recursively from build_handler_tree.
    jb.key("handlers");
    jb.array_start();
    for (i, h) in handler_tree.iter().enumerate() {
        if i > 0 {
            jb.comma();
        }
        jb.raw(h);
    }
    jb.array_end();

    jb.object_end();
    jb.build()
}

// ─── JS Client Embedding ──────────────────────────────────────────
// Invokes the JavaScript code generator and wraps the result for
// in-browser use: strips ESM export keywords and registers classes on
// the window object so they can be discovered dynamically at runtime.

fn embed_js_client(svc: &Service, calls: &[crate::JsTsCallType]) -> String {
    let js = super::js::generate_service_js(svc, calls, true);
    // The JS generator emits ESM-style `export class`.  For direct
    // embedding in a <script> tag, the `export` keyword must be removed.
    let js = js.replace("export class ", "class ");
    // In a browser, `class` declarations at the top level do not create
    // properties on `window`.  Explicit assignment ensures the UI code
    // can find client classes via `window['{Name}Client']`.
    let class_name = to_pascal_case(&svc.name);
    format!(
        "{}\nwindow['Socket'] = Socket;\nwindow['{}Client'] = {}Client;",
        js, class_name, class_name
    )
}

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

// ─── Favicon ───────────────────────────────────────────────────────
// Inlines the SVG favicon as a data: URI so no external file is needed.

const FAVICON_SVG: &str = include_str!("favicon.svg");

fn favicon_data_uri() -> String {
    let svg: String = FAVICON_SVG
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ");
    let encoded: String = svg
        .bytes()
        .map(|b| match b {
            b'#' => "%23".to_string(),
            b'<' => "%3C".to_string(),
            b'>' => "%3E".to_string(),
            b'"' => "%22".to_string(),
            b' ' => "%20".to_string(),
            _ => (b as char).to_string(),
        })
        .collect();
    format!("data:image/svg+xml,{}", encoded)
}

// ─── CSS ──────────────────────────────────────────────────────────
// The complete stylesheet for the documentation UI.  Supports light and
// dark themes via CSS custom properties toggled by a data-theme attribute.

const CSS: &str = r#"
:root {
    --bg-primary: #ffffff;
    --bg-secondary: #f8f9fa;
    --bg-tertiary: #e9ecef;
    --bg-card: #ffffff;
    --text-primary: #212529;
    --text-secondary: #6c757d;
    --text-muted: #adb5bd;
    --border-color: #dee2e6;
    --accent: #0d6efd;
    --accent-hover: #0b5ed7;
    --accent-bg: #e7f1ff;
    --success: #198754;
    --success-bg: #d1e7dd;
    --danger: #dc3545;
    --danger-bg: #f8d7da;
    --warning: #ffc107;
    --warning-bg: #fff3cd;
    --notice-bg: #fef3c7;
    --notice-text: #92400e;
    --notice-border: #f59e0b;
    --code-bg: #f1f3f5;
    --shadow: 0 1px 3px rgba(0,0,0,0.08);
    --shadow-lg: 0 4px 12px rgba(0,0,0,0.1);
    --radius: 8px;
    --radius-sm: 4px;
    --font-mono: 'SF Mono', 'Fira Code', 'Consolas', monospace;
}
[data-theme="dark"] {
    --bg-primary: #111113;
    --bg-secondary: #1a1a1d;
    --bg-tertiary: #222225;
    --bg-card: #1a1a1d;
    --text-primary: #e0e0e0;
    --text-secondary: #909296;
    --text-muted: #5c5f66;
    --border-color: #2c2e33;
    --accent: #339af0;
    --accent-hover: #228be6;
    --accent-bg: #1a2a3a;
    --success: #40c057;
    --success-bg: #1a2e1a;
    --danger: #fa5252;
    --danger-bg: #2e1a1a;
    --warning: #fcc419;
    --warning-bg: #3d3200;
    --notice-bg: #451a03;
    --notice-text: #fde68a;
    --notice-border: #f59e0b;
    --code-bg: #222225;
    --shadow: 0 1px 3px rgba(0,0,0,0.3);
    --shadow-lg: 0 4px 12px rgba(0,0,0,0.4);
}
* { box-sizing: border-box; margin: 0; padding: 0; }
.hidden { display: none !important; }
body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg-primary);
    color: var(--text-primary);
    line-height: 1.6;
    font-size: 16px;
    overflow: hidden;
    height: 100vh;
}
.header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 12px 24px; border-bottom: 1px solid var(--border-color);
    background: var(--bg-secondary); position: sticky; top: 0; z-index: 100;
}
.header h1 { font-size: 24px; font-weight: 600; }
.header .subtitle { color: var(--text-secondary); font-size: 16px; margin-left: 12px; }
#theme-toggle {
    background: var(--bg-tertiary); border: 1px solid var(--border-color);
    border-radius: var(--radius-sm); width: 34px; padding: 0; cursor: pointer;
    color: var(--text-primary); font-size: 18px; display: flex;
    align-items: center; justify-content: center; flex-shrink: 0;
}
#theme-toggle:hover { background: var(--accent-bg); }
.header-controls {
    display: flex; gap: 8px; align-items: center;
}
.header-input {
    height: 34px; padding: 0 10px; font-size: 15px;
    background: var(--bg-primary); border: 1px solid var(--border-color);
    border-radius: var(--radius-sm); color: var(--text-primary);
    font-family: var(--font-mono);
}
.header-input:focus { outline: none; border-color: var(--accent); }
.header-sep { color: var(--text-muted); font-size: 16px; line-height: 34px; }
.header-checkbox {
    display: flex; align-items: center; gap: 4px;
    font-size: 14px; color: var(--text-secondary); cursor: pointer;
    height: 34px; padding: 0 8px; border: 1px solid var(--border-color);
    border-radius: var(--radius-sm); background: var(--bg-primary);
}
.header-checkbox:hover { border-color: var(--accent); }
.header-checkbox input { margin: 0; cursor: pointer; }
.header-controls .btn { height: 34px; padding: 0 14px; font-size: 15px; white-space: nowrap; display: inline-flex; align-items: center; }
.header-controls .btn:disabled { opacity: 0.5; cursor: not-allowed; }
.header-controls .header-input:disabled { opacity: 0.5; cursor: not-allowed; }
.header-controls .header-checkbox:disabled { opacity: 0.5; cursor: not-allowed; }
#disconnect-btn { background: var(--danger); color: #fff; }
#disconnect-btn:hover { opacity: 0.9; }
.main { display: flex; height: calc(100vh - 57px); }
.sidebar {
    width: 320px; min-width: 320px; border-right: 1px solid var(--border-color);
    background: var(--bg-secondary); padding: 16px 0; overflow-y: auto;
    position: sticky; top: 57px; height: calc(100vh - 57px);
}
.sidebar a {
    display: block; padding: 6px 20px; color: var(--text-secondary);
    text-decoration: none; font-size: 15px; cursor: pointer;
}
.sidebar a:hover { color: var(--accent); background: var(--accent-bg); }
.sidebar a.active { color: var(--accent); background: var(--accent-bg); font-weight: 500; }
.sidebar .group-label {
    padding: 12px 20px 4px; font-size: 13px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.5px; color: var(--text-muted);
    cursor: pointer; display: flex; align-items: center; gap: 4px; user-select: none;
}
.sidebar .group-label:hover { color: var(--text-secondary); }
.sidebar .group-label .group-arrow { font-size: 10px; transition: transform 0.2s; }
.sidebar .group-label .group-arrow.collapsed { transform: rotate(-90deg); }
.sidebar .group-children { overflow: hidden; max-height: 500px; transition: max-height 0.2s ease; }
.sidebar .group-children.collapsed { max-height: 0 !important; }
.content { flex: 1; padding: 24px; overflow-y: auto; height: calc(100vh - 57px); }
.panel {
    background: var(--bg-card); border: 1px solid var(--border-color);
    border-radius: var(--radius); padding: 20px; margin-bottom: 16px;
    box-shadow: var(--shadow);
}
.panel h2 { font-size: 20px; margin-bottom: 12px; }
.panel h3 { font-size: 16px; margin-bottom: 8px; color: var(--text-secondary); }
.text-secondary { color: var(--text-secondary); }
.text-muted { color: var(--text-muted); }
.badge {
    display: inline-block; padding: 2px 8px; border-radius: 12px;
    font-size: 13px; font-weight: 500;
}
.badge-custom { background: var(--accent-bg); color: var(--accent); }
.badge-data { background: var(--success-bg); color: var(--success); }
.badge-state { background: var(--bg-tertiary); color: var(--text-secondary); }
.endpoint {
    background: var(--bg-card); border: 1px solid var(--border-color);
    border-radius: var(--radius); margin-bottom: 8px; overflow: hidden;
    box-shadow: var(--shadow);
}
.endpoint-incompatible {
    border-left: 3px solid var(--notice-border);
}
.endpoint-incompatible-notice {
    position: absolute; right: 36px; top: 50%; transform: translateY(-50%);
    padding: 3px 8px;
    background: var(--notice-bg);
    color: var(--notice-text);
    border: 1px solid var(--notice-border);
    border-radius: var(--radius-sm);
    font-size: 13px; font-weight: 600;
    display: none; align-items: center; gap: 4px;
    white-space: nowrap; z-index: 1;
}
.endpoint-incompatible-notice::before {
    content: '\26A0'; font-size: 14px;
}
.endpoint-header {
    position: relative;
    display: flex; align-items: center; justify-content: space-between;
    padding: 12px 16px; cursor: pointer; user-select: none;
}
.endpoint-header:hover { background: var(--bg-secondary); }
.endpoint-title { display: flex; align-items: center; gap: 10px; flex: 1; }
.endpoint-method {
    padding: 2px 8px;
    border-radius: var(--radius-sm); font-size: 13px; font-weight: 600;
    font-family: var(--font-mono); min-width: 44px; text-align: center;
}
.endpoint-method.call {
    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: #fff;
}
.endpoint-method.long-conn { background: linear-gradient(135deg, #f093fb 0%, #f5576c 100%); color: #fff; }
.endpoint-method.get { background: var(--success); color: #fff; }
.endpoint-method.post { background: var(--accent); color: #fff; }
.endpoint-method.put { background: var(--warning); color: #000; }
.endpoint-method.delete { background: var(--danger); color: #fff; }
.endpoint-method.patch { background: var(--warning); color: #000; }
.endpoint-name { font-weight: 600; font-size: 16px; }
.endpoint-desc { color: var(--text-secondary); font-size: 15px; }
.endpoint-toggle {
    color: var(--text-muted); font-size: 14px; transition: transform 0.2s;
}
.endpoint-toggle.open { transform: rotate(90deg); }
.endpoint-body { padding: 0 16px 16px; display: none; }
.endpoint-body.open { display: block; }
.group-section { margin-bottom: 16px; }
.group-section .group-title {
    font-size: 18px; font-weight: 600; padding: 8px 0;
    border-bottom: 1px solid var(--border-color); margin-bottom: 8px;
    color: var(--text-primary); cursor: pointer; display: flex;
    align-items: center; gap: 6px; user-select: none;
}
.group-section .group-title:hover { color: var(--accent); }
.group-section .group-title .group-arrow { font-size: 12px; transition: transform 0.2s; color: var(--text-muted); }
.group-section .group-title .group-arrow.collapsed { transform: rotate(-90deg); }
.params-table { width: 100%; border-collapse: collapse; margin: 12px 0; font-size: 16px; }
.params-table th {
    text-align: left; padding: 6px 10px; border-bottom: 2px solid var(--border-color);
    color: var(--text-secondary); font-weight: 500; font-size: 14px;
}
.params-table td { padding: 6px 10px; border-bottom: 1px solid var(--border-color); }
.params-table code {
    background: var(--code-bg); padding: 1px 5px; border-radius: 3px;
    font-family: var(--font-mono); font-size: 14px;
}
.params-table code[style*="pointer"]:hover {
    background: var(--primary); color: #fff;
}
.return-info { margin-top: 8px; font-size: 15px; }
.af-field-group {
    border: 1px solid var(--border-color); border-radius: var(--radius-sm);
    padding: 10px 12px; margin: 6px 0; background: var(--bg-secondary); width: 100%; box-sizing: border-box;
}
.af-field-group .field-label {
    font-size: 14px; font-weight: 500; color: var(--text-secondary);
    margin-bottom: 4px; display: flex; align-items: center; gap: 6px;
}
.af-field-group .field-label .field-type {
    font-family: var(--font-mono); font-size: 13px; color: var(--text-muted);
}
.af-field-group .field-desc {
    font-size: 13px; color: var(--text-muted); font-style: italic;
}
.af-field-group .field-row { display: flex; align-items: center; gap: 8px; }
af-field, af-array { display: block; width: 100%; }
input[type="text"], input[type="number"], textarea, select {
    background: var(--bg-primary); border: 1px solid var(--border-color);
    border-radius: var(--radius-sm); padding: 6px 10px; color: var(--text-primary);
    font-size: 15px; width: 100%; font-family: inherit;
}
input:focus, textarea:focus, select:focus {
    outline: none; border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-bg);
}
input[type="checkbox"] { width: auto; }
.btn {
    padding: 7px 16px; border-radius: var(--radius-sm); border: none;
    font-size: 15px; font-weight: 500; cursor: pointer; transition: all 0.15s;
}
.btn-primary { background: var(--accent); color: #fff; }
.btn-primary:hover { background: var(--accent-hover); }
.btn-primary:disabled { opacity: 0.7; cursor: not-allowed; }
.btn-loading { position: relative; color: transparent !important; pointer-events: none; }
.btn-loading::after {
    content: '';
    position: absolute;
    width: 14px; height: 14px;
    top: 50%; left: 50%;
    margin: -7px 0 0 -7px;
    border: 2px solid rgba(255,255,255,0.3);
    border-top-color: #fff;
    border-radius: 50%;
    animation: afast-spin 0.6s linear infinite;
}
@keyframes afast-spin { to { transform: rotate(360deg); } }
.btn-secondary { background: var(--bg-tertiary); color: var(--text-primary); border: 1px solid var(--border-color); }
.btn-secondary:hover { background: var(--border-color); }
.btn-danger { background: var(--danger); color: #fff; }
.btn-danger:hover { opacity: 0.9; }
.btn-sm { padding: 4px 10px; font-size: 14px; }
.btn:disabled { opacity: 0.5; cursor: not-allowed; }
.btn-edit-json {
    background: none; border: 1px solid var(--border-color); color: var(--text-muted);
    padding: 6px 10px; font-size: 15px; cursor: pointer; border-radius: var(--radius-sm);
    height: 35px; display: inline-flex; align-items: center; justify-content: center;
    flex-shrink: 0;
}
.btn-edit-json:hover { color: var(--accent); border-color: var(--accent); }
.collapsible-header {
    display: flex; align-items: center; justify-content: space-between;
    cursor: pointer; user-select: none;
}
.collapsible-header h2 { margin: 0; }
.collapsible-arrow { transition: transform 0.2s; font-size: 14px; color: var(--text-muted); }
.collapsible-arrow.open { transform: rotate(90deg); }
.collapsible-body { overflow: hidden; transition: max-height 0.2s ease; }
.btn-row { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
.response-panel {
    margin-top: 12px; border: 1px solid var(--border-color);
    border-radius: var(--radius-sm); overflow: hidden;
}
.response-header {
    padding: 8px 12px; background: var(--bg-secondary); font-size: 14px;
    font-weight: 500; display: flex; justify-content: space-between;
}
.response-body {
    padding: 12px; font-family: var(--font-mono); font-size: 14px;
    white-space: pre-wrap; word-break: break-all; max-height: 400px; overflow-y: auto;
    background: var(--bg-primary); margin: 0;
}
.response-success { color: var(--success); }
.response-error { color: var(--danger); }
.chat-panel { margin-top: 12px; }
.chat-header {
    display: flex; align-items: center; gap: 10px; margin-bottom: 8px;
}
.chat-status { font-size: 15px; }
.chat-log {
    border: 1px solid var(--border-color); border-radius: var(--radius-sm);
    height: 250px; overflow-y: auto; padding: 10px; margin-bottom: 8px;
    background: var(--bg-primary); font-family: var(--font-mono); font-size: 14px;
}
.chat-message { padding: 3px 0; word-break: break-all; }
.chat-sent { color: var(--accent); }
.chat-received { color: var(--success); }
.chat-system { color: var(--text-muted); font-style: italic; }
.chat-error { color: var(--danger); }
.chat-input-area { display: flex; gap: 8px; }
.chat-input-area input { flex: 1; }
.modal-overlay {
    display: none; position: fixed; top: 0; left: 0; right: 0; bottom: 0;
    background: rgba(0,0,0,0.5); z-index: 200; align-items: center; justify-content: center;
}
.modal-overlay.open { display: flex; }
.modal-content {
    background: var(--bg-card); border: 1px solid var(--border-color);
    border-radius: var(--radius); padding: 20px; width: 90%; max-width: 600px;
    box-shadow: var(--shadow-lg);
}
.modal-content h3 { margin-bottom: 12px; font-size: 18px; }
.modal-content textarea {
    width: 100%; height: 200px; font-family: var(--font-mono); font-size: 14px;
    resize: vertical;
}
.modal-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 12px; }
.custom-group { margin-bottom: 16px; }
.custom-group h3 { font-size: 16px; margin-bottom: 6px; }
.service-card {
    display: block; padding: 20px; background: var(--bg-card);
    border: 1px solid var(--border-color); border-radius: var(--radius);
    text-decoration: none; color: var(--text-primary); margin-bottom: 12px;
    box-shadow: var(--shadow); transition: box-shadow 0.15s;
}
.service-card:hover { box-shadow: var(--shadow-lg); border-color: var(--accent); }
.service-card h2 { font-size: 20px; margin-bottom: 4px; }
.service-card p { color: var(--text-secondary); font-size: 16px; }
.service-desc { color: var(--text-secondary); font-size: 15px; margin: 6px 0 2px; }
.empty-state { text-align: center; padding: 40px; color: var(--text-muted); }
.field-hint { font-size: 13px; color: var(--text-muted); margin-top: 2px; line-height: 1.3; }
.field-hint.field-hint-error { color: var(--danger); }
::-webkit-scrollbar { width: 6px; height: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: var(--border-color); border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }
* { scrollbar-width: thin; scrollbar-color: var(--border-color) transparent; }
"#;

// ─── JS: WebComponent + UI Logic ──────────────────────────────────
// Embedded JavaScript that defines custom elements (af-field, af-array)
// and the main application logic.  This code runs entirely in the
// browser and is not processed by the Rust compiler — it is emitted
// verbatim inside a <script> tag.

const UI_JS: &str = r#"
// ─── af-field WebComponent ────────────────────────────────────
class AfField extends HTMLElement {
    constructor() {
        super();
        this._schema = null;
        this._allTypes = null;
        this._inputs = {};
        this._enumSelect = null;
        this._variantContainers = {};
}

    connectedCallback() { this.render(); }

    set schema(s) { this._schema = s; }
    set allTypes(t) { this._allTypes = t; }

    render() {
        if (!this._schema) return;
        this.innerHTML = '';
        this._inputs = {};
        this._variantContainers = {};

        if (this._schema.kind === 'struct') {
            this._renderStruct(this._schema.fields, this);
        } else if (this._schema.kind === 'enum') {
            this._renderEnum(this._schema.variants, this);
        }
}

    _renderStruct(fields, container) {
        for (const field of fields) {
            const group = document.createElement('div');
            group.className = 'af-field-group';

            const label = document.createElement('div');
            label.className = 'field-label';
            let labelHtml = field.name + ' <span class="field-type">' + field.ty + '</span>';
            if (field.desc) {
                labelHtml += ' <span class="field-desc">' + field.desc + '</span>';
            }
            label.innerHTML = labelHtml;
            group.appendChild(label);

            const row = document.createElement('div');
            row.className = 'field-row';

            if (field.structure && this._allTypes && this._allTypes[field.structure]) {
                const sub = document.createElement('af-field');
                sub._schema = this._allTypes[field.structure];
                sub._allTypes = this._allTypes;
                sub.setAttribute('data-field', field.name);
                row.appendChild(sub);
                this._inputs[field.name] = { type: 'af-field', el: sub };
            } else if (field.ty.startsWith('Vec<') && !field.ty.startsWith('Vec<u8>')) {
                const inner = field.ty.slice(4, -1);
                const arr = document.createElement('af-array');
                arr.setAttribute('inner-type', inner);
                arr.setAttribute('data-field', field.name);
                if (this._allTypes) arr._allTypes = this._allTypes;
                row.appendChild(arr);
                this._inputs[field.name] = { type: 'af-array', el: arr };
            } else if (field.ty.startsWith('Option<')) {
                const inner = field.ty.slice(7, -1);
                const optWrap = document.createElement('div');
                optWrap.style.display = 'flex';
                optWrap.style.gap = '6px';
                optWrap.style.alignItems = 'center';
                const cb = document.createElement('input');
                cb.type = 'checkbox';
                cb.checked = false;
                cb.title = 'null?';
                const inp = this._createInputForType(inner, field.name);
                inp.style.display = 'none';
                cb.addEventListener('change', () => { inp.style.display = cb.checked ? '' : 'none'; });
                optWrap.appendChild(cb);
                inp.style.flex = '1';
                optWrap.appendChild(inp);
                row.appendChild(optWrap);
                this._inputs[field.name] = { type: 'option', checkbox: cb, el: inp, inner };
            } else {
                const inp = this._createInputForType(field.ty, field.name);
                row.appendChild(inp);
                this._inputs[field.name] = { type: 'primitive', el: inp };
            }

            // Edit JSON button
            const editBtn = document.createElement('button');
            editBtn.className = 'btn-edit-json';
            editBtn.textContent = '{}';
            editBtn.title = 'Edit JSON';
            const self = this;
            editBtn.addEventListener('click', function(e) {
                e.stopPropagation();
                openFieldJsonModal(self, field.name);
            });
            row.appendChild(editBtn);

            group.appendChild(row);

            if (field.validations && field.validations.length > 0) {
                const hint = document.createElement('div');
                hint.className = 'field-hint';
                hint.textContent = formatValidationHint(field.validations);
                group.appendChild(hint);

                const info = this._inputs[field.name];
                const checkFn = () => {
                    let val = '';
                    if (info) {
                        if (info.type === 'primitive') val = info.el.value;
                        else if (info.type === 'option') val = info.checkbox.checked ? info.el.value : '';
                    }
                    hint.classList.toggle('field-hint-error', !validateValue(val, field.validations));
                };
                if (info && info.type === 'primitive') {
                    info.el.addEventListener('input', checkFn);
                } else if (info && info.type === 'option') {
                    info.el.addEventListener('input', checkFn);
                    info.checkbox.addEventListener('change', checkFn);
                }
            }

            container.appendChild(group);
        }
}

    _renderEnum(variants, container) {
        const sel = document.createElement('select');
        this._enumSelect = sel;
        this._variantContainers = {};

        for (let i = 0; i < variants.length; i++) {
            const opt = document.createElement('option');
            opt.value = i;
            opt.textContent = variants[i].name;
            sel.appendChild(opt);
        }

        container.appendChild(sel);

        const variantWrap = document.createElement('div');
        variantWrap.style.marginTop = '6px';

        for (let i = 0; i < variants.length; i++) {
            const v = variants[i];
            const vDiv = document.createElement('div');
            vDiv.className = 'variant-fields';
            vDiv.style.display = i === 0 ? '' : 'none';

            if (v.fields.length > 0) {
                for (const f of v.fields) {
                    const group = document.createElement('div');
                    group.className = 'af-field-group';
                    const label = document.createElement('div');
                    label.className = 'field-label';
                    let labelHtml = f.name + ' <span class="field-type">' + f.ty + '</span>';
                    if (f.desc) {
                        labelHtml += ' <span class="field-desc">' + f.desc + '</span>';
                    }
                    label.innerHTML = labelHtml;
                    group.appendChild(label);
                    const row = document.createElement('div');
                    row.className = 'field-row';
                    let inpEl = null;
                    if (f.structure && this._allTypes && this._allTypes[f.structure]) {
                        const sub = document.createElement('af-field');
                        sub._schema = this._allTypes[f.structure];
                        sub._allTypes = this._allTypes;
                        row.appendChild(sub);
                    } else {
                        inpEl = this._createInputForType(f.ty, f.name);
                        row.appendChild(inpEl);
                    }
                    group.appendChild(row);

                    if (f.validations && f.validations.length > 0 && inpEl) {
                        const hint = document.createElement('div');
                        hint.className = 'field-hint';
                        hint.textContent = formatValidationHint(f.validations);
                        group.appendChild(hint);
                        inpEl.addEventListener('input', () => {
                            hint.classList.toggle('field-hint-error', !validateValue(inpEl.value, f.validations));
                        });
                    }

                    vDiv.appendChild(group);
                }
            } else {
                const info = document.createElement('div');
                info.className = 'text-muted';
                info.style.padding = '4px 0';
                info.textContent = '(unit variant)';
                vDiv.appendChild(info);
            }

            this._variantContainers[i] = vDiv;
            variantWrap.appendChild(vDiv);
        }

        sel.addEventListener('change', () => {
            for (const [k, v] of Object.entries(this._variantContainers)) {
                v.style.display = k === sel.value ? '' : 'none';
            }
        });

        container.appendChild(variantWrap);
}

    _createInputForType(ty, name) {
        const step = (ty === 'f32' || ty === 'f64') ? ' step="any"' : '';
        const inputType = (ty === 'bool') ? 'checkbox' : (ty.match(/^(i|u)(8|16|32|64)$/) || ty === 'usize' || ty === 'f32' || ty === 'f64') ? 'number' : 'text';
        if (ty === 'Vec<u8>') {
            const ta = document.createElement('textarea');
            ta.placeholder = 'hex or text';
            ta.rows = 2;
            return ta;
        }
        const inp = document.createElement('input');
        inp.type = inputType;
        if (step && inputType === 'number') inp.setAttribute('step', 'any');
        inp.placeholder = ty;
        return inp;
}

    getValue() {
        if (!this._schema) return null;
        if (this._schema.kind === 'struct') {
            const obj = {};
            for (const [name, info] of Object.entries(this._inputs)) {
                if (info.type === 'af-field') {
                    obj[name] = info.el.getValue();
                } else if (info.type === 'af-array') {
                    obj[name] = info.el.getValue();
                } else if (info.type === 'option') {
                    if (info.checkbox.checked) {
                        obj[name] = this._parseInputValue(info.el, info.inner);
                    } else {
                        obj[name] = null;
                    }
                } else {
                    obj[name] = this._parseInputValue(info.el, null);
                }
            }
            return obj;
        }
        if (this._schema.kind === 'enum') {
            const idx = parseInt(this._enumSelect.value);
            const variant = this._schema.variants[idx];
            if (variant.fields.length === 0) return { tag: variant.name, data: null };
            if (variant.fields.length === 1 && variant.fields[0].name.startsWith('__')) {
                const vDiv = this._variantContainers[idx];
                const inp = vDiv.querySelector('input, textarea');
                return { tag: variant.name, data: this._parseInputValue(inp, variant.fields[0].ty) };
            }
            const data = {};
            const vDiv = this._variantContainers[idx];
            const inputs = vDiv.querySelectorAll('.af-field-group');
            inputs.forEach((g, fi) => {
                const f = variant.fields[fi];
                const inp = g.querySelector('input, textarea, af-field, af-array');
                if (inp && inp.tagName === 'AF-FIELD') {
                    data[f.name] = inp.getValue();
                } else if (inp && inp.tagName === 'AF-ARRAY') {
                    data[f.name] = inp.getValue();
                } else {
                    data[f.name] = this._parseInputValue(inp, f.ty);
                }
            });
            return { tag: variant.name, data };
        }
        return null;
}

    setValue(obj) {
        if (!this._schema || !obj) return;
        if (this._schema.kind === 'struct') {
            for (const [name, info] of Object.entries(this._inputs)) {
                if (obj[name] === undefined) continue;
                if (info.type === 'af-field') {
                    info.el.setValue(obj[name]);
                } else if (info.type === 'af-array') {
                    info.el.setValue(obj[name]);
                } else if (info.type === 'option') {
                    if (obj[name] === null) {
                        info.checkbox.checked = false;
                        info.el.style.display = 'none';
                    } else {
                        info.checkbox.checked = true;
                        info.el.style.display = '';
                        this._setInputValue(info.el, obj[name], info.inner);
                    }
                } else {
                    this._setInputValue(info.el, obj[name], null);
                }
            }
        }
        if (this._schema.kind === 'enum' && obj.tag) {
            const idx = this._schema.variants.findIndex(v => v.name === obj.tag);
            if (idx >= 0) {
                this._enumSelect.value = idx;
                this._enumSelect.dispatchEvent(new Event('change'));
                if (obj.data !== null && obj.data !== undefined) {
                    const variant = this._schema.variants[idx];
                    if (variant.fields.length === 1 && variant.fields[0].name.startsWith('__')) {
                        const vDiv = this._variantContainers[idx];
                        const inp = vDiv.querySelector('input, textarea');
                        this._setInputValue(inp, obj.data, variant.fields[0].ty);
                    } else if (typeof obj.data === 'object') {
                        const vDiv = this._variantContainers[idx];
                        const groups = vDiv.querySelectorAll('.af-field-group');
                        groups.forEach((g, fi) => {
                            if (fi < variant.fields.length && obj.data[variant.fields[fi].name] !== undefined) {
                                const inp = g.querySelector('input, textarea');
                                this._setInputValue(inp, obj.data[variant.fields[fi].name], variant.fields[fi].ty);
                            }
                        });
                    }
                }
            }
        }
}

    _parseInputValue(el, ty) {
        if (!el) return null;
        if (el.type === 'checkbox') return el.checked;
        if (el.tagName === 'TEXTAREA') return el.value;
        if (ty === 'i64' || ty === 'u64') return parseInt(el.value) || 0;
        if (ty === 'f32' || ty === 'f64') return parseFloat(el.value) || 0;
        if (el.type === 'number') return parseInt(el.value) || 0;
        return el.value || '';
}

    _setInputValue(el, val, ty) {
        if (!el) return;
        if (el.type === 'checkbox') { el.checked = !!val; return; }
        el.value = val !== null && val !== undefined ? val : '';
}

}
customElements.define('af-field', AfField);

// ─── Validation helpers ───────────────────────────────────────
function formatValidationHint(validations) {
    if (!validations || validations.length === 0) return '';
    return validations.map(r => {
        switch (r.rule) {
            case 'gt': return '> ' + r.value;
            case 'gte': return '>= ' + r.value;
            case 'lt': return '< ' + r.value;
            case 'lte': return '<= ' + r.value;
            case 'len': return 'len ' + r.min + '-' + r.max;
            case 'of': return 'one of: ' + r.values.join(', ');
            default: return r.message || '';
        }
    }).join(', ');
}

function validateValue(val, validations) {
    if (!validations || validations.length === 0) return true;
    if (val === '' || val === null || val === undefined) return true;
    for (const r of validations) {
        switch (r.rule) {
            case 'gt': if (!(Number(val) > r.value)) return false; break;
            case 'gte': if (!(Number(val) >= r.value)) return false; break;
            case 'lt': if (!(Number(val) < r.value)) return false; break;
            case 'lte': if (!(Number(val) <= r.value)) return false; break;
            case 'len': {
                const s = String(val);
                if (s.length < r.min || s.length > r.max) return false;
                break;
            }
            case 'of': if (!r.values.includes(String(val))) return false; break;
        }
}
    return true;
}

// ─── Global JSON edit modal for af-field ──────────────────────
function openFieldJsonModal(afFieldEl, fieldName) {
    const overlay = document.getElementById('json-modal');
    const textarea = document.getElementById('json-modal-text');
    const applyBtn = document.getElementById('json-modal-apply');
    if (!overlay || !textarea || !applyBtn) return;

    const current = afFieldEl.getValue();
    textarea.value = JSON.stringify(fieldName ? current[fieldName] : current, null, 2);
    overlay.classList.add('open');

    const onApply = () => {
        try {
            const parsed = JSON.parse(textarea.value);
            if (fieldName) {
                const info = afFieldEl._inputs[fieldName];
                if (info && info.type === 'af-field') {
                    info.el.setValue(parsed);
                } else if (info && info.type === 'af-array') {
                    info.el.setValue(parsed);
                } else if (info && info.type === 'option') {
                    if (parsed === null) {
                        info.checkbox.checked = false;
                        info.el.style.display = 'none';
                    } else {
                        info.checkbox.checked = true;
                        info.el.style.display = '';
                        afFieldEl._setInputValue(info.el, parsed, info.inner);
                    }
                } else if (info) {
                    afFieldEl._setInputValue(info.el, parsed, null);
                }
            } else {
                afFieldEl.setValue(parsed);
            }
        } catch(e) {
            alert('Invalid JSON: ' + e.message);
        }
        overlay.classList.remove('open');
        cleanup();
    };

    const onClose = () => { overlay.classList.remove('open'); cleanup(); };
    const cleanup = () => {
        applyBtn.removeEventListener('click', onApply);
        document.getElementById('json-modal-close').removeEventListener('click', onClose);
        overlay.removeEventListener('click', onBgClick);
    };
    const onBgClick = (e) => { if (e.target === overlay) onClose(); };

    applyBtn.addEventListener('click', onApply);
    document.getElementById('json-modal-close').addEventListener('click', onClose);
    overlay.addEventListener('click', onBgClick);
}

// ─── af-array WebComponent ────────────────────────────────────
class AfArray extends HTMLElement {
    constructor() {
        super();
        this._items = [];
        this._innerType = '';
        this._allTypes = null;
}

    connectedCallback() {
        this._innerType = this.getAttribute('inner-type') || '';
        this.render();
}

    set allTypes(t) { this._allTypes = t; }

    render() {
        this.innerHTML = '';
        const wrap = document.createElement('div');

        this._items.forEach((item, idx) => {
            const row = document.createElement('div');
            row.style.display = 'flex';
            row.style.gap = '4px';
            row.style.marginBottom = '4px';
            row.style.alignItems = 'center';

            if (this._innerType && this._allTypes && this._allTypes[this._innerType]) {
                const sub = document.createElement('af-field');
                sub._schema = this._allTypes[this._innerType];
                sub._allTypes = this._allTypes;
                sub.style.flex = '1';
                row.appendChild(sub);
            } else {
                const inp = document.createElement('input');
                inp.type = (this._innerType === 'bool') ? 'checkbox' : 'text';
                inp.placeholder = this._innerType;
                inp.value = item !== undefined && item !== null ? item : '';
                inp.style.flex = '1';
                row.appendChild(inp);
            }

            const rm = document.createElement('button');
            rm.className = 'btn btn-sm btn-secondary';
            rm.textContent = '-';
            rm.addEventListener('click', () => { this._items.splice(idx, 1); this.render(); });
            row.appendChild(rm);

            wrap.appendChild(row);
        });

        const addBtn = document.createElement('button');
        addBtn.className = 'btn btn-sm btn-secondary';
        addBtn.textContent = '+ Add';
        addBtn.style.marginTop = '4px';
        addBtn.addEventListener('click', () => { this._items.push(''); this.render(); });
        wrap.appendChild(addBtn);

        this.appendChild(wrap);
}

    getValue() {
        const arr = [];
        const fields = this.querySelectorAll('.af-field, input');
        this._items.forEach((_, idx) => {
            if (idx < fields.length) {
                const el = fields[idx];
                if (el.tagName === 'AF-FIELD') {
                    arr.push(el.getValue());
                } else {
                    arr.push(el.value || '');
                }
            }
        });
        return arr;
}

    setValue(arr) {
        if (!Array.isArray(arr)) return;
        this._items = [...arr];
        this.render();
}
}
customElements.define('af-array', AfArray);

// ─── Main App Logic ──────────────────────────────────────────
(function() {
    const schema = JSON.parse(document.getElementById('schema-data').textContent);

    // Theme toggle
    const toggle = document.getElementById('theme-toggle');
    const savedTheme = localStorage.getItem('afast-doc-theme') || 'dark';
    document.documentElement.setAttribute('data-theme', savedTheme);
    toggle.textContent = savedTheme === 'dark' ? '☀️' : '🌙';
    toggle.addEventListener('click', () => {
        const current = document.documentElement.getAttribute('data-theme');
        const next = current === 'dark' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', next);
        toggle.textContent = next === 'dark' ? '☀️' : '🌙';
        localStorage.setItem('afast-doc-theme', next);
    });

    // Render customs panel
    const customsPanel = document.getElementById('customs-fields');
    if (schema.customs.length > 0) {
        for (const c of schema.customs) {
            const group = document.createElement('div');
            group.className = 'custom-group';
            const h3 = document.createElement('h3');
            h3.textContent = c.typeName;
            if (c.description) {
                const desc = document.createElement('span');
                desc.className = 'text-secondary';
                desc.textContent = ' — ' + c.description;
                h3.appendChild(desc);
            }
            group.appendChild(h3);

            const field = document.createElement('af-field');
            field._schema = schema.types[c.typeName] || { kind: 'struct', fields: [] };
            field._allTypes = schema.types;
            field.setAttribute('data-custom', 'true');
            field.setAttribute('type-name', c.typeName);
            group.appendChild(field);

            customsPanel.appendChild(group);
        }
        // Load from localStorage
        for (const c of schema.customs) {
            const field = customsPanel.querySelector('af-field[data-custom][type-name="' + c.typeName + '"]');
            const saved = localStorage.getItem('afast-custom-' + c.typeName);
            if (field && saved) {
                try { field.setValue(JSON.parse(saved)); } catch(e) {}
            }
        }
    } else {
        document.getElementById('customs-panel').style.display = 'none';
}

    // Customs collapsible toggle
    const customsToggle = document.getElementById('customs-toggle');
    const customsBody = document.getElementById('customs-body');
    const customsArrow = document.getElementById('customs-arrow');
    const customsStorageKey = 'afast-customs-collapsed-' + schema.serviceName;
    const savedCollapsed = localStorage.getItem(customsStorageKey);
    const startCollapsed = savedCollapsed === 'true';
    if (startCollapsed) {
        customsBody.style.display = 'none';
    } else {
        customsArrow.classList.add('open');
}
    customsToggle.addEventListener('click', () => {
        const hidden = customsBody.style.display === 'none';
        customsBody.style.display = hidden ? '' : 'none';
        customsArrow.classList.toggle('open', hidden);
        localStorage.setItem(customsStorageKey, String(!hidden));
    });

    // Save customs button
    document.getElementById('customs-save').addEventListener('click', () => {
        for (const c of schema.customs) {
            const field = customsPanel.querySelector('af-field[data-custom][type-name="' + c.typeName + '"]');
            if (field) {
                localStorage.setItem('afast-custom-' + c.typeName, JSON.stringify(field.getValue()));
            }
        }
        const btn = document.getElementById('customs-save');
        btn.textContent = 'Saved!';
        setTimeout(() => { btn.textContent = 'Save'; }, 1500);
    });

    // Gather custom values
    function gatherCustoms() {
        const vals = {};
        for (const c of schema.customs) {
            const field = customsPanel.querySelector('af-field[data-custom][type-name="' + c.typeName + '"]');
            if (field) vals[c.typeName] = field.getValue();
        }
        return vals;
}

    // Render headers panel
    const headersPanel = document.getElementById('headers-fields');
    if (schema.headers && schema.headers.length > 0) {
        for (const h of schema.headers) {
            const group = document.createElement('div');
            group.className = 'custom-group';
            const h3 = document.createElement('h3');
            h3.textContent = h.typeName;
            if (h.description) {
                const desc = document.createElement('span');
                desc.className = 'text-secondary';
                desc.textContent = ' — ' + h.description;
                h3.appendChild(desc);
            }
            group.appendChild(h3);
            const field = document.createElement('af-field');
            field._schema = schema.types[h.typeName] || { kind: 'struct', fields: [] };
            field._allTypes = schema.types;
            field.setAttribute('data-header', 'true');
            field.setAttribute('type-name', h.typeName);
            group.appendChild(field);
            headersPanel.appendChild(group);
            // Hide standard header fields — they are auto-set by the browser/fetch
            const fieldGroups = field.querySelectorAll('.af-field-group');
            fieldGroups.forEach(fg => {
                const label = fg.querySelector('.field-label');
                if (label) {
                    const fieldName = label.textContent.trim().split(' ')[0];
                    if (isStandardHeader(toHeaderName(fieldName))) {
                        fg.style.display = 'none';
                    }
                }
            });
        }
        for (const h of schema.headers) {
            const field = headersPanel.querySelector('af-field[data-header][type-name="' + h.typeName + '"]');
            const saved = localStorage.getItem('afast-header-' + h.typeName);
            if (field && saved) {
                try { field.setValue(JSON.parse(saved)); } catch(e) {}
            }
        }
    } else {
        document.getElementById('headers-panel').style.display = 'none';
    }

    const headersToggle = document.getElementById('headers-toggle');
    const headersBody = document.getElementById('headers-body');
    const headersArrow = document.getElementById('headers-arrow');
    const headersStorageKey = 'afast-headers-collapsed-' + schema.serviceName;
    const savedHeadersCollapsed = localStorage.getItem(headersStorageKey);
    if (savedHeadersCollapsed === 'true') {
        headersBody.style.display = 'none';
    } else {
        headersArrow.classList.add('open');
    }
    headersToggle.addEventListener('click', () => {
        const hidden = headersBody.style.display === 'none';
        headersBody.style.display = hidden ? '' : 'none';
        headersArrow.classList.toggle('open', hidden);
        localStorage.setItem(headersStorageKey, String(!hidden));
    });

    document.getElementById('headers-save').addEventListener('click', () => {
        for (const h of schema.headers) {
            const field = headersPanel.querySelector('af-field[data-header][type-name="' + h.typeName + '"]');
            if (field) {
                localStorage.setItem('afast-header-' + h.typeName, JSON.stringify(field.getValue()));
            }
        }
        const btn = document.getElementById('headers-save');
        btn.textContent = 'Saved!';
        setTimeout(() => { btn.textContent = 'Save'; }, 1500);
    });

    function gatherHeaders() {
        const vals = {};
        for (const h of (schema.headers || [])) {
            const field = headersPanel.querySelector('af-field[data-header][type-name="' + h.typeName + '"]');
            if (field) {
                const raw = field.getValue();
                if (raw && typeof raw === 'object' && !Array.isArray(raw)) {
                    const filtered = {};
                    for (const [k, v] of Object.entries(raw)) {
                        if (!isStandardHeader(toHeaderName(k))) filtered[k] = v;
                    }
                    vals[h.typeName] = filtered;
                }
            }
        }
        return vals;
    }

    function isStandardHeader(name) {
        return ['content-type','content-length','accept','accept-encoding','accept-language',
            'user-agent','host','connection','cache-control','pragma','origin','referer',
            'cookie','etag','if-none-match','if-modified-since','last-modified','server',
            'date','vary','allow','location','content-disposition','content-encoding',
            'transfer-encoding','upgrade','via','warning','dnt','sec-fetch-dest',
            'sec-fetch-mode','sec-fetch-site','sec-fetch-user'].includes(name);
    }

    function toHeaderName(name) {
        return name.replace(/_/g, '-');
    }

    // ─── Server connection settings ─────────────────────────────
    const LS = (key, def) => localStorage.getItem('afast-doc-' + key) || def;
    const transportSelect = document.getElementById('transport-select');
    const secureCheck = document.getElementById('secure-check');
    const hostInput = document.getElementById('host-input');
    const portInput = document.getElementById('port-input');
    const connectBtn = document.getElementById('connect-btn');
    const disconnectBtn = document.getElementById('disconnect-btn');
    const headerInputs = [transportSelect, secureCheck, hostInput, portInput];

    // Load saved settings
    transportSelect.value = LS('transport', 'ws');
    secureCheck.checked = LS('secure', window.location.protocol === 'https:' ? 'true' : '') === 'true';
    hostInput.value = LS('host', window.location.hostname || 'localhost');
    portInput.value = LS('port', transportSelect.value === 'ws' ? '3000' : '5000');

    function buildUrl() {
        const proto = transportSelect.value === 'ws'
            ? (secureCheck.checked ? 'wss' : 'ws')
            : (secureCheck.checked ? 'https' : 'http');
        const host = hostInput.value.trim() || 'localhost';
        const port = portInput.value.trim();
        return proto + '://' + host + (port ? ':' + port : '');
}

    function setConnectedState(connected) {
        headerInputs.forEach(el => el.disabled = connected);
        connectBtn.classList.toggle('hidden', connected);
        const isHttp = transportSelect.value === 'fetch';
        if (isHttp) {
            disconnectBtn.classList.add('hidden');
        } else {
            disconnectBtn.classList.toggle('hidden', !connected);
        }
}

    function updateConnectLabel() {
        const isHttp = transportSelect.value === 'fetch';
        connectBtn.textContent = isHttp ? 'Save' : 'Connect';
        if (isHttp) disconnectBtn.classList.add('hidden');
}

    transportSelect.addEventListener('change', () => {
        const newPort = transportSelect.value === 'ws' ? '3000' : '5000';
        portInput.placeholder = newPort;
        portInput.value = newPort;
        saveSettings();
        updateConnectLabel();
        updateLongConnHandlers();
        updateOrdinaryHandlers();
    });

    // Set initial button label based on loaded transport
    updateConnectLabel();

    // ─── Client management ──────────────────────────────────────
    const customFns = {};
    for (const c of schema.customs) {
        const typeName = c.typeName;
        customFns[typeName] = async () => gatherCustoms()[typeName] || {};
}
    const headerFns = {};
    for (const h of (schema.headers || [])) {
        const typeName = h.typeName;
        headerFns[typeName] = async () => gatherHeaders()[typeName] || {};
    }

    let client = null;

    function disconnectClient() {
        if (client) {
            try { if (client._ws) client._ws.close(); } catch(e) {}
            client = null;
        }
        setConnectedState(false);
}

    function createClient() {
        disconnectClient();
        const transport = transportSelect.value;
        const host = hostInput.value.trim() || 'localhost';
        const port = parseInt(portInput.value.trim()) || (transport === 'ws' ? 3000 : 5000);
        const tls = secureCheck.checked;

        try {
            for (const key of Object.keys(window)) {
                if (key.endsWith('Client') && typeof window[key] === 'function') {
                    const opts = { host, port, tls, transport };
                    if (schema.customs && schema.customs.length > 0) opts.customs = customFns;
                    if (schema.headers && schema.headers.length > 0) opts.headers = headerFns;
                    client = new window[key](opts);
                    console.log('afast: client created —', key, transport, host + ':' + port);
                    break;
                }
            }
            if (!client) { console.error('afast: no client class found on window'); return; }
        } catch(e) {
            console.error('afast: failed to create client:', e);
            return;
        }

        // Lock UI immediately for WS transport
        if (transport === 'ws') {
            setConnectedState(true);
        }
        updateLongConnHandlers();
        updateOrdinaryHandlers();
}

    function saveSettings() {
        localStorage.setItem('afast-doc-transport', transportSelect.value);
        localStorage.setItem('afast-doc-secure', secureCheck.checked);
        localStorage.setItem('afast-doc-host', hostInput.value.trim());
        localStorage.setItem('afast-doc-port', portInput.value.trim());
}

    function updateLongConnHandlers() {
        const isHttp = transportSelect.value === "fetch";
        document.querySelectorAll(".endpoint").forEach(el => {
            if (el.dataset.longConn === "true") {
                const header = el.querySelector(".endpoint-header");
                const tog = header ? header.querySelector(".endpoint-toggle") : null;
                if (isHttp) {
                    el.classList.add("endpoint-incompatible");
                    el.title = 'This handler requires WebSocket transport. Switch to ws in the top-right settings.';
                    el.querySelectorAll(".endpoint-body button, .endpoint-body input, .endpoint-body af-field").forEach(c => c.disabled = true);
                    if (header) {
                        let notice = header.querySelector(".endpoint-incompatible-notice");
                        if (!notice) {
                            notice = document.createElement("span");
                            notice.className = "endpoint-incompatible-notice";
                            notice.textContent = 'WebSocket required \u{2014} switch to ws';
                            if (tog) header.insertBefore(notice, tog);
                            else header.appendChild(notice);
                        }
                        notice.style.display = "inline-flex";
                    }
                } else {
                    el.classList.remove("endpoint-incompatible");
                    el.title = "";
                    el.querySelectorAll(".endpoint-body button, .endpoint-body input, .endpoint-body af-field").forEach(c => c.disabled = false);
                    const notice = el.querySelector(".endpoint-incompatible-notice");
                    if (notice) notice.style.display = "none";
                }
            }
        });
    }

    function updateOrdinaryHandlers() {
        const isWs = transportSelect.value === "ws";
        document.querySelectorAll(".endpoint").forEach(el => {
            if (el.dataset.ordinary === "true") {
                const header = el.querySelector(".endpoint-header");
                const tog = header ? header.querySelector(".endpoint-toggle") : null;
                if (isWs) {
                    el.classList.add("endpoint-incompatible");
                    el.title = 'This handler requires HTTP transport. Switch to fetch/http in the top-right settings.';
                    el.querySelectorAll(".endpoint-body button, .endpoint-body input, .endpoint-body af-field").forEach(c => c.disabled = true);
                    if (header) {
                        let notice = header.querySelector(".endpoint-incompatible-notice");
                        if (!notice) {
                            notice = document.createElement("span");
                            notice.className = "endpoint-incompatible-notice";
                            notice.textContent = 'HTTP required \u{2014} switch to fetch/http';
                            if (tog) header.insertBefore(notice, tog);
                            else header.appendChild(notice);
                        }
                        notice.style.display = "inline-flex";
                    }
                } else {
                    el.classList.remove("endpoint-incompatible");
                    el.title = "";
                    el.querySelectorAll(".endpoint-body button, .endpoint-body input, .endpoint-body af-field").forEach(c => c.disabled = false);
                    const notice = el.querySelector(".endpoint-incompatible-notice");
                    if (notice) notice.style.display = "none";
                }
            }
        });
    }

    // Connect button: save, create client
    connectBtn.addEventListener('click', () => {
        saveSettings();
        createClient();
    });

    // Disconnect button: close WS, unlock UI
    disconnectBtn.addEventListener('click', () => {
        disconnectClient();
    });

    // Auto-close WS on page unload
    window.addEventListener('beforeunload', () => {
        disconnectClient();
    });

    // ─── Shared group collapse state ────────────────────────────
    const groupState = {};  // path -> boolean (collapsed)
    const groupCallbacks = {};  // path -> [fn]

    function isGroupCollapsed(path) {
        if (!(path in groupState)) {
            groupState[path] = localStorage.getItem('afast-group-' + path) === 'true';
        }
        return groupState[path];
}

    function toggleGroup(path) {
        groupState[path] = !groupState[path];
        localStorage.setItem('afast-group-' + path, groupState[path]);
        // Notify all listeners (sidebar + endpoint)
        (groupCallbacks[path] || []).forEach(fn => fn(groupState[path]));
}

    function onGroupToggle(path, fn) {
        if (!groupCallbacks[path]) groupCallbacks[path] = [];
        groupCallbacks[path].push(fn);
}

    // Helper: display name (apiName if set, else name)
    function displayName(h) { return h.apiName || h.name; }

    // Helper: normalize Rust type string and unwrap Json<T>
    function displayReturnType(rt) {
        if (!rt || rt === '()') return 'void';
        var t = rt.trim();
        // Strip afast:: prefix
        if (t.startsWith('afast::')) t = t.slice(7);
        else if (t.startsWith('afast :: ')) t = t.slice(9);
        // Normalize spaces
        t = t.replace(/ </g, '<').replace(/< /g, '<').replace(/ >/g, '>').replace(/> /g, '>').replace(/ ,/g, ',').replace(/, /g, ',');
        // Unwrap Json<T>
        if (t.startsWith('Json<') && t.endsWith('>')) {
            t = t.slice(5, -1).trim();
        }
        return t;
    }

    // Helper: generate default value from type name using schema types
    const _types = schema.types;
    function defaultForType(ty, validations) {
        if (!ty || ty === '()') return null;
        if (ty === 'number' || ty === 'i8' || ty === 'i16' || ty === 'i32' || ty === 'i64'
            || ty === 'u8' || ty === 'u16' || ty === 'u32' || ty === 'u64' || ty === 'usize') {
            if (validations) {
                for (const r of validations) {
                    if (r.rule === 'gte') return r.value;
                    if (r.rule === 'gt') return r.value + 1;
                    if (r.rule === 'lte') return r.value;
                }
            }
            return 0;
        }
        if (ty === 'f32' || ty === 'f64') {
            if (validations) {
                for (const r of validations) {
                    if (r.rule === 'gte') return r.value;
                    if (r.rule === 'gt') return r.value + 0.1;
                    if (r.rule === 'lte') return r.value;
                }
            }
            return 0;
        }
        if (ty === 'boolean' || ty === 'bool') return false;
        if (ty === 'string' || ty === 'String' || ty === '&str') {
            if (validations) {
                for (const r of validations) {
                    if (r.rule === 'len') return 'x'.repeat(r.min);
                    if (r.rule === 'of' && r.values.length > 0) return r.values[0];
                }
            }
            return '';
        }
        if (ty === 'Uint8Array' || ty === 'Vec<u8>') return [];
        if (ty.startsWith('Vec<') || ty.endsWith('[]')) return [];
        if (ty.startsWith('Option<')) return null;
        const t = _types[ty];
        if (!t) return null;
        if (t.kind === 'struct') {
            const obj = {};
            for (const f of t.fields) obj[f.name] = defaultForType(f.ty, f.validations);
            return obj;
        }
        if (t.kind === 'enum') {
            const v = t.variants[0];
            if (!v) return null;
            if (v.fields.length === 0) return v.name;
            if (v.fields.length === 1) return { tag: v.name, data: defaultForType(v.fields[0].ty, v.fields[0].validations) };
            const data = {};
            for (const f of v.fields) data[f.name] = defaultForType(f.ty, f.validations);
            return { tag: v.name, data };
        }
        return null;
}

    // Helper: copy text to clipboard
    function copyToClipboard(text) {
        navigator.clipboard.writeText(text).then(() => {
            const toast = document.createElement('div');
            toast.textContent = 'Copied!';
            toast.style.cssText = 'position:fixed;top:20px;left:50%;transform:translateX(-50%);background:var(--success);color:#fff;padding:8px 20px;border-radius:6px;font-size:13px;z-index:9999;transition:opacity 0.3s;box-shadow:0 2px 8px rgba(0,0,0,0.2)';
            document.body.appendChild(toast);
            setTimeout(() => { toast.style.opacity = '0'; setTimeout(() => toast.remove(), 300); }, 1200);
        });
}

    // Helper: convert Rust type to TS type string
    function rustToTs(ty) {
        if (['i8','i16','i32','i64','u8','u16','u32','u64','usize','f32','f64'].includes(ty)) return 'number';
        if (ty === 'bool') return 'boolean';
        if (ty === 'String' || ty === '&str') return 'string';
        if (ty === 'Vec<u8>') return 'Uint8Array';
        if (ty.startsWith('Option<')) return rustToTs(ty.slice(7, -1)) + ' | null';
        if (ty.startsWith('Vec<')) return rustToTs(ty.slice(4, -1)) + '[]';
        return ty;
}

    // Helper: generate TS type definition string from schema type
    function typeToTs(typeName) {
        const t = schema.types[typeName];
        if (!t) return typeName;
        if (t.kind === 'struct') {
            const fields = t.fields.map(f => '    ' + f.name + ': ' + rustToTs(f.ty) + ';');
            return 'type ' + typeName + ' = {\n' + fields.join('\n') + '\n}';
        }
        if (t.kind === 'enum') {
            const variants = t.variants.map(v => {
                if (v.fields.length === 0) return '    { tag: \'' + v.name + '\', data: null }';
                if (v.fields.length === 1 && v.fields[0].name.startsWith('__')) return '    { tag: \'' + v.name + '\', data: ' + rustToTs(v.fields[0].ty) + ' }';
                const fields = v.fields.map(f => '        ' + f.name + ': ' + rustToTs(f.ty) + ';');
                return '    { tag: \'' + v.name + '\', data: {\n' + fields.join('\n') + '\n    } }';
            });
            return 'type ' + typeName + ' =\n' + variants.join(' |\n');
        }
        return typeName;
}

    // Build sidebar
    const sidebar = document.getElementById('sidebar');
    function buildSidebar(handlers, container, path, depth) {
        for (const h of handlers) {
            const groupPath = path ? path + '.' + displayName(h) : displayName(h);
            if (h.isGroup) {
                const label = document.createElement('div');
                label.className = 'group-label';
                label.style.paddingLeft = (12 + depth * 16) + 'px';
                const arrow = document.createElement('span');
                arrow.className = 'group-arrow';
                arrow.textContent = '▼';
                const nameSpan = document.createElement('span');
                nameSpan.textContent = displayName(h);
                label.appendChild(arrow);
                label.appendChild(nameSpan);
                container.appendChild(label);

                const childrenWrap = document.createElement('div');
                childrenWrap.className = 'group-children';

                if (h.children.length > 0) {
                    buildSidebar(h.children, childrenWrap, groupPath, depth + 1);
                }

                // Sync with shared state
                const collapsed = isGroupCollapsed(groupPath);
                if (collapsed) {
                    childrenWrap.classList.add('collapsed');
                    arrow.classList.add('collapsed');
                }

                onGroupToggle(groupPath, (c) => {
                    childrenWrap.classList.toggle('collapsed', c);
                    arrow.classList.toggle('collapsed', c);
                });

                label.addEventListener('click', () => toggleGroup(groupPath));
                container.appendChild(childrenWrap);
            } else {
                const a = document.createElement('a');
                a.href = '#endpoint-' + h.offset;
                a.textContent = displayName(h);
                a.style.paddingLeft = (20 + depth * 16) + 'px';
                a.addEventListener('click', (e) => {
                    e.preventDefault();
                    const el = document.getElementById('endpoint-' + h.offset);
                    if (el) {
                        el.scrollIntoView({ behavior: 'smooth', block: 'start' });
                        const body = document.getElementById('body-' + h.offset);
                        const toggleEl = document.getElementById('toggle-' + h.offset);
                        if (body && !body.classList.contains('open')) {
                            body.classList.add('open');
                            toggleEl.classList.add('open');
                        }
                    }
                });
                container.appendChild(a);
            }
        }
}
    buildSidebar(schema.handlers, sidebar, schema.serviceName, 0);

    // Render endpoints
    const endpointsContainer = document.getElementById('endpoints');
    function renderHandlers(handlers, container, depth, path) {
        for (const h of handlers) {
            const groupPath = path ? path + '.' + displayName(h) : displayName(h);
            if (h.isGroup) {
                const section = document.createElement('div');
                section.className = 'group-section';
                section.style.paddingLeft = (depth * 36) + 'px';
                const title = document.createElement('div');
                title.className = 'group-title';
                const arrow = document.createElement('span');
                arrow.className = 'group-arrow';
                arrow.textContent = '▼';
                title.appendChild(arrow);
                const nameSpan = document.createElement('span');
                nameSpan.textContent = displayName(h);
                title.appendChild(nameSpan);
                section.appendChild(title);
                const children = document.createElement('div');
                renderHandlers(h.children, children, depth + 1, groupPath);
                section.appendChild(children);
                container.appendChild(section);

                // Sync with shared state
                const collapsed = isGroupCollapsed(groupPath);
                if (collapsed) {
                    children.style.display = 'none';
                    arrow.classList.add('collapsed');
                }

                onGroupToggle(groupPath, (c) => {
                    children.style.display = c ? 'none' : '';
                    arrow.classList.toggle('collapsed', c);
                });

                title.addEventListener('click', () => toggleGroup(groupPath));
            } else {
                const card = document.createElement('div');
                card.className = 'endpoint';
                card.id = 'endpoint-' + h.offset;
                card.dataset.longConn = h.longConnection;
                card.dataset.ordinary = h.isOrdinary || false;

                // Header
                const header = document.createElement('div');
                header.className = 'endpoint-header';
                header.addEventListener('click', () => {
                    const body = document.getElementById('body-' + h.offset);
                    const tog = document.getElementById('toggle-' + h.offset);
                    body.classList.toggle('open');
                    tog.classList.toggle('open');
                });

                const titleDiv = document.createElement('div');
                titleDiv.className = 'endpoint-title';

                const method = document.createElement('span');
                if (h.isOrdinary) {
                    method.className = 'endpoint-method ' + (h.method || 'get').toLowerCase();
                    method.textContent = h.method || 'GET';
                } else {
                    method.className = 'endpoint-method' + (h.longConnection ? ' long-conn' : ' call');
                    method.textContent = h.longConnection ? 'Long' : 'Call';
                }
                titleDiv.appendChild(method);

                const name = document.createElement('span');
                name.className = 'endpoint-name';
                name.textContent = displayName(h);
                titleDiv.appendChild(name);

                if (h.description) {
                    const desc = document.createElement('span');
                    desc.className = 'endpoint-desc';
                    desc.textContent = h.description;
                    titleDiv.appendChild(desc);
                }

                header.appendChild(titleDiv);

                const tog = document.createElement('span');
                tog.className = 'endpoint-toggle';
                tog.id = 'toggle-' + h.offset;
                tog.textContent = '▶';
                header.appendChild(tog);

                card.appendChild(header);

                // Body
                const body = document.createElement('div');
                body.className = 'endpoint-body';
                body.id = 'body-' + h.offset;

                // Params table
                const dataParams = h.params.filter(p => p.extractor !== 'State' && p.extractor !== 'Receiver' && p.extractor !== 'Sender');
                if (dataParams.length > 0) {
                    const table = document.createElement('table');
                    table.className = 'params-table';
                    table.innerHTML = '<thead><tr><th>Name</th><th>Tag</th><th>Type</th><th>Extractor</th></tr></thead>';
                    const tbody = document.createElement('tbody');
                    for (const p of dataParams) {
                        const tr = document.createElement('tr');
                        const typeCode = document.createElement('code');
                        typeCode.textContent = p.ty;
                        typeCode.style.cursor = 'pointer';
                        typeCode.title = 'Click to copy TypeScript type';
                        typeCode.onclick = function() {
                            if (p.structure && schema.types[p.structure]) {
                                copyToClipboard(typeToTs(p.structure));
                            } else {
                                copyToClipboard(rustToTs(p.ty));
                            }
                        };
                        const tdName = document.createElement('td'); tdName.textContent = p.name;
                        const tdTag = document.createElement('td'); tdTag.textContent = p.tag || '';
                        const tdType = document.createElement('td'); tdType.appendChild(typeCode);
                        const tdExtractor = document.createElement('td');
                        const badge = document.createElement('span');
                        badge.className = 'badge badge-' + p.extractor.toLowerCase();
                        badge.textContent = p.extractor;
                        tdExtractor.appendChild(badge);
                        tr.appendChild(tdName);
                        tr.appendChild(tdTag);
                        tr.appendChild(tdType);
                        tr.appendChild(tdExtractor);
                        tbody.appendChild(tr);
                    }
                    table.appendChild(tbody);
                    body.appendChild(table);
                }

                // Return info
                if (h.returnType && h.returnType !== '()') {
                    const ret = document.createElement('div');
                    ret.className = 'return-info';
                    const isJsonReturn = h.returnType && h.returnType.startsWith('Json<');
                    if (h.returnStructure && schema.types[h.returnStructure] && (!h.isOrdinary || isJsonReturn)) {
                        const copyBtn = document.createElement('button');
                        copyBtn.className = 'btn btn-sm btn-secondary';
                        copyBtn.style.marginRight = '6px';
                        copyBtn.style.verticalAlign = 'middle';
                        copyBtn.textContent = 'Copy TS';
                        copyBtn.onclick = function() { copyToClipboard(typeToTs(h.returnStructure)); };
                        ret.appendChild(copyBtn);
                    }
                    const label = document.createElement('strong');
                    label.textContent = 'Returns: ';
                    ret.appendChild(label);
                    const code = document.createElement('code');
                    code.textContent = displayReturnType(h.returnType);
                    ret.appendChild(code);
                    if (h.returnStructure && schema.types[h.returnStructure]) {
                        const desc = document.createElement('span');
                        desc.className = 'text-secondary';
                        desc.textContent = ' — ' + (schema.types[h.returnStructure].desc || '');
                        ret.appendChild(desc);
                    }
                    body.appendChild(ret);
                }

                if (h.longConnection) {
                    // Chat panel
                    renderChatPanel(h, body);
                } else if (h.isOrdinary) {
                    renderOrdinaryHandler(h, body, path);
                } else {
                    // Data inputs
                    const dataInputs = h.params.filter(p => p.extractor === 'Data');
                    if (dataInputs.length > 0) {
                        const inputsDiv = document.createElement('div');
                        inputsDiv.style.marginTop = '12px';
                        for (const p of dataInputs) {
                            const inputGroup = document.createElement('div');
                            inputGroup.style.marginBottom = '8px';

                            // Header row: label + buttons on same line
                            const headerRow = document.createElement('div');
                            headerRow.style.cssText = 'display:flex;align-items:center;justify-content:space-between;margin-bottom:4px';
                            const label = document.createElement('div');
                            label.className = 'field-label';
                            label.style.marginBottom = '0';
                            label.innerHTML = p.name + ' <span class="field-type">' + p.ty + '</span>';
                            headerRow.appendChild(label);

                            const btnRow = document.createElement('div');
                            btnRow.style.cssText = 'display:flex;align-items:center;gap:6px';
                            const copyReqBtn = document.createElement('button');
                            copyReqBtn.className = 'btn btn-sm btn-secondary';
                            copyReqBtn.textContent = 'Copy Request';
                            copyReqBtn.addEventListener('click', () => {
                                const el = document.querySelector('[data-endpoint="' + h.offset + '"][data-param="' + p.name + '"]');
                                let value;
                                if (el && typeof el.getValue === 'function') {
                                    value = el.getValue();
                                }
                                if (value === undefined || value === null) {
                                    value = defaultForType(p.structure || p.ty);
                                }
                                copyToClipboard(JSON.stringify(value, null, 2));
                            });
                            btnRow.appendChild(copyReqBtn);
                            const applyBtn = document.createElement('button');
                            applyBtn.className = 'btn btn-sm btn-secondary';
                            applyBtn.textContent = 'Apply Data';
                            applyBtn.addEventListener('click', () => {
                                const modal = document.getElementById('json-modal');
                                const textarea = document.getElementById('json-modal-text');
                                const applyAction = document.getElementById('json-modal-apply');
                                const closeAction = document.getElementById('json-modal-close');
                                modal.classList.add('open');
                                textarea.value = '';
                                textarea.placeholder = 'Paste JSON here...';
                                textarea.style.borderColor = '';
                                const onApply = () => {
                                    try {
                                        const data = JSON.parse(textarea.value);
                                        const el2 = document.querySelector('[data-endpoint="' + h.offset + '"][data-param="' + p.name + '"]');
                                        if (el2 && typeof el2.setValue === 'function') {
                                            el2.setValue(data);
                                        } else if (el2) {
                                            if (el2.type === 'checkbox') el2.checked = !!data;
                                            else el2.value = typeof data === 'object' ? JSON.stringify(data) : data;
                                        }
                                        textarea.style.borderColor = '';
                                        modal.classList.remove('open');
                                        cleanup();
                                    } catch(e2) {
                                        textarea.style.borderColor = 'var(--error, #dc3545)';
                                        textarea.placeholder = 'Invalid JSON: ' + e2.message;
                                    }
                                };
                                const onClose = () => { modal.classList.remove('open'); cleanup(); };
                                const cleanup = () => {
                                    applyAction.removeEventListener('click', onApply);
                                    closeAction.removeEventListener('click', onClose);
                                };
                                applyAction.addEventListener('click', onApply);
                                closeAction.addEventListener('click', onClose);
                            });
                            btnRow.appendChild(applyBtn);
                            headerRow.appendChild(btnRow);
                            inputGroup.appendChild(headerRow);

                            if (p.structure && schema.types[p.structure]) {
                                const field = document.createElement('af-field');
                                field._schema = schema.types[p.structure];
                                field._allTypes = schema.types;
                                field.setAttribute('data-endpoint', h.offset);
                                field.setAttribute('data-param', p.name);
                                inputGroup.appendChild(field);
                            } else if (p.ty.startsWith('Vec<') && !p.ty.startsWith('Vec<u8>')) {
                                const inner = p.ty.slice(4, -1);
                                const arr = document.createElement('af-array');
                                arr.setAttribute('inner-type', inner);
                                arr._allTypes = schema.types;
                                arr.setAttribute('data-endpoint', h.offset);
                                arr.setAttribute('data-param', p.name);
                                inputGroup.appendChild(arr);
                            } else {
                                const inp = document.createElement('input');
                                inp.setAttribute('data-endpoint', h.offset);
                                inp.setAttribute('data-param', p.name);
                                inp.placeholder = p.ty;
                                if (p.ty === 'bool') inp.type = 'checkbox';
                                else if (/^(i|u)(8|16|32|64)$/.test(p.ty) || p.ty === 'usize' || p.ty === 'f32' || p.ty === 'f64') {
                                    inp.type = 'number';
                                    if (p.ty === 'f32' || p.ty === 'f64') inp.setAttribute('step', 'any');
                                }
                                inputGroup.appendChild(inp);
                            }
                            inputsDiv.appendChild(inputGroup);
                        }
                        body.appendChild(inputsDiv);
                    }

                    // Button row
                    const sendRow = document.createElement('div');
                    sendRow.style.marginTop = '12px';
                    sendRow.style.display = 'flex';
                    sendRow.style.alignItems = 'center';
                    sendRow.style.gap = '8px';

                    // Spacer
                    const spacer2 = document.createElement('div');
                    spacer2.style.flexGrow = '1';
                    sendRow.appendChild(spacer2);

                    // Send button (right)
                    const sendBtn = document.createElement('button');
                    sendBtn.className = 'btn btn-primary';
                    sendBtn.id = 'send-btn-' + h.offset;
                    sendBtn.textContent = 'Send';
                    sendBtn.addEventListener('click', () => sendRequest(h, sendBtn));
                    sendRow.appendChild(sendBtn);

                    body.appendChild(sendRow);

                    // Response panel
                    const respPanel = document.createElement('div');
                    respPanel.className = 'response-panel';
                    respPanel.id = 'response-' + h.offset;
                    respPanel.style.display = 'none';
                    const respHeader = document.createElement('div');
                    respHeader.className = 'response-header';
                    const respLabel = document.createElement('span');
                    respLabel.textContent = 'Response';
                    respHeader.appendChild(respLabel);
                    const respActions = document.createElement('div');
                    respActions.style.cssText = 'display:flex;align-items:center;gap:8px';
                    const respTime = document.createElement('span');
                    respTime.id = 'response-time-' + h.offset;
                    respActions.appendChild(respTime);
                    const respCopyBtn = document.createElement('button');
                    respCopyBtn.className = 'btn btn-sm btn-secondary';
                    respCopyBtn.textContent = 'Copy';
                    respCopyBtn.addEventListener('click', () => {
                        const rb = document.getElementById('response-body-' + h.offset);
                        if (rb) copyToClipboard(rb.textContent);
                    });
                    respActions.appendChild(respCopyBtn);
                    respHeader.appendChild(respActions);
                    respPanel.appendChild(respHeader);
                    const respBody = document.createElement('pre');
                    respBody.className = 'response-body';
                    respBody.id = 'response-body-' + h.offset;
                    respPanel.appendChild(respBody);
                    body.appendChild(respPanel);
                }

                card.appendChild(body);
                container.appendChild(card);
            }
        }
}
    renderHandlers(schema.handlers, endpointsContainer, 0, schema.serviceName);
    updateLongConnHandlers();
    updateOrdinaryHandlers();

    // Restore cached results
    (function restoreCache(handlers) {
        for (const h of handlers) {
            if (h.children && h.children.length > 0) restoreCache(h.children);
            if (h.isGroup) continue;
            const cached = localStorage.getItem('afast-result-' + h.path);
            if (!cached) continue;
            const respPanel = document.getElementById('response-' + h.offset);
            const respBody = document.getElementById('response-body-' + h.offset);
            if (!respPanel || !respBody) continue;
            respPanel.style.display = 'block';
            respBody.textContent = cached;
            respBody.className = 'response-body response-success';
        }
    })(schema.handlers);

    // Send request
    async function sendRequest(handler, sendBtn) {
        const respPanel = document.getElementById('response-' + handler.offset);
        const respBody = document.getElementById('response-body-' + handler.offset);
        const respTime = document.getElementById('response-time-' + handler.offset);

        respPanel.style.display = 'block';
        respBody.textContent = 'Loading...';
        respBody.className = 'response-body';
        respTime.textContent = '';

        // Auto-connect if client not ready
        if (!client) {
            saveSettings();
            createClient();
            if (!client) {
                respBody.textContent = 'Error: Failed to create client. Check server settings.';
                respBody.className = 'response-body response-error';
                return;
            }
        }

        // Show loading on send button
        if (sendBtn) {
            sendBtn.disabled = true;
            sendBtn.classList.add('btn-loading');
        }

        const start = performance.now();
        const cacheKey = 'afast-result-' + handler.path;

        try {
            // Gather data values
            const dataParams = handler.params.filter(p => p.extractor === 'Data');
            const args = [];
            for (const p of dataParams) {
                const el = document.querySelector('[data-endpoint="' + handler.offset + '"][data-param="' + p.name + '"]');
                if (el) {
                    if (el.tagName === 'AF-FIELD') {
                        args.push(el.getValue());
                    } else if (el.tagName === 'AF-ARRAY') {
                        args.push(el.getValue());
                    } else if (el.type === 'checkbox') {
                        args.push(el.checked);
                    } else if (el.type === 'number') {
                        args.push(parseFloat(el.value) || 0);
                    } else {
                        args.push(el.value || '');
                    }
                } else {
                    args.push(null);
                }
            }

            // Navigate to handler method
            const pathParts = (handler.apiPath || handler.path).split('.');
            let fn = client.apis;
            for (const part of pathParts) {
                fn = fn[part.replace(/^:/, '')];
            }

            const result = await fn(...args);
            const elapsed = (performance.now() - start).toFixed(1);

            respBody.textContent = JSON.stringify(result, null, 2);
            respBody.className = 'response-body response-success';
            respTime.textContent = elapsed + 'ms';

            // Cache result
            try { localStorage.setItem(cacheKey, JSON.stringify(result)); } catch(_) {}
        } catch(e) {
            const elapsed = (performance.now() - start).toFixed(1);
            respBody.textContent = 'Error: ' + e.message;
            respBody.className = 'response-body response-error';
            respTime.textContent = elapsed + 'ms';
        } finally {
            // Stop loading
            if (sendBtn) {
                sendBtn.disabled = false;
                sendBtn.classList.remove('btn-loading');
            }
        }
}

    // ─── Ordinary HTTP Handler ──────────────────────────────────────

    function renderOrdinaryHandler(h, body, path) {
        // Strip service name (first segment) to build URL prefix
        const parts = (path || '').split('.').filter(Boolean).slice(1);
        const urlPrefix = parts.join('/');
        const httpPath = h.httpPath || '';
        const urlPath = (urlPrefix ? '/' + urlPrefix : '') + (httpPath && !httpPath.startsWith('/') ? '/' + httpPath : httpPath);

        // URL display
        const urlDiv = document.createElement('div');
        urlDiv.style.cssText = 'margin:12px 0;padding:8px 12px;background:var(--bg-tertiary);border-radius:var(--radius-sm);font-family:var(--font-mono);font-size:13px;word-break:break-all;';
        urlDiv.textContent = (urlPath || '/').replace(/\/{2,}/g, '/');
        body.appendChild(urlDiv);

        const pathParams = h.params.filter(p => p.extractor === 'Param');
        const queryParams = h.params.filter(p => p.extractor === 'Query');
        const bodyParams = h.params.filter(p => p.extractor === 'Body');

        // Path param inputs
        pathParams.forEach(p => {
            const inputGroup = document.createElement('div');
            inputGroup.style.marginBottom = '8px';
            const label = document.createElement('div');
            label.className = 'field-label';
            label.innerHTML = '<span class="badge badge-state">path</span> :' + p.name + ' <span class="field-type">' + displayReturnType(p.ty) + '</span>' + (p.tag ? ' <span class="text-secondary" style="font-size:11px">' + p.tag + '</span>' : '');
            inputGroup.appendChild(label);
            if (p.structure && schema.types[p.structure]) {
                const field = document.createElement('af-field');
                field._schema = schema.types[p.structure];
                field._allTypes = schema.types;
                field.setAttribute('data-endpoint', h.offset);
                field.setAttribute('data-param', p.name);
                field.setAttribute('data-extractor', 'Param');
                inputGroup.appendChild(field);
            } else {
                const inp = document.createElement('input');
                inp.setAttribute('data-endpoint', h.offset);
                inp.setAttribute('data-param', p.name);
                inp.setAttribute('data-extractor', 'Param');
                inp.placeholder = displayReturnType(p.ty);
                if (/^(i|u)(8|16|32|64)$/.test(p.ty) || p.ty === 'usize' || p.ty === 'f32' || p.ty === 'f64') {
                    inp.type = 'number';
                    if (p.ty === 'f32' || p.ty === 'f64') inp.setAttribute('step', 'any');
                }
                inputGroup.appendChild(inp);
            }
            body.appendChild(inputGroup);
        });

        // Query param inputs
        queryParams.forEach(p => {
            const inputGroup = document.createElement('div');
            inputGroup.style.marginBottom = '8px';
            const label = document.createElement('div');
            label.className = 'field-label';
            label.innerHTML = '<span class="badge badge-data">query</span> ' + p.name + ' <span class="field-type">' + displayReturnType(p.ty) + '</span>' + (p.tag ? ' <span class="text-secondary" style="font-size:11px">' + p.tag + '</span>' : '');
            inputGroup.appendChild(label);
            if (p.structure && schema.types[p.structure]) {
                const field = document.createElement('af-field');
                field._schema = schema.types[p.structure];
                field._allTypes = schema.types;
                field.setAttribute('data-endpoint', h.offset);
                field.setAttribute('data-param', p.name);
                field.setAttribute('data-extractor', 'Query');
                inputGroup.appendChild(field);
            } else {
                const inp = document.createElement('input');
                inp.setAttribute('data-endpoint', h.offset);
                inp.setAttribute('data-param', p.name);
                inp.setAttribute('data-extractor', 'Query');
                inp.placeholder = displayReturnType(p.ty);
                if (p.ty === 'bool') inp.type = 'checkbox';
                else if (/^(i|u)(8|16|32|64)$/.test(p.ty) || p.ty === 'usize' || p.ty === 'f32' || p.ty === 'f64') {
                    inp.type = 'number';
                    if (p.ty === 'f32' || p.ty === 'f64') inp.setAttribute('step', 'any');
                }
                inputGroup.appendChild(inp);
            }
            body.appendChild(inputGroup);
        });

        // Body input
        bodyParams.forEach(p => {
            const inputGroup = document.createElement('div');
            inputGroup.style.marginBottom = '8px';
            const label = document.createElement('div');
            label.className = 'field-label';
            label.innerHTML = '<span class="badge badge-data">body</span> ' + p.name + ' <span class="field-type">' + displayReturnType(p.ty) + '</span>' + (p.tag ? ' <span class="text-secondary" style="font-size:11px">' + p.tag + '</span>' : '');
            inputGroup.appendChild(label);
            if (p.structure && schema.types[p.structure]) {
                const field = document.createElement('af-field');
                field._schema = schema.types[p.structure];
                field._allTypes = schema.types;
                field.setAttribute('data-endpoint', h.offset);
                field.setAttribute('data-param', p.name);
                field.setAttribute('data-extractor', 'Body');
                inputGroup.appendChild(field);
            } else {
                const ta = document.createElement('textarea');
                ta.setAttribute('data-endpoint', h.offset);
                ta.setAttribute('data-param', p.name);
                ta.setAttribute('data-extractor', 'Body');
                ta.style.cssText = 'width:100%;height:80px;font-family:var(--font-mono);font-size:13px;';
                ta.placeholder = 'JSON body...';
                inputGroup.appendChild(ta);
            }
            body.appendChild(inputGroup);
        });

        // Send button row
        const sendRow = document.createElement('div');
        sendRow.style.marginTop = '12px';
        sendRow.style.display = 'flex';
        sendRow.style.alignItems = 'center';
        sendRow.style.gap = '8px';

        // Copy Request button (left)
        const copyReqBtn = document.createElement('button');
        copyReqBtn.className = 'btn btn-secondary';
        copyReqBtn.textContent = 'Copy Request';
        copyReqBtn.addEventListener('click', () => {
            copyToClipboard(buildCurlCommand(h, path));
        });
        sendRow.appendChild(copyReqBtn);

        if (bodyParams.length > 0) {
            const applyBtn = document.createElement('button');
            applyBtn.className = 'btn btn-secondary';
            applyBtn.textContent = 'Apply Data';
            applyBtn.addEventListener('click', () => {
                const modal = document.getElementById('json-modal');
                const textarea = document.getElementById('json-modal-text');
                const applyAction = document.getElementById('json-modal-apply');
                const closeAction = document.getElementById('json-modal-close');
                modal.classList.add('open');
                textarea.value = '';
                textarea.placeholder = 'Paste JSON here...';
                textarea.style.borderColor = '';
                const onApply = () => {
                    try {
                        const data = JSON.parse(textarea.value);
                        bodyParams.forEach(p => {
                            const el = document.querySelector('[data-endpoint="' + h.offset + '"][data-param="' + p.name + '"][data-extractor="Body"]');
                            if (el && typeof el.setValue === 'function') {
                                el.setValue(data);
                            } else if (el) {
                                el.value = typeof data === 'object' ? JSON.stringify(data) : data;
                            }
                        });
                        textarea.style.borderColor = '';
                        modal.classList.remove('open');
                        cleanup();
                    } catch(e) {
                        textarea.style.borderColor = 'var(--error, #dc3545)';
                        textarea.placeholder = 'Invalid JSON: ' + e.message;
                    }
                };
                const onClose = () => { modal.classList.remove('open'); cleanup(); };
                const cleanup = () => {
                    applyAction.removeEventListener('click', onApply);
                    closeAction.removeEventListener('click', onClose);
                };
                applyAction.addEventListener('click', onApply);
                closeAction.addEventListener('click', onClose);
            });
            sendRow.appendChild(applyBtn);
        }

        // Spacer
        const spacer = document.createElement('div');
        spacer.style.flexGrow = '1';
        sendRow.appendChild(spacer);

        // Send button (right)
        const sendBtn = document.createElement('button');
        sendBtn.className = 'btn btn-primary';
        sendBtn.id = 'send-btn-' + h.offset;
        sendBtn.textContent = 'Send';
        sendBtn.addEventListener('click', () => sendOrdinaryRequest(h, path, sendBtn));
        sendRow.appendChild(sendBtn);

        body.appendChild(sendRow);

        // Response panel
        const respPanel = document.createElement('div');
        respPanel.className = 'response-panel';
        respPanel.id = 'response-' + h.offset;
        respPanel.style.display = 'none';
        const respHeader = document.createElement('div');
        respHeader.className = 'response-header';
        const respLabel = document.createElement('span');
        respLabel.textContent = 'Response';
        respHeader.appendChild(respLabel);
        const respActions = document.createElement('div');
        respActions.style.cssText = 'display:flex;align-items:center;gap:8px';
        const respTime = document.createElement('span');
        respTime.id = 'response-time-' + h.offset;
        respActions.appendChild(respTime);
        const respCopyBtn = document.createElement('button');
        respCopyBtn.className = 'btn btn-sm btn-secondary';
        respCopyBtn.textContent = 'Copy';
        respCopyBtn.addEventListener('click', () => {
            const rb = document.getElementById('response-body-' + h.offset);
            if (rb) copyToClipboard(rb.textContent);
        });
        respActions.appendChild(respCopyBtn);
        respHeader.appendChild(respActions);
        respPanel.appendChild(respHeader);
        const respBody = document.createElement('pre');
        respBody.className = 'response-body';
        respBody.id = 'response-body-' + h.offset;
        respPanel.appendChild(respBody);
        body.appendChild(respPanel);
}

    async function sendOrdinaryRequest(handler, path, sendBtn) {
        const respPanel = document.getElementById('response-' + handler.offset);
        const respBody = document.getElementById('response-body-' + handler.offset);
        const respTime = document.getElementById('response-time-' + handler.offset);

        respPanel.style.display = 'block';
        respBody.textContent = 'Loading...';
        respBody.className = 'response-body';
        respTime.textContent = '';

        if (sendBtn) {
            sendBtn.disabled = true;
            sendBtn.classList.add('btn-loading');
        }

        const start = performance.now();

        try {
            const parts = (path || '').split('.').filter(Boolean).slice(1);
            const urlPrefix = parts.join('/');
            const httpPath = handler.httpPath || '';
            const urlPath = (urlPrefix ? '/' + urlPrefix : '') + (httpPath && !httpPath.startsWith('/') ? '/' + httpPath : httpPath);
            let url = buildUrl() + '/' + urlPath.replace(/^\/+/, '');

            // Substitute path params
            const pathParams = handler.params.filter(p => p.extractor === 'Param');
            for (const p of pathParams) {
                const el = document.querySelector('[data-endpoint="' + handler.offset + '"][data-param="' + p.name + '"][data-extractor="Param"]');
                if (!el) continue;
                if (el.tagName === 'AF-FIELD') {
                    // Complex param — get struct values and substitute each field
                    const obj = el.getValue();
                    if (obj && typeof obj === 'object') {
                        for (const [k, v] of Object.entries(obj)) {
                            url = url.replace(':' + k, encodeURIComponent(String(v !== null && v !== undefined ? v : '')));
                        }
                    }
                } else {
                    // Simple param — substitute directly
                    const val = el.type === 'checkbox' ? el.checked : (el.value || '');
                    url = url.replace(':' + p.name, encodeURIComponent(val));
                }
            }

            // Build query string
            const qs = new URLSearchParams();
            const queryParams = handler.params.filter(p => p.extractor === 'Query');
            for (const p of queryParams) {
                const el = document.querySelector('[data-endpoint="' + handler.offset + '"][data-param="' + p.name + '"][data-extractor="Query"]');
                if (el) {
                    if (el.tagName === 'AF-FIELD') {
                        const v = el.getValue();
                        if (v !== undefined && v !== null) {
                            if (typeof v === 'object' && !Array.isArray(v)) {
                                for (const [k, val] of Object.entries(v)) {
                                    if (val !== undefined && val !== null) qs.append(k, String(val));
                                }
                            } else {
                                qs.append(p.name, String(v));
                            }
                        }
                    } else if (el.type === 'checkbox') {
                        qs.append(p.name, el.checked);
                    } else if (el.value) {
                        qs.append(p.name, el.value);
                    }
                }
            }
            const qsStr = qs.toString();
            if (qsStr) url += '?' + qsStr;

            // Build headers
            const headers = {};
            // Merge global header values first
            const globalHeaders = gatherHeaders();
            for (const typeName of Object.keys(globalHeaders)) {
                const val = globalHeaders[typeName];
                if (val && typeof val === 'object' && !Array.isArray(val)) {
                    for (const [k, v] of Object.entries(val)) {
                        if (v !== undefined && v !== null) headers[k] = String(v);
                    }
                }
            }
            const hasBody = !['GET', 'HEAD'].includes(handler.method || 'GET');
            if (hasBody) headers['Content-Type'] = 'application/json';

            // Build body
            let fetchBody = undefined;
            const bodyParams = handler.params.filter(p => p.extractor === 'Body');
            if (bodyParams.length > 0 && hasBody) {
                const p = bodyParams[0];
                const el = document.querySelector('[data-endpoint="' + handler.offset + '"][data-param="' + p.name + '"][data-extractor="Body"]');
                if (el) {
                    if (el.tagName === 'AF-FIELD') {
                        fetchBody = JSON.stringify(el.getValue());
                    } else {
                        fetchBody = el.value || '{}';
                    }
                }
            }

            const resp = await fetch(url, {
                method: handler.method || 'GET',
                headers: headers,
                body: fetchBody,
            });

            const elapsed = (performance.now() - start).toFixed(1);
            const text = await resp.text();

            let formatted;
            try { formatted = JSON.stringify(JSON.parse(text), null, 2); } catch(e) { formatted = text; }
            respBody.textContent = formatted;

            if (resp.ok) {
                respBody.className = 'response-body response-success';
            } else {
                respBody.className = 'response-body response-error';
            }
            respTime.textContent = elapsed + 'ms | ' + resp.status;
        } catch(e) {
            const elapsed = (performance.now() - start).toFixed(1);
            respBody.textContent = 'Error: ' + e.message;
            respBody.className = 'response-body response-error';
            respTime.textContent = elapsed + 'ms';
        } finally {
            if (sendBtn) {
                sendBtn.disabled = false;
                sendBtn.classList.remove('btn-loading');
            }
        }
}

    function buildCurlCommand(handler, path) {
        const pathParts = (path || '').split('.').filter(Boolean).slice(1);
        const urlPrefix = pathParts.join('/');
        const httpPath = handler.httpPath || '';
        const urlPath = (urlPrefix ? '/' + urlPrefix : '') + (httpPath && !httpPath.startsWith('/') ? '/' + httpPath : httpPath);
        let url = buildUrl() + '/' + urlPath.replace(/^\/+/, '');
        const method = handler.method || 'GET';
        const parts = ['curl', '-X', method];

        // Path params
        const pathParams = handler.params.filter(p => p.extractor === 'Param');
        for (const p of pathParams) {
            const el = document.querySelector('[data-endpoint="' + handler.offset + '"][data-param="' + p.name + '"][data-extractor="Param"]');
            if (el && el.tagName === 'AF-FIELD') {
                const obj = el.getValue();
                if (obj && typeof obj === 'object') {
                    for (const [k, v] of Object.entries(obj)) {
                        url = url.replace(':' + k, encodeURIComponent(String(v !== null && v !== undefined ? v : '{' + k + '}')));
                    }
                }
            } else {
                let val = '{' + p.name + '}';
                if (el) val = el.value || val;
                url = url.replace(':' + p.name, val);
            }
        }

        // Query params
        const queryParams = handler.params.filter(p => p.extractor === 'Query');
        if (queryParams.length > 0) {
            const qsParts = [];
            for (const p of queryParams) {
                const el = document.querySelector('[data-endpoint="' + handler.offset + '"][data-param="' + p.name + '"][data-extractor="Query"]');
                let val = '{' + p.name + '}';
                if (el) val = el.value || val;
                qsParts.push(p.name + '=' + val);
            }
            url += '?' + qsParts.join('&');
        }

        parts.push('"' + url + '"');

        // Headers — use global header values
        const globalHdrs = gatherHeaders();
        for (const typeName of Object.keys(globalHdrs)) {
            const val = globalHdrs[typeName];
            if (val && typeof val === 'object' && !Array.isArray(val)) {
                for (const [k, v] of Object.entries(val)) {
                    if (v !== undefined && v !== null && v !== '') {
                        parts.push('-H "' + k + ': ' + v + '"');
                    }
                }
            }
        }

        if (!['GET', 'HEAD'].includes(method) || handler.params.some(p => p.extractor === 'Body')) {
            parts.push('-H "Content-Type: application/json"');
        }

        // Body
        if (!['GET', 'HEAD'].includes(method)) {
            const bodyParams = handler.params.filter(p => p.extractor === 'Body');
            if (bodyParams.length > 0) {
                let val = '{}';
                const p = bodyParams[0];
                const el = document.querySelector('[data-endpoint="' + handler.offset + '"][data-param="' + p.name + '"][data-extractor="Body"]');
                if (el) {
                    if (el.tagName === 'AF-FIELD') {
                        val = JSON.stringify(el.getValue());
                    } else {
                        val = el.value || val;
                    }
                }
                parts.push("-d '" + val + "'");
            }
        }

        return parts.join(' ');
}

    // Chat panel
    function renderChatPanel(handler, container) {
        const panel = document.createElement('div');
        panel.className = 'chat-panel';

        const header = document.createElement('div');
        header.className = 'chat-header';

        const status = document.createElement('span');
        status.className = 'chat-status';
        status.textContent = '● Disconnected';
        status.style.color = 'var(--text-secondary)';
        header.appendChild(status);

        const connectBtn = document.createElement('button');
        connectBtn.className = 'btn btn-sm btn-primary';
        connectBtn.textContent = 'Connect';
        header.appendChild(connectBtn);

        const disconnectBtn = document.createElement('button');
        disconnectBtn.className = 'btn btn-sm btn-danger';
        disconnectBtn.textContent = 'Disconnect';
        disconnectBtn.disabled = true;
        header.appendChild(disconnectBtn);

        panel.appendChild(header);

        // Data param inputs
        const dataInputs = handler.params.filter(p => p.extractor === 'Data');
        if (dataInputs.length > 0) {
            const inputsDiv = document.createElement('div');
            inputsDiv.style.cssText = 'margin:8px 0';
            for (const p of dataInputs) {
                const inputGroup = document.createElement('div');
                inputGroup.style.marginBottom = '6px';
                const label = document.createElement('div');
                label.className = 'field-label';
                label.innerHTML = p.name + ' <span class="field-type">' + p.ty + '</span>';
                inputGroup.appendChild(label);
                if (p.structure && schema.types[p.structure]) {
                    const field = document.createElement('af-field');
                    field._schema = schema.types[p.structure];
                    field._allTypes = schema.types;
                    field.setAttribute('data-endpoint', handler.offset);
                    field.setAttribute('data-param', p.name);
                    inputGroup.appendChild(field);
                } else {
                    const inp = document.createElement('input');
                    inp.setAttribute('data-endpoint', handler.offset);
                    inp.setAttribute('data-param', p.name);
                    inp.placeholder = p.ty;
                    inputGroup.appendChild(inp);
                }
                inputsDiv.appendChild(inputGroup);
            }
            panel.appendChild(inputsDiv);
        }

        const log = document.createElement('div');
        log.className = 'chat-log';
        panel.appendChild(log);

        const inputArea = document.createElement('div');
        inputArea.className = 'chat-input-area';
        const input = document.createElement('input');
        input.type = 'text';
        input.placeholder = 'Type a message...';
        input.disabled = true;
        inputArea.appendChild(input);
        const sendBtn = document.createElement('button');
        sendBtn.className = 'btn btn-primary';
        sendBtn.textContent = 'Send';
        sendBtn.disabled = true;
        inputArea.appendChild(sendBtn);
        panel.appendChild(inputArea);

        let socket = null;

        function addMsg(text, cls) {
            const div = document.createElement('div');
            div.className = 'chat-message ' + cls;
            div.textContent = text;
            log.appendChild(div);
            log.scrollTop = log.scrollHeight;
        }

        connectBtn.addEventListener('click', async () => {
            try {
                if (!client) {
                    addMsg('Error: Please connect first using the top-right Connect button', 'chat-error');
                    return;
                }

                // Gather data params
                const dataParams = handler.params.filter(p => p.extractor === 'Data');
                const args = [];
                for (const p of dataParams) {
                    const el = document.querySelector('[data-endpoint="' + handler.offset + '"][data-param="' + p.name + '"]');
                    if (el && el.tagName === 'AF-FIELD') {
                        args.push(el.getValue());
                    } else if (el && typeof el.value !== 'undefined') {
                        args.push(el.value || '');
                    } else {
                        args.push(defaultForType(p.structure || p.ty));
                    }
                }

                // Navigate to handler
                const pathParts = (handler.apiPath || handler.path).split('.');
                let fn = client.apis;
                for (const part of pathParts) fn = fn[part.replace(/^:/, '')];

                // Add callback as last arg
                args.push((data, send) => {
                    const text = new TextDecoder().decode(data);
                    addMsg(text, 'chat-received');
                });

                socket = await fn(...args);

                status.textContent = '● Connected';
                status.style.color = 'var(--success)';
                input.disabled = false;
                sendBtn.disabled = false;
                connectBtn.disabled = true;
                disconnectBtn.disabled = false;
                addMsg('Connected', 'chat-system');
            } catch(e) {
                addMsg('Error: ' + e.message, 'chat-error');
            }
        });

        sendBtn.addEventListener('click', () => {
            const text = input.value;
            if (text && socket && !socket.isClosed) {
                socket.send(text);
                addMsg(text, 'chat-sent');
                input.value = '';
            }
        });

        input.addEventListener('keydown', (e) => { if (e.key === 'Enter') sendBtn.click(); });

        disconnectBtn.addEventListener('click', async () => {
            if (socket && !socket.isClosed) {
                await socket.close();
                addMsg('Disconnected', 'chat-system');
            }
            status.textContent = '● Disconnected';
            status.style.color = 'var(--text-secondary)';
            input.disabled = true;
            sendBtn.disabled = true;
            connectBtn.disabled = false;
            disconnectBtn.disabled = true;
            socket = null;
        });

        container.appendChild(panel);
}
})();
"#;

// ─── HTML Generation ──────────────────────────────────────────────

/// Generates the root index.html page listing all registered services
/// with handler counts and descriptions.  Each service name links to
/// its dedicated documentation page at /doc/{service}.
pub(crate) fn generate_index_html(services: &[Service], doc_title: Option<&str>) -> String {
    let theme_icon = "☀️";
    let page_title = doc_title.unwrap_or("afast — API Documentation");
    let h1_title = doc_title.unwrap_or("afast API Documentation");
    let mut cards = String::new();
    for svc in services {
        if svc.name.is_empty() {
            continue;
        }
        let count = crate::service::count_handlers(&svc.handlers);
        let desc_html = if svc.desc.is_empty() {
            String::new()
        } else {
            format!(r#"<p class="service-desc">{}</p>"#, svc.desc)
        };
        cards.push_str(&format!(
            r#"<a href="/doc/{}" class="service-card"><h2>{}</h2>{}<p>{} handler(s)</p></a>"#,
            svc.name, svc.name, desc_html, count
        ));
    }

    let favicon = favicon_data_uri();

    format!(
        r#"<!-- Auto-generated by afast. DO NOT EDIT. -->
<!DOCTYPE html>
<html lang="en" data-theme="dark">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="icon" type="image/svg+xml" href="{favicon}">
<title>{page_title}</title>
<style>{css}</style>
</head>
<body>
<div class="header">
    <h1>{h1_title}</h1>
    <button id="theme-toggle">{theme_icon}</button>
</div>
<div class="content" style="max-width:700px;margin:0 auto;padding:32px 24px;">
    <div class="service-list">{cards}</div>
</div>
<script>
(function(){{
    const saved = localStorage.getItem('afast-doc-theme') || 'dark';
    document.documentElement.setAttribute('data-theme', saved);
    document.getElementById('theme-toggle').textContent = saved === 'dark' ? '☀️' : '🌙';
    document.getElementById('theme-toggle').addEventListener('click', function(){{
        const c = document.documentElement.getAttribute('data-theme');
        const n = c === 'dark' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', n);
        this.textContent = n === 'dark' ? '☀️' : '🌙';
        localStorage.setItem('afast-doc-theme', n);
    }});
}})();
</script>
</body>
</html>"#,
        css = CSS,
        cards = cards,
    )
}

/// Generates a complete HTML documentation page for a single service.
/// Embeds the JavaScript client library, the schema JSON, the UI
/// web components, and the interactive endpoint explorer.  Port numbers
/// are extracted from the address strings for default UI values.
pub(crate) fn generate_service_html(
    services: &[Service],
    service_name: &str,
    doc_title: Option<&str>,
    http_addr: &str,
    ws_addr: Option<&str>,
) -> Result<String, DocError> {
    let svc = services
        .iter()
        .find(|s| s.name == service_name)
        .ok_or_else(|| DocError::ServiceNotFound(service_name.to_string()))?;

    let js_client = embed_js_client(svc, &[crate::JsTsCallType::Fetch, crate::JsTsCallType::Ws]);
    let schema_json = build_schema(svc);
    let title = match doc_title {
        Some(t) => format!("{} — {}", svc.name, t),
        None => format!("{} — API Documentation", svc.name),
    };
    let http_port = http_addr
        .rsplit(':')
        .next_back()
        .and_then(|s| s.parse::<u16>().ok());
    let ws_port = ws_addr.and_then(|a| {
        a.rsplit(':')
            .next_back()
            .and_then(|s| s.parse::<u16>().ok())
    });
    let theme_icon = "☀️";
    let desc_line = if svc.desc.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p style="margin:0;color:var(--text-secondary);font-size:13px;">{}</p>"#,
            svc.desc
        )
    };

    let favicon = favicon_data_uri();

    Ok(format!(
        r#"<!-- Auto-generated by afast. DO NOT EDIT. -->
<!DOCTYPE html>
<html lang="en" data-theme="dark">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="icon" type="image/svg+xml" href="{favicon}">
<title>{title}</title>
<style>{css}</style>
</head>
<body>
<div class="header">
    <div style="display:flex;align-items:baseline;">
        <h1>{service_name}</h1>
        <span class="subtitle">API Documentation</span>
    </div>
    {desc_line}
    <div class="header-controls">
        <a href="/doc" class="btn btn-secondary header-btn">All Services</a>
        <select id="transport-select" class="header-input">
            <option value="ws">ws</option>
            <option value="fetch">http</option>
        </select>
        <label class="header-checkbox" title="Use secure connection (wss/https)">
            <input type="checkbox" id="secure-check"> TLS
        </label>
        <input id="host-input" type="text" placeholder="localhost" class="header-input" style="width:140px;">
        <span class="header-sep">:</span>
        <input id="port-input" type="text" placeholder="3000" class="header-input" style="width:64px;">
        <button id="connect-btn" class="btn btn-primary header-btn">Save</button>
        <button id="disconnect-btn" class="btn header-btn hidden">Disconnect</button>
        <button id="theme-toggle" class="header-input">{theme_icon}</button>
    </div>
</div>
<div class="main">
    <nav class="sidebar" id="sidebar"></nav>
    <div class="content">
        <div class="panel" id="customs-panel">
            <div class="collapsible-header" id="customs-toggle">
                <h2>Custom Parameters</h2>
                <span class="collapsible-arrow" id="customs-arrow">▶</span>
            </div>
            <div class="collapsible-body" id="customs-body">
                <p class="text-secondary" style="font-size:13px;margin:8px 0 12px;">These values are sent with every request and saved to localStorage.</p>
                <div id="customs-fields"></div>
                <div class="btn-row">
                    <button class="btn btn-primary" id="customs-save">Save</button>
                </div>
            </div>
        </div>
        <div class="panel" id="headers-panel">
            <div class="collapsible-header" id="headers-toggle">
                <h2>Header Parameters</h2>
                <span class="collapsible-arrow" id="headers-arrow">▶</span>
            </div>
            <div class="collapsible-body" id="headers-body">
                <p class="text-secondary" style="font-size:13px;margin:8px 0 12px;">These values are sent as HTTP headers with every ordinary request and saved to localStorage.</p>
                <div id="headers-fields"></div>
                <div class="btn-row">
                    <button class="btn btn-primary" id="headers-save">Save</button>
                </div>
            </div>
        </div>
        <div id="endpoints"></div>
    </div>
</div>

<!-- JSON Edit Modal -->
<div class="modal-overlay" id="json-modal">
    <div class="modal-content">
        <h3>Edit JSON</h3>
        <textarea id="json-modal-text"></textarea>
        <div class="modal-actions">
            <button class="btn btn-secondary" id="json-modal-close">Cancel</button>
            <button class="btn btn-primary" id="json-modal-apply">Apply</button>
        </div>
    </div>
</div>

<!-- Schema Data -->
<script id="schema-data" type="application/json">
{schema_json}
</script>

<!-- Embedded JS Client -->
<script>
{js_client}
</script>

<!-- WebComponent + UI Logic -->
<script>
{ui_js}
</script>
</body>
</html>"#,
        css = CSS,
        js_client = js_client,
        ui_js = UI_JS
            .replace(
                "'3000' : '5000'",
                &format!(
                    "'{}' : '{}'",
                    ws_port.unwrap_or(3000),
                    http_port.unwrap_or(5000)
                )
            )
            .replace(
                "? 3000 : 5000)",
                &format!(
                    "? {} : {})",
                    ws_port.unwrap_or(3000),
                    http_port.unwrap_or(5000)
                )
            ),
        schema_json = schema_json,
        title = title,
        service_name = svc.name,
    ))
}

/// Errors that can occur during documentation generation.
pub enum DocError {
    /// The requested service name was not found in the application's
    /// registered services.
    ServiceNotFound(String),
}

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocError::ServiceNotFound(name) => write!(f, "service '{}' not found", name),
        }
    }
}

/// Writes the complete documentation site to `dir`: an index.html page
/// and one {service}.html page per registered service.  Each page is
/// fully self-contained with embedded CSS, JavaScript, and schema data.
pub(crate) fn write_docs(
    services: &[Service],
    dir: &std::path::Path,
    doc_title: Option<&str>,
) -> Result<(), DocError> {
    use std::fs;

    fs::create_dir_all(dir).map_err(|e| DocError::ServiceNotFound(e.to_string()))?;

    // Generate and write the landing page listing all services.
    let index = generate_index_html(services, doc_title);
    fs::write(dir.join("index.html"), index)
        .map_err(|e| DocError::ServiceNotFound(e.to_string()))?;

    // Generate and write one page per service with its full endpoint
    // explorer UI.
    for svc in services {
        if svc.name.is_empty() {
            continue;
        }
        let html = generate_service_html(services, &svc.name, doc_title, "", None)?;
        fs::write(dir.join(format!("{}.html", svc.name)), html)
            .map_err(|e| DocError::ServiceNotFound(e.to_string()))?;
    }

    Ok(())
}
