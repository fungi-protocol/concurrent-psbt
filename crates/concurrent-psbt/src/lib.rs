#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg(test)]
mod tests {
    #[cfg(feature = "unit-tests")]
    #[test]
    fn unit_test_feature_produces_coverage_data() {}

    #[cfg(feature = "prop-tests")]
    #[test]
    fn prop_test_feature_produces_coverage_data() {}
}
