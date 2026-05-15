use std::collections::BTreeMap;

use crate::lattice::join::{Join, JoinMut};
use crate::lattice::partial::{JoinResult, PartialJoin};

// TODO Deref newtype instead of extension traits?

pub trait BTreeMapExt {
    type Key;
    type Value: PartialJoin;
    fn wrap(self) -> BTreeMap<Self::Key, JoinResult<Self::Value>>;
}

impl<K, V> BTreeMapExt for BTreeMap<K, V>
where
    K: Ord,
    V: PartialJoin,
{
    type Key = K;
    type Value = V;

    fn wrap(self) -> BTreeMap<K, JoinResult<V>> {
        self.into_iter().map(|(k, v)| (k, v.wrap())).collect()
    }
}

pub trait ResultContainer: Sized {
    type Key;
    type Value: PartialJoin;

    fn try_unwrap(self) -> Result<BTreeMap<Self::Key, Self::Value>, Self>;

    fn is_ok(&self) -> bool;
}

impl<K, V> ResultContainer for BTreeMap<K, JoinResult<V>>
where
    K: Ord,
    V: PartialJoin,
{
    type Key = K;
    type Value = V;

    fn is_ok(&self) -> bool {
        self.values().all(|v| v.is_ok())
    }

    fn try_unwrap(self) -> Result<BTreeMap<Self::Key, Self::Value>, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        self.into_iter()
            .map(|(k, v)| v.map(|v| (k, v)))
            .collect::<Result<_, _>>()
            .map_err(|_| panic!("all entries verified to be Ok"))
    }
}

impl<K, V> JoinMut for BTreeMap<K, V>
where
    K: Ord,
    V: Join,
{
    fn join_mut(&mut self, other: Self) {
        for (k, v) in other.into_iter() {
            let lub = match self.remove(&k) {
                Some(prev) => prev.join(v),
                None => v,
            };

            self.insert(k, lub);
        }
    }
}

// TODO prop testing and full coverage
#[test]
fn test_btree() {
    use crate::lattice::partial::Conflict;

    let a: BTreeMap<u8, ()> = [(0, ())].into();
    let b: BTreeMap<u8, ()> = [(1, ())].into();

    assert_eq!(a.clone().join(a.clone()), a.clone());
    assert_eq!(a.join(b), [(0, ()), (1, ())].into());

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Foo(u8);

    impl PartialJoin for Foo {
        fn try_join(self, other: Self) -> JoinResult<Self> {
            if self == other {
                Ok(self)
            } else {
                Err(Conflict(vec![self, other]))
            }
        }
    }

    let foo = Foo(0).wrap();
    _ = foo.join(Foo(0).wrap());

    let a: BTreeMap<u8, Foo> = [(0, Foo(0))].into();
    let b: BTreeMap<u8, Foo> = [(1, Foo(0))].into();

    // Inject into a BTree<u8, Result<Foo, FooErr>>
    assert_eq!(
        a.clone().wrap().join(b.clone().wrap()),
        [(0, Ok(Foo(0))), (1, Ok(Foo(0)))].into()
    );

    // Flatten a BTree<u8, Result<Foo, FooErr>> into a Result<BTree<u8, Foo>, Btree<u8, Result<Foo, FooErr>>
    assert_eq!(
        a.clone().wrap().join(b.clone().wrap()).try_unwrap(),
        Ok([(0, Foo(0)), (1, Foo(0))].into())
    );

    let c: BTreeMap<u8, Foo> = [(0, Foo(1))].into();

    assert_eq!(
        a.clone().wrap().join(c.clone().wrap()),
        [(0, Err(Conflict(vec![Foo(0), Foo(1)])))].into()
    );

    assert_eq!(
        a.clone().wrap().join(c.clone().wrap()).try_unwrap(),
        Err([(0, Err(Conflict(vec![Foo(0), Foo(1)])))].into())
    );
}
