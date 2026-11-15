//! The leaf hashes of a BIP 371 taproot key origin.

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::TapLeafHash;
use bitcoin::bip32::KeySource;
use bitcoin::secp256k1::XOnlyPublicKey;

use super::joinable_struct_macro::MirroredField;
use crate::canonical::Canonicalizable;
use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

/// The leaf hashes of a taproot key in `PSBT_{IN,OUT}_TAP_BIP32_DERIVATION`.
///
/// Two values are equal when they hold the same hashes, in any order and with
/// any repetition. A [`List`](Self::List) keeps the hashes as they were
/// given; a [`Set`](Self::Set) is the canonical form.
#[derive(Debug, Clone)]
pub enum TapLeafHashes {
    /// The hashes in the order, and with the repetitions, they were given in.
    List(Vec<TapLeafHash>),
    /// The distinct hashes, in their canonical order.
    Set(BTreeSet<TapLeafHash>),
}

impl TapLeafHashes {
    fn set(&self) -> BTreeSet<&TapLeafHash> {
        match self {
            Self::List(leaves) => leaves.iter().collect(),
            Self::Set(leaves) => leaves.iter().collect(),
        }
    }

    /// Whether `self` and `other` are represented the same way, not just
    /// equal.
    fn is_identical(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Set(a), Self::Set(b)) => a == b,
            _ => false,
        }
    }
}

impl PartialEq for TapLeafHashes {
    fn eq(&self, other: &Self) -> bool {
        self.set() == other.set()
    }
}

impl Eq for TapLeafHashes {}

impl Canonicalizable for TapLeafHashes {
    fn canonicalize(&mut self) {
        if let Self::List(leaves) = self {
            *self = Self::Set(std::mem::take(leaves).into_iter().collect());
        }
    }
}

impl From<Vec<TapLeafHash>> for TapLeafHashes {
    fn from(leaves: Vec<TapLeafHash>) -> Self {
        Self::List(leaves)
    }
}

/// A [`Set`](TapLeafHashes::Set) gives its hashes in their canonical order.
impl From<TapLeafHashes> for Vec<TapLeafHash> {
    fn from(leaves: TapLeafHashes) -> Self {
        match leaves {
            TapLeafHashes::List(leaves) => leaves,
            TapLeafHashes::Set(leaves) => leaves.into_iter().collect(),
        }
    }
}

/// Equal origins join as the left one if both are represented the same way,
/// and in canonical form otherwise, so that the join does not depend on the
/// order of its operands.
impl PartialJoin for (TapLeafHashes, KeySource) {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        if self != other {
            return Err(Conflict::from_values([self, other]));
        }
        let (mut leaves, source) = self;
        if !leaves.is_identical(&other.0) {
            leaves.canonicalize();
        }
        Ok((leaves, source))
    }
}

/// `PSBT_{IN,OUT}_TAP_BIP32_DERIVATION` as psbt-v2 stores it.
type V2TapKeyOrigins = BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, KeySource)>;

/// Converts `tap_key_origins` between psbt-v2's leaf hash lists and
/// [`TapLeafHashes`], which it canonicalizes as sets.
pub(crate) enum TapKeyOrigins {}

impl MirroredField for TapKeyOrigins {
    type V2 = V2TapKeyOrigins;
    type Value = BTreeMap<XOnlyPublicKey, (TapLeafHashes, KeySource)>;

    fn from_v2(origins: Self::V2) -> Self::Value {
        origins
            .into_iter()
            .map(|(key, (leaves, source))| (key, (leaves.into(), source)))
            .collect()
    }

    fn canonicalize(origins: &mut Self::Value) {
        for (leaves, _) in origins.values_mut() {
            leaves.canonicalize();
        }
    }

    fn into_v2(origins: Self::Value) -> Self::V2 {
        origins
            .into_iter()
            .map(|(key, (leaves, source))| (key, (leaves.into(), source)))
            .collect()
    }
}

#[cfg(all(test, feature = "prop-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(super) mod tests {
    use bitcoin::bip32::{DerivationPath, Fingerprint};
    use bitcoin::hashes::Hash;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use proptest::prelude::*;

    use super::*;
    use crate::Lift;
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

    fn arb_v2_origin() -> impl Strategy<Value = (Vec<TapLeafHash>, KeySource)> {
        (arb_leaves(), 0u8..2).prop_map(|(leaves, source)| {
            let source = (Fingerprint::from([source; 4]), DerivationPath::master());
            (leaves, source)
        })
    }

    /// An origin whose leaf hashes are a list or a set.
    fn arb_origin() -> impl Strategy<Value = (TapLeafHashes, KeySource)> {
        (arb_v2_origin(), any::<bool>()).prop_map(|((leaves, source), canonical)| {
            let mut leaves = TapLeafHashes::from(leaves);
            if canonical {
                leaves.canonicalize();
            }
            (leaves, source)
        })
    }

    /// Whether every key's leaf hashes are in canonical form.
    pub(crate) fn all_canonical(
        origins: &BTreeMap<XOnlyPublicKey, (TapLeafHashes, KeySource)>,
    ) -> bool {
        origins
            .values()
            .all(|(leaves, _)| matches!(leaves, TapLeafHashes::Set(_)))
    }

    fn canonicalized(mut leaves: TapLeafHashes) -> TapLeafHashes {
        leaves.canonicalize();
        leaves
    }

    fn arb_result_origin() -> impl Strategy<Value = JoinResult<(TapLeafHashes, KeySource)>> {
        prop_oneof![
            arb_origin().prop_map(Ok),
            (arb_origin(), arb_origin()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
        ]
    }

    pub(crate) fn arb_v2_tap_key_origins() -> impl Strategy<Value = V2TapKeyOrigins> {
        proptest::collection::btree_map(1u8..4, arb_v2_origin(), 0..3).prop_map(|origins| {
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
        fn canonicalize_makes_an_equal_set(a in arb_leaves()) {
            let canonical = canonicalized(TapLeafHashes::from(a.clone()));
            prop_assert_eq!(&canonical, &TapLeafHashes::from(a.clone()));
            prop_assert!(matches!(canonical, TapLeafHashes::Set(_)));
            prop_assert!(canonicalized(canonical.clone()).is_identical(&canonical));
        }

        #[test]
        fn join_does_not_depend_on_the_order(a in arb_origin(), b in arb_origin()) {
            if let (Ok(ab), Ok(ba)) = (a.clone().try_join(b.clone()), b.try_join(a)) {
                prop_assert!(ab.0.is_identical(&ba.0));
            }
        }

        #[test]
        fn join_keeps_identical_lists(a in arb_leaves()) {
            let source = (Fingerprint::default(), DerivationPath::master());
            let left = (TapLeafHashes::from(a.clone()), source.clone());
            let right = (TapLeafHashes::from(a.clone()), source);
            let (joined, _) = left.try_join(right).expect("equal values join");
            prop_assert!(joined.is_identical(&TapLeafHashes::from(a)));
        }

        #[test]
        fn join_canonicalizes_differing_lists(a in arb_leaves()) {
            prop_assume!(!a.is_empty());
            let source = (Fingerprint::default(), DerivationPath::master());
            let left = (TapLeafHashes::from(a.clone()), source.clone());
            let right = (TapLeafHashes::from(shuffled(&a)), source);
            let (joined, _) = left.try_join(right).expect("equal values join");
            prop_assert!(joined.is_identical(&canonicalized(TapLeafHashes::from(a))));
        }

        #[test]
        fn v2_round_trip_keeps_the_lists(origins in arb_v2_tap_key_origins()) {
            prop_assert_eq!(TapKeyOrigins::into_v2(TapKeyOrigins::from_v2(origins.clone())), origins);
        }

        #[test]
        fn canonical_origins_have_sorted_distinct_lists(origins in arb_v2_tap_key_origins()) {
            let mut canonical = TapKeyOrigins::from_v2(origins.clone());
            TapKeyOrigins::canonicalize(&mut canonical);
            let expected: V2TapKeyOrigins = origins
                .into_iter()
                .map(|(key, (mut leaves, source))| {
                    leaves.sort();
                    leaves.dedup();
                    (key, (leaves, source))
                })
                .collect();
            prop_assert_eq!(TapKeyOrigins::into_v2(canonical), expected);
        }
    }
}
