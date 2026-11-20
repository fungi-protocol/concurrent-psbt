#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

// Let every `#[test]`, including `proptest!` tests, save a trace
// when `NEXTEST_TRACE_DIR` is non-empty.
#[cfg(test)]
#[macro_use]
extern crate test_tracing_macros;

mod decode;
mod fields;
pub mod scrub;
pub use scrub::{Error, scrub};
