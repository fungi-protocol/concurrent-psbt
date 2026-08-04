use psbt_v2::bitcoin::Transaction;
use psbt_v2::bitcoin::consensus::encode::{VarInt, deserialize, serialize};
use psbt_v2::raw::Pair;

use crate::decode::PairDecode;
use crate::fields::{GlobalInsensitive, InputInsensitive, OutputInsensitive, PSBT_V2};

/// PSBT magic bytes: "psbt\xff"
const MAGIC: [u8; 5] = [0x70, 0x73, 0x62, 0x74, 0xff];

/// Scrub a PSBT, retaining only non-sensitive fields safe to share with untrusted peers.
///
/// Buffers the global map to detect version and input/output counts, then streams
/// the remaining maps applying per-map-type filters. Both PSBT v0 and v2 are supported.
pub fn scrub(psbt: &[u8]) -> Result<Vec<u8>, Error> {
    if psbt.get(..5) != Some(&MAGIC) {
        return Err(Error::InvalidMagic);
    }
    let mut r = &psbt[5..];
    let mut out = Vec::with_capacity(psbt.len());
    out.extend_from_slice(&MAGIC);

    // Buffer the global map to detect version and input/output counts before streaming the rest.
    let mut global: Vec<Pair> = Vec::new();
    while let Some(pair) = Pair::decode(&mut r)? {
        global.push(pair);
    }

    let (n_inputs, n_outputs) = get_number_of_inputs_and_outputs(&global)?;
    for pair in &global {
        if GlobalInsensitive::contains(pair.key.type_value) {
            encode_pair(&mut out, pair);
        }
    }
    out.push(0x00);

    for _ in 0..n_inputs {
        while let Some(pair) = Pair::decode(&mut r)? {
            if InputInsensitive::contains(pair.key.type_value) {
                encode_pair(&mut out, &pair);
            }
        }
        out.push(0x00);
    }

    for _ in 0..n_outputs {
        while let Some(pair) = Pair::decode(&mut r)? {
            if OutputInsensitive::contains(pair.key.type_value) {
                encode_pair(&mut out, &pair);
            }
        }
        out.push(0x00);
    }

    if !r.is_empty() {
        return Err(Error::UnexpectedTrailingBytes);
    }

    Ok(out)
}

/// Errors that can occur while scrubbing a PSBT.
#[derive(Debug, PartialEq)]
pub enum Error {
    InvalidMagic,
    UnexpectedEof,
    InvalidGlobal,
    /// A pair declared a key longer than `MAX_VEC_SIZE`, which would force an
    /// oversized allocation before the key could be read.
    OversizedKey,
    UnexpectedTrailingBytes,
}

impl std::fmt::Display for Error {
    // Excluded from coverage so the prop-test-only run is not required to format every variant;
    // the messages are asserted in the unit tests.
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidMagic => write!(f, "invalid PSBT magic bytes"),
            Error::UnexpectedEof => write!(f, "unexpected end of input"),
            Error::InvalidGlobal => write!(f, "invalid or missing global map fields"),
            Error::OversizedKey => write!(f, "key exceeds maximum allowed size"),
            Error::UnexpectedTrailingBytes => write!(f, "unexpected trailing bytes"),
        }
    }
}

impl std::error::Error for Error {}

fn get_number_of_inputs_and_outputs(global: &[Pair]) -> Result<(u64, u64), Error> {
    let is_v2 = global
        .iter()
        .find(|p| p.key.type_value == GlobalInsensitive::Version as u8 && p.key.key.is_empty())
        .and_then(|p| p.value.first().copied())
        .map(|v| v == PSBT_V2)
        .unwrap_or(false);

    if is_v2 {
        let n_in = global
            .iter()
            .find(|p| {
                p.key.type_value == GlobalInsensitive::InputCount as u8 && p.key.key.is_empty()
            })
            .and_then(|p| deserialize(&p.value).ok())
            .map(|VarInt(n)| n)
            .ok_or(Error::InvalidGlobal)?;
        let n_out = global
            .iter()
            .find(|p| {
                p.key.type_value == GlobalInsensitive::OutputCount as u8 && p.key.key.is_empty()
            })
            .and_then(|p| deserialize(&p.value).ok())
            .map(|VarInt(n)| n)
            .ok_or(Error::InvalidGlobal)?;
        return Ok((n_in, n_out));
    }
    let tx_bytes = global
        .iter()
        .find(|p| p.key.type_value == GlobalInsensitive::UnsignedTx as u8 && p.key.key.is_empty())
        .map(|p| p.value.as_slice())
        .ok_or(Error::InvalidGlobal)?;
    let tx: Transaction = deserialize(tx_bytes).map_err(|_| Error::InvalidGlobal)?;
    Ok((tx.input.len() as u64, tx.output.len() as u64))
}

fn encode_pair(out: &mut Vec<u8>, pair: &Pair) {
    out.extend_from_slice(&serialize(&VarInt::from(pair.key.key.len() + 1)));
    out.extend_from_slice(&serialize(&pair.key.type_value));
    out.extend_from_slice(&pair.key.key);
    out.extend_from_slice(&serialize(&VarInt::from(pair.value.len() as u64)));
    out.extend_from_slice(&pair.value);
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "unit-tests")]
    mod unit {
        #![allow(dead_code)]
        use super::super::*;
        use psbt_v2::raw::Key;

        /// Longest key a pair may declare before `decode` rejects it outright.
        const MAX_KEY_LEN: u64 = psbt_v2::bitcoin::consensus::encode::MAX_VEC_SIZE as u64;

        fn kv(type_value: u8, key_suffix: &[u8], val: &[u8]) -> Vec<u8> {
            let pair = Pair {
                key: Key {
                    type_value,
                    key: key_suffix.to_vec(),
                },
                value: val.to_vec(),
            };
            let mut buf = Vec::new();
            encode_pair(&mut buf, &pair);
            buf
        }

        fn kv_global(key: GlobalInsensitive, val: &[u8]) -> Vec<u8> {
            kv(key as u8, &[], val)
        }

        fn kv_input(key: InputInsensitive, key_suffix: &[u8], val: &[u8]) -> Vec<u8> {
            kv(key as u8, key_suffix, val)
        }

        fn kv_output(key: OutputInsensitive, val: &[u8]) -> Vec<u8> {
            kv(key as u8, &[], val)
        }

        fn v2_global(input_count: u8, output_count: u8, extra: &[Vec<u8>]) -> Vec<u8> {
            let mut map = Vec::new();
            map.extend(kv_global(GlobalInsensitive::Version, &[2, 0, 0, 0]));
            map.extend(kv_global(GlobalInsensitive::TxVersion, &[2, 0, 0, 0]));
            map.extend(kv_global(GlobalInsensitive::InputCount, &[input_count]));
            map.extend(kv_global(GlobalInsensitive::OutputCount, &[output_count]));
            for e in extra {
                map.extend(e);
            }
            map.push(0x00);
            map
        }

        fn v2_psbt(
            input_count: u8,
            output_count: u8,
            global_extra: &[Vec<u8>],
            maps: &[Vec<u8>],
        ) -> Vec<u8> {
            let mut buf = MAGIC.to_vec();
            buf.extend(v2_global(input_count, output_count, global_extra));
            for m in maps {
                buf.extend(m);
            }
            buf
        }

        fn dummy_tx(input_count: u8, output_count: u8) -> Vec<u8> {
            let mut tx = Vec::new();
            tx.extend_from_slice(&1u32.to_le_bytes());
            tx.push(input_count);
            for _ in 0..input_count {
                tx.extend_from_slice(&[0u8; 32]);
                tx.extend_from_slice(&0u32.to_le_bytes());
                tx.push(0x00);
                tx.extend_from_slice(&u32::MAX.to_le_bytes());
            }
            tx.push(output_count);
            for _ in 0..output_count {
                tx.extend_from_slice(&1000u64.to_le_bytes());
                tx.push(0x00);
            }
            tx.extend_from_slice(&0u32.to_le_bytes());
            tx
        }

        #[test]
        fn scrub_empty_v2_roundtrip() {
            let psbt = v2_psbt(0, 0, &[], &[]);
            assert_eq!(scrub(&psbt).unwrap(), psbt);
        }

        #[test]
        fn invalid_global_v0_invalid_tx() {
            // v0 PSBT with invalid transaction data
            let mut psbt = MAGIC.to_vec();
            psbt.extend(kv_global(GlobalInsensitive::UnsignedTx, &[0xFF, 0xFF]));
            psbt.push(0x00);
            assert_eq!(scrub(&psbt), Err(Error::InvalidGlobal));
        }

        /// A PSBT whose first pair declares a key of `key_len` bytes but supplies none of them.
        fn psbt_with_declared_key_len(key_len: u64) -> Vec<u8> {
            let mut psbt = MAGIC.to_vec();
            // A pair's leading VarInt covers the type byte plus the key bytes.
            psbt.extend(serialize(&VarInt::from(key_len + 1)));
            psbt
        }

        #[test]
        fn oversized_key_rejected_before_allocating() {
            // One byte past the limit: rejected on the declared length alone, without
            // reading (or allocating) the key.
            let psbt = psbt_with_declared_key_len(MAX_KEY_LEN + 1);
            assert_eq!(scrub(&psbt), Err(Error::OversizedKey));
        }

        #[test]
        fn key_at_size_limit_is_accepted() {
            // Exactly at the limit the length check must pass, so decoding proceeds and
            // fails on the missing key bytes instead.
            let psbt = psbt_with_declared_key_len(MAX_KEY_LEN);
            assert_eq!(scrub(&psbt), Err(Error::UnexpectedEof));
        }

        #[test]
        fn truncated_varint_is_not_an_oversized_key() {
            // 0xFF introduces an 8-byte VarInt, but only four bytes follow.
            let mut psbt = MAGIC.to_vec();
            psbt.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
            assert_eq!(scrub(&psbt), Err(Error::UnexpectedEof));
        }

        #[test]
        fn error_messages() {
            assert_eq!(Error::InvalidMagic.to_string(), "invalid PSBT magic bytes");
            assert_eq!(Error::UnexpectedEof.to_string(), "unexpected end of input");
            assert_eq!(
                Error::InvalidGlobal.to_string(),
                "invalid or missing global map fields"
            );
            assert_eq!(
                Error::OversizedKey.to_string(),
                "key exceeds maximum allowed size"
            );
            assert_eq!(
                Error::UnexpectedTrailingBytes.to_string(),
                "unexpected trailing bytes"
            );
        }

        #[test]
        fn invalid_pair_value_truncated() {
            // Pair with VarInt-encoded value size but missing value data
            let mut psbt = MAGIC.to_vec();
            psbt.extend(v2_global(1, 1, &[]));
            psbt.push(0x00); // End global
            psbt.push(0x05); // VarInt key size
            psbt.push(InputInsensitive::WitnessUtxo as u8);
            // Missing key data and value should trigger UnexpectedEof
            assert_eq!(scrub(&psbt), Err(Error::UnexpectedEof));
        }

        #[test]
        fn scrub_input_with_multiple_maps() {
            let witness_utxo = kv_input(InputInsensitive::WitnessUtxo, &[], &[0xAA]);
            let amount = kv_output(
                OutputInsensitive::Amount,
                &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            );

            let mut input = Vec::new();
            for _ in 0..2 {
                input.extend(&witness_utxo);
                // BIP32_DERIVATION (sensitive)
                input.extend(&kv(0x06, &[0x02, 0x03], &[0xFF]));
                input.push(0x00);
            }

            let mut output = Vec::new();
            for _ in 0..2 {
                output.extend(&amount);
                // PROPRIETARY (sensitive)
                output.extend(&kv(0xFC, &[0x01], &[0xFF]));
                output.push(0x00);
            }

            let psbt = v2_psbt(2, 2, &[], &[input, output.clone()]);
            let result = scrub(&psbt).unwrap();

            let mut expected_input = Vec::new();
            for _ in 0..2 {
                expected_input.extend(&witness_utxo);
                expected_input.push(0x00);
            }

            let mut expected_output = Vec::new();
            for _ in 0..2 {
                expected_output.extend(&amount);
                expected_output.push(0x00);
            }

            let expected = v2_psbt(2, 2, &[], &[expected_input, expected_output]);
            assert_eq!(result, expected);
        }

        #[test]
        fn scrub_v2_tx_not_modifiable_strips_sensitive_fields() {
            // PSBT_GLOBAL_TX_MODIFIABLE = 0x06 set to 0. tx not modifiable, scrubbing still applies.
            let tx_not_modifiable = kv_global(GlobalInsensitive::TxModifiable, &[0x00]);
            let witness_utxo = kv_input(InputInsensitive::WitnessUtxo, &[], &[0xAA]);
            let amount = kv_output(
                OutputInsensitive::Amount,
                &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            );

            // POR_COMMITMENT (sensitive global)
            let sensitive_global = kv(0x09, &[], &[0xDE, 0xAD]);

            let mut input_map = Vec::new();
            input_map.extend(&witness_utxo);
            // BIP32_DERIVATION (sensitive input)
            input_map.extend(&kv(0x06, &[0x02, 0x03], &[0xFF]));
            input_map.push(0x00);

            let mut output_map = Vec::new();
            output_map.extend(&amount);
            // PROPRIETARY (sensitive output)
            output_map.extend(&kv(0xFC, &[0x01], &[0xFF]));
            output_map.push(0x00);

            let psbt = v2_psbt(
                1,
                1,
                &[sensitive_global, tx_not_modifiable.clone()],
                &[input_map, output_map],
            );
            let result = scrub(&psbt).unwrap();

            let mut expected_input = Vec::new();
            expected_input.extend(&witness_utxo);
            expected_input.push(0x00);

            let mut expected_output = Vec::new();
            expected_output.extend(&amount);
            expected_output.push(0x00);

            let expected = v2_psbt(
                1,
                1,
                &[tx_not_modifiable],
                &[expected_input, expected_output],
            );
            assert_eq!(result, expected);
        }

        #[test]
        fn scrub_v0() {
            let tx = dummy_tx(1, 1);
            let unsigned_tx = kv_global(GlobalInsensitive::UnsignedTx, &tx);
            // POR_COMMITMENT (sensitive)
            let sensitive_global = kv(0x09, &[], &[0xDE, 0xAD]);

            let mut global = Vec::new();
            global.extend(&unsigned_tx);
            global.extend(&sensitive_global);
            global.push(0x00);

            let witness_utxo = kv_input(InputInsensitive::WitnessUtxo, &[], &[0xAA]);
            // TAP_BIP32_DERIVATION (sensitive)
            let tap_bip32_input = kv(0x06, &[0x02, 0x03], &[0xFF]);

            let mut input_map = Vec::new();
            input_map.extend(&witness_utxo);
            input_map.extend(&tap_bip32_input);
            input_map.push(0x00);

            // unknown output key type (sensitive)
            let mut output_map = kv(0x17, &[], &[0xCC]);

            output_map.push(0x00);

            let mut psbt = MAGIC.to_vec();
            psbt.extend(&global);
            psbt.extend(&input_map);
            psbt.extend(&output_map);

            let result = scrub(&psbt).unwrap();

            let mut expected_global = Vec::new();
            expected_global.extend(&unsigned_tx);
            expected_global.push(0x00);
            let mut expected_input = Vec::new();
            expected_input.extend(&witness_utxo);
            expected_input.push(0x00);

            let mut expected = MAGIC.to_vec();
            expected.extend(&expected_global);
            expected.extend(&expected_input);
            expected.extend(vec![0x00]);

            assert_eq!(result, expected);
        }

        #[test]
        fn v2_detected_when_another_field_precedes_version() {
            // Version detection must key off the VERSION field specifically, not merely the
            // first global pair with an empty key suffix. TX_VERSION is 1 here so mistaking it
            // for VERSION would read as v0.
            let mut global = Vec::new();
            global.extend(kv_global(GlobalInsensitive::TxVersion, &[1, 0, 0, 0]));
            global.extend(kv_global(GlobalInsensitive::Version, &[2, 0, 0, 0]));
            global.extend(kv_global(GlobalInsensitive::InputCount, &[0]));
            global.extend(kv_global(GlobalInsensitive::OutputCount, &[0]));
            global.push(0x00);

            let mut psbt = MAGIC.to_vec();
            psbt.extend(&global);
            // Every global here is insensitive, so scrubbing is a no-op.
            assert_eq!(scrub(&psbt).unwrap(), psbt);
        }

        #[test]
        fn v0_unsigned_tx_found_after_another_global() {
            // Same for UNSIGNED_TX: an explicit VERSION=0 pair precedes it, and picking that
            // pair's value as the transaction would fail to deserialize.
            let tx = dummy_tx(1, 1);
            let unsigned_tx = kv_global(GlobalInsensitive::UnsignedTx, &tx);
            let version = kv_global(GlobalInsensitive::Version, &[0, 0, 0, 0]);

            let mut psbt = MAGIC.to_vec();
            psbt.extend(&version);
            psbt.extend(&unsigned_tx);
            psbt.push(0x00);
            psbt.push(0x00); // empty input map
            psbt.push(0x00); // empty output map

            assert_eq!(scrub(&psbt).unwrap(), psbt);
        }

        #[test]
        fn invalid_magic() {
            assert_eq!(scrub(b"not a psbt"), Err(Error::InvalidMagic));
        }

        #[test]
        fn unexpected_eof_truncated_after_magic() {
            assert_eq!(scrub(&MAGIC), Err(Error::UnexpectedEof));
        }

        #[test]
        fn unexpected_eof_truncated_mid_map() {
            let mut psbt = MAGIC.to_vec();
            psbt.push(0x05); // key length = 5 but no data follows
            assert_eq!(scrub(&psbt), Err(Error::UnexpectedEof));
        }

        #[test]
        fn unexpected_eof_trailing_bytes() {
            // A complete PSBT followed by leftover bytes must be rejected.
            let mut psbt = v2_psbt(0, 0, &[], &[]);
            psbt.push(0xFF);
            assert_eq!(scrub(&psbt), Err(Error::UnexpectedTrailingBytes));
        }

        #[test]
        fn invalid_global_v2_missing_counts() {
            // VERSION present and v2, but INPUT_COUNT and OUTPUT_COUNT absent.
            let mut psbt = MAGIC.to_vec();
            psbt.extend(kv_global(GlobalInsensitive::Version, &[2, 0, 0, 0]));
            psbt.push(0x00);
            assert_eq!(scrub(&psbt), Err(Error::InvalidGlobal));
        }

        #[test]
        fn invalid_global_v0_missing_unsigned_tx() {
            // v0 PSBT with no UNSIGNED_TX field.
            let mut psbt = MAGIC.to_vec();
            // POR_COMMITMENT (sensitive), not UNSIGNED_TX
            psbt.extend(kv(0x09, &[], &[0xDE, 0xAD]));
            psbt.push(0x00);
            assert_eq!(scrub(&psbt), Err(Error::InvalidGlobal));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::super::*;
        use proptest::prelude::*;
        use psbt_v2::raw::Key;

        /// Longest key a pair may declare before `decode` rejects it outright.
        const MAX_KEY_LEN: u64 = psbt_v2::bitcoin::consensus::encode::MAX_VEC_SIZE as u64;

        fn arb_value() -> impl Strategy<Value = Vec<u8>> {
            proptest::collection::vec(any::<u8>(), 0..=64)
        }

        fn encoded_pair(type_value: u8, key: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
            let pair = Pair {
                key: Key { type_value, key },
                value,
            };
            let mut buf = Vec::new();
            encode_pair(&mut buf, &pair);
            buf
        }

        fn encoded_global(key: GlobalInsensitive, value: Vec<u8>) -> Vec<u8> {
            encoded_pair(key as u8, vec![], value)
        }

        fn encoded_input(key: InputInsensitive, key_suffix: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
            encoded_pair(key as u8, key_suffix, value)
        }

        fn encoded_output(key: OutputInsensitive, key_suffix: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
            encoded_pair(key as u8, key_suffix, value)
        }

        #[derive(Clone, Copy, Debug)]
        enum InsensitiveField {
            Global(GlobalInsensitive),
            Input(InputInsensitive),
            Output(OutputInsensitive),
        }

        fn arb_global_insensitive() -> impl Strategy<Value = GlobalInsensitive> {
            use GlobalInsensitive::*;
            proptest::sample::select(
                [
                    UnsignedTx,
                    TxVersion,
                    FallbackLocktime,
                    InputCount,
                    OutputCount,
                    TxModifiable,
                    Version,
                ]
                .to_vec(),
            )
        }

        fn arb_input_insensitive() -> impl Strategy<Value = InputInsensitive> {
            use InputInsensitive::*;
            proptest::sample::select(
                [
                    NonWitnessUtxo,
                    WitnessUtxo,
                    SighashType,
                    RedeemScript,
                    WitnessScript,
                    FinalScriptsig,
                    FinalScriptwitness,
                    PreviousTxid,
                    OutputIndex,
                    Sequence,
                    RequiredTimeLocktime,
                    RequiredHeightLocktime,
                    TapKeySig,
                    TapScriptSig,
                    TapLeafScript,
                ]
                .to_vec(),
            )
        }

        fn arb_output_insensitive() -> impl Strategy<Value = OutputInsensitive> {
            use OutputInsensitive::*;
            proptest::sample::select([Amount, Script].to_vec())
        }

        fn arb_insensitive_field() -> impl Strategy<Value = InsensitiveField> {
            prop_oneof![
                arb_global_insensitive().prop_map(InsensitiveField::Global),
                arb_input_insensitive().prop_map(InsensitiveField::Input),
                arb_output_insensitive().prop_map(InsensitiveField::Output),
            ]
        }

        fn encode_insensitive(field: InsensitiveField, value: Vec<u8>) -> Vec<u8> {
            match field {
                InsensitiveField::Global(key) => encoded_global(key, value),
                InsensitiveField::Input(key) => encoded_input(key, vec![], value),
                InsensitiveField::Output(key) => encoded_output(key, vec![], value),
            }
        }

        fn build_psbt_with_insensitive(field: InsensitiveField, pair: &[u8]) -> Vec<u8> {
            let mut test_psbt = MAGIC.to_vec();
            let (n_inputs, n_outputs) = match field {
                InsensitiveField::Global(_) => (0, 0),
                InsensitiveField::Input(_) => (1, 0),
                InsensitiveField::Output(_) => (0, 1),
            };
            let extra_global = match field {
                InsensitiveField::Global(_) => &[pair][..],
                _ => &[],
            };
            append_v2_global_fields(&mut test_psbt, n_inputs, n_outputs, extra_global);
            if !matches!(field, InsensitiveField::Global(_)) {
                test_psbt.extend_from_slice(pair);
                test_psbt.push(0x00);
            }
            test_psbt
        }

        fn arb_pair() -> impl Strategy<Value = Vec<u8>> {
            (any::<u8>(), arb_value()).prop_map(|(t, v)| encoded_pair(t, vec![], v))
        }

        fn arb_map() -> impl Strategy<Value = Vec<u8>> {
            proptest::collection::vec(arb_pair(), 0..=4).prop_map(|pairs| {
                let mut map: Vec<u8> = pairs.into_iter().flatten().collect();
                map.push(0x00);
                map
            })
        }

        fn append_v2_global_fields(
            psbt: &mut Vec<u8>,
            n_inputs: u8,
            n_outputs: u8,
            extra: &[&[u8]],
        ) {
            psbt.extend(encoded_global(GlobalInsensitive::Version, vec![2, 0, 0, 0]));
            psbt.extend(encoded_global(
                GlobalInsensitive::TxVersion,
                vec![2, 0, 0, 0],
            ));
            psbt.extend(encoded_global(
                GlobalInsensitive::InputCount,
                vec![n_inputs],
            ));
            psbt.extend(encoded_global(
                GlobalInsensitive::OutputCount,
                vec![n_outputs],
            ));
            for field in extra {
                psbt.extend_from_slice(field);
            }
            psbt.push(0x00);
        }

        /// A minimal but consensus-valid transaction with the requested input and output counts,
        /// as carried by a v0 PSBT's `UNSIGNED_TX` global.
        fn unsigned_tx(n_inputs: u8, n_outputs: u8) -> Vec<u8> {
            let mut tx = Vec::new();
            tx.extend_from_slice(&1u32.to_le_bytes()); // version
            tx.push(n_inputs);
            for _ in 0..n_inputs {
                tx.extend_from_slice(&[0u8; 32]); // previous txid
                tx.extend_from_slice(&0u32.to_le_bytes()); // previous vout
                tx.push(0x00); // empty script_sig
                tx.extend_from_slice(&u32::MAX.to_le_bytes()); // sequence
            }
            tx.push(n_outputs);
            for _ in 0..n_outputs {
                tx.extend_from_slice(&1000u64.to_le_bytes()); // amount
                tx.push(0x00); // empty script_pubkey
            }
            tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
            tx
        }

        prop_compose! {
            /// A v0 PSBT: no VERSION global, so the counts come from `UNSIGNED_TX`.
            fn arb_v0_psbt()(
                n_inputs in 1u8..=3,
                n_outputs in 0u8..=3,
            )(
                input_maps in proptest::collection::vec(arb_map(), n_inputs as usize),
                output_maps in proptest::collection::vec(arb_map(), n_outputs as usize),
                n_inputs in Just(n_inputs),
                n_outputs in Just(n_outputs),
            ) -> Vec<u8> {
                // No arbitrary extra globals here: a generated VERSION pair would switch the
                // PSBT to v2 and change which branch is under test.
                let mut psbt = MAGIC.to_vec();
                psbt.extend(encoded_global(
                    GlobalInsensitive::UnsignedTx,
                    unsigned_tx(n_inputs, n_outputs),
                ));
                psbt.push(0x00);
                for map in input_maps { psbt.extend(map); }
                for map in output_maps { psbt.extend(map); }
                psbt
            }
        }

        prop_compose! {
            fn arb_v2_psbt()(
                n_inputs in 0u8..=3,
                n_outputs in 0u8..=3,
            )(
                extra_global in arb_map().prop_map(|m| m[..m.len()-1].to_vec()),
                input_maps in proptest::collection::vec(arb_map(), n_inputs as usize),
                output_maps in proptest::collection::vec(arb_map(), n_outputs as usize),
                n_inputs in Just(n_inputs),
                n_outputs in Just(n_outputs),
            ) -> Vec<u8> {
                let mut psbt = MAGIC.to_vec();
                append_v2_global_fields(
                    &mut psbt,
                    n_inputs,
                    n_outputs,
                    &[&extra_global],
                );
                for map in input_maps { psbt.extend(map); }
                for map in output_maps { psbt.extend(map); }
                psbt
            }
        }

        proptest! {
            #[test]
            fn idempotent(psbt in arb_v2_psbt()) {
                if let Ok(once) = scrub(&psbt) {
                    let twice = scrub(&once).expect("scrub of scrubbed output must succeed");
                    prop_assert_eq!(once, twice);
                }
            }

            #[test]
            fn output_is_valid_psbt(psbt in arb_v2_psbt()) {
                if let Ok(scrubbed) = scrub(&psbt) {
                    prop_assert!(scrub(&scrubbed).is_ok());
                }
            }

            #[test]
            fn sensitive_fields_absent_from_output(
                sensitive_type in proptest::sample::select(vec![
                    0x02u8, // PARTIAL_SIG
                    0x06,   // BIP32_DERIVATION
                    0x16,   // TAP_BIP32_DERIVATION
                    0x17,   // TAP_INTERNAL_KEY
                    0xFC,   // PROPRIETARY
                ])
            ) {
                let sensitive_pair = encoded_pair(sensitive_type, vec![0xAA], vec![0xBB]);

                let mut test_psbt = MAGIC.to_vec();
                append_v2_global_fields(&mut test_psbt, 1, 0, &[]);
                test_psbt.extend(&sensitive_pair);
                test_psbt.push(0x00);

                let result = scrub(&test_psbt).unwrap();
                prop_assert!(!result.windows(sensitive_pair.len()).any(|w| w == sensitive_pair));
            }

            #[test]
            fn insensitive_fields_preserved(
                value in arb_value(),
                field in arb_insensitive_field(),
            ) {
                let pair = encode_insensitive(field, value);
                let test_psbt = build_psbt_with_insensitive(field, &pair);

                let result = scrub(&test_psbt).unwrap();
                prop_assert!(result.windows(pair.len()).any(|w| w == pair));
            }

            /// v0 PSBTs take their input and output counts from `UNSIGNED_TX`, so scrubbing
            /// must round-trip them just as it does v2.
            #[test]
            fn v0_scrub_is_idempotent(psbt in arb_v0_psbt()) {
                let once = scrub(&psbt).expect("valid v0 PSBT must scrub");
                let twice = scrub(&once).expect("scrub of scrubbed output must succeed");
                prop_assert_eq!(once, twice);
            }

            /// The `UNSIGNED_TX` global is insensitive, so a v0 PSBT keeps it verbatim and the
            /// scrubbed output still describes the same number of inputs and outputs.
            #[test]
            fn v0_unsigned_tx_preserved(
                n_inputs in 1u8..=3,
                n_outputs in 0u8..=3,
            ) {
                let pair = encoded_global(
                    GlobalInsensitive::UnsignedTx,
                    unsigned_tx(n_inputs, n_outputs),
                );
                let mut psbt = MAGIC.to_vec();
                psbt.extend(&pair);
                psbt.push(0x00);
                // One empty map per declared input and output.
                psbt.extend(std::iter::repeat_n(
                    0x00,
                    n_inputs as usize + n_outputs as usize,
                ));

                let result = scrub(&psbt).expect("valid v0 PSBT must scrub");
                prop_assert!(result.windows(pair.len()).any(|w| w == pair));
            }

            /// Anything not starting with the magic bytes is rejected before parsing.
            #[test]
            fn invalid_magic_rejected(psbt in proptest::collection::vec(any::<u8>(), 0..=32)) {
                prop_assume!(psbt.get(..5) != Some(&MAGIC[..]));
                prop_assert_eq!(scrub(&psbt), Err(Error::InvalidMagic));
            }

            /// Bytes left over once every declared map has been consumed are an error, however
            /// well-formed the PSBT before them was.
            #[test]
            fn trailing_bytes_rejected(
                psbt in arb_v2_psbt(),
                trailing in proptest::collection::vec(any::<u8>(), 1..=8),
            ) {
                prop_assume!(scrub(&psbt).is_ok());
                let mut extended = psbt;
                extended.extend(trailing);
                prop_assert_eq!(scrub(&extended), Err(Error::UnexpectedTrailingBytes));
            }

            /// Any declared key length above the limit is rejected on the length alone,
            /// while the limit itself stays decodable (and here fails on the absent key bytes).
            #[test]
            fn oversized_key_rejected(excess in 1u64..=u32::MAX as u64) {
                let mut psbt = MAGIC.to_vec();
                psbt.extend(serialize(&VarInt::from(MAX_KEY_LEN + excess + 1)));
                prop_assert_eq!(scrub(&psbt), Err(Error::OversizedKey));

                let mut at_limit = MAGIC.to_vec();
                at_limit.extend(serialize(&VarInt::from(MAX_KEY_LEN + 1)));
                prop_assert_eq!(scrub(&at_limit), Err(Error::UnexpectedEof));
            }
        }
    }
}
