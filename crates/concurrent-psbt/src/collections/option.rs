use crate::lattice::join::Join;
use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

/// [`Join`] for `Option<V>`: `None` is the identity element.
///
/// - `None ⊔ None = None`
/// - `None ⊔ Some(v) = Some(v)`
/// - `Some(a) ⊔ Some(b) = Some(a.join(b))`
impl<V> Join for Option<V>
where
    V: Join,
{
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (None, None) => None,
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => Some(a.join(b)),
        }
    }
}

/// Extension trait for lifting `Option<V>` into the result domain.
#[allow(dead_code)]
pub(crate) trait OptionExt {
    type Item: PartialJoin;
    /// Lift each inner value to `Ok`, preserving `None`.
    fn wrap(self) -> Option<JoinResult<Self::Item>>;
}

impl<V: PartialJoin> OptionExt for Option<V> {
    type Item = V;
    fn wrap(self) -> Option<JoinResult<V>> {
        self.map(|v| v.wrap())
    }
}

/// Extension trait for `Option<JoinResult<V>>` — conflict detection and extraction.
#[allow(dead_code)]
pub(crate) trait ResultOptionExt {
    type Value: PartialJoin;
    /// Returns `true` if the option is `None` or contains `Ok`.
    fn is_ok(&self) -> bool;
    /// Extract the inner value, or return the conflict.
    ///
    /// Unlike keyed result containers, which return `Err(Self)` because
    /// multiple entries may conflict, this returns `Err(Conflict<V>)`
    /// directly via `Option::transpose`.
    fn try_unwrap(self) -> Result<Option<Self::Value>, Conflict<Self::Value>>;
}

impl<V: PartialJoin> ResultOptionExt for Option<JoinResult<V>> {
    type Value = V;

    fn is_ok(&self) -> bool {
        !matches!(self, Some(Err(_)))
    }

    fn try_unwrap(self) -> Result<Option<V>, Conflict<V>> {
        self.transpose()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Foo(u8);

    impl PartialJoin for Foo {
        fn try_join(self, other: Self) -> JoinResult<Self> {
            if self == other {
                Ok(self)
            } else {
                Err(Conflict::from_values([self, other]))
            }
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn none_join_none() {
            assert_eq!(Join::join(None::<()>, None), None);
        }
        #[test]
        fn some_join_none() {
            assert_eq!(Join::join(Some(()), None), Some(()));
        }
        #[test]
        fn none_join_some() {
            assert_eq!(Join::join(None, Some(())), Some(()));
        }
        #[test]
        fn some_join_some() {
            assert_eq!(Join::join(Some(()), Some(())), Some(()));
        }
        #[test]
        fn wrap_none() {
            assert_eq!(None::<Foo>.wrap(), None);
        }
        #[test]
        fn wrap_some() {
            assert_eq!(Some(Foo(0)).wrap(), Some(Ok(Foo(0))));
        }
        #[test]
        fn try_unwrap_ok() {
            assert_eq!(Some(Foo(0)).wrap().try_unwrap(), Ok(Some(Foo(0))));
        }

        #[test]
        fn wrapped_none_join_some() {
            assert_eq!(
                None::<Foo>.wrap().join(Some(Foo(0)).wrap()),
                Some(Ok(Foo(0)))
            );
        }

        #[test]
        fn wrapped_some_join_none() {
            assert_eq!(Some(Foo(0)).wrap().join(None), Some(Ok(Foo(0))));
        }

        #[test]
        fn wrapped_equal_join() {
            assert_eq!(
                Some(Foo(0)).wrap().join(Some(Foo(0)).wrap()),
                Some(Ok(Foo(0)))
            );
        }

        #[test]
        fn wrapped_conflict_join() {
            assert_eq!(
                Some(Foo(0)).wrap().join(Some(Foo(1)).wrap()),
                Some(Err(Conflict::from_values([Foo(0), Foo(1)])))
            );
        }

        #[test]
        fn try_unwrap_conflict() {
            assert_eq!(
                Some(Foo(0)).wrap().join(Some(Foo(1)).wrap()).try_unwrap(),
                Err(Conflict::from_values([Foo(0), Foo(1)]))
            );
        }

        #[test]
        fn is_ok_true() {
            assert!(Some(Foo(0)).wrap().is_ok());
        }
        #[test]
        fn is_ok_none() {
            assert!(None::<JoinResult<Foo>>.is_ok());
        }

        #[test]
        fn is_ok_false() {
            let err: Option<JoinResult<Foo>> = Some(Err(Conflict::from_values([Foo(0), Foo(1)])));
            assert!(!err.is_ok());
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        fn arb_foo() -> impl Strategy<Value = Foo> {
            (0u8..4).prop_map(Foo)
        }

        fn arb_option_join_result() -> impl Strategy<Value = Option<JoinResult<Foo>>> {
            prop_oneof![
                Just(None),
                arb_foo().prop_map(|v| Some(Ok(v))),
                proptest::collection::hash_set(arb_foo(), 1..=3)
                    .prop_map(|s| Some(Err(Conflict::from_values(s)))),
            ]
        }

        // Lattice laws via macro
        assert_join_laws!(arb_option_join_result());

        // Type-specific: Option semantics
        proptest! {
            #[test]
            fn none_is_identity(a in arb_option_join_result()) {
                prop_assert_eq!(a.clone().join(None), a.clone());
                prop_assert_eq!(None.join(a.clone()), a);
            }

            #[test]
            fn wrap_some_is_ok(v in arb_foo()) {
                let wrapped = Some(v).wrap();
                prop_assert!(wrapped.is_some());
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn is_ok_consistent_with_try_unwrap(a in arb_option_join_result()) {
                prop_assert_eq!(a.clone().is_ok(), a.try_unwrap().is_ok());
            }
        }
    }
}
