//! Request and connection lifecycle hooks.
//!
//! Hooks allow users to intercept handler execution for observability,
//! tracing, logging, or custom middleware. Each hook returns a _guard_
//! object whose methods are called at the corresponding lifecycle point.
//!
//! # Example
//!
//! ```ignore
//! use afast::hook::{Hook, RequestContext, RequestGuard};
//!
//! struct Timing;
//!
//! impl Hook for Timing {
//!     fn before_request(&self, _ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
//!         Some(Box::new(std::time::Instant::now()))
//!     }
//! }
//!
//! impl RequestGuard for std::time::Instant {
//!     fn on_response(&mut self, ctx: &RequestContext, _resp: &[u8]) {
//!         println!("{} took {:?}", ctx.handler_name, self.elapsed());
//!     }
//! }
//! ```

use std::sync::Arc;

use crate::StateMap;

/// A request-scoped context passed to hooks and guards.
///
/// Contains static metadata about the handler being invoked and the
/// transport it arrived on.  The `state` field gives access to shared
/// application state (useful for extracting tracing contexts, etc.).
pub struct RequestContext {
    /// Handler name (the Rust function name).
    pub handler_name: &'static str,
    /// Human-readable description from `#[handler(desc(...))]`.
    pub handler_desc: &'static str,
    /// Transport that delivered this request: `"tcp"`, `"ws"`, or `"http"`.
    pub transport: &'static str,
    /// Handler offset in the binary dispatch table.
    pub handler_id: usize,
    /// Shared application state.
    pub state: Arc<StateMap>,
}

/// Extension point for request lifecycle events.
///
/// Implement this trait and register it with [`AFast::hook()`](crate::AFast::hook).
/// Each method has a default no-op implementation so you only need to override
/// the ones you care about.
pub trait Hook: Send + Sync + 'static {
    /// Called before a binary-protocol handler executes.
    ///
    /// Return a `RequestGuard` to receive `on_response` / `on_error` callbacks.
    fn before_request(&self, _ctx: &RequestContext) -> Option<Box<dyn RequestGuard>> {
        None
    }

    /// Called before a long-connection handler spawns.
    ///
    /// Return a `ConnectionGuard` to receive `on_disconnect` callbacks.
    fn on_connect(&self, _ctx: &RequestContext) -> Option<Box<dyn ConnectionGuard>> {
        None
    }
}

/// Guard returned by [`Hook::before_request`].
///
/// Receives callbacks when the handler completes or fails.  The guard is
/// dropped automatically after the callback, so it can hold resources
/// (spans, timers, file handles, etc.) that are cleaned up on drop.
pub trait RequestGuard: Send + 'static {
    /// Called when the handler returns `Ok(bytes)`.
    fn on_response(&mut self, _ctx: &RequestContext, _response: &[u8]) {}
    /// Called when the handler returns `Err(e)`.
    fn on_error(&mut self, _ctx: &RequestContext, _error: &crate::Error) {}
}

/// Guard returned by [`Hook::on_connect`].
///
/// Receives a callback when the long-connection is closed.
pub trait ConnectionGuard: Send + 'static {
    /// Called when the connection is dropped or closed.
    fn on_disconnect(&mut self, _ctx: &RequestContext) {}
}
