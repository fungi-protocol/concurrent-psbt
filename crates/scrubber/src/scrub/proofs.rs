//! Kani harnesses for the scrubber, run by `checks.formal-kani-scrubber`.

use super::*;

/// `Pair::decode` never panics, and whatever it accepts re-encodes to exactly the bytes it
/// consumed: the scrubber's output encoding agrees with its input decoding.
#[kani::proof]
#[kani::unwind(7)]
#[kani::solver(kissat)]
fn decode_encode_roundtrip() {
    let buf: [u8; 6] = kani::any();
    let len: usize = kani::any_where(|n| *n <= buf.len());
    let input = &buf[..len];
    let mut rest = input;
    if let Ok(Some(pair)) = Pair::decode(&mut rest) {
        let consumed = &input[..len - rest.len()];
        let mut out = Vec::new();
        encode_pair(&mut out, &pair.key, &pair.value);
        assert!(out.as_slice() == consumed);
    }
}

/// A concrete v2 global map declaring one input and no outputs. Keeping it concrete spares
/// CBMC from exploring `Transaction` deserialization (the v0 path), so the symbolic bytes go
/// to the input map that the filter actually acts on.
const V2_ONE_INPUT_GLOBALS: [u8; 16] = [
    0x01, 0xfb, 0x04, 0x02, 0x00, 0x00, 0x00, // PSBT_GLOBAL_VERSION = 2
    0x01, 0x04, 0x01, 0x01, // PSBT_GLOBAL_INPUT_COUNT = 1
    0x01, 0x05, 0x01, 0x00, // PSBT_GLOBAL_OUTPUT_COUNT = 0
    0x00, // end of global map
];

/// On a v2 PSBT whose input map is up to 6 arbitrary bytes, `scrub` never panics, never keeps
/// a sensitive input field, and its output is a fixpoint.
#[kani::proof]
#[kani::unwind(8)]
#[kani::solver(kissat)]
fn scrub_input_map() {
    let body: [u8; 6] = kani::any();
    let len: usize = kani::any_where(|n| *n <= body.len());
    let mut psbt = MAGIC.to_vec();
    psbt.extend_from_slice(&V2_ONE_INPUT_GLOBALS);
    psbt.extend_from_slice(&body[..len]);
    if let Ok(once) = scrub(&psbt) {
        let mut input_map = &once[MAGIC.len() + V2_ONE_INPUT_GLOBALS.len()..];
        while let Ok(Some(pair)) = Pair::decode(&mut input_map) {
            assert!(InputInsensitive::contains(pair.key.type_value));
        }
        assert!(scrub(&once) == Ok(once.clone()));
    }
}
