use std::collections::BTreeMap;

use crate::lattice::conflicts::Conflicts;
use crate::lattice::join::{Join, JoinMut};
use crate::lattice::lift::{Lift, ResultWrapped};

impl<K: Ord, V: Lift> Lift for BTreeMap<K, V> {
    type Result = BTreeMap<K, V::Result>;

    fn wrap(self) -> Self::Result {
        self.into_iter().map(|(k, v)| (k, v.wrap())).collect()
    }
}

impl<K: Ord, R: ResultWrapped> ResultWrapped for BTreeMap<K, R> {
    type Clean = BTreeMap<K, R::Clean>;

    fn is_ok(&self) -> bool {
        self.values().all(R::is_ok)
    }

    fn try_unwrap(self) -> Result<Self::Clean, Self> {
        if !self.is_ok() {
            return Err(self);
        }
        Ok(self
            .into_iter()
            .map(|(k, v)| (k, unwrap_after_is_ok(v)))
            .collect())
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn unwrap_after_is_ok<R: ResultWrapped>(value: R) -> R::Clean {
    match value.try_unwrap() {
        Ok(value) => value,
        Err(_) => unreachable!("is_ok() guard verified all entries"),
    }
}

impl<K, R: Conflicts> Conflicts for BTreeMap<K, R> {
    type Item<'a>
        = (&'a K, R::Item<'a>)
    where
        Self: 'a;

    fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>> {
        self.iter()
            .flat_map(|(key, value)| value.conflicts().map(move |conflict| (key, conflict)))
    }
}

/// [`JoinMut`] for `BTreeMap`: matching keys are joined, disjoint keys are unioned.
impl<K: Ord, V: Join> JoinMut for BTreeMap<K, V> {
    fn join_mut(&mut self, other: Self) {
        for (k, v) in other {
            let lub = match self.remove(&k) {
                Some(prev) => prev.join(v),
                None => v,
            };
            self.insert(k, lub);
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct Val(u8);

    impl PartialJoin for Val {
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
        fn lift_lifts_each_value() {
            let a: BTreeMap<u8, Val> = [(0, Val(0))].into();
            assert_eq!(a.wrap(), [(0, Ok(Val(0)))].into());
        }

        #[test]
        fn lifted_disjoint() {
            let a: BTreeMap<u8, Val> = [(0, Val(0))].into();
            let b: BTreeMap<u8, Val> = [(1, Val(0))].into();
            assert_eq!(
                a.wrap().join(b.wrap()),
                [(0, Ok(Val(0))), (1, Ok(Val(0)))].into()
            );
        }

        #[test]
        fn try_unwrap_ok() {
            let a: BTreeMap<u8, Val> = [(0, Val(0))].into();
            let b: BTreeMap<u8, Val> = [(1, Val(0))].into();
            assert_eq!(
                a.wrap().join(b.wrap()).try_unwrap(),
                Ok([(0, Val(0)), (1, Val(0))].into())
            );
        }

        #[test]
        fn lifted_conflict() {
            let a: BTreeMap<u8, Val> = [(0, Val(0))].into();
            let c: BTreeMap<u8, Val> = [(0, Val(1))].into();
            assert_eq!(
                a.wrap().join(c.wrap()),
                [(0, Err(Conflict::from_values([Val(0), Val(1)])))].into()
            );
        }

        #[test]
        fn try_unwrap_conflict() {
            let a: BTreeMap<u8, Val> = [(0, Val(0))].into();
            let c: BTreeMap<u8, Val> = [(0, Val(1))].into();
            assert_eq!(
                a.wrap().join(c.wrap()).try_unwrap(),
                Err([(0, Err(Conflict::from_values([Val(0), Val(1)])))].into())
            );
        }

        #[test]
        fn join_mut_in_place() {
            let mut a: BTreeMap<u8, ()> = [(0, ())].into();
            a.join_mut([(1, ())].into());
            assert_eq!(a, [(0, ()), (1, ())].into());
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        fn arb_val() -> impl Strategy<Value = Val> {
            (0u8..4).prop_map(Val)
        }

        fn arb_join_result() -> impl Strategy<Value = JoinResult<Val>> {
            prop_oneof![
                arb_val().prop_map(Ok),
                proptest::collection::hash_set(arb_val(), 1..=3)
                    .prop_map(|s| Err(Conflict::from_values(s))),
            ]
        }

        fn arb_btreemap() -> impl Strategy<Value = BTreeMap<u8, JoinResult<Val>>> {
            proptest::collection::btree_map(0u8..4, arb_join_result(), 0..=3)
        }

        // Lattice laws via macro
        assert_join_laws!(arb_btreemap());

        // Type-specific
        proptest! {
            #[test]
            fn lift_try_unwrap_roundtrip(
                m in proptest::collection::btree_map(0u8..4, arb_val(), 0..=3)
            ) {
                let len = m.len();
                let lifted: BTreeMap<u8, JoinResult<Val>> = m.clone().wrap();
                prop_assert_eq!(lifted.len(), len);
                prop_assert!(lifted.is_ok());
                prop_assert_eq!(lifted.try_unwrap(), Ok(m));
            }

            #[test]
            fn is_ok_reflects_content(m in arb_btreemap()) {
                let has_conflict = m.values().any(|v| v.is_err());
                prop_assert_eq!(m.is_ok(), !has_conflict);
            }

            #[test]
            fn visits_each_conflict_under_its_key(m in arb_btreemap()) {
                let visited: std::collections::BTreeMap<_, _> = m.conflicts().collect();
                let expected: std::collections::BTreeMap<_, _> = m.iter()
                    .filter_map(|(key, value)| value.as_ref().err().map(|conflict| (key, conflict)))
                    .collect();
                prop_assert_eq!(visited, expected);
            }

            #[test]
            fn try_unwrap_err_when_conflict(
                m in arb_btreemap()
                    .prop_filter("need conflict", |m| m.values().any(|v| v.is_err()))
            ) {
                prop_assert!(m.try_unwrap().is_err());
            }
        }
    }
}
