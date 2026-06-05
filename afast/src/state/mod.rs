//! Type-map for storing shared application state.
//!
//! The [`StateMap`] stores at most one value per Rust type, keyed by [`TypeId`].
//! Handlers access state through the [`State<T>`](crate::State) extractor.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// A type-map for storing application state values.
///
/// Each value is keyed by its Rust type (`TypeId`). At most one value per
/// type can be stored. Uses `HashMap` for O(1) lookup.
///
/// # Example
///
/// ```
/// use afast::state::StateMap;
///
/// struct MyConfig {
///     db_url: String,
/// }
///
/// let mut map = StateMap::new();
/// map.insert(MyConfig { db_url: "localhost".into() });
/// assert!(map.get::<MyConfig>().is_some());
/// ```
pub struct StateMap {
    entries: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl StateMap {
    /// Creates an empty state map.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Inserts a value, keyed by its type. Replaces any existing value of the
    /// same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.entries.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Retrieves a reference to the value of type `T`, if it was previously
    /// inserted. O(1) lookup via `HashMap`.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.entries
            .get(&TypeId::of::<T>())
            .and_then(|val| val.downcast_ref::<T>())
    }
}

impl Default for StateMap {
    fn default() -> Self {
        Self::new()
    }
}
