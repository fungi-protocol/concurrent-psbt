//! Raw BIP 174/370 keymap access over the serialized PSBT byte stream.
//!
//! The low-level fragment viewer/editor operates on the RAW key-value maps
//! (global / per-input / per-output), including entries the typed
//! `psbt_v2::v2::Psbt` structs parse into dedicated fields. `psbt_v2` keeps
//! its `Map::get_pairs` trait crate-private, so this module re-derives the
//! pairs from the one stable public surface: the serialized PSBT bytes
//! (`<magic> <global-map> <input-map>* <output-map>*`, each map a sequence of
//! `<keylen><key><valuelen><value>` pairs terminated by `0x00`).
//!
//! Keys are handled as opaque byte strings (`<keytype> <keydata>`, compact
//! size type prefix included) so known, unknown, and proprietary entries all
//! round-trip byte-identically.

use psbt_v2::v2::Psbt;

use crate::{Error, Result};

/// One raw `<key> -> <value>` pair. `key` carries the compact-size `keytype`
/// prefix plus `keydata`; `value` is the raw `valuedata`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawPair {
    pub(crate) key: Vec<u8>,
    pub(crate) value: Vec<u8>,
}

/// The raw maps of one PSBT: `global`, then one map per input and output (in
/// the same order as the typed `Psbt` vectors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawMaps {
    pub(crate) global: Vec<RawPair>,
    pub(crate) inputs: Vec<Vec<RawPair>>,
    pub(crate) outputs: Vec<Vec<RawPair>>,
}

const MAGIC: &[u8] = b"psbt\xff";

/// Decompose a PSBT into its raw maps (via one serialize round-trip).
pub(crate) fn raw_maps(psbt: &Psbt) -> Result<RawMaps> {
    let bytes = Psbt::serialize(psbt);
    let mut cursor = Cursor {
        bytes: &bytes,
        position: 0,
    };
    if !bytes.starts_with(MAGIC) {
        return Err(Error::new("serialized PSBT is missing the psbt magic"));
    }
    cursor.position = MAGIC.len();

    let global = read_map(&mut cursor)?;
    let inputs = (0..psbt.inputs.len())
        .map(|_| read_map(&mut cursor))
        .collect::<Result<Vec<_>>>()?;
    let outputs = (0..psbt.outputs.len())
        .map(|_| read_map(&mut cursor))
        .collect::<Result<Vec<_>>>()?;
    if cursor.position != bytes.len() {
        return Err(Error::new(format!(
            "serialized PSBT has {} trailing bytes after the last map",
            bytes.len() - cursor.position
        )));
    }
    Ok(RawMaps {
        global,
        inputs,
        outputs,
    })
}

/// Reassemble serialized PSBT bytes from raw maps (the write half of
/// [`raw_maps`]; `/api/edit` re-parses the result, so a malformed edit can
/// never mint an unparseable fragment). Only the webgui edit route writes.
#[cfg(feature = "webgui")]
pub(crate) fn serialize_maps(maps: &RawMaps) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    write_map(&mut bytes, &maps.global);
    for map in &maps.inputs {
        write_map(&mut bytes, map);
    }
    for map in &maps.outputs {
        write_map(&mut bytes, map);
    }
    bytes
}

/// Split a raw key into its compact-size `keytype` and the trailing
/// `keydata`. Errors on an empty or truncated key.
pub(crate) fn split_key_type(key: &[u8]) -> Result<(u64, &[u8])> {
    let mut cursor = Cursor {
        bytes: key,
        position: 0,
    };
    let key_type = read_compact_size(&mut cursor)?;
    Ok((key_type, &key[cursor.position..]))
}

/// Parse the BIP 174 proprietary-key envelope out of `keydata` (the bytes
/// after the `0xFC` keytype): `<prefixlen><prefix><subtype><subkeydata>`.
/// Returns `None` when the envelope does not parse (the entry is still shown
/// as an opaque proprietary key).
pub(crate) fn split_proprietary(key_data: &[u8]) -> Option<(Vec<u8>, u64, Vec<u8>)> {
    let mut cursor = Cursor {
        bytes: key_data,
        position: 0,
    };
    let prefix_len = read_compact_size(&mut cursor).ok()?;
    let prefix_len = usize::try_from(prefix_len).ok()?;
    let prefix = cursor.take(prefix_len).ok()?.to_vec();
    let subtype = read_compact_size(&mut cursor).ok()?;
    let sub_key = key_data[cursor.position..].to_vec();
    Some((prefix, subtype, sub_key))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.position.checked_add(count).ok_or_else(|| {
            Error::new("PSBT map length overflows")
        })?;
        if end > self.bytes.len() {
            return Err(Error::new(format!(
                "PSBT map is truncated: wanted {count} bytes at offset {}, only {} remain",
                self.position,
                self.bytes.len() - self.position
            )));
        }
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }
}

fn read_compact_size(cursor: &mut Cursor) -> Result<u64> {
    let first = cursor.take(1)?[0];
    Ok(match first {
        0xFD => u64::from(u16::from_le_bytes(cursor.take(2)?.try_into().unwrap())),
        0xFE => u64::from(u32::from_le_bytes(cursor.take(4)?.try_into().unwrap())),
        0xFF => u64::from_le_bytes(cursor.take(8)?.try_into().unwrap()),
        byte => u64::from(byte),
    })
}

#[cfg(feature = "webgui")]
fn write_compact_size(bytes: &mut Vec<u8>, value: u64) {
    match value {
        0..=0xFC => bytes.push(value as u8),
        0xFD..=0xFFFF => {
            bytes.push(0xFD);
            bytes.extend_from_slice(&(value as u16).to_le_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            bytes.push(0xFE);
            bytes.extend_from_slice(&(value as u32).to_le_bytes());
        }
        _ => {
            bytes.push(0xFF);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn read_map(cursor: &mut Cursor) -> Result<Vec<RawPair>> {
    let mut pairs = Vec::new();
    loop {
        let key_len = read_compact_size(cursor)?;
        if key_len == 0 {
            return Ok(pairs);
        }
        let key = cursor.take(usize::try_from(key_len).map_err(length_error)?)?.to_vec();
        let value_len = read_compact_size(cursor)?;
        let value = cursor
            .take(usize::try_from(value_len).map_err(length_error)?)?
            .to_vec();
        pairs.push(RawPair { key, value });
    }
}

fn length_error<E>(_: E) -> Error {
    Error::new("PSBT map length exceeds usize")
}

#[cfg(feature = "webgui")]
fn write_map(bytes: &mut Vec<u8>, pairs: &[RawPair]) {
    for pair in pairs {
        write_compact_size(bytes, pair.key.len() as u64);
        bytes.extend_from_slice(&pair.key);
        write_compact_size(bytes, pair.value.len() as u64);
        bytes.extend_from_slice(&pair.value);
    }
    bytes.push(0x00);
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn fixture_psbt() -> Psbt {
        let created = crate::commands::create::create_psbt(crate::cli::CreateConfig {
            inputs: vec![crate::cli::OutPointArg {
                txid: "0000000000000000000000000000000000000000000000000000000000000001"
                    .parse()
                    .unwrap(),
                vout: 7,
            }],
            outputs: vec![],
            seed: None,
            allow_short_seed: false,
            ordering: crate::cli::OrderingArg::Unset,
            network: crate::cli::NetworkArg(bitcoin::Network::Regtest),
        })
        .unwrap();
        created
    }

    #[test]
    fn raw_maps_decompose_the_serialized_psbt() {
        let psbt = fixture_psbt();
        let maps = raw_maps(&psbt).unwrap();
        assert_eq!(maps.inputs.len(), 1);
        assert!(maps.outputs.is_empty());
        assert!(!maps.global.is_empty());
    }

    #[cfg(feature = "webgui")]
    #[test]
    fn raw_maps_round_trip_byte_identically() {
        let psbt = fixture_psbt();
        let maps = raw_maps(&psbt).unwrap();
        assert_eq!(serialize_maps(&maps), Psbt::serialize(&psbt));
    }

    #[cfg(feature = "webgui")]
    #[test]
    fn compact_size_round_trips_at_the_boundaries() {
        for value in [
            0u64,
            0xFC,
            0xFD,
            0xFFFF,
            0x1_0000,
            0xFFFF_FFFF,
            0x1_0000_0000,
        ] {
            let mut bytes = Vec::new();
            write_compact_size(&mut bytes, value);
            let mut cursor = Cursor {
                bytes: &bytes,
                position: 0,
            };
            assert_eq!(read_compact_size(&mut cursor).unwrap(), value);
            assert_eq!(cursor.position, bytes.len());
        }
    }

    #[test]
    fn split_key_type_reads_the_compact_size_prefix() {
        let (key_type, key_data) = split_key_type(&[0x02, 0xAA, 0xBB]).unwrap();
        assert_eq!(key_type, 0x02);
        assert_eq!(key_data, &[0xAA, 0xBB]);

        // Multi-byte compact-size key types survive.
        let (key_type, key_data) = split_key_type(&[0xFD, 0x00, 0x01, 0xCC]).unwrap();
        assert_eq!(key_type, 0x100);
        assert_eq!(key_data, &[0xCC]);

        assert!(split_key_type(&[]).is_err());
    }

    #[test]
    fn split_proprietary_parses_the_bip174_envelope() {
        // prefix "cpsb", subtype 0x01, subkeydata 0xAB.
        let key_data = [&[0x04][..], b"cpsb", &[0x01, 0xAB][..]].concat();
        let (prefix, subtype, sub_key) = split_proprietary(&key_data).unwrap();
        assert_eq!(prefix, b"cpsb");
        assert_eq!(subtype, 0x01);
        assert_eq!(sub_key, vec![0xAB]);

        // A truncated envelope is tolerated as opaque (None), not an error.
        assert_eq!(split_proprietary(&[0x09, 0x61]), None);
    }

}
