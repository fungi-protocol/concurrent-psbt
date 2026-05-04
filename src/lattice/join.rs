/// Trait for infallible join operation.
pub trait Join: Sized {
    /// Merge two values of a given type into a new value of the same type
    /// incorporating the information of both inputs.
    ///
    /// This operation should be associative, commutative and idempotent.
    fn join(self, other: Self) -> Self;

    // {
    //     self.join_mut(other);
    //     self
    // }
    // fn join_mut(&mut self, other: Self);
}

// TODO no blanket PartialJoin for Join, instead add struct<T:Join> Partial(T) that impls PartialJoin
//
// impl<T> PartialJoin for T
// where
//     T: Join,
// {
//     type Error = std::convert::Infallible;

//     // TODO rename to try_join
//     fn join(&self, other: &Self) -> Result<Self, Self::Error> {
//         Ok(Join::join(self, other))
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    impl Join for core::convert::Infallible {
        fn join(self, _other: Self) -> Self {
            self
        }
    }

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
