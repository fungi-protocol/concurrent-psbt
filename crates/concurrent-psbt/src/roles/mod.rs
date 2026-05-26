//! BIP 370 PSBT roles.
//!
//! Each submodule implements one role of the BIP 370 construction
//! state machine: [`constructor`] adds inputs and outputs, and
//! [`sorter`] finalizes their ordering.

pub mod constructor;
mod creator;
pub mod sorter;
