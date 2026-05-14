#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() >= 4 && data[0] == b'B' && data[1] == b'O' && data[2] == b'O' && data[3] == b'M' {
        panic!("deliberate crash for fuzzer verification");
    }
});
