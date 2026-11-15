#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[macro_use]
mod lattice;
mod values;

mod canonical;
mod collections;
#[macro_use]
mod psbt;

pub use canonical::{Canonicalizable, Canonicalization, Canonicalize, Preserve};
pub use lattice::join::{Join, JoinMut};
pub use lattice::partial::{Conflict, JoinResult, PartialJoin};
pub use psbt::TapLeafHashes;
pub use psbt::input;
pub use psbt::output;
