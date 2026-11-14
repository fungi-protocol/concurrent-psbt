#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;

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
    not(all(test, feature = "unit-tests")),
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
        fn inputs_spending_different_outpoints_conflict() {
            let joined = make_input(1, 0).wrap().join(make_input(2, 0).wrap());
            let mut fields = Vec::new();
            joined.for_each_conflict(|field, _, _| fields.push(field.to_owned()));
            assert_eq!(fields, ["previous_txid"]);
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
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

        assert_join_laws!(arb_result_input());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip(i in arb_input()) {
                prop_assert_eq!(i.clone().wrap().try_unwrap(), Ok(i));
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(r in arb_result_input()) {
                prop_assert_eq!(r.is_ok(), r.try_unwrap().is_ok());
            }

        }
    }
}
