use std::collections::HashMap;
use std::hash::Hash;

use crate::lattice::conflicts::Conflicts;
use crate::lattice::join::{Join, JoinMut};
use crate::lattice::lift::{Lift, ResultWrapped};

/// [`JoinMut`] (in-place join) for `HashMap`: matching keys are joined, disjoint keys are unioned.
impl<K, V> JoinMut for HashMap<K, V>
where
    K: Hash + Eq,
    V: Join,
{
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

impl<K: Hash + Eq, V: Lift> Lift for HashMap<K, V> {
    type Result = HashMap<K, V::Result>;

    fn wrap(self) -> Self::Result {
        self.into_iter()
            .map(|(key, value)| (key, value.wrap()))
            .collect()
    }
}

impl<K, R: Conflicts> Conflicts for HashMap<K, R> {
    type Item<'a>
        = (&'a K, R::Item<'a>)
    where
        Self: 'a;

    fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>> {
        self.iter()
            .flat_map(|(key, value)| value.conflicts().map(move |conflict| (key, conflict)))
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn unwrap_after_is_ok<V: ResultWrapped>(value: V) -> V::Clean {
    match value.try_unwrap() {
        Ok(value) => value,
        Err(_) => unreachable!("is_ok() guard verified all entries"),
    }
}

impl<K: Hash + Eq, V: ResultWrapped> ResultWrapped for HashMap<K, V> {
    type Clean = HashMap<K, V::Clean>;

    fn is_ok(&self) -> bool {
        self.values().all(|v| v.is_ok())
    }

    fn try_unwrap(self) -> Result<HashMap<K, V::Clean>, Self> {
        if !self.is_ok() {
            return Err(self);
        }
        let clean = self
            .into_iter()
            .map(|(key, value)| (key, unwrap_after_is_ok(value)))
            .collect::<HashMap<_, _>>();
        Ok(clean)
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
        fn nested_maps_lift_join_and_preserve_conflicts() {
            let a: HashMap<u8, Option<HashMap<u8, Val>>> =
                [(1, Some([(2, Val(3))].into())), (4, None)].into();
            let b: HashMap<u8, Option<HashMap<u8, Val>>> = [(1, Some([(2, Val(5))].into()))].into();
            assert_eq!(ResultWrapped::try_unwrap(a.clone().wrap()), Ok(a.clone()));
            let joined = a.wrap().join(b.wrap());
            assert!(!ResultWrapped::is_ok(&joined));
            assert_eq!(
                ResultWrapped::try_unwrap(joined.clone()),
                Err(joined.clone())
            );
            let keys: Vec<_> = joined
                .conflicts()
                .map(|(outer, (inner, _))| (*outer, *inner))
                .collect();
            assert_eq!(keys, [(1, 2)]);
        }

        #[test]
        fn wrap_preserves_keys() {
            let m: HashMap<&str, Val> = [("a", Val(1)), ("b", Val(2))].into();
            let w = m.wrap();
            assert_eq!(w.len(), 2);
            assert_eq!(w.get("a"), Some(&Ok(Val(1))));
        }

        #[test]
        fn try_unwrap_ok() {
            let m: HashMap<&str, Val> = [("a", Val(1))].into();
            assert_eq!(m.clone().wrap().try_unwrap().unwrap(), m);
        }

        #[test]
        fn join_mut_in_place() {
            let mut a: HashMap<u8, ()> = [(0, ())].into();
            a.join_mut([(1, ())].into());
            assert_eq!(a.len(), 2);
        }

        #[test]
        fn wrapped_conflict() {
            let a: HashMap<u8, Val> = [(0, Val(0))].into();
            let c: HashMap<u8, Val> = [(0, Val(1))].into();
            assert_eq!(
                a.wrap().join(c.wrap()),
                [(0, Err(Conflict::from_values([Val(0), Val(1)])))].into()
            );
        }

        #[test]
        fn try_unwrap_conflict() {
            let a: HashMap<u8, Val> = [(0, Val(0))].into();
            let c: HashMap<u8, Val> = [(0, Val(1))].into();
            assert_eq!(
                a.wrap().join(c.wrap()).try_unwrap(),
                Err([(0, Err(Conflict::from_values([Val(0), Val(1)])))].into())
            );
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

        fn arb_hashmap() -> impl Strategy<Value = HashMap<u8, JoinResult<Val>>> {
            proptest::collection::hash_map(0u8..4, arb_join_result(), 0..=3)
        }

        // Lattice laws via macro
        assert_join_laws!(arb_hashmap());

        // Type-specific
        proptest! {
            #[test]
            fn lift_try_unwrap_roundtrip(m in proptest::collection::hash_map(0u8..4, arb_val(), 0..=3)) {
                let len = m.len();
                let wrapped: HashMap<u8, JoinResult<Val>> = m.clone().wrap();
                prop_assert_eq!(wrapped.len(), len);
                prop_assert!(wrapped.is_ok());
                prop_assert_eq!(wrapped.try_unwrap(), Ok(m));
            }

            #[test]
            fn is_ok_reflects_content(
                m in proptest::collection::hash_map(0u8..4, arb_join_result(), 0..=3)
            ) {
                let has_conflict = m.values().any(|v| v.is_err());
                prop_assert_eq!(m.is_ok(), !has_conflict);
            }

            #[test]
            fn visits_each_conflict_under_its_key(m in arb_hashmap()) {
                let visited: std::collections::BTreeMap<_, _> = m.conflicts().collect();
                let expected: std::collections::BTreeMap<_, _> = m.iter()
                    .filter_map(|(key, value)| value.as_ref().err().map(|conflict| (key, conflict)))
                    .collect();
                prop_assert_eq!(visited, expected);
            }

            #[test]
            fn try_unwrap_err_when_conflict(
                m in proptest::collection::hash_map(0u8..4, arb_join_result(), 1..=3)
                    .prop_filter("need conflict", |m| m.values().any(|v| v.is_err()))
            ) {
                prop_assert!(m.try_unwrap().is_err());
            }
        }
    }
}
