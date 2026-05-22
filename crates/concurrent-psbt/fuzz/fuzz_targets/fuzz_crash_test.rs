#![no_main]

use concurrent_psbt_fuzz_support::fuzzer_verify_crash;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    fuzzer_verify_crash(data);
});
