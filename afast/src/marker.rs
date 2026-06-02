//! Conditional serialization/deserialization with optional marker support.
//!
//! When the `marker` feature is enabled, `serialize` and `deserialize` use
//! `to_bytes_with(marker)` / `from_bytes_with(data, marker)` from afastdata,
//! which allows `#[afast(skip_with("marker"))]` fields to be conditionally
//! skipped based on the active marker value.
//!
//! The marker string is passed via `Arc<String>` through the [`StateMap`](crate::StateMap)
//! and extracted by handler code at runtime. There is no global mutable state.
//!
//! For code-generation filtering, a [`OnceLock`] stores the marker set at
//! application startup. This is read-only after initialization and does not
//! require a mutex.

use std::sync::OnceLock;

/// Write-once marker for code-generation filtering. Set once at startup
/// via [`set_codegen_marker`], then read immutably by [`should_include_field`].
static CODEGEN_MARKER: OnceLock<String> = OnceLock::new();

/// Sets the code-generation marker. Must be called once at application
/// startup, before any code generation runs.
///
/// # Panics
///
/// Panics if called more than once.
pub fn set_codegen_marker(marker: &str) {
    CODEGEN_MARKER
        .set(marker.to_string())
        .expect("codegen marker already set");
}

/// Returns the code-generation marker, or `"afast"` if never set.
fn codegen_marker() -> &'static str {
    CODEGEN_MARKER.get().map(|s| s.as_str()).unwrap_or("afast")
}

/// Serializes a value using the given marker string.
///
/// When the `marker` feature is enabled, calls `to_bytes_with(marker)`.
/// Otherwise falls back to plain `to_bytes()`.
pub fn serialize<T: afastdata::AFastSerialize>(value: &T, _marker: &str) -> Vec<u8> {
    #[cfg(feature = "marker")]
    {
        value.to_bytes_with(_marker)
    }
    #[cfg(not(feature = "marker"))]
    {
        let _ = _marker;
        value.to_bytes()
    }
}

/// Deserializes a value using the given marker string.
///
/// When the `marker` feature is enabled, calls `from_bytes_with(data, marker)`.
/// Otherwise falls back to plain `from_bytes()`.
pub fn deserialize<T: afastdata::AFastDeserialize>(
    data: &[u8],
    _marker: &str,
) -> Result<(T, usize), afastdata::Error> {
    #[cfg(feature = "marker")]
    {
        T::from_bytes_with(data, _marker)
    }
    #[cfg(not(feature = "marker"))]
    {
        let _ = _marker;
        T::from_bytes(data)
    }
}

/// Returns `true` if the field should be included in generated client code
/// given the active marker setting.
///
/// - Fields with `skip == true` are always excluded.
/// - Fields with a non-empty `skip_with` are excluded when it matches the
///   code-generation marker.
/// - All other fields are included.
pub fn should_include_field(field: &crate::handler::FieldMeta) -> bool {
    if field.skip {
        return false;
    }
    if !field.skip_with.is_empty() && field.skip_with == codegen_marker() {
        return false;
    }
    true
}
