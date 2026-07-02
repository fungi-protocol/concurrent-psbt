#![forbid(unsafe_code)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[macro_use]
mod lattice;
mod values;

mod collections;
#[macro_use]
mod psbt;
pub mod roles;

pub use roles::sorter;
pub use roles::sorter as sort;

pub use lattice::join::{Join, JoinMut};
pub use lattice::partial::{Conflict, JoinResult, PartialJoin};
pub use psbt::fee;
pub use psbt::global;
pub use psbt::input;
pub use psbt::output;
pub use psbt::removal;
pub use psbt::tx;

pub mod payments;

/// Proprietary field prefix for all concurrent-psbt extensions.
///
/// All fields defined by this crate use this prefix in their
/// [`ProprietaryKey`](psbt_v2::raw::ProprietaryKey). The subtype byte
/// distinguishes individual fields.
pub const PROPRIETARY_PREFIX: &[u8] = b"concurrent-psbt";
