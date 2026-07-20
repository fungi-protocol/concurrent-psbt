#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

/// Deliberately uncovered code used to prove the 100% coverage gate fails closed.
pub fn uncovered_coverage_fixture() -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "unit-tests")]
    #[test]
    fn unit_test_feature_produces_coverage_data() {}

    #[cfg(feature = "prop-tests")]
    #[test]
    fn prop_test_feature_produces_coverage_data() {}
}
