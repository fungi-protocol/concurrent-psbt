fn main() {
    loop {
        honggfuzz::fuzz!(|data: &[u8]| {
            concurrent_psbt_fuzz_targets::crash_test(data);
        });
    }
}
