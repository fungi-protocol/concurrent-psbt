use std::hash::Hash;

use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

/// A marker trait for value types that should have a trivial join based on equality.
pub trait IdempotentValue: PartialEq + Eq + Hash {}

impl<T> PartialJoin for T
where
    T: IdempotentValue,
{
    fn try_join(self, other: Self) -> JoinResult<Self> {
        if self == other {
            Ok(self)
        } else {
            Err(Conflict::from((self, other)))
        }
    }
}

impl IdempotentValue for u32 {}
impl IdempotentValue for u8 {}
impl IdempotentValue for usize {}

impl IdempotentValue for Vec<u8> {}

impl IdempotentValue for (Vec<bitcoin::TapLeafHash>, bitcoin::bip32::KeySource) {}
impl IdempotentValue for (bitcoin::ScriptBuf, bitcoin::taproot::LeafVersion) {}

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

#[test]
fn test_idempotent_value_join() {
    use crate::lattice::join::Join;
    let a = 3u8;
    let b = 0u8;
    let c = 42u8;

    let ab = Conflict(vec![a, b]);
    let ba = Conflict(vec![b, a]); // order is preserved for provenance
    let abc = Conflict(vec![a, b, c]);

    assert_eq!(PartialJoin::try_join(a, a), Ok(a));

    assert_eq!(PartialJoin::try_join(a, b), Err(ab.clone()));

    assert_eq!(PartialJoin::try_join(b, a), Err(ba.clone()));

    assert_eq!(Join::join(Ok(a), Ok(b)), Err(ab.clone()));

    assert_eq!(Join::join(Ok(b), Ok(a)), Err(ba.clone()));

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
