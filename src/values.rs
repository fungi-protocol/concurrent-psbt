use std::collections::HashSet;
use std::hash::Hash;

use crate::lattice::join::Join;
use crate::lattice::partial::{Absorb, JoinResult, PartialJoin};

// FIXME replace with Error in partial.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictingValues<T: Hash + Eq>(pub(crate) HashSet<T>); // FIXME HashSet? BTreeSet? EqSet? semantically an unordered multiset

impl<T: Hash + Eq> From<(T, T)> for ConflictingValues<T> {
    fn from((a, b): (T, T)) -> Self {
        Self([a, b].into())
    }
}

impl<T: Hash + Eq + Clone> Join for ConflictingValues<T> {
    fn join(self, other: Self) -> Self {
        Self(self.0.union(&other.0).cloned().collect())
    }
}

impl<T: IdempotentValue> Absorb<T> for ConflictingValues<T> {
    fn absorb(self, other: T) -> Self {
        // TODO simplify, exploit mutability of self now that self is consumed
        Self(self.0.into_iter().chain(Some(other)).into_iter().collect())
    }
}

// struct IdempotentValueLattice<T : Clone + PartialEq>(T) + impl<T> PartialJoin for IdempotentValue<T>?
// or just blanket impl for PartialEq + Clone?
//
// QuotientLattice -> pick arbitrarily? not partial

/// A marker trait for value types that should have a trivial join based on equality.
pub trait IdempotentValue: PartialJoin + PartialEq + Eq + Hash {}

// Blanket impl conflicts with one based on T: Join, so replaced with macro
//
// impl<T> PartialJoin for T
// where
//     T: IdempotentValue,
// {
//     type Error = ConflictingValues<T>;

//     /// A join that succeeds when the two values are identical and fails otherwise
//     fn join(&self, other: &Self) -> JoinResult<Self> {
//         if self == other {
//             Ok(self.clone())
//         } else {
//             Err(Self::Error::from((self.clone(), other.clone())))
//         }
//     }
// }

macro_rules! impl_idempotent_value_for {
    ($($ty:ty),* $(,)?) => {
        $(
            impl PartialJoin for $ty {
                type Error = ConflictingValues<$ty>;

                /// A join that succeeds when the two values are identical and fails otherwise
                fn try_join(self, other: Self) -> JoinResult<Self> {
                    if self == other {
                        Ok(self)
                    } else {
                        Err(Self::Error::from((self, other)))
                    }
                }
            }

            impl IdempotentValue for $ty {}
        )*
    };
}

impl_idempotent_value_for!(u32);
impl_idempotent_value_for!(u8);
impl_idempotent_value_for!(usize);

impl_idempotent_value_for!(Vec<u8>);

// FIXME this is not able to proceed via generic tuple collection because the
// error types are different
impl_idempotent_value_for!((Vec<bitcoin::TapLeafHash>, bitcoin::bip32::KeySource));
impl_idempotent_value_for!((bitcoin::ScriptBuf, bitcoin::taproot::LeafVersion));

impl_idempotent_value_for!(bitcoin::absolute::Height);
impl_idempotent_value_for!(bitcoin::absolute::Time);
impl_idempotent_value_for!(bitcoin::Amount);
impl_idempotent_value_for!(bitcoin::bip32::KeySource);
impl_idempotent_value_for!(bitcoin::ecdsa::Signature);
impl_idempotent_value_for!(bitcoin::locktime::absolute::LockTime);
impl_idempotent_value_for!(bitcoin::OutPoint);
impl_idempotent_value_for!(bitcoin::ScriptBuf);
impl_idempotent_value_for!(bitcoin::secp256k1::XOnlyPublicKey);
impl_idempotent_value_for!(bitcoin::Sequence);
impl_idempotent_value_for!(bitcoin::TapLeafHash);
impl_idempotent_value_for!(bitcoin::TapNodeHash);
impl_idempotent_value_for!(bitcoin::taproot::LeafVersion);
impl_idempotent_value_for!(bitcoin::taproot::Signature);
impl_idempotent_value_for!(bitcoin::taproot::TapTree);
impl_idempotent_value_for!(bitcoin::Transaction);
impl_idempotent_value_for!(bitcoin::transaction::Version);
impl_idempotent_value_for!(bitcoin::Txid);
impl_idempotent_value_for!(bitcoin::TxOut);
impl_idempotent_value_for!(bitcoin::Witness);

impl_idempotent_value_for!(psbt_v2::PsbtSighashType);
impl_idempotent_value_for!(psbt_v2::Version);

// impl_idempotent_value_for!(Vec<u8>);
// impl_idempotent_value_for!(Vec<bitcoin::TapLeafHash>);

#[test]
fn test_idempotent_value_join() {
    let a = 3u8;
    let b = 0u8;
    let c = 42u8;

    let ab = ConflictingValues([a, b].into());
    let abc = ConflictingValues([a, b, c].into());

    assert_eq!(PartialJoin::try_join(a, a), Ok(a));

    assert_eq!(PartialJoin::try_join(a, b), Err(ab.clone()));

    assert_eq!(PartialJoin::try_join(b, a), Err(ab.clone()));

    assert_eq!(Join::join(Ok(a), Ok(b)), Err(ab.clone()));

    assert_eq!(Join::join(Ok(b), Ok(a)), Err(ab.clone()));

    assert_eq!(Join::join(Join::join(Ok(a), Ok(b)), Ok(b)), Err(ab.clone()));

    assert_eq!(Join::join(ab.clone(), abc.clone()), abc.clone());
    assert_eq!(
        Join::join(Err::<u8, _>(ab.clone()), Err(abc.clone())),
        Err(abc.clone())
    );

    assert_eq!(
        Join::join(PartialJoin::try_join(a, b), Ok(c)),
        Err(abc.clone())
    );

    assert_eq!(
        Join::join(PartialJoin::try_join(a, b), PartialJoin::try_join(b, c)),
        Err(abc.clone())
    );
}
