use concurrent_psbt_fuzz_support::fuzz_join_inputs;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        fuzz_join_inputs(data);
    });
}
