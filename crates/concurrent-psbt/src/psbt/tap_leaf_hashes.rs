//! The leaf hashes of a BIP 371 taproot key origin.

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::TapLeafHash;
use bitcoin::bip32::KeySource;
use bitcoin::secp256k1::XOnlyPublicKey;

use crate::values::IdempotentValue;

/// The leaf hashes of a taproot key in `PSBT_{IN,OUT}_TAP_BIP32_DERIVATION`.
///
/// Two values are equal when they hold the same hashes, in any order and with
/// any repetition. The hashes keep the order they were given in, and joining
/// equal values keeps the left one.
#[derive(Debug, Clone)]
pub struct TapLeafHashes(Vec<TapLeafHash>);

impl TapLeafHashes {
    /// The leaf hashes, in the order they were given in.
    pub fn as_slice(&self) -> &[TapLeafHash] {
        &self.0
    }

    fn set(&self) -> BTreeSet<&TapLeafHash> {
        self.0.iter().collect()
    }
}

impl PartialEq for TapLeafHashes {
    fn eq(&self, other: &Self) -> bool {
        self.set() == other.set()
    }
}

impl Eq for TapLeafHashes {}

// Every value is built here, so sorting and deduplicating `leaves` here would
// make equal values structurally equal.
impl From<Vec<TapLeafHash>> for TapLeafHashes {
    fn from(leaves: Vec<TapLeafHash>) -> Self {
        Self(leaves)
    }
}

impl From<TapLeafHashes> for Vec<TapLeafHash> {
    fn from(leaves: TapLeafHashes) -> Self {
        leaves.0
    }
}

impl IdempotentValue for (TapLeafHashes, KeySource) {}

/// `PSBT_{IN,OUT}_TAP_BIP32_DERIVATION` as psbt-v2 stores it.
type V2TapKeyOrigins = BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, KeySource)>;

/// `PSBT_{IN,OUT}_TAP_BIP32_DERIVATION` with set equality on the leaf hashes.
type TapKeyOrigins = BTreeMap<XOnlyPublicKey, (TapLeafHashes, KeySource)>;

pub(crate) fn tap_key_origins_from_v2(origins: V2TapKeyOrigins) -> TapKeyOrigins {
    origins
        .into_iter()
        .map(|(key, (leaves, source))| (key, (leaves.into(), source)))
        .collect()
}

pub(crate) fn tap_key_origins_into_v2(origins: TapKeyOrigins) -> V2TapKeyOrigins {
    origins
        .into_iter()
        .map(|(key, (leaves, source))| (key, (leaves.into(), source)))
        .collect()
}

#[cfg(all(test, feature = "prop-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use bitcoin::bip32::{DerivationPath, Fingerprint};
    use bitcoin::hashes::Hash;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use proptest::prelude::*;

    use super::*;
    use crate::lattice::join::Join;
    use crate::lattice::partial::{JoinResult, PartialJoin};

    fn arb_leaves() -> impl Strategy<Value = Vec<TapLeafHash>> {
        proptest::collection::vec(0u8..3, 0..4).prop_map(|bytes| {
            bytes
                .into_iter()
                .map(|byte| TapLeafHash::from_byte_array([byte; 32]))
                .collect()
        })
    }

    /// `leaves` in reverse, with each hash repeated.
    fn shuffled(leaves: &[TapLeafHash]) -> Vec<TapLeafHash> {
        leaves.iter().rev().flat_map(|&leaf| [leaf, leaf]).collect()
    }

    fn arb_origin() -> impl Strategy<Value = (TapLeafHashes, KeySource)> {
        (arb_leaves(), 0u8..2).prop_map(|(leaves, source)| {
            let source = (Fingerprint::from([source; 4]), DerivationPath::master());
            (leaves.into(), source)
        })
    }

    fn arb_result_origin() -> impl Strategy<Value = JoinResult<(TapLeafHashes, KeySource)>> {
        prop_oneof![
            arb_origin().prop_map(Ok),
            (arb_origin(), arb_origin()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
        ]
    }

    fn arb_v2_tap_key_origins() -> impl Strategy<Value = V2TapKeyOrigins> {
        let origin = arb_origin().prop_map(|(leaves, source)| (leaves.into(), source));
        proptest::collection::btree_map(1u8..4, origin, 0..3).prop_map(|origins| {
            origins
                .into_iter()
                .map(|(seed, origin)| {
                    let secret_key = SecretKey::from_slice(&[seed; 32]).expect("nonzero seed");
                    (secret_key.x_only_public_key(&Secp256k1::new()).0, origin)
                })
                .collect()
        })
    }

    assert_partial_join_laws!(arb_origin(), arb_result_origin());

    proptest! {
        #[test]
        fn reordered_and_repeated_hashes_are_equal(a in arb_leaves()) {
            prop_assert_eq!(TapLeafHashes::from(shuffled(&a)), TapLeafHashes::from(a));
        }

        #[test]
        fn an_extra_hash_is_unequal(a in arb_leaves(), extra in 3u8..6) {
            let mut b = a.clone();
            b.push(TapLeafHash::from_byte_array([extra; 32]));
            prop_assert_ne!(TapLeafHashes::from(a), TapLeafHashes::from(b));
        }

        #[test]
        fn join_keeps_the_left_list(a in arb_leaves()) {
            let source = (Fingerprint::default(), DerivationPath::master());
            let left = (TapLeafHashes::from(a.clone()), source.clone());
            let right = (TapLeafHashes::from(shuffled(&a)), source);
            let (joined, _) = left.try_join(right).expect("equal values join");
            prop_assert_eq!(joined.as_slice(), a.as_slice());
        }

        #[test]
        fn v2_round_trip_keeps_the_lists(origins in arb_v2_tap_key_origins()) {
            prop_assert_eq!(tap_key_origins_into_v2(tap_key_origins_from_v2(origins.clone())), origins);
        }
    }
}
