#![allow(clippy::result_large_err)]

//! Per-input field type and input set merging.
//!
//! [`ResultInput`] wraps each per-input field in the internal result domain.
//! [`InputSet`] and [`ResultInputSet`] are sets of inputs identified by the
//! outpoint each spends. Joining two sets joins the inputs that spend the same
//! outpoint field by field, and fields that differ become conflicts.

use std::collections::BTreeMap;

use bitcoin::bip32::KeySource;
use bitcoin::hashes::{hash160, ripemd160, sha256, sha256d};
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::locktime::absolute;
use bitcoin::taproot::{ControlBlock, LeafVersion, TapLeafHash, TapNodeHash};
use bitcoin::{OutPoint, ScriptBuf, Sequence, Transaction, TxOut, Txid, Witness, ecdsa, taproot};
use psbt_v2::PsbtSighashType;
use psbt_v2::raw;

use super::tap_leaf_hashes::{TapKeyOrigins, TapLeafHashes};

joinable_struct! {
    /// A PSBT input, mirroring the BIP 370 per-input map.
    source: Input,
    /// Result-domain counterpart of [`Input`].
    ///
    /// Each field holds a clean value or a conflict. [`Input::wrap`] lifts an
    /// [`Input`] into this type, and [`ResultInput::try_unwrap`] returns it if
    /// no field conflicts.
    result: ResultInput,
    v2: psbt_v2::v2::Input,
    fields: {
        pub previous_txid: Txid,
        pub spent_output_index: u32,
        pub sequence: Option<Sequence>,
        pub min_time: Option<absolute::Time>,
        pub min_height: Option<absolute::Height>,
        pub non_witness_utxo: Option<Transaction>,
        pub witness_utxo: Option<TxOut>,
        pub partial_sigs: BTreeMap<PublicKey, ecdsa::Signature>,
        pub sighash_type: Option<PsbtSighashType>,
        pub redeem_script: Option<ScriptBuf>,
        pub witness_script: Option<ScriptBuf>,
        pub bip32_derivations: BTreeMap<PublicKey, KeySource>,
        pub final_script_sig: Option<ScriptBuf>,
        pub final_script_witness: Option<Witness>,
        pub ripemd160_preimages: BTreeMap<ripemd160::Hash, Vec<u8>>,
        pub sha256_preimages: BTreeMap<sha256::Hash, Vec<u8>>,
        pub hash160_preimages: BTreeMap<hash160::Hash, Vec<u8>>,
        pub hash256_preimages: BTreeMap<sha256d::Hash, Vec<u8>>,
        pub tap_key_sig: Option<taproot::Signature>,
        pub tap_script_sigs: BTreeMap<(XOnlyPublicKey, TapLeafHash), taproot::Signature>,
        pub tap_scripts: BTreeMap<ControlBlock, (ScriptBuf, LeafVersion)>,
        pub tap_key_origins: BTreeMap<XOnlyPublicKey, (TapLeafHashes, KeySource)> = TapKeyOrigins,
        pub tap_internal_key: Option<XOnlyPublicKey>,
        pub tap_merkle_root: Option<TapNodeHash>,
        pub proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        pub unknowns: BTreeMap<raw::Key, Vec<u8>>,
    },
    proprietary: {},
}

impl Input {
    /// Create an input spending `outpoint`, with every other field unset.
    pub fn new(outpoint: &OutPoint) -> Self {
        Self::from_v2_fields(psbt_v2::v2::Input::new(outpoint))
    }
}

impl From<psbt_v2::v2::Input> for Input {
    fn from(input: psbt_v2::v2::Input) -> Self {
        let Ok(input) = Self::decode_v2::<std::convert::Infallible>(input);
        input
    }
}

impl From<Input> for psbt_v2::v2::Input {
    fn from(input: Input) -> Self {
        let base = psbt_v2::v2::Input::new(&out_point(&input));
        input.encode_v2(base)
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

pub use super::input_set::{DuplicateInput, InputSet, ResultInputSet};
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod tests {
    use super::*;
    use crate::lattice::join::Join;

    #[cfg(feature = "unit-tests")]
    pub(crate) mod unit {
        use super::*;
        use bitcoin::hashes::Hash;

        pub(crate) fn make_input(txid_byte: u8, vout: u32) -> Input {
            let txid = bitcoin::Txid::from_byte_array([txid_byte; 32]);
            Input::new(&bitcoin::OutPoint { txid, vout })
        }

        #[test]
        fn inputs_spending_different_outpoints_conflict() {
            let joined = make_input(1, 0).wrap().join(make_input(2, 0).wrap());
            let mut fields = Vec::new();
            joined.for_each_conflict(|field, _, _| fields.push(field.to_owned()));
            assert_eq!(fields, ["previous_txid"]);
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }

        #[test]
        fn v2_conversion_round_trips_a_populated_input() {
            let mut input = make_input(1, 2);
            input.sequence = Some(Sequence(3));
            input.proprietaries.insert(
                raw::ProprietaryKey {
                    prefix: vec![1],
                    subtype: 2,
                    key: vec![],
                },
                vec![4],
            );
            let v2 = psbt_v2::v2::Input::from(input.clone());
            assert_eq!(
                (v2.previous_txid, v2.spent_output_index, v2.sequence),
                (input.previous_txid, 2, Some(Sequence(3)))
            );
            assert_eq!(Input::from(v2), input);
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

        /// Wrapped inputs, and joins of two, drawn with optional and map fields
        /// populated so the join laws exercise every field category.
        pub(crate) fn arb_result_input() -> impl Strategy<Value = ResultInput> {
            prop_oneof![
                arb_input_with_fields().prop_map(|i| i.wrap()),
                (arb_input_with_fields(), arb_input_with_fields())
                    .prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        // An input with some optional and proprietary fields populated.
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
            fn wrap_try_unwrap_roundtrip(i in arb_input_with_fields()) {
                prop_assert_eq!(i.clone().wrap().try_unwrap(), Ok(i));
            }

            #[test]
            fn v2_roundtrip(i in arb_input_with_fields()) {
                prop_assert_eq!(Input::from(psbt_v2::v2::Input::from(i.clone())), i);
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(r in arb_result_input()) {
                prop_assert_eq!(r.is_ok(), r.try_unwrap().is_ok());
            }
        }

        assert_join_laws!(arb_result_input());
    }
}
