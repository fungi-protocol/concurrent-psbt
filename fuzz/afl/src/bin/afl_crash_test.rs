fn main() {
    afl::fuzz!(|data: &[u8]| {
        concurrent_psbt_fuzz_targets::crash_test(data);
    });
}
