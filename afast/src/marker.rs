//! Conditional serialization/deserialization with optional marker support.
//!
//! When the `marker` feature is enabled, `__serialize` and `__deserialize` use
//! `to_bytes_with(marker)` / `from_bytes_with(data, marker)` from afastdata,
//! which allows `#[afast(skip_with("marker"))]` fields to be conditionally
//! skipped based on the active marker value.
//!
//! When the `marker` feature is disabled, they fall back to plain
//! `to_bytes()` / `from_bytes()`.

// ── Marker-enabled implementation ──────────────────────────────────
#[cfg(feature = "marker")]
mod inner {
    use std::sync::Mutex;

    /// Global marker string, lazily initialized to `"afast"`.
    static MARKER: Mutex<Option<String>> = Mutex::new(None);

    /// Sets the global marker. Typically called by [`AFast::marker()`](crate::AFast::marker).
    pub fn set_marker(marker: &str) {
        let mut lock = MARKER.lock().unwrap();
        *lock = Some(marker.to_string());
    }

    /// Returns the current marker, falling back to `"afast"` if never set.
    pub fn get_marker() -> String {
        let lock = MARKER.lock().unwrap();
        lock.clone().unwrap_or_else(|| "afast".to_string())
    }

    pub fn serialize<T: afastdata::AFastSerialize>(value: &T) -> Vec<u8> {
        let marker = get_marker();
        value.to_bytes_with(&marker)
    }

    pub fn deserialize<T: afastdata::AFastDeserialize>(
        data: &[u8],
    ) -> Result<(T, usize), afastdata::Error> {
        let marker = get_marker();
        T::from_bytes_with(data, &marker)
    }
}

// ── Plain (no-marker) fallback ─────────────────────────────────────
#[cfg(not(feature = "marker"))]
mod inner {
    pub fn serialize<T: afastdata::AFastSerialize>(value: &T) -> Vec<u8> {
        value.to_bytes()
    }

    pub fn deserialize<T: afastdata::AFastDeserialize>(
        data: &[u8],
    ) -> Result<(T, usize), afastdata::Error> {
        T::from_bytes(data)
    }

    /// Returns the default marker `"afast"` when the marker feature is disabled.
    pub fn get_marker() -> String {
        "afast".to_string()
    }
}

#[cfg(feature = "marker")]
pub use inner::set_marker;
pub use inner::{deserialize, get_marker, serialize};

/// Returns `true` if the field should be included in generated client code
/// given the current marker setting.
///
/// - Fields with `skip == true` are always excluded.
/// - Fields with a non-empty `skip_with` are excluded when it matches the
///   active marker.
/// - All other fields are included.
pub fn should_include_field(field: &crate::handler::FieldMeta) -> bool {
    if field.skip {
        return false;
    }
    if !field.skip_with.is_empty() {
        let marker = get_marker();
        if field.skip_with == marker {
            return false;
        }
    }
    true
}
