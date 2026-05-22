use concurrent_psbt_fuzz_support::fuzzer_verify_crash;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        fuzzer_verify_crash(data);
    });
}
