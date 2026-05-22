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

        #[test]
        fn wrap_default_output_is_ok() {
            assert!(Output::default().wrap().is_ok());
        }

        #[test]
        fn wrap_try_unwrap_roundtrip() {
            assert!(Output::default().wrap().try_unwrap().is_ok());
        }

        #[test]
        fn join_identical_outputs_is_ok() {
            assert!(
                Output::default()
                    .wrap()
                    .join(Output::default().wrap())
                    .is_ok()
            );
        }

        #[test]
        fn join_different_amounts_conflicts() {
            let a = Output {
                amount: bitcoin::Amount::from_sat(100),
                ..Output::default()
            };
            let b = Output {
                amount: bitcoin::Amount::from_sat(200),
                ..Output::default()
            };
            assert!(!a.wrap().join(b.wrap()).is_ok());
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

            #[test]
            fn idempotent(a in arb_result_output()) {
                prop_assert_eq!(a.clone().join(a.clone()), a);
            }

            #[test]
            fn commutative(a in arb_result_output(), b in arb_result_output()) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn associative(
                a in arb_result_output(),
                b in arb_result_output(),
                c in arb_result_output(),
            ) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).join(c.clone()),
                    a.join(b.join(c)),
                );
            }
        }
    }
}
