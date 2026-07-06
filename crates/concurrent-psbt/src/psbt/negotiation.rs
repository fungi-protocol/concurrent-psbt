#![allow(clippy::result_large_err)]

//! Payment negotiation and confirmation as global proprietary fields.
//!
//! Two grow-only sets ride the global proprietary map, one entry per element
//! keyed by a 16-byte element id:
//!
//! - `PSBT_GLOBAL_PAYMENT` (subtype `0x20`): a payment a participant wants
//!   constructed — recipient script, amount, optional label — plus a `kind`
//!   byte distinguishing real from dummy padding.
//! - `PSBT_GLOBAL_CONFIRMATION` (subtype `0x21`): a `(peer_id, unique_id)`
//!   attestation from the confirmation protocol (`contrib/design/ptj-net.md`
//!   Layer 4).
//!
//! Values are opaque `Vec<u8>` at the field layer, so an encrypted element
//! (leading `format` byte `0x01`) joins and stores identically to a plaintext
//! one; only the [`Payment`]/[`Confirmation`] codecs here understand the
//! plaintext form. Encryption and dummy generation live in the `ptj` CLI.

use bitcoin::hashes::{Hash, HashEngine, sha256t_hash_newtype};
use psbt_v2::raw;
use psbt_v2::v2::{Global, Psbt};

use crate::output::OutputUniqueIdExt;

sha256t_hash_newtype! {
    /// Tag for the order-independent unordered unique id.
    pub struct UnorderedIdTag = hash_str("concurrent-psbt/unordered-unique-id");

    /// The 32-byte content id a confirmation attests to.
    #[hash_newtype(forward)]
    pub struct UnorderedIdHash(_);
}

/// Compute the order-independent 32-byte unique id of a PSBT's content.
///
/// Hashes the input outpoints and `(unique_id, amount, script)` outputs each
/// in sorted order, so all participants who have converged on the same set of
/// inputs and outputs agree regardless of ordering. This is the value a
/// [`Confirmation`] attests to (`contrib/design/ptj-net.md` Layer 4).
///
/// The **live-set projection** is applied first: tombstoned (removed) inputs and
/// outputs are dropped before hashing, so the confirmed id commits to what will
/// actually be signed rather than to phantom removed elements. When the
/// `removal` feature is off the projection is the identity, so the id is
/// byte-identical to a non-removal build — no behaviour change there.
pub fn unordered_unique_id(psbt: &Psbt) -> [u8; 32] {
    // Project out tombstoned elements before hashing (see module docs).
    let mut live_inputs = psbt.inputs.clone();
    let mut live_outputs = psbt.outputs.clone();
    crate::removal::retain_live_inputs(&psbt.global, &mut live_inputs);
    crate::removal::retain_live_outputs(&psbt.global, &mut live_outputs);

    let mut inputs: Vec<Vec<u8>> = live_inputs
        .iter()
        .map(|input| {
            let mut bytes = Vec::with_capacity(36);
            bytes.extend_from_slice(input.previous_txid.as_ref());
            bytes.extend_from_slice(&input.spent_output_index.to_le_bytes());
            bytes
        })
        .collect();
    inputs.sort_unstable();

    let mut outputs: Vec<Vec<u8>> = live_outputs
        .iter()
        .map(|output| {
            let mut bytes = Vec::new();
            if let Some(uid) = output.unique_id() {
                bytes.extend_from_slice(uid.as_bytes());
            }
            bytes.push(0xff); // separator between uid and value fields
            bytes.extend_from_slice(&output.amount.to_sat().to_le_bytes());
            bytes.extend_from_slice(output.script_pubkey.as_bytes());
            bytes
        })
        .collect();
    outputs.sort_unstable();

    let mut engine = UnorderedIdHash::engine();
    engine.input(&(inputs.len() as u64).to_le_bytes());
    for input in &inputs {
        engine.input(&(input.len() as u64).to_le_bytes());
        engine.input(input);
    }
    engine.input(&(outputs.len() as u64).to_le_bytes());
    for output in &outputs {
        engine.input(&(output.len() as u64).to_le_bytes());
        engine.input(output);
    }
    UnorderedIdHash::from_engine(engine).to_byte_array()
}

/// Error decoding a negotiation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationError {
    /// The record's leading format byte was not `FORMAT_PLAINTEXT` (e.g. an
    /// encrypted blob was handed to a plaintext decoder).
    NotPlaintext,
    /// The record ended before all declared fields were read.
    Truncated,
    /// A payment label was not valid UTF-8.
    InvalidLabel,
}

impl std::fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPlaintext => write!(f, "record is not a plaintext negotiation record"),
            Self::Truncated => write!(f, "negotiation record truncated"),
            Self::InvalidLabel => write!(f, "payment label is not valid UTF-8"),
        }
    }
}

impl std::error::Error for NegotiationError {}

type Result<T> = std::result::Result<T, NegotiationError>;

/// Subtype for `PSBT_GLOBAL_PAYMENT`.
pub const PSBT_GLOBAL_PAYMENT_SUBTYPE: u8 = 0x20;
/// Subtype for `PSBT_GLOBAL_CONFIRMATION`.
pub const PSBT_GLOBAL_CONFIRMATION_SUBTYPE: u8 = 0x21;

/// Leading byte of a plaintext element record.
pub const FORMAT_PLAINTEXT: u8 = 0x00;
/// Leading byte of an encrypted element record (ciphertext follows).
pub const FORMAT_ENCRYPTED: u8 = 0x01;

/// Real payment (as opposed to dummy padding).
pub const PAYMENT_KIND_REAL: u8 = 0x00;
/// Dummy payment, generated to pad the visible payment-graph degree.
pub const PAYMENT_KIND_DUMMY: u8 = 0x01;

fn element_key(subtype: u8, id: &[u8; 16]) -> raw::ProprietaryKey {
    raw::ProprietaryKey {
        prefix: crate::PROPRIETARY_PREFIX.to_vec(),
        subtype,
        key: id.to_vec(),
    }
}

fn entries(global: &Global, subtype: u8) -> Vec<([u8; 16], Vec<u8>)> {
    global
        .proprietaries
        .iter()
        .filter(|(key, _)| key.prefix == crate::PROPRIETARY_PREFIX && key.subtype == subtype)
        .filter_map(|(key, value)| {
            <[u8; 16]>::try_from(key.key.as_slice())
                .ok()
                .map(|id| (id, value.clone()))
        })
        .collect()
}

/// Extension trait on [`Global`] for the negotiation proprietary sets.
pub trait GlobalNegotiationExt {
    /// Every payment element as `(id, opaque blob)`.
    fn payments(&self) -> Vec<([u8; 16], Vec<u8>)>;
    /// Insert or replace a payment element by id.
    fn add_payment(&mut self, id: [u8; 16], blob: Vec<u8>);
    /// Every confirmation element as `(id, opaque blob)`.
    fn confirmations(&self) -> Vec<([u8; 16], Vec<u8>)>;
    /// Insert or replace a confirmation element by id.
    fn add_confirmation(&mut self, id: [u8; 16], blob: Vec<u8>);
    /// Remove the entire negotiation band (payments and confirmations).
    ///
    /// Negotiation metadata has done its job once ordering begins; it must not
    /// leak into the signing artifact.
    fn clear_negotiation(&mut self);
}

impl GlobalNegotiationExt for Global {
    fn payments(&self) -> Vec<([u8; 16], Vec<u8>)> {
        entries(self, PSBT_GLOBAL_PAYMENT_SUBTYPE)
    }

    fn add_payment(&mut self, id: [u8; 16], blob: Vec<u8>) {
        self.proprietaries
            .insert(element_key(PSBT_GLOBAL_PAYMENT_SUBTYPE, &id), blob);
    }

    fn confirmations(&self) -> Vec<([u8; 16], Vec<u8>)> {
        entries(self, PSBT_GLOBAL_CONFIRMATION_SUBTYPE)
    }

    fn add_confirmation(&mut self, id: [u8; 16], blob: Vec<u8>) {
        self.proprietaries
            .insert(element_key(PSBT_GLOBAL_CONFIRMATION_SUBTYPE, &id), blob);
    }

    fn clear_negotiation(&mut self) {
        self.proprietaries.retain(|key, _| {
            !(key.prefix == crate::PROPRIETARY_PREFIX
                && matches!(
                    key.subtype,
                    PSBT_GLOBAL_PAYMENT_SUBTYPE | PSBT_GLOBAL_CONFIRMATION_SUBTYPE
                ))
        });
    }
}

/// A payment a participant wants constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    /// Real or dummy padding.
    pub kind: u8,
    /// Opaque 32-byte identity of the payer (a tagged-hash-compressed peer id).
    pub payer: [u8; 32],
    /// Amount in satoshis.
    pub amount_sats: u64,
    /// Recipient scriptPubKey.
    pub script_pubkey: Vec<u8>,
    /// Optional UTF-8 label.
    pub label: String,
}

impl Payment {
    /// Encode the plaintext record (leading `FORMAT_PLAINTEXT` byte).
    pub fn encode(&self) -> Vec<u8> {
        let label = self.label.as_bytes();
        let mut out =
            Vec::with_capacity(1 + 1 + 32 + 8 + 2 + self.script_pubkey.len() + 1 + label.len());
        out.push(FORMAT_PLAINTEXT);
        out.push(self.kind);
        out.extend_from_slice(&self.payer);
        out.extend_from_slice(&self.amount_sats.to_le_bytes());
        out.extend_from_slice(
            &u16::try_from(self.script_pubkey.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(&self.script_pubkey);
        out.push(u8::try_from(label.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(label);
        out
    }

    /// Decode a plaintext record. Errors if the leading byte is not
    /// [`FORMAT_PLAINTEXT`] (e.g. an encrypted blob) or the record is malformed.
    pub fn decode(bytes: &[u8]) -> Result<Payment> {
        let mut r = Reader::new(bytes);
        if r.u8()? != FORMAT_PLAINTEXT {
            return Err(NegotiationError::NotPlaintext);
        }
        let kind = r.u8()?;
        let payer = r.array32()?;
        let amount_sats = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
        let spk_len = u16::from_le_bytes(r.take(2)?.try_into().unwrap()) as usize;
        let script_pubkey = r.take(spk_len)?.to_vec();
        let label_len = r.u8()? as usize;
        let label = String::from_utf8(r.take(label_len)?.to_vec())
            .map_err(|_| NegotiationError::InvalidLabel)?;
        Ok(Payment {
            kind,
            payer,
            amount_sats,
            script_pubkey,
            label,
        })
    }

    /// `true` for a dummy padding payment.
    pub fn is_dummy(&self) -> bool {
        self.kind == PAYMENT_KIND_DUMMY
    }
}

/// A `(peer_id, unique_id)` confirmation attestation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    /// Opaque 32-byte peer identity.
    pub peer_id: [u8; 32],
    /// The 32-byte unordered unique id the peer is confirming.
    pub unique_id: [u8; 32],
}

impl Confirmation {
    /// Encode the plaintext record (leading `FORMAT_PLAINTEXT` byte).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 32 + 32);
        out.push(FORMAT_PLAINTEXT);
        out.extend_from_slice(&self.peer_id);
        out.extend_from_slice(&self.unique_id);
        out
    }

    /// Decode a plaintext record.
    pub fn decode(bytes: &[u8]) -> Result<Confirmation> {
        let mut r = Reader::new(bytes);
        if r.u8()? != FORMAT_PLAINTEXT {
            return Err(NegotiationError::NotPlaintext);
        }
        let peer_id = r.array32()?;
        let unique_id = r.array32()?;
        Ok(Confirmation { peer_id, unique_id })
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(NegotiationError::Truncated)?;
        if end > self.bytes.len() {
            return Err(NegotiationError::Truncated);
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn array32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        fn sample_payment(kind: u8) -> Payment {
            Payment {
                kind,
                payer: [7u8; 32],
                amount_sats: 12_345,
                script_pubkey: vec![0x51, 0x20, 0xAB],
                label: "coffee".into(),
            }
        }

        #[test]
        fn payment_roundtrip() {
            let p = sample_payment(PAYMENT_KIND_REAL);
            assert_eq!(Payment::decode(&p.encode()).unwrap(), p);
        }

        #[test]
        fn payment_empty_label_and_script() {
            let p = Payment {
                kind: PAYMENT_KIND_DUMMY,
                payer: [0u8; 32],
                amount_sats: 0,
                script_pubkey: vec![],
                label: String::new(),
            };
            let decoded = Payment::decode(&p.encode()).unwrap();
            assert_eq!(decoded, p);
            assert!(decoded.is_dummy());
        }

        #[test]
        fn confirmation_roundtrip() {
            let c = Confirmation {
                peer_id: [1u8; 32],
                unique_id: [2u8; 32],
            };
            assert_eq!(Confirmation::decode(&c.encode()).unwrap(), c);
        }

        #[test]
        fn decode_rejects_encrypted_format() {
            let mut blob = sample_payment(PAYMENT_KIND_REAL).encode();
            blob[0] = FORMAT_ENCRYPTED;
            assert_eq!(Payment::decode(&blob), Err(NegotiationError::NotPlaintext));
            let mut c = Confirmation {
                peer_id: [0; 32],
                unique_id: [0; 32],
            }
            .encode();
            c[0] = FORMAT_ENCRYPTED;
            assert_eq!(
                Confirmation::decode(&c),
                Err(NegotiationError::NotPlaintext)
            );
        }

        #[test]
        fn decode_rejects_truncated() {
            let blob = sample_payment(PAYMENT_KIND_REAL).encode();
            assert_eq!(
                Payment::decode(&blob[..blob.len() - 2]),
                Err(NegotiationError::Truncated)
            );
            assert_eq!(Payment::decode(&[]), Err(NegotiationError::Truncated));
            assert_eq!(
                Confirmation::decode(&[FORMAT_PLAINTEXT, 1, 2]),
                Err(NegotiationError::Truncated)
            );
        }

        #[test]
        fn error_display_covers_variants() {
            for e in [
                NegotiationError::NotPlaintext,
                NegotiationError::Truncated,
                NegotiationError::InvalidLabel,
            ] {
                assert!(!e.to_string().is_empty());
            }
        }

        #[test]
        fn global_payment_set_grows_and_reads() {
            let mut g = psbt_v2::v2::Global::default();
            assert!(g.payments().is_empty());
            let p = sample_payment(PAYMENT_KIND_REAL);
            g.add_payment([9u8; 16], p.encode());
            g.add_payment([8u8; 16], p.encode());
            assert_eq!(g.payments().len(), 2);
            // same id replaces, not appends
            g.add_payment([9u8; 16], p.encode());
            assert_eq!(g.payments().len(), 2);
        }

        #[test]
        fn global_confirmation_set_and_clear() {
            let mut g = psbt_v2::v2::Global::default();
            let c = Confirmation {
                peer_id: [3; 32],
                unique_id: [4; 32],
            };
            g.add_confirmation([1u8; 16], c.encode());
            g.add_payment([2u8; 16], sample_payment(PAYMENT_KIND_REAL).encode());
            assert_eq!(g.confirmations().len(), 1);
            assert_eq!(g.payments().len(), 1);
            g.clear_negotiation();
            assert!(g.confirmations().is_empty());
            assert!(g.payments().is_empty());
        }

        #[test]
        fn unordered_id_is_order_independent() {
            let mut a = psbt_v2::v2::Psbt {
                global: psbt_v2::v2::Global::default(),
                inputs: vec![],
                outputs: vec![],
            };
            let mut o1 = psbt_v2::v2::Output {
                amount: bitcoin::Amount::from_sat(1000),
                ..Default::default()
            };
            o1.set_unique_id(crate::output::UniqueId::new(vec![1; 16]));
            let mut o2 = psbt_v2::v2::Output {
                amount: bitcoin::Amount::from_sat(2000),
                ..Default::default()
            };
            o2.set_unique_id(crate::output::UniqueId::new(vec![2; 16]));
            a.outputs = vec![o1.clone(), o2.clone()];
            let mut b = a.clone();
            b.outputs = vec![o2, o1];
            assert_eq!(unordered_unique_id(&a), unordered_unique_id(&b));
        }

        #[test]
        fn unordered_id_changes_with_content() {
            let base = psbt_v2::v2::Psbt {
                global: psbt_v2::v2::Global::default(),
                inputs: vec![],
                outputs: vec![],
            };
            let mut other = base.clone();
            other.outputs = vec![psbt_v2::v2::Output {
                amount: bitcoin::Amount::from_sat(1),
                ..Default::default()
            }];
            assert_ne!(unordered_unique_id(&base), unordered_unique_id(&other));
        }

        #[test]
        fn opaque_encrypted_blob_survives_field_layer() {
            let mut g = psbt_v2::v2::Global::default();
            let mut blob = sample_payment(PAYMENT_KIND_REAL).encode();
            blob[0] = FORMAT_ENCRYPTED; // pretend-encrypted; field layer is agnostic
            g.add_payment([5u8; 16], blob.clone());
            assert_eq!(g.payments()[0].1, blob);
        }
    }
}
