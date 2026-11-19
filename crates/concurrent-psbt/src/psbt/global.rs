#![allow(clippy::result_large_err)]
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "global serialization is used by UnorderedPsbt")
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
    result: ResultGlobal,
    v2: psbt_v2::v2::Global,
    fields: {
        tx_version: transaction::Version,
        fallback_lock_time: Option<absolute::LockTime>,
        tx_modifiable_flags: u8,
        xpubs: BTreeMap<Xpub, KeySource>,
        proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        unknowns: BTreeMap<raw::Key, Vec<u8>>,
    },
    proprietary: {},
}

impl Default for Global {
    fn default() -> Self {
        Self::from_v2_fields(psbt_v2::v2::Global::default())
    }
}

/// Drops the version and the input and output counts.
impl From<psbt_v2::v2::Global> for Global {
    fn from(global: psbt_v2::v2::Global) -> Self {
        let Ok(global) = Self::decode_v2::<std::convert::Infallible>(global);
        global
    }
}

impl Global {
    /// Convert to a BIP 370 global map for a PSBT with `input_count` inputs
    /// and `output_count` outputs.
    pub(crate) fn into_v2(self, input_count: usize, output_count: usize) -> psbt_v2::v2::Global {
        self.encode_v2(psbt_v2::v2::Global {
            input_count,
            output_count,
            ..psbt_v2::v2::Global::default()
        })
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
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use bitcoin::Network;
        use bitcoin::bip32::{DerivationPath, Xpriv};
        use bitcoin::secp256k1::Secp256k1;
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

        /// The master xpub of `seed`, with its own fingerprint as key source.
        fn xpub_from_seed(seed: u8) -> (Xpub, KeySource) {
            let xpriv = Xpriv::new_master(Network::Bitcoin, &[seed; 16]).expect("valid seed");
            let xpub = Xpub::from_priv(&Secp256k1::new(), &xpriv);
            (xpub, (xpub.fingerprint(), DerivationPath::master()))
        }

        fn arb_global() -> impl Strategy<Value = Global> {
            (
                proptest::bool::ANY,
                0u8..4,
                proptest::bool::ANY,
                proptest::option::of(0u8..2),
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
                    |(use_v1, flags, has_lock_time, xpub, proprietaries, unknowns)| Global {
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
                        xpubs: xpub.map(xpub_from_seed).into_iter().collect(),
                        proprietaries,
                        unknowns,
                    },
                )
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
