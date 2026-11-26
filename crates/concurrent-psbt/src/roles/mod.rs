//! BIP 370 PSBT roles.
//!
//! Each submodule implements one role of the BIP 370 construction
//! state machine: [`creator`] produces empty PSBTs, [`constructor`]
//! adds inputs and outputs, and [`sorter`] finalizes their ordering.
//! [`dynamic`] picks the constructor or sorter that a parsed PSBT's flags
//! select, without knowing them in advance.

pub mod constructor;
pub mod creator;
pub mod dynamic;
pub mod sorter;

pub use creator::Creator;
