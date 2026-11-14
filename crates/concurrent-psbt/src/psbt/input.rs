#![allow(clippy::result_large_err)]

use bitcoin::bip32::KeySource;
use bitcoin::hashes::{hash160, ripemd160, sha256, sha256d};
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::locktime::absolute;
use bitcoin::taproot::{ControlBlock, LeafVersion, TapLeafHash, TapNodeHash};
use bitcoin::{OutPoint, ScriptBuf, Sequence, Transaction, TxOut, Txid, Witness, ecdsa, taproot};
use psbt_v2::PsbtSighashType;
use psbt_v2::raw;

use super::tap_leaf_hashes::{TapLeafHashes, tap_key_origins_from_v2, tap_key_origins_into_v2};

joinable_struct! {
    /// A PSBT input, mirroring the BIP 370 per-input map.
    source: Input,
    /// Result-domain counterpart of [`Input`].
    ///
    /// Each field holds a clean value or a conflict. [`Input::wrap`] lifts an
    /// [`Input`] into this type, and [`ResultInput::try_unwrap`] returns it if
    /// no field conflicts.
    result: ResultInput,
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
        pub tap_key_origins: BTreeMap<XOnlyPublicKey, (TapLeafHashes, KeySource)>,
        pub tap_internal_key: Option<XOnlyPublicKey>,
        pub tap_merkle_root: Option<TapNodeHash>,
        pub proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        pub unknowns: BTreeMap<raw::Key, Vec<u8>>,
    }
}

impl Input {
    /// Create an input spending `outpoint`, with every other field unset.
    pub fn new(outpoint: &OutPoint) -> Self {
        psbt_v2::v2::Input::new(outpoint).into()
    }
}

impl From<psbt_v2::v2::Input> for Input {
    fn from(input: psbt_v2::v2::Input) -> Self {
        let psbt_v2::v2::Input {
            previous_txid,
            spent_output_index,
            sequence,
            min_time,
            min_height,
            non_witness_utxo,
            witness_utxo,
            partial_sigs,
            sighash_type,
            redeem_script,
            witness_script,
            bip32_derivations,
            final_script_sig,
            final_script_witness,
            ripemd160_preimages,
            sha256_preimages,
            hash160_preimages,
            hash256_preimages,
            tap_key_sig,
            tap_script_sigs,
            tap_scripts,
            tap_key_origins,
            tap_internal_key,
            tap_merkle_root,
            proprietaries,
            unknowns,
            ..
        } = input;
        Self {
            previous_txid,
            spent_output_index,
            sequence,
            min_time,
            min_height,
            non_witness_utxo,
            witness_utxo,
            partial_sigs,
            sighash_type,
            redeem_script,
            witness_script,
            bip32_derivations,
            final_script_sig,
            final_script_witness,
            ripemd160_preimages,
            sha256_preimages,
            hash160_preimages,
            hash256_preimages,
            tap_key_sig,
            tap_script_sigs,
            tap_scripts,
            tap_key_origins: tap_key_origins_from_v2(tap_key_origins),
            tap_internal_key,
            tap_merkle_root,
            proprietaries,
            unknowns,
        }
    }
}

impl From<Input> for psbt_v2::v2::Input {
    #[expect(
        clippy::needless_update,
        reason = "keeps building if psbt-v2 adds fields this crate does not support yet"
    )]
    fn from(input: Input) -> Self {
        let outpoint = out_point(&input);
        Self {
            previous_txid: input.previous_txid,
            spent_output_index: input.spent_output_index,
            sequence: input.sequence,
            min_time: input.min_time,
            min_height: input.min_height,
            non_witness_utxo: input.non_witness_utxo,
            witness_utxo: input.witness_utxo,
            partial_sigs: input.partial_sigs,
            sighash_type: input.sighash_type,
            redeem_script: input.redeem_script,
            witness_script: input.witness_script,
            bip32_derivations: input.bip32_derivations,
            final_script_sig: input.final_script_sig,
            final_script_witness: input.final_script_witness,
            ripemd160_preimages: input.ripemd160_preimages,
            sha256_preimages: input.sha256_preimages,
            hash160_preimages: input.hash160_preimages,
            hash256_preimages: input.hash256_preimages,
            tap_key_sig: input.tap_key_sig,
            tap_script_sigs: input.tap_script_sigs,
            tap_scripts: input.tap_scripts,
            tap_key_origins: tap_key_origins_into_v2(input.tap_key_origins),
            tap_internal_key: input.tap_internal_key,
            tap_merkle_root: input.tap_merkle_root,
            proprietaries: input.proprietaries,
            unknowns: input.unknowns,
            ..psbt_v2::v2::Input::new(&outpoint)
        }
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
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

        fn make_input(txid_byte: u8, vout: u32) -> Input {
            let txid = bitcoin::Txid::from_byte_array([txid_byte; 32]);
            Input::new(&bitcoin::OutPoint { txid, vout })
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
            result.for_each_conflict(|field, _, _| fields.push(field.to_owned()));

            assert_eq!(fields, ALL_CONFLICT_FIELDS);
        }

        #[test]
        fn v2_conversion_keeps_every_field() {
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
                let wrapped = i.wrap();
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn v2_roundtrip(i in arb_input()) {
                prop_assert_eq!(Input::from(psbt_v2::v2::Input::from(i.clone())), i);
            }

            #[test]
            fn is_ok_try_unwrap_consistent(r in arb_result_input()) {
                if r.is_ok() {
                    prop_assert!(r.try_unwrap().is_ok());
                } else {
                    prop_assert!(r.try_unwrap().is_err());
                }
            }

        }
    }
}
