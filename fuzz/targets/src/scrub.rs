//! Properties of [`scrubber::scrub`] over PSBT bytes.
//!
//! Every property here is stated without the scrubber's own allow-lists or
//! decoder, so that a bug in either cannot hide itself: maps are split by a
//! decoder of their own, PSBT validity is decided by psbt-v2, and the fields
//! that must not survive are listed from BIP 174 and BIP 371.

use std::collections::BTreeMap;

use arbitrary::{Result, Unstructured};
use psbt_v2::bitcoin::consensus::encode::{Decodable, VarInt, serialize};

const MAGIC: &[u8] = b"psbt\xff";

/// Fields that identify the wallet or hold secrets, which a scrubbed PSBT must
/// not carry. This is a floor, not the scrubber's allow-list: it only names
/// fields whose meaning is settled, and the scrubber may drop more.
const GLOBAL_SENSITIVE: &[u8] = &[
    0x01, // XPUB
    0xFC, // PROPRIETARY
];
const INPUT_SENSITIVE: &[u8] = &[
    0x02, // PARTIAL_SIG
    0x06, // BIP32_DERIVATION
    0x0A, // RIPEMD160 preimage
    0x0B, // SHA256 preimage
    0x0C, // HASH160 preimage
    0x0D, // HASH256 preimage
    0x16, // TAP_BIP32_DERIVATION
    0x17, // TAP_INTERNAL_KEY
    0xFC, // PROPRIETARY
];
const OUTPUT_SENSITIVE: &[u8] = &[
    0x02, // BIP32_DERIVATION
    0x05, // TAP_INTERNAL_KEY
    0x07, // TAP_BIP32_DERIVATION
    0xFC, // PROPRIETARY
];

/// Scrub either the raw bytes or a PSBT assembled from them, and check what
/// comes out.
///
/// Raw bytes exercise the decoder on malformed framing. Assembled PSBTs are
/// well framed and carry the fields psbt-v2 requires, so the fuzzer spends its
/// effort on the filtering and on the comparison with psbt-v2 instead of on
/// rediscovering the encoding.
pub fn scrub(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let (psbt, counts) = if u.arbitrary().unwrap_or(false) {
        match assemble(&mut u) {
            Ok((psbt, counts)) => (psbt, Some(counts)),
            Err(_) => return,
        }
    } else {
        (data.get(1..).unwrap_or_default().to_vec(), None)
    };
    check(&psbt, counts);
}

fn check(psbt: &[u8], assembled_counts: Option<(usize, usize)>) {
    // psbt-v2 is asked about the input with each map in the order scrub
    // emits, because its answer can depend on the order of pairs: it reads a
    // PSBT_OUT_SCRIPT whose key carries data as the script itself, and only
    // reports a duplicate once the script read first is not empty.
    let reference = canonical(psbt).and_then(|psbt| reference_counts(&psbt));

    let scrubbed = match scrubber::scrub(psbt) {
        Ok(scrubbed) => scrubbed,
        Err(e) => {
            assert!(
                reference.is_none(),
                "scrub rejects a PSBT that psbt-v2 accepts: {e}"
            );
            return;
        }
    };

    assert_eq!(
        scrubber::scrub(&scrubbed).as_ref(),
        Ok(&scrubbed),
        "scrub is not idempotent"
    );

    let input_maps = maps(psbt).expect("scrub accepted bytes that do not split into maps");
    let output_maps = maps(&scrubbed).expect("scrub emitted bytes that do not split into maps");
    assert_eq!(
        input_maps.len(),
        output_maps.len(),
        "scrub changed the number of maps"
    );

    for (input, output) in input_maps.iter().zip(&output_maps) {
        assert!(
            output.windows(2).all(|w| w[0].0 < w[1].0),
            "scrub emitted a map whose keys are not strictly increasing"
        );
        // Of duplicate keys, the first one read is the one kept.
        let mut first = BTreeMap::new();
        for (key, value) in input {
            first.entry(key).or_insert(value);
        }
        for (key, value) in output {
            assert_eq!(
                first.get(key),
                Some(&value),
                "scrub emitted a pair that is not in the corresponding input map"
            );
        }
    }

    if let Some((n_inputs, n_outputs)) = reference.or(assembled_counts) {
        assert_eq!(
            output_maps.len(),
            1 + n_inputs + n_outputs,
            "scrub changed the number of inputs or outputs"
        );
        for (i, map) in output_maps.iter().enumerate() {
            let sensitive = match i {
                0 => GLOBAL_SENSITIVE,
                i if i <= n_inputs => INPUT_SENSITIVE,
                _ => OUTPUT_SENSITIVE,
            };
            for (key, _) in map {
                assert!(
                    !sensitive.contains(&key.0),
                    "scrub kept sensitive field {:#04x} in map {i}",
                    key.0
                );
            }
        }
    }

    if reference.is_some() {
        assert_eq!(
            reference_counts(&scrubbed),
            reference,
            "psbt-v2 does not accept the scrubbed form of a PSBT it accepts"
        );
    }
}

/// The numbers of inputs and outputs, if psbt-v2 accepts the bytes as a PSBT
/// of either version.
fn reference_counts(psbt: &[u8]) -> Option<(usize, usize)> {
    if let Ok(p) = psbt_v2::v2::Psbt::deserialize(psbt) {
        return Some((p.inputs.len(), p.outputs.len()));
    }
    psbt_v2::v0::Psbt::deserialize(psbt)
        .ok()
        .map(|p| (p.inputs.len(), p.outputs.len()))
}

/// The PSBT with each map's pairs sorted by key and, of duplicate keys, only
/// the first kept, as scrub writes them.
fn canonical(psbt: &[u8]) -> Option<Vec<u8>> {
    let mut out = MAGIC.to_vec();
    for map in maps(psbt)? {
        let mut first = BTreeMap::new();
        for (key, value) in map {
            first.entry(key).or_insert(value);
        }
        for ((type_value, key), value) in first {
            encode_pair(&mut out, type_value, &key, &value);
        }
        out.push(0x00);
    }
    Some(out)
}

/// A key: its type and the bytes after the type.
type Key = (u8, Vec<u8>);
type Map = Vec<(Key, Vec<u8>)>;

/// Split PSBT bytes into maps of pairs, in the order they appear, without
/// interpreting any of them.
fn maps(psbt: &[u8]) -> Option<Vec<Map>> {
    let mut r = psbt.strip_prefix(MAGIC)?;
    let mut maps = Vec::new();
    while !r.is_empty() {
        let mut map = Vec::new();
        loop {
            let VarInt(len) = Decodable::consensus_decode(&mut r).ok()?;
            if len == 0 {
                break;
            }
            let (key, rest) = r.split_at_checked(usize::try_from(len).ok()?)?;
            r = rest;
            let value: Vec<u8> = Decodable::consensus_decode(&mut r).ok()?;
            map.push(((key[0], key[1..].to_vec()), value));
        }
        maps.push(map);
    }
    Some(maps)
}

fn encode_pair(buf: &mut Vec<u8>, type_value: u8, key: &[u8], value: &[u8]) {
    buf.extend(serialize(&VarInt::from(key.len() + 1)));
    buf.push(type_value);
    buf.extend(key);
    buf.extend(serialize(&VarInt::from(value.len())));
    buf.extend(value);
}

/// A PSBT of version 0 or 2 with up to three inputs and outputs, carrying the
/// fields its version requires plus up to four arbitrary pairs per map, in
/// an arbitrary order.
fn assemble(u: &mut Unstructured) -> Result<(Vec<u8>, (usize, usize))> {
    let v2: bool = u.arbitrary()?;
    // A version 0 transaction without inputs reads back as a segwit one, so
    // its counts would not be the ones assembled here.
    let n_inputs = u.int_in_range(if v2 { 0 } else { 1 }..=3)?;
    let n_outputs = u.int_in_range(0..=3)?;

    let mut global = Vec::new();
    let mut inputs = vec![Vec::new(); n_inputs];
    let mut outputs = vec![Vec::new(); n_outputs];
    if v2 {
        global.push((0xFB, vec![], 2u32.to_le_bytes().to_vec()));
        global.push((0x02, vec![], 2u32.to_le_bytes().to_vec()));
        global.push((0x04, vec![], serialize(&VarInt::from(n_inputs))));
        global.push((0x05, vec![], serialize(&VarInt::from(n_outputs))));
        for input in &mut inputs {
            input.push((0x0E, vec![], u.bytes(32)?.to_vec()));
            input.push((0x0F, vec![], u.bytes(4)?.to_vec()));
        }
        for output in &mut outputs {
            output.push((0x03, vec![], u.bytes(8)?.to_vec()));
            output.push((0x04, vec![], script(u)?));
        }
    } else {
        global.push((0x00, vec![], unsigned_tx(u, n_inputs, n_outputs)?));
    }

    // A second copy of a global field that determines the counts would make
    // them depend on which copy is read, so extra global pairs never repeat one.
    for _ in 0..u.int_in_range(0..=4)? {
        let (type_value, mut key, value) = pair(u)?;
        if key.is_empty() && [0x00, 0x04, 0x05, 0xFB].contains(&type_value) {
            key.push(0x00);
        }
        global.push((type_value, key, value));
    }
    for map in inputs.iter_mut().chain(&mut outputs) {
        for _ in 0..u.int_in_range(0..=4)? {
            map.push(pair(u)?);
        }
    }

    let mut psbt = MAGIC.to_vec();
    for map in std::iter::once(&mut global)
        .chain(&mut inputs)
        .chain(&mut outputs)
    {
        shuffle(u, map)?;
        for (type_value, key, value) in map.iter() {
            encode_pair(&mut psbt, *type_value, key, value);
        }
        psbt.push(0x00);
    }
    Ok((psbt, (n_inputs, n_outputs)))
}

/// A pair whose type is usually one BIP 174 assigns in some map, so that
/// both kept and dropped fields turn up in every kind of map.
fn pair(u: &mut Unstructured) -> Result<(u8, Vec<u8>, Vec<u8>)> {
    let type_value = if u.arbitrary()? {
        u.int_in_range(0x00..=0x1C)?
    } else {
        u.arbitrary()?
    };
    let key_len = u.int_in_range(0..=33)?;
    let key = u.bytes(key_len)?.to_vec();
    let value_len = u.int_in_range(0..=40)?;
    let value = u.bytes(value_len)?.to_vec();
    Ok((type_value, key, value))
}

fn script(u: &mut Unstructured) -> Result<Vec<u8>> {
    let len = u.int_in_range(0..=34)?;
    Ok(u.bytes(len)?.to_vec())
}

/// A consensus-encoded transaction with empty script sigs and no witnesses,
/// as BIP 174 requires of `PSBT_GLOBAL_UNSIGNED_TX`.
fn unsigned_tx(u: &mut Unstructured, n_inputs: usize, n_outputs: usize) -> Result<Vec<u8>> {
    let mut tx = 2u32.to_le_bytes().to_vec();
    tx.extend(serialize(&VarInt::from(n_inputs)));
    for _ in 0..n_inputs {
        tx.extend(u.bytes(32 + 4)?); // previous txid and output index
        tx.push(0x00); // empty script sig
        tx.extend(u.bytes(4)?); // sequence
    }
    tx.extend(serialize(&VarInt::from(n_outputs)));
    for _ in 0..n_outputs {
        tx.extend(u.bytes(8)?); // amount
        let script = script(u)?;
        tx.extend(serialize(&VarInt::from(script.len())));
        tx.extend(script);
    }
    tx.extend(u.bytes(4)?); // lock time
    Ok(tx)
}

fn shuffle<T>(u: &mut Unstructured, items: &mut [T]) -> Result<()> {
    for i in (1..items.len()).rev() {
        items.swap(i, u.int_in_range(0..=i)?);
    }
    Ok(())
}
