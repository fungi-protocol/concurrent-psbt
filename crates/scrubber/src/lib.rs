#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

// Make every `#[test]`, including `proptest!` tests, also save a trace.
#[cfg(test)]
#[macro_use]
extern crate test_tracing_macros;

mod decode;
mod fields;
pub mod scrub;
pub use scrub::{Error, scrub};
