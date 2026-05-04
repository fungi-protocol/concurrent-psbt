use crate::lattice::join::Join;

/// A result of a fallible Join. This is a value from the completion of the
/// partial lattice into the co-domain of results.
pub type JoinResult<V> = Result<V, <V as PartialJoin>::Error>;

/// Trait for a values implementing fallible join operation. Only value types
/// should implement this, containers should always be `Join` by wrapping the
/// contained values in an `Ok` `JoinResult` if they aren't `Join` themselves.
pub trait PartialJoin: Sized {
    /// The error type for when `join` fails.
    type Error: Join + Absorb<Self>;

    /// Merge two values of a given type into a new value of the same type
    /// incorporating the information of both inputs.
    ///
    /// This operation should be associative, commutative and idempotent.
    fn try_join(self, other: Self) -> JoinResult<Self> {
        self.into_ok().join(other.into_ok())
    }

    /// Wrap a value in an `Ok`, injecting it into the Result co-domain lattice.
    fn into_ok(self) -> JoinResult<Self> {
        Ok(self)
    }
}

/// Specialized homomorphic-ish from the value branch to the error branch of a
/// Result.
///
/// Given a value T, if join(T, T) -> Result<T, E>, then E may implement
/// Absorb<T> to incorporate additional non-error T values. This allows the
/// co-domain of PartialJoin::try_join to itself be a join semi lattice, so
/// Result<T, E> can implement Join.
pub trait Absorb<T> {
    /// Consume self and T, incorporate T into self and return the result.
    fn absorb(self, other: T) -> Self;
}

// impl<T: Join> PartialJoin for T {
//     type Error = core::convert::Infallible;

//     fn try_join(self, other: Self) -> JoinResult<Self> {
//         Ok(self.join(other))
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    impl<T> Absorb<T> for () {
        fn absorb(self, _other: T) -> Self {
            ()
        }
    }

    impl<T> Absorb<T> for core::convert::Infallible {
        fn absorb(self, _other: T) -> Self {
            self
        }
    }

    // impl PartialJoin for () {
    //     type Error = core::convert::Infallible;

    //     fn try_join(self, _other: Self) -> JoinResult<Self> {
    //         Ok(())
    //     }
    // }

    #[test]
    fn test_trait_definition() {
        // assert_eq!(PartialJoin::try_join((), ()), Ok(()));

        // TODO move to values
        // fn assert_impl_partial_join_and_clone<T: PartialJoin + Clone>() {}
        // assert_impl_partial_join_and_clone::<()>();
        // assert_impl_partial_join_and_clone::<u8>();
        // assert_impl_partial_join_and_clone::<Vec<u8>>();
        // assert_impl_partial_join_and_clone::<crate::collections::vec::VecWrapper<u8>>();
        // assert_impl_partial_join_and_clone::<Option<u8>>();
        // assert_impl_partial_join_and_clone::<Option<Vec<u8>>>();
        // assert_impl_partial_join_and_clone::<crate::input::Input>();
        // assert_impl_partial_join_and_clone::<crate::output::Output>();
        // assert_impl_partial_join_and_clone::<Result<crate::input::Input, ()>>();
        // assert_impl_partial_join_and_clone::<JoinResult<crate::input::Input>>();
        // assert_impl_partial_join_and_clone::<JoinResult<crate::output::Output>>();
    }
}
