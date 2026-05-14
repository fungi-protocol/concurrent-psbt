use crate::lattice::join::Join;
use crate::lattice::partial::{JoinResult, PartialJoin};

impl<V> Join for Option<V>
where
    V: Join,
{
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (None, None) => None,
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => Some(a.join(b)),
        }
    }
}

pub trait OptionExt {
    type Item: PartialJoin;

    fn into_ok(self) -> Option<JoinResult<Self::Item>>;
}

impl<V: PartialJoin> OptionExt for Option<V> {
    type Item = V;

    // TODO rename into_ok to lift
    fn into_ok(self) -> Option<JoinResult<V>> {
        self.map(|v| v.into_ok())
    }
}

pub trait ResultOptionExt {
    fn is_ok(&self) -> bool;
}

impl<V: PartialJoin> ResultOptionExt for Option<JoinResult<V>> {
    fn is_ok(&self) -> bool {
        match self {
            Some(Err(_)) => false,
            _ => true,
        }
    }
}

#[test]
fn test_join_option() {
    assert_eq!(Join::join(None::<()>, None), None);
    assert_eq!(Join::join(Some(()), None), Some(()));
    assert_eq!(Join::join(None, Some(())), Some(()));
    assert_eq!(Join::join(Some(()), Some(())), Some(()));

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Foo(u8);

    impl PartialJoin for Foo {
        type Error = ();

        fn try_join(self, other: Self) -> JoinResult<Self> {
            if self == other {
                Ok(self)
            } else {
                Err(())
            }
        }
    }

    assert_eq!(None::<Foo>.into_ok(), None);

    let a = Some(Foo(0u8));

    assert_eq!(a.into_ok(), Some(Ok(Foo(0u8))));
    assert_eq!(a.into_ok().transpose(), Ok(a));
    assert_eq!(a.into_ok().join(None), a.into_ok());
    assert_eq!(a.into_ok().join(a.into_ok()), a.into_ok());
    assert_eq!(None::<Foo>.into_ok().join(a.into_ok()), a.into_ok());

    let b = Some(Foo(1u8));
    assert_eq!(a.into_ok().join(b.into_ok()), Some(Err(())));
    assert_eq!(a.into_ok().join(b.into_ok()).transpose(), Err(()));
}

// let a = 1u8;

// assert_eq!(PartialJoin::try_join(None::<u8>, None), Ok(None));
// assert_eq!(PartialJoin::try_join(Some(a), None), Ok(Some(a)));
// assert_eq!(PartialJoin::try_join(None, Some(a)), Ok(Some(a)));
// assert_eq!(PartialJoin::try_join(Some(a), Some(a)), Ok(Some(a)));

// let b = 2u8;

// assert_eq!(
//     PartialJoin::try_join(Some(a), Some(b)),
//     Err(crate::values::ConflictingValues([1u8, 2].into()))
// );

// let ab = PartialJoin::try_join(Some(b), Some(a));
// let bb = PartialJoin::try_join(Some(b), Some(b));

// let conflict = crate::values::ConflictingValues([a, b].into());

// assert_eq!(ab, Err(conflict.clone()));
// assert_eq!(bb, Ok(Some(b)));
// assert_eq!(Join::join(ab.clone(), bb.clone()), Err(conflict),);
