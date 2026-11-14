//! BIP 370 PSBT roles.
//!
//! Each submodule implements one role of the BIP 370 construction
//! state machine: [`creator`] produces empty PSBTs, [`constructor`]
//! adds inputs and outputs, and [`sorter`] finalizes their ordering.

pub mod constructor;
pub mod creator;
pub mod sorter;

pub use creator::Creator;
