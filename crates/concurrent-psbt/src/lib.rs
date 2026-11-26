#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

// Let every `#[test]`, including `proptest!` tests, save a trace
// when `NEXTEST_TRACE_DIR` is non-empty.
#[cfg(test)]
#[macro_use]
extern crate test_tracing_macros;

#[macro_use]
mod lattice;
mod values;

mod collections;
#[macro_use]
mod psbt;

pub use lattice::join::{Join, JoinMut};
pub use lattice::partial::{Conflict, JoinResult, PartialJoin};
pub use psbt::input;
pub use psbt::output;

pub use lattice::conflicts::Conflicts;
pub use lattice::lift::{Lift, ResultWrapped};
pub use psbt::global;
