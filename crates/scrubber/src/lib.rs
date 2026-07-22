#![forbid(unsafe_code)]

mod decode;
mod fields;
pub mod scrub;
pub use scrub::{Error, scrub};
