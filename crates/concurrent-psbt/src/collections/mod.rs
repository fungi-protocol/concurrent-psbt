//! Join implementations for standard library collection types.
//!
//! Collections lift and unwrap their contents recursively. Conflict iterators
//! preserve the keys at each level.

pub(crate) mod btreemap;
pub(crate) mod hashmap;
pub(crate) mod option;
