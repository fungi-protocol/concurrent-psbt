#![allow(clippy::result_large_err)]

//! Explicit fee contribution as a grow-only global proprietary set.
//!
//! `PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION` (subtype `0x22`) is the third field
//! in the negotiation band (after payment `0x20` and confirmation `0x21`). It
//! is structurally identical to [`crate::negotiation::Payment`]: one entry per
//! contribution keyed by a random 16-byte uuid, value an opaque `Vec<u8>` at
//! the field layer. The only difference is the codec — each entry carries a
//! bare little-endian `u64` amount of satoshis explicitly contributed as fees
//! (spec `psbt.md` § Termination):
//!
//! > `PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION`
//! > keydata = 16 byte uuid
//! > value = u64 amount of funds explicitly contributed as fees
//! > all such fee contributions should also be added
//!
//! Termination uses the *sum* of all contributions ([`total_declared_fee`]):
//! effective feerate/size estimation is out of scope, so participants declare
//! nominal fee sats and the Session decides it has enough to proceed to signing
//! once the summed declared fee clears whatever the protocol layer requires.
//!
//! Like the negotiation records, the value is opaque `Vec<u8>` so an encrypted
//! entry (leading `FORMAT_ENCRYPTED` byte) joins and stores identically to a
//! plaintext one; only [`FeeContribution`] here understands the plaintext form.
//! Encryption reuses the negotiation group-key machinery in the `ptj` CLI.

// This module lives at `crates/concurrent-psbt/src/psbt/fee.rs`, a sibling of
// `psbt/negotiation.rs`, and is re-exported from lib.rs as `pub use psbt::fee;`
// (mirroring `pub use psbt::negotiation;`). Sibling references use the crate
// re-export path `crate::negotiation`.
use psbt_v2::raw;
use psbt_v2::v2::Global;

use crate::negotiation::{FORMAT_PLAINTEXT, NegotiationError};

type Result<T> = std::result::Result<T, NegotiationError>;

/// Subtype for `PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION`.
///
/// Third slot in the negotiation band (`0x20` payment, `0x21` confirmation,
/// `0x22` fee).
pub const PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE: u8 = 0x22;

fn element_key(id: &[u8; 16]) -> raw::ProprietaryKey {
    raw::ProprietaryKey {
        prefix: crate::PROPRIETARY_PREFIX.to_vec(),
        subtype: PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE,
        key: id.to_vec(),
    }
}

fn entries(global: &Global) -> Vec<([u8; 16], Vec<u8>)> {
    global
        .proprietaries
        .iter()
        .filter(|(key, _)| {
            key.prefix == crate::PROPRIETARY_PREFIX
                && key.subtype == PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE
        })
        .filter_map(|(key, value)| {
            <[u8; 16]>::try_from(key.key.as_slice())
                .ok()
                .map(|id| (id, value.clone()))
        })
        .collect()
}

/// Extension trait on [`Global`] for the explicit-fee-contribution set.
///
/// Mirrors [`crate::negotiation::GlobalNegotiationExt`]; kept as a separate
/// trait so the fee feature is self-contained and can be feature-gated
/// independently of the payment/confirmation band.
pub trait GlobalFeeExt {
    /// Every fee-contribution element as `(id, opaque blob)`.
    fn fee_contributions(&self) -> Vec<([u8; 16], Vec<u8>)>;

    /// Insert or replace a fee contribution by id.
    ///
    /// Same id replaces (idempotent at the map level); a fresh id grows the
    /// set. The blob is opaque here — pass a plaintext [`FeeContribution`]
    /// encoding, or a ciphertext produced by the CLI's negotiation encryptor.
    fn add_fee_contribution(&mut self, id: [u8; 16], blob: Vec<u8>);

    /// Remove every fee-contribution entry from the proprietary map.
    ///
    /// Fee metadata is negotiation state; it must not leak into the signing
    /// artifact, so ordering/signing paths call this alongside
    /// `clear_negotiation()`.
    fn clear_fee_contributions(&mut self);
}

impl GlobalFeeExt for Global {
    fn fee_contributions(&self) -> Vec<([u8; 16], Vec<u8>)> {
        entries(self)
    }

    fn add_fee_contribution(&mut self, id: [u8; 16], blob: Vec<u8>) {
        self.proprietaries.insert(element_key(&id), blob);
    }

    fn clear_fee_contributions(&mut self) {
        self.proprietaries.retain(|key, _| {
            !(key.prefix == crate::PROPRIETARY_PREFIX
                && key.subtype == PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE)
        });
    }
}

/// A single explicit fee contribution: a nominal sat amount a participant
/// declares it is contributing to fees.
///
/// The nominal value is only meaningful when the contributing input carries a
/// `PSBT_IN_NON_WITNESS_UTXO` or `PSBT_IN_WITNESS_UTXO` for the coin's value
/// (spec § Termination); this codec does not enforce that — it is a wire record
/// only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeContribution {
    /// Amount in satoshis explicitly contributed as fees.
    pub amount_sats: u64,
}

impl FeeContribution {
    /// Encode the plaintext record: `FORMAT_PLAINTEXT || amount_sats (u64 LE)`.
    ///
    /// The leading format byte matches the payment/confirmation codecs so the
    /// CLI encryptor and the `decode`/decrypt dispatch treat all three record
    /// families uniformly.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 8);
        out.push(FORMAT_PLAINTEXT);
        out.extend_from_slice(&self.amount_sats.to_le_bytes());
        out
    }

    /// Decode a plaintext record. Errors if the leading byte is not
    /// [`FORMAT_PLAINTEXT`] (e.g. an encrypted blob) or the record is the wrong
    /// length.
    pub fn decode(bytes: &[u8]) -> Result<FeeContribution> {
        // Empty or non-plaintext leading byte (e.g. an encrypted blob handed to
        // the plaintext decoder) => NotPlaintext, matching the Payment/
        // Confirmation codecs' `r.u8()? != FORMAT_PLAINTEXT` behaviour.
        if bytes.first() != Some(&FORMAT_PLAINTEXT) {
            return Err(NegotiationError::NotPlaintext);
        }
        let body = &bytes[1..];
        let amount = <[u8; 8]>::try_from(body).map_err(|_| NegotiationError::Truncated)?;
        Ok(FeeContribution {
            amount_sats: u64::from_le_bytes(amount),
        })
    }
}

/// Sum of all plaintext fee contributions declared in `global`, saturating.
///
/// This is the termination quantity of the spec § Termination: "all such fee
/// contributions should also be added". Encrypted or malformed entries are
/// skipped — the caller only sees the sum of contributions it can read. Because
/// distinct contributions are keyed by distinct uuids, a re-emitted identical
/// contribution (same uuid) is deduplicated by the map and does *not*
/// double-count.
///
/// Note this is a *read/projection* over the current least upper bound; it is
/// not part of the join. It is monotone non-decreasing as the set grows, which
/// is exactly what a termination threshold wants: once enough fee is declared,
/// more contributions never take it back below the threshold.
#[must_use]
pub fn total_declared_fee(global: &Global) -> u64 {
    global
        .fee_contributions()
        .into_iter()
        .filter_map(|(_, blob)| FeeContribution::decode(&blob).ok())
        .fold(0u64, |acc, c| acc.saturating_add(c.amount_sats))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use crate::fee::PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE as FEE_SUBTYPE;
        use crate::negotiation::{FORMAT_ENCRYPTED, GlobalNegotiationExt, PSBT_GLOBAL_PAYMENT_SUBTYPE};

        #[test]
        fn fee_roundtrip() {
            let f = FeeContribution {
                amount_sats: 4_200,
            };
            assert_eq!(FeeContribution::decode(&f.encode()).unwrap(), f);
        }

        #[test]
        fn fee_zero_and_max_roundtrip() {
            for amount in [0u64, 1, u64::MAX] {
                let f = FeeContribution {
                    amount_sats: amount,
                };
                assert_eq!(FeeContribution::decode(&f.encode()).unwrap(), f);
            }
        }

        #[test]
        fn decode_rejects_encrypted_format() {
            let mut blob = FeeContribution { amount_sats: 5 }.encode();
            blob[0] = FORMAT_ENCRYPTED;
            assert_eq!(
                FeeContribution::decode(&blob),
                Err(NegotiationError::NotPlaintext)
            );
        }

        #[test]
        fn decode_rejects_wrong_length() {
            // empty
            assert_eq!(FeeContribution::decode(&[]), Err(NegotiationError::NotPlaintext));
            // format byte only
            assert_eq!(
                FeeContribution::decode(&[FORMAT_PLAINTEXT]),
                Err(NegotiationError::Truncated)
            );
            // 7-byte body (one short)
            assert_eq!(
                FeeContribution::decode(&[FORMAT_PLAINTEXT, 1, 2, 3, 4, 5, 6, 7]),
                Err(NegotiationError::Truncated)
            );
            // 9-byte body (one long)
            assert_eq!(
                FeeContribution::decode(&[FORMAT_PLAINTEXT, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
                Err(NegotiationError::Truncated)
            );
        }

        #[test]
        fn global_fee_set_grows_and_reads() {
            let mut g = Global::default();
            assert!(g.fee_contributions().is_empty());
            g.add_fee_contribution([1u8; 16], FeeContribution { amount_sats: 100 }.encode());
            g.add_fee_contribution([2u8; 16], FeeContribution { amount_sats: 250 }.encode());
            assert_eq!(g.fee_contributions().len(), 2);
            // same id replaces, not appends
            g.add_fee_contribution([1u8; 16], FeeContribution { amount_sats: 100 }.encode());
            assert_eq!(g.fee_contributions().len(), 2);
        }

        #[test]
        fn total_sums_all_plaintext_contributions() {
            let mut g = Global::default();
            g.add_fee_contribution([1u8; 16], FeeContribution { amount_sats: 100 }.encode());
            g.add_fee_contribution([2u8; 16], FeeContribution { amount_sats: 250 }.encode());
            g.add_fee_contribution([3u8; 16], FeeContribution { amount_sats: 3 }.encode());
            assert_eq!(total_declared_fee(&g), 353);
        }

        #[test]
        fn total_skips_undecodable_and_saturates() {
            let mut g = Global::default();
            g.add_fee_contribution([1u8; 16], FeeContribution { amount_sats: u64::MAX }.encode());
            g.add_fee_contribution([2u8; 16], FeeContribution { amount_sats: 10 }.encode());
            // undecodable (encrypted) entry is skipped, not an error
            let mut enc = FeeContribution { amount_sats: 5 }.encode();
            enc[0] = FORMAT_ENCRYPTED;
            g.add_fee_contribution([3u8; 16], enc);
            // MAX + 10 saturates at MAX
            assert_eq!(total_declared_fee(&g), u64::MAX);
        }

        #[test]
        fn clear_removes_only_fee_band() {
            let mut g = Global::default();
            g.add_fee_contribution([1u8; 16], FeeContribution { amount_sats: 100 }.encode());
            g.add_payment([9u8; 16], vec![0u8]); // a payment in the 0x20 band
            g.clear_fee_contributions();
            assert!(g.fee_contributions().is_empty());
            // payment band untouched
            assert_eq!(g.payments().len(), 1);
        }

        #[test]
        fn fee_entries_do_not_alias_payment_entries() {
            // Same uuid under fee (0x22) vs payment (0x20) are distinct keys.
            let mut g = Global::default();
            g.add_fee_contribution([7u8; 16], FeeContribution { amount_sats: 1 }.encode());
            g.add_payment([7u8; 16], vec![0u8]);
            assert_eq!(g.fee_contributions().len(), 1);
            assert_eq!(g.payments().len(), 1);
            assert_ne!(PSBT_GLOBAL_PAYMENT_SUBTYPE, FEE_SUBTYPE);
        }

        #[test]
        fn opaque_encrypted_blob_survives_field_layer() {
            let mut g = Global::default();
            let mut blob = FeeContribution { amount_sats: 42 }.encode();
            blob[0] = FORMAT_ENCRYPTED; // pretend-encrypted; field layer is agnostic
            g.add_fee_contribution([5u8; 16], blob.clone());
            assert_eq!(g.fee_contributions()[0].1, blob);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use crate::collections::btreemap::BTreeMapExt;
        use crate::lattice::join::Join;
        use crate::lattice::partial::JoinResult;
        use crate::negotiation::FORMAT_ENCRYPTED;
        use proptest::prelude::*;
        use std::collections::BTreeMap;

        // A fee-contribution set is exactly the field
        // `Global.proprietaries: BTreeMap<ProprietaryKey, Vec<u8>>` restricted
        // to the 0x22 subtype, so it inherits the map's join wholesale
        // (`Vec<u8>: IdempotentValue`, disjoint keys union, matching keys
        // value-join). The three lattice laws for that join are already
        // established generically by the collections/btreemap.rs + values.rs +
        // lattice/partial.rs prop suites, so re-testing them here is strictly
        // redundant. We nonetheless pin them to *this feature's* wire shape
        // (real ProprietaryKey uuids in the 0x22 band, real FeeContribution
        // blobs) so the CRDT laws are demonstrated directly on fee records, per
        // the deliverable. This introduces NO new joinable type — it exercises
        // the same `BTreeMap` join Global already uses.
        //
        // The fee-specific property beyond the laws is that the read-time
        // `total_declared_fee` projection is monotone non-decreasing as the set
        // grows under join, which is what makes it a sound termination signal.

        // A fee-band uuid, drawn from a small space so collisions (the
        // interesting shared-key join cases) actually occur.
        fn arb_fee_key() -> impl Strategy<Value = raw::ProprietaryKey> {
            proptest::collection::vec(0u8..3, 16..=16).prop_map(|id| raw::ProprietaryKey {
                prefix: crate::PROPRIETARY_PREFIX.to_vec(),
                subtype: PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE,
                key: id,
            })
        }

        // A fee-record blob: a well-formed plaintext contribution or a
        // "pretend-encrypted" opaque blob. Both are just `Vec<u8>` at the field
        // layer. Small amount space so equal-blob (dedup) and unequal-blob
        // (conflict) shared-key cases both arise.
        fn arb_fee_blob() -> impl Strategy<Value = Vec<u8>> {
            prop_oneof![
                (0u64..4).prop_map(|amt| FeeContribution { amount_sats: amt }.encode()),
                Just({
                    let mut b = FeeContribution { amount_sats: 0 }.encode();
                    b[0] = FORMAT_ENCRYPTED;
                    b
                }),
            ]
        }

        // The fee band lifted into the result domain, exactly as `Global` joins
        // its `proprietaries` field (each `Vec<u8>` value wrapped as `Ok`).
        fn arb_fee_band()
        -> impl Strategy<Value = BTreeMap<raw::ProprietaryKey, JoinResult<Vec<u8>>>> {
            proptest::collection::btree_map(arb_fee_key(), arb_fee_blob(), 0..=4)
                .prop_map(BTreeMapExt::wrap)
        }

        // Idempotent / commutative / associative for the fee-band map join.
        assert_join_laws!(arb_fee_band());

        // Build a set of distinct-uuid fee contributions.
        fn fee_amounts() -> impl Strategy<Value = Vec<u64>> {
            proptest::collection::vec(any::<u64>(), 0..6)
        }

        proptest! {
            // Adding a fresh (disjoint-uuid) contribution never lowers the
            // declared total — termination monotonicity.
            #[test]
            fn sum_monotone_under_disjoint_add(
                amounts in fee_amounts(),
                extra_amt in any::<u64>(),
            ) {
                let mut g = Global::default();
                for (i, amt) in amounts.iter().enumerate() {
                    let mut id = [0u8; 16];
                    id[0..8].copy_from_slice(&(i as u64).to_le_bytes());
                    g.add_fee_contribution(id, FeeContribution { amount_sats: *amt }.encode());
                }
                let before = total_declared_fee(&g);
                let mut extra_id = [0xffu8; 16];
                extra_id[0] = 0xfe; // disjoint from the sequential ids above
                g.add_fee_contribution(extra_id, FeeContribution { amount_sats: extra_amt }.encode());
                let after = total_declared_fee(&g);
                prop_assert!(after >= before);
            }

            // Re-adding the same uuid+amount is idempotent for the total (the
            // map dedups; the summand is counted once).
            #[test]
            fn sum_idempotent_on_reemit(amt in any::<u64>()) {
                let mut g = Global::default();
                g.add_fee_contribution([3u8; 16], FeeContribution { amount_sats: amt }.encode());
                let once = total_declared_fee(&g);
                g.add_fee_contribution([3u8; 16], FeeContribution { amount_sats: amt }.encode());
                let twice = total_declared_fee(&g);
                prop_assert_eq!(once, twice);
            }
        }
    }
}
