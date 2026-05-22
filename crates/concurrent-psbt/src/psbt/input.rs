#![allow(clippy::result_large_err)]

//! Per-input field type and input set merging.
//!
//! [`ResultInput`] wraps each per-input field in the internal result domain.
//! [`InputSet`] and [`ResultInputSet`] are `HashMap<OutPoint, _>` collections
//! that key inputs by the outpoint they spend, enabling order-insensitive merging.

use crate::collections::hashmap::HashMapResultValue;

use bitcoin::bip32::KeySource;
use bitcoin::hashes::{hash160, ripemd160, sha256, sha256d};
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::locktime::absolute;
use bitcoin::taproot::{ControlBlock, LeafVersion, TapLeafHash, TapNodeHash};
use bitcoin::{OutPoint, ScriptBuf, Sequence, Transaction, TxOut, Txid, Witness, ecdsa, taproot};
use psbt_v2::raw;
use psbt_v2::{PsbtSighashType, v2::Input};

joinable_struct! {
    /// Result-domain representation of a PSBT input.
    ///
    /// Each field mirrors its counterpart in [`psbt_v2::v2::Input`] but is
    /// represented in the internal result domain, recording either a clean value
    /// or an accumulated conflict.
    /// Use [`InputExt::wrap`] to lift a clean [`psbt_v2::v2::Input`] into this type, and
    /// [`ResultInput::try_unwrap`] to recover the clean value once all conflicts are resolved.
    source: Input,
    result: ResultInput,
    ext: InputExt,
    fields: {
        previous_txid: Txid,
        spent_output_index: u32,
        sequence: Option<Sequence>,
        min_time: Option<absolute::Time>,
        min_height: Option<absolute::Height>,
        non_witness_utxo: Option<Transaction>,
        witness_utxo: Option<TxOut>,
        partial_sigs: BTreeMap<PublicKey, ecdsa::Signature>,
        sighash_type: Option<PsbtSighashType>,
        redeem_script: Option<ScriptBuf>,
        witness_script: Option<ScriptBuf>,
        bip32_derivations: BTreeMap<PublicKey, KeySource>,
        final_script_sig: Option<ScriptBuf>,
        final_script_witness: Option<Witness>,
        ripemd160_preimages: BTreeMap<ripemd160::Hash, Vec<u8>>,
        sha256_preimages: BTreeMap<sha256::Hash, Vec<u8>>,
        hash160_preimages: BTreeMap<hash160::Hash, Vec<u8>>,
        hash256_preimages: BTreeMap<sha256d::Hash, Vec<u8>>,
        tap_key_sig: Option<taproot::Signature>,
        tap_script_sigs: BTreeMap<(XOnlyPublicKey, TapLeafHash), taproot::Signature>,
        tap_scripts: BTreeMap<ControlBlock, (ScriptBuf, LeafVersion)>,
        tap_key_origins: BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, KeySource)>,
        tap_internal_key: Option<XOnlyPublicKey>,
        tap_merkle_root: Option<TapNodeHash>,
        proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        unknowns: BTreeMap<raw::Key, Vec<u8>>,
    }
}

impl HashMapResultValue for ResultInput {
    type Clean = Input;

    fn is_ok(&self) -> bool {
        ResultInput::is_ok(self)
    }

    fn try_unwrap(self) -> Result<Input, Self> {
        ResultInput::try_unwrap(self)
    }
}

/// Extract the [`OutPoint`] this input spends (previous txid + output index).
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn out_point(input: &Input) -> OutPoint {
    OutPoint {
        txid: input.previous_txid,
        vout: input.spent_output_index,
    }
}

pub use super::input_set::{InputSet, ResultInputSet};
/// Subtype for `PSBT_IN_SORT_KEY`.
///
/// Up to 32 bytes of arbitrary data used as a lexicographic sort key.
/// Must be distinct across all inputs in a PSBT.
pub const PSBT_IN_SORT_KEY_SUBTYPE: u8 = 0x10;

/// Extension trait on [`psbt_v2::v2::Input`] for accessing the sort key proprietary field.
pub trait InputSortKeyExt {
    /// Get the sort key, if set.
    fn sort_key(&self) -> Option<&[u8]>;
    /// Set the sort key.
    #[cfg(test)]
    fn set_sort_key(&mut self, key: Vec<u8>);
}

impl InputSortKeyExt for Input {
    fn sort_key(&self) -> Option<&[u8]> {
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: crate::PROPRIETARY_PREFIX.to_vec(),
            subtype: PSBT_IN_SORT_KEY_SUBTYPE,
            key: vec![],
        };
        self.proprietaries.get(&key).map(|v| v.as_slice())
    }

    #[cfg(test)]
    fn set_sort_key(&mut self, sort_key: Vec<u8>) {
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: crate::PROPRIETARY_PREFIX.to_vec(),
            subtype: PSBT_IN_SORT_KEY_SUBTYPE,
            key: vec![],
        };
        self.proprietaries.insert(key, sort_key);
    }
}
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod tests {
    use super::*;
    use crate::lattice::join::Join;
    use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};
    use bitcoin::hashes::Hash;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};

    const ALL_CONFLICT_FIELDS: &[&str] = &[
        "previous_txid",
        "spent_output_index",
        "sequence",
        "min_time",
        "min_height",
        "non_witness_utxo",
        "witness_utxo",
        "partial_sigs",
        "sighash_type",
        "redeem_script",
        "witness_script",
        "bip32_derivations",
        "final_script_sig",
        "final_script_witness",
        "ripemd160_preimages",
        "sha256_preimages",
        "hash160_preimages",
        "hash256_preimages",
        "tap_key_sig",
        "tap_script_sigs",
        "tap_scripts",
        "tap_key_origins",
        "tap_internal_key",
        "tap_merkle_root",
        "proprietaries",
        "unknowns",
    ];

    fn empty_conflict<T: PartialJoin>() -> JoinResult<T> {
        Err(Conflict::from_values([]))
    }

    fn all_fields_conflict(seed: u8) -> ResultInput {
        let outpoint = OutPoint {
            txid: Txid::from_byte_array([seed; 32]),
            vout: u32::from(seed),
        };
        let mut result = Input::new(&outpoint).wrap();

        result.previous_txid = empty_conflict();
        result.spent_output_index = empty_conflict();
        result.sequence = Some(empty_conflict());
        result.min_time = Some(empty_conflict());
        result.min_height = Some(empty_conflict());
        result.non_witness_utxo = Some(empty_conflict());
        result.witness_utxo = Some(empty_conflict());
        result.sighash_type = Some(empty_conflict());
        result.redeem_script = Some(empty_conflict());
        result.witness_script = Some(empty_conflict());
        result.final_script_sig = Some(empty_conflict());
        result.final_script_witness = Some(empty_conflict());
        result.tap_key_sig = Some(empty_conflict());
        result.tap_internal_key = Some(empty_conflict());
        result.tap_merkle_root = Some(empty_conflict());

        let secret_key = SecretKey::from_slice(&[seed; 32]).expect("bounded nonzero seed");
        let secp_public_key =
            bitcoin::secp256k1::PublicKey::from_secret_key(&Secp256k1::new(), &secret_key);
        let public_key = PublicKey::new(secp_public_key);
        let (x_only_public_key, _) = secp_public_key.x_only_public_key();
        let mut control_block = vec![0xc0];
        control_block.extend(x_only_public_key.serialize());
        let control_block = ControlBlock::decode(&control_block).expect("valid control block");

        result.partial_sigs.insert(public_key, empty_conflict());
        result
            .bip32_derivations
            .insert(public_key, empty_conflict());
        result.ripemd160_preimages.insert(
            ripemd160::Hash::from_byte_array([seed; 20]),
            empty_conflict(),
        );
        result
            .sha256_preimages
            .insert(sha256::Hash::from_byte_array([seed; 32]), empty_conflict());
        result
            .hash160_preimages
            .insert(hash160::Hash::from_byte_array([seed; 20]), empty_conflict());
        result
            .hash256_preimages
            .insert(sha256d::Hash::from_byte_array([seed; 32]), empty_conflict());
        result.tap_script_sigs.insert(
            (x_only_public_key, TapLeafHash::from_byte_array([seed; 32])),
            empty_conflict(),
        );
        result.tap_scripts.insert(control_block, empty_conflict());
        result
            .tap_key_origins
            .insert(x_only_public_key, empty_conflict());
        result.proprietaries.insert(
            raw::ProprietaryKey {
                prefix: vec![seed],
                subtype: seed,
                key: vec![seed],
            },
            empty_conflict(),
        );
        result.unknowns.insert(
            raw::Key {
                type_value: seed,
                key: vec![seed],
            },
            empty_conflict(),
        );

        result
    }

    #[cfg(feature = "unit-tests")]
    pub(crate) mod unit {
        use super::*;
        use bitcoin::hashes::Hash;

        pub(crate) fn make_input(txid_byte: u8, vout: u32) -> Input {
            let txid = bitcoin::Txid::from_byte_array([txid_byte; 32]);
            Input::new(&bitcoin::OutPoint { txid, vout })
        }

        #[test]
        fn wrap_input_is_ok() {
            assert!(make_input(1, 0).wrap().is_ok());
        }

        #[test]
        fn wrap_try_unwrap_roundtrip() {
            assert!(make_input(1, 0).wrap().try_unwrap().is_ok());
        }

        #[test]
        fn join_identical_inputs_is_ok() {
            assert!(
                make_input(1, 0)
                    .wrap()
                    .join(make_input(1, 0).wrap())
                    .is_ok()
            );
        }

        #[test]
        fn join_different_txids_conflicts() {
            assert!(
                !make_input(1, 0)
                    .wrap()
                    .join(make_input(2, 0).wrap())
                    .is_ok()
            );
        }

        #[test]
        fn try_unwrap_conflicting_returns_err() {
            assert!(
                make_input(1, 0)
                    .wrap()
                    .join(make_input(2, 0).wrap())
                    .try_unwrap()
                    .is_err()
            );
        }

        #[test]
        fn for_each_conflict_reports_every_conflicted_field() {
            let result = all_fields_conflict(1);
            let mut fields = Vec::new();
            result.for_each_conflict(|field, _| fields.push(field.to_owned()));

            assert_eq!(fields, ALL_CONFLICT_FIELDS);
        }

        #[test]
        fn out_point_extraction() {
            let op = out_point(&make_input(1, 42));
            assert_eq!(op.txid, bitcoin::Txid::from_byte_array([1; 32]));
            assert_eq!(op.vout, 42);
        }
    }

    #[cfg(feature = "prop-tests")]
    pub(crate) mod prop {
        use super::*;
        use bitcoin::hashes::Hash;
        use proptest::prelude::*;

        // Small domain for outpoints to ensure collisions
        pub(crate) fn arb_outpoint() -> impl Strategy<Value = OutPoint> {
            (0u8..3, 0u32..2).prop_map(|(txid_byte, vout)| OutPoint {
                txid: Txid::from_byte_array([txid_byte; 32]),
                vout,
            })
        }

        pub(crate) fn arb_input() -> impl Strategy<Value = Input> {
            arb_outpoint().prop_map(|op| Input::new(&op))
        }

        pub(crate) fn arb_result_input() -> impl Strategy<Value = ResultInput> {
            prop_oneof![
                arb_input().prop_map(|i| i.wrap()),
                (arb_input(), arb_input()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        // Input with randomised optional fields to exercise all wrap/join paths.
        pub(crate) fn arb_input_with_fields() -> impl Strategy<Value = Input> {
            (
                arb_outpoint(),
                proptest::option::of(0u32..8),      // sequence
                proptest::option::of(1u64..10_000), // witness_utxo value (sats)
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)), // redeem_script
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)), // witness_script
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)), // final_script_sig
                proptest::collection::btree_map(
                    proptest::collection::vec(0u8..255, 1..=4),
                    proptest::collection::vec(0u8..255, 0..=4),
                    0..=2,
                ), // proprietaries
            )
                .prop_map(
                    |(op, seq, utxo_val, redeem, witness_s, final_sig, props)| {
                        let mut input = Input {
                            sequence: seq.map(Sequence),
                            witness_utxo: utxo_val.map(|v| TxOut {
                                value: bitcoin::Amount::from_sat(v),
                                script_pubkey: ScriptBuf::new(),
                            }),
                            redeem_script: redeem.map(ScriptBuf::from_bytes),
                            witness_script: witness_s.map(ScriptBuf::from_bytes),
                            final_script_sig: final_sig.map(ScriptBuf::from_bytes),
                            ..Input::new(&op)
                        };
                        for (k, v) in props {
                            input.proprietaries.insert(
                                raw::ProprietaryKey {
                                    prefix: b"test".to_vec(),
                                    subtype: k[0],
                                    key: k,
                                },
                                v,
                            );
                        }
                        input
                    },
                )
        }

        proptest! {
            #[test]
            fn for_each_conflict_reports_every_conflicted_field(seed in 1u8..=0x7f) {
                let result = all_fields_conflict(seed);
                let mut fields = Vec::new();
                result.for_each_conflict(|field, _| fields.push(field.to_owned()));

                prop_assert_eq!(fields, ALL_CONFLICT_FIELDS);
            }

            // ── 1. wrap → try_unwrap roundtrip ──────────────────────────
            #[test]
            fn wrap_try_unwrap_roundtrip_input(a in arb_input_with_fields()) {
                let wrapped = a.wrap();
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn is_ok_try_unwrap_consistent(r in arb_result_input()) {
                if r.is_ok() {
                    prop_assert!(r.try_unwrap().is_ok());
                } else {
                    prop_assert!(r.try_unwrap().is_err());
                }
            }

            // ── 2. ResultInput field-level join ─────────────────────────
            #[test]
            fn field_join_same_outpoint_compatible(
                op in arb_outpoint(),
                seq in proptest::option::of(0u32..4),
                val in proptest::option::of(1u64..100),
            ) {
                // Two inputs for the SAME outpoint with the SAME optional values
                // must always join successfully.
                let a = Input {
                    sequence: seq.map(Sequence),
                    witness_utxo: val.map(|v| TxOut {
                        value: bitcoin::Amount::from_sat(v),
                        script_pubkey: ScriptBuf::new(),
                    }),
                    ..Input::new(&op)
                };
                let b = a.clone();
                let joined = a.wrap().join(b.wrap());
                prop_assert!(joined.is_ok());
                let unwrapped = joined.try_unwrap().unwrap();
                prop_assert_eq!(unwrapped.sequence, seq.map(Sequence));
            }

            #[test]
            fn field_join_different_sequence_conflicts(
                op in arb_outpoint(),
                s1 in 0u32..100,
                s2 in 100u32..200,
            ) {
                // Two inputs for the same outpoint with DIFFERENT sequences
                // must produce a conflict.
                let a = Input { sequence: Some(Sequence(s1)), ..Input::new(&op) };
                let b = Input { sequence: Some(Sequence(s2)), ..Input::new(&op) };
                let joined = a.wrap().join(b.wrap());
                prop_assert!(!joined.is_ok());
                prop_assert!(joined.try_unwrap().is_err());
            }

            #[test]
            fn field_join_none_vs_some_is_ok(
                op in arb_outpoint(),
                s in 0u32..100,
            ) {
                // None ⊔ Some(x) == Some(x) — always ok
                let a = Input { sequence: None, ..Input::new(&op) };
                let b = Input { sequence: Some(Sequence(s)), ..Input::new(&op) };
                let joined = a.wrap().join(b.wrap());
                prop_assert!(joined.is_ok());
                let unwrapped = joined.try_unwrap().unwrap();
                prop_assert_eq!(unwrapped.sequence, Some(Sequence(s)));
            }

            #[test]
            fn field_join_witness_utxo_conflict(
                op in arb_outpoint(),
                v1 in 1u64..500,
                v2 in 500u64..1000,
            ) {
                let utxo = |v| TxOut {
                    value: bitcoin::Amount::from_sat(v),
                    script_pubkey: ScriptBuf::new(),
                };
                let a = Input { witness_utxo: Some(utxo(v1)), ..Input::new(&op) };
                let b = Input { witness_utxo: Some(utxo(v2)), ..Input::new(&op) };
                let joined = a.wrap().join(b.wrap());
                prop_assert!(!joined.is_ok());
            }

        }

        proptest! {
            #[test]
            fn out_point_roundtrip(i in arb_input()) {
                let op = out_point(&i);
                prop_assert_eq!(op.txid, i.previous_txid);
                prop_assert_eq!(op.vout, i.spent_output_index);
            }

            // ResultInput lattice laws
            #[test]
            fn result_input_idempotent(a in arb_result_input()) {
                prop_assert_eq!(a.clone().join(a.clone()), a);
            }

            #[test]
            fn result_input_commutative(a in arb_result_input(), b in arb_result_input()) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn result_input_associative(
                a in arb_result_input(),
                b in arb_result_input(),
                c in arb_result_input(),
            ) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).join(c.clone()),
                    a.join(b.join(c))
                );
            }

        }
    }
}
