#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[macro_use]
mod lattice;

pub use lattice::join::{Join, JoinMut};
pub use lattice::partial::{Conflict, JoinResult, PartialJoin};
