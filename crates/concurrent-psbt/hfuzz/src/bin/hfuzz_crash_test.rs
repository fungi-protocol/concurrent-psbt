use concurrent_psbt_fuzz_support::fuzzer_verify_crash;
use honggfuzz::fuzz;

fn main() {
    loop {
        fuzz!(|data: &[u8]| {
            fuzzer_verify_crash(data);
        });
    }
}
