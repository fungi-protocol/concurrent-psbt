use crate::lattice::join::Join;
use crate::lattice::lift::{ConflictVisitor, Lift, ResultValue, VisitConflicts};

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

/// `None` lifts to `None`, and `Some` lifts its value.
impl<V: Lift> Lift for Option<V> {
    type Result = Option<V::Result>;

    fn lift(self) -> Self::Result {
        self.map(Lift::lift)
    }
}

/// `None` has no conflicts.
impl<R: ResultValue> ResultValue for Option<R> {
    type Clean = Option<R::Clean>;

    fn is_ok(&self) -> bool {
        self.as_ref().is_none_or(R::is_ok)
    }

    fn try_unwrap(self) -> Result<Self::Clean, Self> {
        self.map(R::try_unwrap).transpose().map_err(Some)
    }
}

impl<R: VisitConflicts> VisitConflicts for Option<R> {
    fn visit_conflicts(&self, f: &mut ConflictVisitor<'_>) {
        if let Some(value) = self {
            value.visit_conflicts(f);
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

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
        fn lift_none() {
            assert_eq!(None::<Foo>.lift(), None);
        }

        #[test]
        fn lift_some() {
            assert_eq!(Some(Foo(0)).lift(), Some(Ok(Foo(0))));
        }

        #[test]
        fn try_unwrap_ok() {
            assert_eq!(Some(Foo(0)).lift().try_unwrap(), Ok(Some(Foo(0))));
        }

        #[test]
        fn lifted_conflict_join() {
            assert_eq!(
                Some(Foo(0)).lift().join(Some(Foo(1)).lift()),
                Some(Err(Conflict::from_values([Foo(0), Foo(1)])))
            );
        }

        #[test]
        fn try_unwrap_conflict() {
            let conflict = Some(Foo(0)).lift().join(Some(Foo(1)).lift());
            assert_eq!(conflict.clone().try_unwrap(), Err(conflict));
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
                arb_foo().prop_map(|v| Some(v).lift()),
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
            fn is_ok_consistent_with_try_unwrap(a in arb_option_join_result()) {
                prop_assert_eq!(a.clone().is_ok(), a.try_unwrap().is_ok());
            }

            #[test]
            fn visits_the_conflict_inside_some(a in arb_option_join_result()) {
                let mut visited = Vec::new();
                a.visit_conflicts(&mut |key, conflict| {
                    visited.push((key.is_none(), format!("{conflict:?}")));
                });
                let expected = match &a {
                    Some(Err(conflict)) => vec![(true, format!("{conflict:?}"))],
                    _ => vec![],
                };
                prop_assert_eq!(visited, expected);
            }
        }
    }
}
