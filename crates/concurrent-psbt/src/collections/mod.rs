//! Join implementations for standard library collection types.
//!
//! Each collection type provides crate-internal joins for the result domain,
//! plus helper traits for lifting (`wrap`) and lowering (`try_unwrap`) between
//! clean and result representations.

pub(crate) mod btreemap;
pub(crate) mod hashmap;
pub(crate) mod option;
