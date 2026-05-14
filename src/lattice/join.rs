/// Trait for infallible join operation.
pub trait Join: Sized {
    /// Merge two values of a given type into a new value of the same type
    /// incorporating the information of both inputs.
    ///
    /// This operation should be associative, commutative and idempotent.
    fn join(self, other: Self) -> Self;
}

#[allow(dead_code)]
pub trait JoinMut: Join {
    fn join_mut(&mut self, other: Self);

    fn join(mut self, other: Self) -> Self {
        self.join_mut(other);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Join for () {
        fn join(self, _other: Self) -> Self {
            self
        }
    }

    #[test]
    fn test_trait_definition() {
        assert_eq!(Join::join((), ()), ());
    }

    // TODO move to result mod
    // fn assert_impl_join_and_clone<T: Join + Clone>() {}
    // assert_impl_join_and_clone::<Result<(), core::convert::Infallible>>();
    // assert_impl_join_and_clone::<Result<u8, crate::values::ConflictingValues<u8>>>();
    // assert_impl_join_and_clone::<Result<crate::input::Input, ()>>();
}
