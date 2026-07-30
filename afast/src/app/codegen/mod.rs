//! Client code generation for TypeScript, JavaScript, Kotlin, Rust, C#, API docs,
//! and the on-demand `/code` endpoint.
//!
//! Each submodule is gated by a Cargo feature so unused generators are not
//! compiled:
//! - `ts` → [`ts`] — TypeScript
//! - `js` → [`js`] — JavaScript
//! - `kt` → [`kt`] — Kotlin
//! - `rs` → [`rs`] — Rust
//! - `cs` → [`cs`] — C# / .NET
//! - `doc` → [`doc`] — Interactive API documentation HTML
//! - `code` → [`code`] — On-demand `/code/{service}/{lang}` HTTP endpoint

pub(crate) mod buf;
#[cfg(feature = "code")]
pub mod code;
#[cfg(feature = "cs")]
pub mod cs;
#[cfg(feature = "doc")]
pub mod doc;
#[cfg(feature = "js")]
pub mod js;
#[cfg(feature = "kt")]
pub mod kt;
#[cfg(feature = "rs")]
pub mod rs;
#[cfg(feature = "ts")]
pub mod ts;
