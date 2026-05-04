use std::collections::BTreeMap;

use crate::lattice::join::Join;
use crate::lattice::partial::{JoinResult, PartialJoin};

// TODO Deref newtype instead of extension traits?

// TODO tests
//
trait ResultCollection: Join {
    type Item: PartialJoin;

    fn transpose(self) -> JoinResult<Self::Item>;
}

pub trait BTreeMapExt {
    type Key;
    type Value: PartialJoin;
    fn into_ok(self) -> BTreeMap<Self::Key, JoinResult<Self::Value>>;
}

impl<K, V> BTreeMapExt for BTreeMap<K, V>
where
    K: Ord,
    V: PartialJoin,
{
    type Key = K;
    type Value = V;

    fn into_ok(self) -> BTreeMap<K, JoinResult<V>> {
        self.into_iter().map(|(k, v)| (k, v.into_ok())).collect()
    }
}

pub trait Transpose: Sized {
    type Key;
    type Value: PartialJoin;

    fn transpose(self) -> Result<BTreeMap<Self::Key, Self::Value>, Self>;

    fn is_ok(&self) -> bool;
}

impl<K, V> Transpose for BTreeMap<K, JoinResult<V>>
where
    K: Ord,
    V: PartialJoin,
{
    type Key = K;
    type Value = V;

    fn is_ok(&self) -> bool {
        self.values().all(|v| v.is_ok())
    }

    fn transpose(self) -> Result<BTreeMap<Self::Key, Self::Value>, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        self.into_iter()
            .map(|(k, v)| v.map(|v| (k, v)))
            .collect::<Result<_, _>>()
            .map_err(|_| panic!("all entries verified to be Ok"))
    }
}

impl<K, V> Join for BTreeMap<K, V>
where
    K: Ord,
    V: Join,
{
    fn join(self, other: Self) -> Self {
        let mut new: BTreeMap<K, V> = BTreeMap::new();

        for (k, v) in self.into_iter().chain(other) {
            // TODO itertools.merge_join_by to .collect() in one streaming pass
            let lub = match new.remove(&k) {
                Some(prev) => prev.join(v),
                None => v,
            };

            new.insert(k, lub);
        }

        new

        // itertools::merge_join_by(self, other, |(ka, _), (kb, _)| ka.cmp(kb))
        //     .map(|e| match e {
        //         EitherOrBoth::Both((k, a), (_, b)) => (k, a.join(b)),
        //         EitherOrBoth::Left(e) => e,
        //         EitherOrBoth::Right(e) => e,
        //     })
        //     .collect()
    }
}

#[test]
fn test_btree() {
    let a: BTreeMap<u8, ()> = [(0, ())].into();
    let b: BTreeMap<u8, ()> = [(1, ())].into();

    assert_eq!(a.clone().join(a.clone()), a.clone());
    assert_eq!(a.join(b), [(0, ()), (1, ())].into());

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Foo(u8);

    #[derive(Debug, PartialEq, Eq)]
    struct FooErr(Vec<u8>);

    impl PartialJoin for Foo {
        type Error = FooErr;

        fn try_join(self, other: Self) -> JoinResult<Self> {
            if self == other {
                Ok(self)
            } else {
                Err(FooErr(vec![self.0, other.0]))
            }
        }
    }

    impl crate::lattice::partial::Absorb<Foo> for FooErr {
        fn absorb(self, other: Foo) -> Self {
            self.join(FooErr(vec![other.0]))
        }
    }

    impl Join for FooErr {
        fn join(mut self, other: Self) -> Self {
            for v in other.0 {
                if !self.0.contains(&v) {
                    self.0.push(v)
                }
            }

            self
        }
    }

    let foo = Foo(0).into_ok();
    _ = foo.join(Foo(0).into_ok());

    let a: BTreeMap<u8, Foo> = [(0, Foo(0))].into();
    let b: BTreeMap<u8, Foo> = [(1, Foo(0))].into();

    // Inject into a BTree<u8, Result<Foo, FooErr>>
    assert_eq!(
        a.clone().into_ok().join(b.clone().into_ok()),
        [(0, Ok(Foo(0))), (1, Ok(Foo(0)))].into()
    );

    // Flatten a BTree<u8, Result<Foo, FooErr>> into a Result<BTree<u8, Foo>, Btree<u8, Result<Foo, FooErr>>
    assert_eq!(
        a.clone().into_ok().join(b.clone().into_ok()).transpose(),
        Ok([(0, Foo(0)), (1, Foo(0))].into())
    );

    let c: BTreeMap<u8, Foo> = [(0, Foo(1))].into();

    assert_eq!(
        a.clone().into_ok().join(c.clone().into_ok()),
        [(0, Err(FooErr([0, 1].into())))].into()
    );

    assert_eq!(
        a.clone().into_ok().join(c.clone().into_ok()).transpose(),
        Err([(0, Err(FooErr([0, 1].into())))].into())
    );
}
