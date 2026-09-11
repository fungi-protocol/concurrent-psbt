#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod decode;
mod fields;
pub mod scrub;
pub use scrub::{Error, scrub};
