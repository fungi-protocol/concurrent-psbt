use crate::lattice::join::{Join, JoinMut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict<V>(pub Vec<V>);

impl<V: Eq> JoinMut for Conflict<V> {
    fn join_mut(&mut self, other: Self) {
        for value in other.0 {
            if !self.0.contains(&value) {
                self.0.push(value)
            }
        }
    }
}

impl<V> From<(V, V)> for Conflict<V> {
    fn from((a, b): (V, V)) -> Self {
        Self([a, b].into())
    }
}

impl<T: Eq> Absorb<T> for Conflict<T> {
    fn absorb(mut self, other: T) -> Self {
        if !self.0.contains(&other) {
            self.0.push(other);
        }
        self
    }
}

/// A result of a fallible Join. This is a value from the completion of the
/// partial lattice into the co-domain of results.
pub type JoinResult<V> = Result<V, Conflict<V>>;

/// Trait for a values implementing fallible join operation. Only value types
/// should implement this, containers should always be `Join` by wrapping the
/// contained values in an `Ok` `JoinResult` if they aren't `Join` themselves.
pub trait PartialJoin: Sized + Eq {
    /// Merge two values of a given type into a new value of the same type
    /// incorporating the information of both inputs.
    ///
    /// This operation should be associative, commutative and idempotent.
    fn try_join(self, other: Self) -> JoinResult<Self> {
        self.wrap().join(other.wrap())
    }

    /// Wrap a value in an `Ok`, injecting it into the Result co-domain lattice.
    fn wrap(self) -> JoinResult<Self> {
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

#[cfg(test)]
mod tests {
    use super::*;

    impl<T> Absorb<T> for () {
        fn absorb(self, _other: T) -> Self {}
    }

    impl crate::lattice::join::Join for core::convert::Infallible {
        fn join(self, _other: Self) -> Self {
            self
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
