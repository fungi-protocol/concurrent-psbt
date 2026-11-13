#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| concurrent_psbt_fuzz_targets::scrub(data));
