//! Per-request context container.
//!
//! The [`RequestCtx`] stores typed values keyed by [`TypeId`], similar to
//! [`StateMap`](crate::state::StateMap) but scoped to a single request
//! or connection.  It is cheap to clone (Arc-wrapped) and uses interior
//! mutability so hooks can insert values before the handler reads them.
//!
//! # Example
//!
//! ```ignore
//! // In a hook:
//! request_ctx.insert(RequestId("abc-123".into()));
//!
//! // In a handler:
//! #[handler(desc("..."))]
//! async fn my_handler(ctx: Ctx<RequestId>) -> afast::Result<Resp> {
//!     let id = ctx.0 .0;
//!     // ...
//! }
//! ```

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Per-request context container.
///
/// Stores at most one value per Rust type, keyed by [`TypeId`].
/// Cheap to clone — all clones share the same underlying data.
///
/// Unlike [`StateMap`](crate::state::StateMap) which is application-global,
/// `RequestCtx` is scoped to a single request (HTTP) or connection (WS/TCP/SSE).
///
/// Hooks can insert values into the context via [`insert`](RequestCtx::insert),
/// and handlers retrieve them via the [`Ctx<T>`](crate::Ctx) extractor.
pub struct RequestCtx {
    inner: Arc<RequestCtxInner>,
}

struct RequestCtxInner {
    entries: RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
}

impl RequestCtx {
    /// Creates an empty context.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RequestCtxInner {
                entries: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Inserts a value, keyed by its type.
    /// Replaces any existing value of the same type.
    ///
    /// If the lock is poisoned (another thread panicked while holding it),
    /// the poison is recovered automatically.
    pub fn insert<T: Send + Sync + 'static>(&self, value: T) {
        let mut entries = self
            .inner
            .entries
            .write()
            .unwrap_or_else(|e| e.into_inner());
        entries.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Retrieves a clone of the value of type `T`, if it was previously
    /// inserted.  Returns `None` if no value of that type exists.
    ///
    /// Requires `T: Clone` because the `RwLock` read guard cannot escape
    /// this method.  For large values, wrap in `Arc` to make cloning cheap.
    ///
    /// If the lock is poisoned (another thread panicked while holding it),
    /// the poison is recovered automatically.
    pub fn get<T: Send + Sync + Clone + 'static>(&self) -> Option<T> {
        let entries = self.inner.entries.read().unwrap_or_else(|e| e.into_inner());
        entries
            .get(&TypeId::of::<T>())
            .and_then(|val| val.downcast_ref::<T>())
            .cloned()
    }
}

impl Clone for RequestCtx {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Default for RequestCtx {
    fn default() -> Self {
        Self::new()
    }
}
