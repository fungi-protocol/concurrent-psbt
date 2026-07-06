//! BIP 370 PSBT roles.
//!
//! Each submodule implements one role of the BIP 370 construction
//! state machine: [`creator`] produces empty PSBTs, [`constructor`]
//! adds inputs and outputs, [`sorter`] finalizes their ordering, and
//! [`combiner`] merges ordered PSBTs during the update/sign phase.

pub mod combiner;
pub mod constructor;
pub mod creator;
pub mod sorter;

pub use creator::Creator;
