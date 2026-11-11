use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

/// Marker trait for scalar types where join is equality-based:
/// equal values merge, different values produce a conflict.
pub(crate) trait IdempotentValue: Eq + Clone {}

impl<T: IdempotentValue> PartialJoin for T {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        if self == other {
            Ok(self)
        } else {
            Err(Conflict::from_values([self, other]))
        }
    }
}

impl IdempotentValue for u8 {}
impl IdempotentValue for u32 {}
impl IdempotentValue for usize {}
impl IdempotentValue for Vec<u8> {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct Scalar(u8);
    impl IdempotentValue for Scalar {}

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn equal_values_join_ok() {
            assert_eq!(Scalar(1).try_join(Scalar(1)), Ok(Scalar(1)));
        }

        #[test]
        fn different_values_conflict() {
            assert_eq!(
                Scalar(0).try_join(Scalar(1)),
                Err(Conflict::from_values([Scalar(0), Scalar(1)]))
            );
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use crate::lattice::join::Join;
        use proptest::prelude::*;

        fn arb_scalar() -> impl Strategy<Value = Scalar> {
            (0u8..4).prop_map(Scalar)
        }

        fn arb_result_scalar() -> impl Strategy<Value = JoinResult<Scalar>> {
            prop_oneof![
                arb_scalar().prop_map(Ok),
                (arb_scalar(), arb_scalar()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        assert_partial_join_laws!(arb_scalar(), arb_result_scalar());
    }
}
