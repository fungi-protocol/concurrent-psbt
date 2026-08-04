#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

// Make every `#[test]`, including `proptest!` tests, also save a trace.
#[cfg(test)]
#[macro_use]
extern crate test_tracing_macros;

#[macro_use]
mod lattice;
mod values;

mod collections;
mod psbt;

pub use lattice::join::{Join, JoinMut};
pub use lattice::partial::{Conflict, JoinResult, PartialJoin};
