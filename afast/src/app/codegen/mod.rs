// Client code generation for TypeScript, JavaScript, Kotlin, API docs, and the
// on-demand /code endpoint. Each module is gated by a Cargo feature so unused
// generators are not compiled.

#[cfg(feature = "code")]
pub mod code;
#[cfg(feature = "doc")]
pub mod doc;
#[cfg(feature = "js")]
pub mod js;
#[cfg(feature = "kt")]
pub mod kt;
#[cfg(feature = "ts")]
pub mod ts;
