#![forbid(unsafe_code)]
#![allow(unused_features)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tracing_subscriber::prelude::*;

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn record_test<T>(name: &str, test: impl FnOnce() -> T) -> T {
        let Some(trace_dir) = std::env::var_os("NEXTEST_TRACE_DIR") else {
            return test();
        };

        std::fs::create_dir_all(&trace_dir).expect("create nextest trace directory");
        let trace_file = Path::new(&trace_dir).join(format!("{name}.json"));
        let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file(trace_file)
            .include_args(true)
            .build();
        let subscriber = tracing_subscriber::registry().with(chrome_layer);
        let result = tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("test", test.name = name);
            let _entered = span.enter();
            test()
        });
        drop(guard);
        result
    }

    #[cfg(feature = "unit-tests")]
    #[test]
    fn unit_test_feature_produces_coverage_data() {
        record_test("unit_test_feature_produces_coverage_data", || {});
    }

    #[cfg(feature = "prop-tests")]
    #[test]
    fn prop_test_feature_produces_coverage_data() {
        record_test("prop_test_feature_produces_coverage_data", || {});
    }
}
