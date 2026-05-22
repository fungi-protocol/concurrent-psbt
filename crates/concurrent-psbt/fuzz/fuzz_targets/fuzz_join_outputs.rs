#![no_main]

use concurrent_psbt_fuzz_support::fuzz_join_outputs;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    fuzz_join_outputs(data);
});
