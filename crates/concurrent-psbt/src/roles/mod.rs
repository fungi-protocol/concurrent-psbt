//! BIP 370 PSBT roles.
//!
//! Each submodule implements one role of the BIP 370 construction
//! state machine: [`sorter`] finalizes input and output ordering.

pub mod sorter;
