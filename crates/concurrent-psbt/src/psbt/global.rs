#![allow(clippy::result_large_err)]

//! PSBT global field merging.
//!
//! [`ResultGlobal`] wraps each global field in the internal result domain,
//! enabling field-by-field merging that accumulates conflicts without short-circuiting.
//! [`GlobalExt::wrap`] lifts a clean [`Global`] into the result domain.

use bitcoin::bip32::{KeySource, Xpub};
use bitcoin::locktime::absolute;
use bitcoin::transaction;

use psbt_v2::Version;
use psbt_v2::raw;
use psbt_v2::v2::Global;

joinable_struct! {
    /// Result-domain wrapper around a BIP 370 [`Global`].
    ///
    /// Produced by joining two [`Global`] values via [`GlobalExt::wrap`].
    /// Use [`ResultGlobal::is_ok`] to check for conflicts and
    /// [`ResultGlobal::try_unwrap`] to extract.
    source: Global,
    result: ResultGlobal,
    ext: GlobalExt,
    fields: {
        version: Version,
        tx_version: transaction::Version,
        fallback_lock_time: Option<absolute::LockTime>,
        tx_modifiable_flags: u8,
        input_count: usize,
        output_count: usize,
        xpubs: BTreeMap<Xpub, KeySource>,
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
        fn wrap_default_global_is_ok() {
            let g = Global::default();
            assert!(g.wrap().is_ok());
        }

        #[test]
        fn wrap_try_unwrap_roundtrip() {
            let g = Global::default();
            assert!(g.wrap().try_unwrap().is_ok());
        }

        #[test]
        fn join_identical_globals_is_ok() {
            let a = Global::default().wrap();
            let b = Global::default().wrap();
            assert!(a.join(b).is_ok());
        }

        #[test]
        fn join_different_versions_conflicts() {
            let mut a = Global::default();
            let mut b = Global::default();
            a.tx_version = transaction::Version::ONE;
            b.tx_version = transaction::Version::TWO;
            let joined = a.wrap().join(b.wrap());
            assert!(!joined.is_ok());
            assert!(joined.try_unwrap().is_err());
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        fn arb_proprietary_key() -> impl Strategy<Value = raw::ProprietaryKey> {
            (
                proptest::collection::vec(0u8..=255, 1..=8),
                any::<u8>(),
                proptest::collection::vec(0u8..=255, 0..=4),
            )
                .prop_map(|(prefix, subtype, key)| raw::ProprietaryKey {
                    prefix,
                    subtype,
                    key,
                })
        }

        fn arb_global() -> impl Strategy<Value = Global> {
            (
                proptest::bool::ANY,
                0u8..4,
                proptest::bool::ANY,
                0usize..4,
                0usize..4,
                proptest::collection::btree_map(
                    arb_proprietary_key(),
                    proptest::collection::vec(0u8..=255, 0..=8),
                    0..=2,
                ),
                proptest::collection::btree_map(
                    (any::<u8>(), proptest::collection::vec(0u8..=255, 0..=4))
                        .prop_map(|(type_value, key)| raw::Key { type_value, key }),
                    proptest::collection::vec(0u8..=255, 0..=8),
                    0..=2,
                ),
            )
                .prop_map(
                    |(
                        use_v1,
                        flags,
                        has_lock_time,
                        in_count,
                        out_count,
                        proprietaries,
                        unknowns,
                    )| {
                        Global {
                            tx_version: if use_v1 {
                                transaction::Version::ONE
                            } else {
                                transaction::Version::TWO
                            },
                            tx_modifiable_flags: flags,
                            fallback_lock_time: if has_lock_time {
                                Some(absolute::LockTime::from_consensus(500_000))
                            } else {
                                None
                            },
                            input_count: in_count,
                            output_count: out_count,
                            proprietaries,
                            unknowns,
                            ..Global::default()
                        }
                    },
                )
        }

        fn arb_result_global() -> impl Strategy<Value = ResultGlobal> {
            prop_oneof![
                arb_global().prop_map(|g| g.wrap()),
                (arb_global(), arb_global()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        proptest! {
            #[test]
            fn idempotent(a in arb_result_global()) {
                prop_assert_eq!(a.clone().join(a.clone()), a);
            }

            #[test]
            fn commutative(a in arb_result_global(), b in arb_result_global()) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn associative(a in arb_result_global(), b in arb_result_global(), c in arb_result_global()) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).join(c.clone()),
                    a.join(b.join(c)),
                );
            }

            #[test]
            fn wrap_try_unwrap_roundtrip(a in arb_global()) {
                let wrapped = a.wrap();
                let unwrapped = wrapped.clone().try_unwrap().expect("freshly wrapped");
                prop_assert_eq!(unwrapped.wrap(), wrapped);
            }

            #[test]
            fn is_ok_consistency(a in arb_result_global()) {
                if a.is_ok() {
                    prop_assert!(a.try_unwrap().is_ok());
                } else {
                    prop_assert!(a.try_unwrap().is_err());
                }
            }
        }
    }
}
