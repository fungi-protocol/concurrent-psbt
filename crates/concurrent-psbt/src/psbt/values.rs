//! [`IdempotentValue`](crate::values::IdempotentValue) implementations for
//! bitcoin and psbt-v2 types used as PSBT fields.

use crate::values::IdempotentValue;

impl IdempotentValue for bitcoin::absolute::Height {}
impl IdempotentValue for bitcoin::absolute::Time {}
impl IdempotentValue for bitcoin::Amount {}
impl IdempotentValue for bitcoin::bip32::KeySource {}
impl IdempotentValue for bitcoin::ecdsa::Signature {}
impl IdempotentValue for bitcoin::locktime::absolute::LockTime {}
impl IdempotentValue for bitcoin::OutPoint {}
impl IdempotentValue for bitcoin::ScriptBuf {}
impl IdempotentValue for bitcoin::secp256k1::XOnlyPublicKey {}
impl IdempotentValue for bitcoin::Sequence {}
impl IdempotentValue for bitcoin::TapLeafHash {}
impl IdempotentValue for bitcoin::TapNodeHash {}
impl IdempotentValue for bitcoin::taproot::LeafVersion {}
impl IdempotentValue for bitcoin::taproot::Signature {}
impl IdempotentValue for bitcoin::taproot::TapTree {}
impl IdempotentValue for bitcoin::Transaction {}
impl IdempotentValue for bitcoin::transaction::Version {}
impl IdempotentValue for bitcoin::Txid {}
impl IdempotentValue for bitcoin::TxOut {}
impl IdempotentValue for bitcoin::Witness {}

impl IdempotentValue for psbt_v2::PsbtSighashType {}
impl IdempotentValue for psbt_v2::Version {}

impl IdempotentValue for (bitcoin::ScriptBuf, bitcoin::taproot::LeafVersion) {}

impl IdempotentValue for (Vec<bitcoin::TapLeafHash>, bitcoin::bip32::KeySource) {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    #[cfg(feature = "unit-tests")]
    mod unit {
        use bitcoin::TapLeafHash;
        use bitcoin::bip32::{DerivationPath, Fingerprint, KeySource};
        use bitcoin::hashes::Hash;

        use crate::lattice::partial::{Conflict, PartialJoin};

        fn key_source() -> KeySource {
            (Fingerprint::from([0u8; 4]), DerivationPath::master())
        }

        fn leaf(byte: u8) -> TapLeafHash {
            TapLeafHash::from_byte_array([byte; 32])
        }

        #[test]
        fn reordered_tap_leaf_hashes_conflict() {
            let a = (vec![leaf(1), leaf(2)], key_source());
            let b = (vec![leaf(2), leaf(1)], key_source());
            assert_eq!(
                a.clone().try_join(b.clone()),
                Err(Conflict::from_values([a, b]))
            );
        }

        #[test]
        fn different_tap_leaf_hashes_conflict() {
            let a = (vec![leaf(1)], key_source());
            let b = (vec![leaf(2)], key_source());
            assert_eq!(
                a.clone().try_join(b.clone()),
                Err(Conflict::from_values([a, b]))
            );
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use bitcoin::TapLeafHash;
        use bitcoin::bip32::{DerivationPath, Fingerprint, KeySource};
        use bitcoin::hashes::Hash;
        use proptest::prelude::*;

        use crate::lattice::partial::{Conflict, PartialJoin};

        fn key_source(byte: u8) -> KeySource {
            (Fingerprint::from([byte; 4]), DerivationPath::master())
        }

        fn leaves(bytes: &[u8]) -> Vec<TapLeafHash> {
            bytes
                .iter()
                .map(|byte| TapLeafHash::from_byte_array([*byte; 32]))
                .collect()
        }

        proptest! {
            #[test]
            fn tap_key_origins_use_structural_equality(
                a in proptest::collection::vec(0u8..3, 0..5),
                b in proptest::collection::vec(0u8..3, 0..5),
                source_a in 0u8..2,
                source_b in 0u8..2,
            ) {
                let a = (leaves(&a), key_source(source_a));
                let b = (leaves(&b), key_source(source_b));
                prop_assert_eq!(a.clone().try_join(a.clone()), Ok(a.clone()));
                let expected = if a == b {
                    Ok(a.clone())
                } else {
                    Err(Conflict::from_values([a.clone(), b.clone()]))
                };
                prop_assert_eq!(a.try_join(b), expected);
            }

            #[test]
            fn repeated_tap_leaf_hashes_conflict(bytes in proptest::collection::vec(any::<u8>(), 1..8)) {
                let a = (leaves(&bytes), key_source(0));
                let mut b = a.clone();
                b.0.push(b.0[0]);
                prop_assert_eq!(a.clone().try_join(b.clone()), Err(Conflict::from_values([a, b])));
            }

            #[test]
            fn different_key_sources_conflict(bytes in proptest::collection::vec(any::<u8>(), 0..8)) {
                let a = (leaves(&bytes), key_source(0));
                let b = (leaves(&bytes), key_source(1));
                prop_assert!(a.try_join(b).is_err());
            }
        }
    }
}
