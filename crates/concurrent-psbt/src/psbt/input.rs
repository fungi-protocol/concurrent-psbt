#![allow(clippy::result_large_err)]

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

/// Extract the [`OutPoint`] this input spends (previous txid + output index).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used by InputSet once inputs are keyed by spent outpoint"
    )
)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn out_point(input: &Input) -> OutPoint {
    OutPoint {
        txid: input.previous_txid,
        vout: input.spent_output_index,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use bitcoin::hashes::Hash;

        fn make_input(txid_byte: u8, vout: u32) -> Input {
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
        fn out_point_extraction() {
            let op = out_point(&make_input(1, 42));
            assert_eq!(op.txid, bitcoin::Txid::from_byte_array([1; 32]));
            assert_eq!(op.vout, 42);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use bitcoin::hashes::Hash;
        use proptest::prelude::*;

        fn arb_outpoint() -> impl Strategy<Value = OutPoint> {
            (0u8..3, 0u32..2).prop_map(|(txid_byte, vout)| OutPoint {
                txid: Txid::from_byte_array([txid_byte; 32]),
                vout,
            })
        }

        fn arb_input() -> impl Strategy<Value = Input> {
            (
                arb_outpoint(),
                proptest::option::of(0u32..8),
                proptest::option::of(1u64..10_000),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
            )
                .prop_map(|(op, seq, utxo_val, redeem, witness)| Input {
                    sequence: seq.map(Sequence),
                    witness_utxo: utxo_val.map(|v| TxOut {
                        value: bitcoin::Amount::from_sat(v),
                        script_pubkey: ScriptBuf::new(),
                    }),
                    redeem_script: redeem.map(ScriptBuf::from_bytes),
                    witness_script: witness.map(ScriptBuf::from_bytes),
                    ..Input::new(&op)
                })
        }

        fn arb_result_input() -> impl Strategy<Value = ResultInput> {
            prop_oneof![
                arb_input().prop_map(|i| i.wrap()),
                (arb_input(), arb_input()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        proptest! {
            #[test]
            fn out_point_roundtrip(i in arb_input()) {
                let op = out_point(&i);
                prop_assert_eq!(op.txid, i.previous_txid);
                prop_assert_eq!(op.vout, i.spent_output_index);
            }

            #[test]
            fn wrap_try_unwrap_roundtrip(i in arb_input()) {
                let wrapped = i.wrap();
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

            #[test]
            fn idempotent(a in arb_result_input()) {
                prop_assert_eq!(a.clone().join(a.clone()), a);
            }

            #[test]
            fn commutative(a in arb_result_input(), b in arb_result_input()) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn associative(
                a in arb_result_input(),
                b in arb_result_input(),
                c in arb_result_input(),
            ) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).join(c.clone()),
                    a.join(b.join(c)),
                );
            }
        }
    }
}
