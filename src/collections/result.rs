use crate::lattice::join::Join;
use crate::lattice::partial::{Absorb, JoinResult, PartialJoin};

impl<V> Join for Result<V, V::Error>
where
    V: PartialJoin,
    V::Error: Join + Absorb<V>,
{
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Ok(a), Ok(b)) => a.try_join(b),
            (Err(a), Err(b)) => Err(a.join(b)),
            (Err(a), Ok(b)) => Err(a.absorb(b)),
            (Ok(a), Err(b)) => Err(b.absorb(a)),
        }
    }

    // // FIXME clone needed for this, replace_with can do unsafely
    // fn join_mut(&mut self, other: Self) {
    //     std::mem::replace(self, self.join(other));
    // }
}

// impl<V> Join for JoinResult<V>
// where
//     JoinResult<V>: Join,
//     V: Join,
//     V::Error: Join + Absorb<V>,
// {
//     type Error = V::Error;

//     fn try_join(self, other: Self) -> JoinResult<Self> {
//         self.join(other)
//     }
// }

// impl<V, E> Join for JoinResult<V>
// where
//     V: PartialJoin<Error = E> + Clone,

//     // The associated error must be `Join`, not just `PartialJoin` as well as
//     // allow the value type to be transformed to an error type in so that
//     // `join(Ok(a),Err(b) => Err(join(a, b))` can be infallible.
//     E: Join + Absorb<V>,
// {
//     fn join(self, other: Self) -> Self {
//         match (self, other) {
//             (Ok(a), Ok(b)) => a.try_join(b),
//             (Err(a), Err(b)) => Err(a.join(b)),
//             (Err(a), Ok(b)) => Err(a.absorb(b.clone())),
//             (Ok(a), Err(b)) => Err(b.absorb(a.clone())),
//         }
//     }
// }

// // TODO remove
// // blanket impl does this
// impl<V, E> PartialJoin for Result<V, E>
// where
//     V: PartialJoin<Error = E> + Clone,
//     E: Join + Absorb<V>,
//     V::Error: Clone,
// {
//     type Error = std::convert::Infallible;

//     fn join(&self, other: &Self) -> JoinResult<Self> {
//         Ok(Join::join(self, other))
//     }
// }

#[test]
fn test_trait_bounds() {
    // fn assert_impl_partial_join_and_clone<T: PartialJoin + Clone>() {}

    // forall (V: Join): (V : PartialJoin)
    // forall (V: PartialJoin, E: Absorb): (Result<V, E> : Join)
    // => (Result<V: Join, E> : Join)
    // => (Result<V: Join, E> : PartialJoin)
}

#[test]
fn test_join_result() {
    // assert_eq!(PartialJoin::try_join((), ()), Ok(()));
    assert_eq!(Join::join((), ()), ());
    // assert_eq!(Join::join(PartialJoin::try_join((), ()), Ok(())), Ok(()));
}

#[test]
fn test_join_result_option() {
    // TODO Result<Option<T: Join>>
    // TODO Result<Option<T: PartialJoin>>
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_join_result() {
        let a = Foo(0u8);
        let b = Foo(1u8);

        assert_eq!(a.try_join(a), Ok(a));
        assert_eq!(a.try_join(b), Err(()));

        assert_eq!(a.into_ok().join(a.into_ok()), Ok(a));
        assert_eq!(a.into_ok().join(b.into_ok()), Err(()));
    }
}
