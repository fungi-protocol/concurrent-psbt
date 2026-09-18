/// Trait for infallible join operation.
pub trait Join: Sized {
    /// Merge two values of a given type into a new value of the same type
    /// incorporating the information of both inputs.
    ///
    /// This operation should be associative, commutative and idempotent.
    #[must_use]
    fn join(self, other: Self) -> Self;
}

/// In-place variant of [`Join`].
///
/// Implementing `JoinMut` provides `Join` automatically via blanket impl.
pub trait JoinMut: Sized {
    /// Merge `other` into `self` in place.
    fn join_mut(&mut self, other: Self);
}

impl<T: JoinMut> Join for T {
    fn join(mut self, other: Self) -> Self {
        self.join_mut(other);
        self
    }
}

/// Proptest macros for verifying lattice laws. Available when `prop-tests`
/// feature is enabled.
///
/// # `assert_join_laws!($strategy)`
///
/// Generates idempotent, commutative, and associative property tests for
/// any type implementing [`Join`] + `Clone` + `PartialEq` + `Debug`.
#[cfg(feature = "prop-tests")]
#[allow(unused_macros)] // used by downstream commits (partial.rs, collection modules)
macro_rules! assert_join_laws {
    ($strategy:expr) => {
        proptest! {
            #[test]
            fn idempotent(a in $strategy) {
                prop_assert_eq!(a.clone().join(a.clone()), a);
            }

            #[test]
            fn commutative(a in $strategy, b in $strategy) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn associative(a in $strategy, b in $strategy, c in $strategy) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).join(c.clone()),
                    a.join(b.join(c)),
                );
            }
        }
    };
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    impl JoinMut for u32 {
        fn join_mut(&mut self, other: Self) {
            *self = (*self).max(other);
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        impl Join for () {
            fn join(self, _other: Self) -> Self {
                self
            }
        }

        #[test]
        fn unit_type_join() {
            assert_eq!(Join::join((), ()), ());
        }

        #[test]
        fn join_mut_in_place() {
            let mut a = 1u32;
            a.join_mut(2);
            assert_eq!(a, 2);
        }

        #[test]
        fn blanket_join_from_join_mut() {
            assert_eq!(Join::join(1u32, 2), 2);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        assert_join_laws!(any::<u32>());
    }
}
