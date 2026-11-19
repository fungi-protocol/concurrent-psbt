#![allow(clippy::result_large_err)]

//! PSBT input fields.
//!
//! [`ResultInput`] joins fields independently, retaining incompatible values
//! as conflicts.

use std::collections::BTreeMap;

use bitcoin::bip32::KeySource;
use bitcoin::hashes::{hash160, ripemd160, sha256, sha256d};
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::locktime::absolute;
use bitcoin::taproot::{ControlBlock, LeafVersion, TapLeafHash, TapNodeHash};
use bitcoin::{OutPoint, ScriptBuf, Sequence, Transaction, TxOut, Txid, Witness, ecdsa, taproot};
use psbt_v2::PsbtSighashType;
use psbt_v2::raw;

joinable_struct! {
    /// A PSBT input, mirroring the BIP 370 per-input map.
    source: Input,
    /// Result-domain counterpart of [`Input`].
    result: ResultInput,
    v2: psbt_v2::v2::Input,
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
        use bitcoin::bip32::{DerivationPath, Fingerprint};
        use bitcoin::hashes::Hash;
        use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey};
        use bitcoin::sighash::TapSighashType;
        use std::collections::BTreeMap;

        pub(crate) fn make_input(txid_byte: u8, vout: u32) -> Input {
            let txid = bitcoin::Txid::from_byte_array([txid_byte; 32]);
            Input::new(&bitcoin::OutPoint { txid, vout })
        }

        #[test]
        fn inputs_spending_different_outpoints_conflict() {
            let joined = make_input(1, 0).wrap().join(make_input(2, 0).wrap());
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }

        /// An input with every field set, so a conversion that drops one
        /// changes the input.
        fn populated_input() -> Input {
            let secp = Secp256k1::new();
            let secret_key = SecretKey::from_slice(&[1; 32]).expect("valid secret key");
            let keypair = Keypair::from_secret_key(&secp, &secret_key);
            let public_key = PublicKey::new(secret_key.public_key(&secp));
            let (x_only_public_key, _) = keypair.x_only_public_key();
            let message = Message::from_digest([2; 32]);
            let ecdsa_signature =
                ecdsa::Signature::sighash_all(secp.sign_ecdsa(&message, &secret_key));
            let taproot_signature = taproot::Signature {
                signature: secp.sign_schnorr_no_aux_rand(&message, &keypair),
                sighash_type: TapSighashType::Default,
            };
            let leaf = TapLeafHash::from_byte_array([3; 32]);
            let source = (Fingerprint::from([4; 4]), DerivationPath::master());
            let mut control_block = vec![0xc0];
            control_block.extend(x_only_public_key.serialize());
            let control_block = ControlBlock::decode(&control_block).expect("valid control block");
            let script = ScriptBuf::from_bytes(vec![5]);
            Input {
                sequence: Some(Sequence(6)),
                min_time: Some(absolute::Time::MIN),
                min_height: Some(absolute::Height::MIN),
                non_witness_utxo: Some(Transaction {
                    version: bitcoin::transaction::Version::TWO,
                    lock_time: absolute::LockTime::ZERO,
                    input: vec![],
                    output: vec![],
                }),
                witness_utxo: Some(TxOut {
                    value: bitcoin::Amount::from_sat(7),
                    script_pubkey: script.clone(),
                }),
                partial_sigs: BTreeMap::from([(public_key, ecdsa_signature)]),
                sighash_type: Some(PsbtSighashType::from_u32(1)),
                redeem_script: Some(script.clone()),
                witness_script: Some(script.clone()),
                bip32_derivations: BTreeMap::from([(public_key, source.clone())]),
                final_script_sig: Some(script.clone()),
                final_script_witness: Some(Witness::from_slice(&[[8]])),
                ripemd160_preimages: BTreeMap::from([(
                    ripemd160::Hash::from_byte_array([9; 20]),
                    vec![9],
                )]),
                sha256_preimages: BTreeMap::from([(
                    sha256::Hash::from_byte_array([10; 32]),
                    vec![10],
                )]),
                hash160_preimages: BTreeMap::from([(
                    hash160::Hash::from_byte_array([11; 20]),
                    vec![11],
                )]),
                hash256_preimages: BTreeMap::from([(
                    sha256d::Hash::from_byte_array([12; 32]),
                    vec![12],
                )]),
                tap_key_sig: Some(taproot_signature),
                tap_script_sigs: BTreeMap::from([((x_only_public_key, leaf), taproot_signature)]),
                tap_scripts: BTreeMap::from([(control_block, (script, LeafVersion::TapScript))]),
                tap_key_origins: BTreeMap::from([(x_only_public_key, (vec![leaf], source))]),
                tap_internal_key: Some(x_only_public_key),
                tap_merkle_root: Some(TapNodeHash::from_byte_array([13; 32])),
                proprietaries: BTreeMap::from([(
                    raw::ProprietaryKey {
                        prefix: vec![14],
                        subtype: 15,
                        key: vec![],
                    },
                    vec![16],
                )]),
                unknowns: BTreeMap::from([(
                    raw::Key {
                        type_value: 0xfe,
                        key: vec![17],
                    },
                    vec![18],
                )]),
                ..make_input(19, 20)
            }
        }

        #[test]
        fn v2_conversion_round_trips_a_populated_input() {
            let input = populated_input();
            assert_eq!(Input::from(psbt_v2::v2::Input::from(input.clone())), input);
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
            fn wrap_try_unwrap_roundtrip(i in prop_oneof![arb_input(), arb_input_with_fields()]) {
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
