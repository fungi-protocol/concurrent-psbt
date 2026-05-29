use psbt_v2::bitcoin::Transaction;
use psbt_v2::bitcoin::consensus::encode::{Decodable, Encodable, MAX_VEC_SIZE, VarInt};
use psbt_v2::bitcoin::io;
use psbt_v2::raw::{Key, Pair};

/// PSBT magic bytes: "psbt\xff"
const MAGIC: [u8; 5] = [0x70, 0x73, 0x62, 0x74, 0xff];

/// Global map key types retained when scrubbing (non-sensitive).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum GlobalInsensitive {
    UnsignedTx = 0x00,
    TxVersion = 0x02,
    FallbackLocktime = 0x03,
    InputCount = 0x04,
    OutputCount = 0x05,
    TxModifiable = 0x06,
    Version = 0xFB,
}

impl GlobalInsensitive {
    const ALL: &[Self] = &[
        Self::UnsignedTx,
        Self::TxVersion,
        Self::FallbackLocktime,
        Self::InputCount,
        Self::OutputCount,
        Self::TxModifiable,
        Self::Version,
    ];

    fn contains(type_value: u8) -> bool {
        Self::ALL.iter().any(|k| *k as u8 == type_value)
    }
}

/// Input map key types retained when scrubbing (non-sensitive).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum InputInsensitive {
    NonWitnessUtxo = 0x00,
    WitnessUtxo = 0x01,
    SighashType = 0x03,
    RedeemScript = 0x04,
    WitnessScript = 0x05,
    FinalScriptsig = 0x07,
    FinalScriptwitness = 0x08,
    PreviousTxid = 0x0e,
    OutputIndex = 0x0f,
    Sequence = 0x10,
    RequiredTimeLocktime = 0x11,
    RequiredHeightLocktime = 0x12,
    TapKeySig = 0x13,
    TapScriptSig = 0x14,
    TapLeafScript = 0x15,
}

impl InputInsensitive {
    const ALL: &[Self] = &[
        Self::NonWitnessUtxo,
        Self::WitnessUtxo,
        Self::SighashType,
        Self::RedeemScript,
        Self::WitnessScript,
        Self::FinalScriptsig,
        Self::FinalScriptwitness,
        Self::PreviousTxid,
        Self::OutputIndex,
        Self::Sequence,
        Self::RequiredTimeLocktime,
        Self::RequiredHeightLocktime,
        Self::TapKeySig,
        Self::TapScriptSig,
        Self::TapLeafScript,
    ];

    fn contains(type_value: u8) -> bool {
        Self::ALL.iter().any(|k| *k as u8 == type_value)
    }
}

/// Output map key types retained when scrubbing (non-sensitive).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputInsensitive {
    Amount = 0x03,
    Script = 0x04,
}

impl OutputInsensitive {
    const ALL: &[Self] = &[Self::Amount, Self::Script];

    fn contains(type_value: u8) -> bool {
        Self::ALL.iter().any(|k| *k as u8 == type_value)
    }
}

/// Errors that can occur while scrubbing a PSBT.
#[derive(Debug, PartialEq)]
pub enum Error {
    InvalidMagic,
    UnexpectedEof,
    InvalidGlobal,
}

impl std::fmt::Display for Error {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidMagic => write!(f, "invalid PSBT magic bytes"),
            Error::UnexpectedEof => write!(f, "unexpected end of input"),
            Error::InvalidGlobal => write!(f, "invalid or missing global map fields"),
        }
    }
}

impl std::error::Error for Error {}

/// Scrub a PSBT, retaining only non-sensitive fields safe to share with untrusted peers.
///
/// Buffers the global map to detect version and input/output counts, then streams
/// the remaining maps applying per-map-type filters. Both PSBT v0 and v2 are supported.
pub fn scrub(psbt: &[u8]) -> Result<Vec<u8>, Error> {
    if psbt.get(..5) != Some(&MAGIC) {
        return Err(Error::InvalidMagic);
    }
    let mut r = &psbt[5..];
    let mut out = MAGIC.to_vec();

    // Buffer the global map to detect version and input/output counts before streaming the rest.
    let mut global: Vec<Pair> = Vec::new();
    while let Some(pair) = Pair::decode(&mut r)? {
        global.push(pair);
    }

    let is_v2 = global
        .iter()
        .find(|p| p.key.type_value == 0xFB && p.key.key.is_empty())
        .and_then(|p| p.value.first().copied())
        .map(|v| v == 2)
        .unwrap_or(false);

    let (n_inputs, n_outputs) = if is_v2 {
        let n_in = global
            .iter()
            .find(|p| p.key.type_value == 0x04 && p.key.key.is_empty())
            .and_then(|p| VarInt::consensus_decode(&mut p.value.as_slice()).ok())
            .map(|VarInt(n)| n)
            .ok_or(Error::InvalidGlobal)?;
        let n_out = global
            .iter()
            .find(|p| p.key.type_value == 0x05 && p.key.key.is_empty())
            .and_then(|p| VarInt::consensus_decode(&mut p.value.as_slice()).ok())
            .map(|VarInt(n)| n)
            .ok_or(Error::InvalidGlobal)?;
        (n_in, n_out)
    } else {
        let tx_bytes = global
            .iter()
            .find(|p| p.key.type_value == 0x00 && p.key.key.is_empty())
            .map(|p| p.value.as_slice())
            .ok_or(Error::InvalidGlobal)?;
        let tx =
            Transaction::consensus_decode(&mut &tx_bytes[..]).map_err(|_| Error::InvalidGlobal)?;
        (tx.input.len() as u64, tx.output.len() as u64)
    };

    for pair in &global {
        if GlobalInsensitive::contains(pair.key.type_value) {
            encode_pair(&mut out, pair);
        }
    }
    out.push(0x00);

    let total_maps = n_inputs
        .checked_add(n_outputs)
        .ok_or(Error::InvalidGlobal)?;
    for map_idx in 0..(total_maps) {
        while let Some(pair) = Pair::decode(&mut r)? {
            let keep = if map_idx < n_inputs {
                InputInsensitive::contains(pair.key.type_value)
            } else {
                OutputInsensitive::contains(pair.key.type_value)
            };
            if keep {
                encode_pair(&mut out, &pair);
            }
        }
        out.push(0x00);
    }

    Ok(out)
}

// Workaround: Pair::decode and Key::decode are pub(crate) in psbt-v2 0.3.0.
// This replicates the upstream logic exactly using the same bitcoin primitives.
// This should be upstreamed in the future.
trait PairDecode: Sized {
    fn decode<R: io::Read + ?Sized>(r: &mut R) -> Result<Option<Self>, Error>;
}

impl PairDecode for Pair {
    fn decode<R: io::Read + ?Sized>(r: &mut R) -> Result<Option<Self>, Error> {
        let VarInt(byte_size) = VarInt::consensus_decode(r).map_err(|_| Error::UnexpectedEof)?;
        if byte_size == 0 {
            return Ok(None);
        }
        let key_byte_size = byte_size - 1;
        if key_byte_size > MAX_VEC_SIZE as u64 {
            return Err(Error::UnexpectedEof);
        }
        let type_value: u8 = Decodable::consensus_decode(r).map_err(|_| Error::UnexpectedEof)?;
        let mut key = Vec::with_capacity(key_byte_size as usize);
        for _ in 0..key_byte_size {
            key.push(Decodable::consensus_decode(r).map_err(|_| Error::UnexpectedEof)?);
        }
        let value: Vec<u8> = Decodable::consensus_decode(r).map_err(|_| Error::UnexpectedEof)?;
        Ok(Some(Pair {
            key: Key { type_value, key },
            value,
        }))
    }
}

fn encode_pair(out: &mut Vec<u8>, pair: &Pair) {
    VarInt::from(pair.key.key.len() + 1)
        .consensus_encode(out)
        .expect("Vec<u8> write is infallible");
    pair.key
        .type_value
        .consensus_encode(out)
        .expect("Vec<u8> write is infallible");
    out.extend_from_slice(&pair.key.key);
    VarInt::from(pair.value.len() as u64)
        .consensus_encode(out)
        .expect("Vec<u8> write is infallible");
    out.extend_from_slice(&pair.value);
}

#[cfg(any(test, feature = "unit-tests"))]
mod tests {
    #![allow(dead_code)]
    use super::*;

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

    #[test]
    fn invalid_pair_key_too_large() {
        // Pair key exceeds MAX_VEC_SIZE — this tests the boundary check
        let mut psbt = MAGIC.to_vec();
        psbt.push(0xFF); // VarInt indicating huge size
        psbt.push(0xFF);
        psbt.push(0xFF);
        psbt.push(0xFF);
        psbt.push(0xFF); // This creates a byte_size that would overflow MAX_VEC_SIZE
        assert_eq!(scrub(&psbt), Err(Error::UnexpectedEof));
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
