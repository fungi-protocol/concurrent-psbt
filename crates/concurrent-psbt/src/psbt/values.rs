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
/// `self`'s representation (its leaf order) is kept on `Ok`.
impl PartialJoin for (Vec<bitcoin::TapLeafHash>, bitcoin::bip32::KeySource) {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        fn sorted(leaves: &[bitcoin::TapLeafHash]) -> Vec<bitcoin::TapLeafHash> {
            let mut copy = leaves.to_vec();
            copy.sort_unstable();
            copy
        }

        if self.1 == other.1 && sorted(&self.0) == sorted(&other.0) {
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
        fn reordered_tap_leaf_hashes_join_ok() {
            let a = (vec![leaf(1), leaf(2)], key_source());
            let b = (vec![leaf(2), leaf(1)], key_source());
            // Same leaves as a multiset: joins Ok, keeping self's representation.
            assert_eq!(a.clone().try_join(b), Ok(a));
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
}
