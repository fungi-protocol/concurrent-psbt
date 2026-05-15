#![no_main]

use libfuzzer_sys::fuzz_target;

// TODO simplify this target... is the feature needed or is it better to have
// this do nothing by default so it's easy to run all fuzzing targets?
fuzz_target!(|data: &[u8]| {
    // Multi-step state machine: requires coverage guidance to solve.
    // Each step depends on the previous, so random fuzzing is unlikely
    // to find the crash within the timeout.
    #[cfg(fuzzer_verify_crash)]
    {
        let mut state: u32 = 0;
        for &b in data {
            state = match (state, b) {
                (0, 0x7a) => 1,
                (1, 0x39) => 2,
                (2, 0xf1) => 3,
                (3, 0x42) => 4,
                (4, 0xce) => 5,
                (5, 0x8d) => 6,
                (6, 0xa7) => 7,
                (7, 0x05) => panic!("fuzzer verification crash"),
                _ => 0,
            };
        }
    }
    let _ = data;
});
