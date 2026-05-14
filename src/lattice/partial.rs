use crate::lattice::join::Join;

// TODO improve ergonomics for the trivial case of Conflict(Value(x), Value(y))
// which is by far the most common
// TODO if A+B+C fails, where A+B and B+C generate conflicts in different parts
// fields, given a conflict it will not be possible to know which input led to
// the conflict. not clear how to fix this so maybe they should just be a flat
// list that preserves the order, so since useful provenance seems to require
// working one pair at a time and handling errors as soon as they come up.
/// An error type that remembers the argument position for conflicting values
pub enum Error<V> {
    Value(V),
    Conflict(Box<[Error<V>; 2]>),
}

impl<V> Error<V> {
    /// Flattens the conflict hierarchy to a list of conflicting value
    pub fn values(&self) -> Box<dyn Iterator<Item = &V> + '_> {
        use Error::*;
        match self {
            Value(v) => Box::new(std::iter::once(v)),
            Conflict(c) => Box::new(c.iter().flat_map(|e| e.values())),
        }
    }

    pub fn into_values(self) -> impl Iterator<Item = V> {
        let mut out = Vec::new();
        self.collect_into(&mut out);
        out.into_iter()
    }

    // Can be a tiny vec since len() = 2 will be most common case
    fn collect_into(self, out: &mut Vec<V>) {
        use Error::*;
        match self {
            Value(v) => out.push(v),
            Conflict(c) => {
                let [a, b] = *c;
                a.collect_into(out);
                b.collect_into(out);
            }
        }
    }
}

impl<V> Join for Error<V> {
    fn join(self, other: Self) -> Self {
        Error::Conflict(Box::new([self, other]))
    }
}

/// A result of a fallible Join. This is a value from the completion of the
/// partial lattice into the co-domain of results.
pub type JoinResult<V> = Result<V, <V as PartialJoin>::Error>;

/// Trait for a values implementing fallible join operation. Only value types
/// should implement this, containers should always be `Join` by wrapping the
/// contained values in an `Ok` `JoinResult` if they aren't `Join` themselves.
pub trait PartialJoin: Sized {
    /// The error type for when `join` fails.
    // FIXME should always be the Error concrete error type defined above
    // The outer error should always be Conflict
    type Error: Join + Absorb<Self>;

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
        fn absorb(self, _other: T) -> Self {
            ()
        }
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
