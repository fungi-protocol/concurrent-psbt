use concurrent_psbt_fuzz_support::fuzz_join_inputs;
use honggfuzz::fuzz;

fn main() {
    loop {
        fuzz!(|data: &[u8]| {
            fuzz_join_inputs(data);
        });
    }
}
