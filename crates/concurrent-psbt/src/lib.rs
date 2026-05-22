#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[macro_use]
mod lattice;
mod values;

mod collections;
#[macro_use]
mod psbt;

pub use lattice::join::{Join, JoinMut};
pub use lattice::partial::{Conflict, JoinResult, PartialJoin};
pub use psbt::global;
