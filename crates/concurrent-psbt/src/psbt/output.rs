#![allow(clippy::result_large_err)]

use bitcoin::bip32::KeySource;
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::taproot::{TapLeafHash, TapTree};
use bitcoin::{Amount, ScriptBuf};
use psbt_v2::raw;
use psbt_v2::v2::Output;

joinable_struct! {
    /// Result-domain representation of a PSBT output.
    ///
    /// Each field mirrors its counterpart in [`psbt_v2::v2::Output`] but is
    /// represented in the internal result domain, recording either a clean value
    /// or an accumulated conflict.
    source: Output,
    result: ResultOutput,
    ext: OutputExt,
    fields: {
        amount: Amount,
        script_pubkey: ScriptBuf,
        redeem_script: Option<ScriptBuf>,
        witness_script: Option<ScriptBuf>,
        bip32_derivations: BTreeMap<PublicKey, KeySource>,
        tap_internal_key: Option<XOnlyPublicKey>,
        tap_tree: Option<TapTree>,
        tap_key_origins: BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, KeySource)>,
        proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        unknowns: BTreeMap<raw::Key, Vec<u8>>,
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
        use bitcoin::secp256k1::{Secp256k1, SecretKey};

        const ALL_CONFLICT_FIELDS: &[&str] = &[
            "amount",
            "script_pubkey",
            "redeem_script",
            "witness_script",
            "bip32_derivations",
            "tap_internal_key",
            "tap_tree",
            "tap_key_origins",
            "proprietaries",
            "unknowns",
        ];

        fn empty_conflict<T: PartialJoin>() -> JoinResult<T> {
            Err(Conflict::from_values([]))
        }

        fn all_fields_conflict(seed: u8) -> ResultOutput {
            let mut result = Output::default().wrap();
            result.amount = empty_conflict();
            result.script_pubkey = empty_conflict();
            result.redeem_script = Some(empty_conflict());
            result.witness_script = Some(empty_conflict());
            result.tap_internal_key = Some(empty_conflict());
            result.tap_tree = Some(empty_conflict());

            let secret_key = SecretKey::from_slice(&[seed; 32]).expect("bounded nonzero seed");
            let secp_public_key =
                bitcoin::secp256k1::PublicKey::from_secret_key(&Secp256k1::new(), &secret_key);
            let public_key = PublicKey::new(secp_public_key);
            let (x_only_public_key, _) = secp_public_key.x_only_public_key();
            result
                .bip32_derivations
                .insert(public_key, empty_conflict());
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

        #[test]
        fn try_unwrap_conflicting_returns_err() {
            let a = Output {
                amount: bitcoin::Amount::from_sat(100),
                ..Output::default()
            };
            let b = Output {
                amount: bitcoin::Amount::from_sat(200),
                ..Output::default()
            };
            assert!(a.wrap().join(b.wrap()).try_unwrap().is_err());
        }

        #[test]
        fn for_each_conflict_reports_every_conflicted_field() {
            let result = all_fields_conflict(1);
            let mut fields = Vec::new();
            result.for_each_conflict(|field, _| fields.push(field.to_owned()));
            assert_eq!(fields, ALL_CONFLICT_FIELDS);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        fn arb_output() -> impl Strategy<Value = Output> {
            (
                any::<u64>(),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
            )
                .prop_map(|(amount, redeem, witness)| Output {
                    amount: bitcoin::Amount::from_sat(amount),
                    script_pubkey: ScriptBuf::new(),
                    redeem_script: redeem.map(ScriptBuf::from_bytes),
                    witness_script: witness.map(ScriptBuf::from_bytes),
                    ..Output::default()
                })
        }

        fn arb_result_output() -> impl Strategy<Value = ResultOutput> {
            prop_oneof![
                arb_output().prop_map(|o| o.wrap()),
                (arb_output(), arb_output()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        assert_join_laws!(arb_result_output());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip(o in arb_output()) {
                let wrapped = o.wrap();
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn is_ok_reflects_content(r in arb_result_output()) {
                if r.is_ok() {
                    prop_assert!(r.try_unwrap().is_ok());
                } else {
                    prop_assert!(r.try_unwrap().is_err());
                }
            }

        }
    }
}
