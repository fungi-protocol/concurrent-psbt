#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[macro_use]
mod lattice;
mod values;

mod collections;
#[macro_use]
mod psbt;

#[cfg(test)]
mod tests {
    #[cfg(feature = "unit-tests")]
    #[test]
    fn unit_test_feature_produces_coverage_data() {}

    #[cfg(feature = "prop-tests")]
    #[test]
    fn prop_test_feature_produces_coverage_data() {}
}

pub use lattice::join::{Join, JoinMut};
pub use lattice::partial::{Conflict, JoinResult, PartialJoin};
pub use psbt::global;
