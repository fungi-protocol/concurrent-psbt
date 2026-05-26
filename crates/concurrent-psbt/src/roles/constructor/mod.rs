//! The BIP 370 Constructor role: adding inputs and outputs.
//!
//! [`typed::Constructor`] tracks modifiability in the type system.
#![allow(clippy::result_large_err)]

pub mod typed;

pub use typed::{Constructor, ResultConstructor};
