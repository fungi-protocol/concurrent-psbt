//! [`IdempotentValue`](crate::values::IdempotentValue) implementations for
//! bitcoin and psbt-v2 types used as PSBT fields.

use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};
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

/// BIP 371 taproot BIP 32 derivation values: `(Vec<TapLeafHash>, KeySource)`.
///
/// The leaf-hash list is the set of tap leaves the key participates in; BIP 371
/// imposes no ordering on it, so join equality is order-insensitive: two values
/// join iff the key sources match and the leaf lists are equal as multisets.
///
/// Both leaf lists are sorted before comparing, and the sorted form is what
/// the join returns, on `Ok` and inside a conflict alike. The result therefore
/// does not depend on which side's ordering was seen first, which is what
/// makes the join commutative and associative under structural equality.
impl PartialJoin for (Vec<bitcoin::TapLeafHash>, bitcoin::bip32::KeySource) {
    fn try_join(mut self, mut other: Self) -> JoinResult<Self> {
        self.0.sort_unstable();
        other.0.sort_unstable();

        if self == other {
            Ok(self)
        } else {
            Err(Conflict::from_values([self, other]))
        }
    }
}

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
        fn reordered_tap_leaf_hashes_join_to_sorted_order() {
            let sorted = (vec![leaf(1), leaf(2)], key_source());
            let reversed = (vec![leaf(2), leaf(1)], key_source());
            assert_eq!(
                reversed.clone().try_join(reversed.clone()),
                Ok(sorted.clone())
            );
            assert_eq!(reversed.try_join(sorted.clone()), Ok(sorted));
        }

        #[test]
        fn different_tap_leaf_hashes_conflict() {
            let a = (vec![leaf(2), leaf(1)], key_source());
            let b = (vec![leaf(3)], key_source());
            let sorted_a = (vec![leaf(1), leaf(2)], key_source());
            assert_eq!(
                a.try_join(b.clone()),
                Err(Conflict::from_values([sorted_a, b]))
            );
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use bitcoin::TapLeafHash;
        use bitcoin::bip32::{DerivationPath, Fingerprint, KeySource};
        use bitcoin::hashes::Hash;
        use proptest::prelude::*;

        use crate::lattice::join::Join;
        use crate::lattice::partial::{JoinResult, PartialJoin};

        type TapDerivation = (Vec<TapLeafHash>, KeySource);

        fn key_source(byte: u8) -> KeySource {
            (Fingerprint::from([byte; 4]), DerivationPath::master())
        }

        fn leaves(bytes: &[u8]) -> Vec<TapLeafHash> {
            bytes
                .iter()
                .map(|byte| TapLeafHash::from_byte_array([*byte; 32]))
                .collect()
        }

        // Leaf lists in sorted order, the form every join result takes.
        fn arb_tap_derivation() -> impl Strategy<Value = TapDerivation> {
            (proptest::collection::vec(0u8..4, 0..4), 0u8..2).prop_map(|(mut bytes, source)| {
                bytes.sort_unstable();
                (leaves(&bytes), key_source(source))
            })
        }

        fn arb_result_tap_derivation() -> impl Strategy<Value = JoinResult<TapDerivation>> {
            prop_oneof![
                arb_tap_derivation().prop_map(Ok),
                (arb_tap_derivation(), arb_tap_derivation())
                    .prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        assert_partial_join_laws!(arb_tap_derivation(), arb_result_tap_derivation());

        proptest! {
            #[test]
            fn leaf_order_does_not_affect_the_join(
                bytes in proptest::collection::vec(0u8..4, 0..4),
                other in arb_tap_derivation(),
            ) {
                let forward = (leaves(&bytes), key_source(0));
                let mut backward = forward.clone();
                backward.0.reverse();
                prop_assert_eq!(forward.try_join(other.clone()), backward.try_join(other));
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
