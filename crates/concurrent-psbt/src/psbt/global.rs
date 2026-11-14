#![allow(clippy::result_large_err)]
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "exported once UnorderedPsbt composes the PSBT types"
    )
)]

//! PSBT global fields and their merging.
//!
//! [`Global`] holds the global fields of a PSBT, and [`ResultGlobal`] wraps
//! each of them in the internal result domain, enabling field-by-field merging
//! that accumulates conflicts without short-circuiting.

use std::collections::BTreeMap;

use bitcoin::bip32::{KeySource, Xpub};
use bitcoin::locktime::absolute;
use bitcoin::transaction;

use psbt_v2::raw;

joinable_struct! {
    /// The global fields of a PSBT.
    ///
    /// Mirrors the BIP 370 global map except `PSBT_GLOBAL_VERSION`, which is
    /// always 2, and the input and output counts, which are filled in when
    /// converting to a BIP 370 PSBT.
    source: Global,
    /// Result-domain counterpart of [`Global`].
    ///
    /// Produced by joining [`Global`] values lifted with [`Global::wrap`].
    /// Use [`ResultGlobal::is_ok`] to check for conflicts and
    /// [`ResultGlobal::try_unwrap`] to extract.
    result: ResultGlobal,
    fields: {
        pub tx_version: transaction::Version,
        pub fallback_lock_time: Option<absolute::LockTime>,
        pub tx_modifiable_flags: u8,
        pub xpubs: BTreeMap<Xpub, KeySource>,
        pub proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        pub unknowns: BTreeMap<raw::Key, Vec<u8>>,
    }
}

impl Default for Global {
    fn default() -> Self {
        psbt_v2::v2::Global::default().into()
    }
}

/// Drops the version and the input and output counts.
impl From<psbt_v2::v2::Global> for Global {
    fn from(global: psbt_v2::v2::Global) -> Self {
        let psbt_v2::v2::Global {
            tx_version,
            fallback_lock_time,
            tx_modifiable_flags,
            xpubs,
            proprietaries,
            unknowns,
            ..
        } = global;
        Self {
            tx_version,
            fallback_lock_time,
            tx_modifiable_flags,
            xpubs,
            proprietaries,
            unknowns,
        }
    }
}

impl Global {
    /// Convert to a BIP 370 global map for a PSBT with `input_count` inputs
    /// and `output_count` outputs.
    #[expect(
        clippy::needless_update,
        reason = "psbt-v2's silent-payments feature adds fields this crate does not support yet"
    )]
    pub(crate) fn into_v2(self, input_count: usize, output_count: usize) -> psbt_v2::v2::Global {
        psbt_v2::v2::Global {
            version: psbt_v2::Version::TWO,
            tx_version: self.tx_version,
            fallback_lock_time: self.fallback_lock_time,
            tx_modifiable_flags: self.tx_modifiable_flags,
            input_count,
            output_count,
            xpubs: self.xpubs,
            proprietaries: self.proprietaries,
            unknowns: self.unknowns,
            ..psbt_v2::v2::Global::default()
        }
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
        fn join_different_versions_conflicts() {
            let mut a = Global::default();
            let mut b = Global::default();
            a.tx_version = transaction::Version::ONE;
            b.tx_version = transaction::Version::TWO;
            let joined = a.wrap().join(b.wrap());
            assert!(!joined.is_ok());
            assert!(joined.try_unwrap().is_err());
        }

        #[test]
        fn default_is_an_empty_v2_global() {
            assert_eq!(
                Global::default().into_v2(0, 0),
                psbt_v2::v2::Global::default()
            );
        }

        #[test]
        fn for_each_conflict_reports_the_conflicted_field() {
            let mut a = Global::default();
            let mut b = Global::default();
            a.tx_version = transaction::Version::ONE;
            b.tx_version = transaction::Version::TWO;
            let mut fields = Vec::new();
            a.wrap()
                .join(b.wrap())
                .for_each_conflict(|field, _, _| fields.push(field.to_owned()));
            assert_eq!(fields, ["tx_version"]);
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
                .prop_map(|(use_v1, flags, has_lock_time, proprietaries, unknowns)| {
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
                        proprietaries,
                        unknowns,
                        ..Global::default()
                    }
                })
        }

        fn arb_result_global() -> impl Strategy<Value = ResultGlobal> {
            prop_oneof![
                arb_global().prop_map(|g| g.wrap()),
                (arb_global(), arb_global()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        assert_join_laws!(arb_result_global());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip(a in arb_global()) {
                let wrapped = a.wrap();
                let unwrapped = wrapped.clone().try_unwrap().expect("freshly wrapped");
                prop_assert_eq!(unwrapped.wrap(), wrapped);
            }

            #[test]
            fn v2_roundtrip_derives_version_and_counts(
                a in arb_global(),
                input_count in 0usize..4,
                output_count in 0usize..4,
            ) {
                let v2 = a.clone().into_v2(input_count, output_count);
                prop_assert_eq!(v2.version, psbt_v2::Version::TWO);
                prop_assert_eq!((v2.input_count, v2.output_count), (input_count, output_count));
                prop_assert_eq!(Global::from(v2), a);
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
