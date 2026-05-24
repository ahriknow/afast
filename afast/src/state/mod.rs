use std::any::{Any, TypeId};

/// A type-map for storing application state values.
///
/// Each value is keyed by its Rust type (`TypeId`). At most one value per
/// type can be stored. Uses linear-scan lookup because the number of
/// registered state types in a typical application (1–10) is small enough
/// that a vector scan is faster than hashing.
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
    entries: Vec<(TypeId, Box<dyn Any + Send + Sync>)>,
}

impl StateMap {
    /// Creates an empty state map.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Inserts a value, keyed by its type. Replaces any existing value of the
    /// same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        let tid = TypeId::of::<T>();
        for (id, slot) in self.entries.iter_mut() {
            if *id == tid {
                *slot = Box::new(value);
                return;
            }
        }
        self.entries.push((tid, Box::new(value)));
    }

    /// Retrieves a reference to the value of type `T`, if it was previously
    /// inserted. Uses linear scan which is faster than hashing for the
    /// typical small number of state types.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        let tid = TypeId::of::<T>();
        for (id, val) in &self.entries {
            if *id == tid {
                return val.downcast_ref::<T>();
            }
        }
        None
    }
}

impl Default for StateMap {
    fn default() -> Self {
        Self::new()
    }
}
