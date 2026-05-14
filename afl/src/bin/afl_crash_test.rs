fn main() {
    afl::fuzz!(|data: &[u8]| {
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
}
