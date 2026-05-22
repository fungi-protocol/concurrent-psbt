use std::collections::BTreeMap;

use crate::lattice::join::{Join, JoinMut};
use crate::lattice::partial::{JoinResult, PartialJoin};

/// Extension trait for lifting `BTreeMap<K, V>` values to `Ok`, entering the result domain.
#[allow(dead_code)]
pub(crate) trait BTreeMapExt {
    type Key;
    type Value: PartialJoin;
    /// Lift each value to `Ok`, preserving keys.
    fn wrap(self) -> BTreeMap<Self::Key, JoinResult<Self::Value>>;
}

impl<K: Ord, V: PartialJoin> BTreeMapExt for BTreeMap<K, V> {
    type Key = K;
    type Value = V;
    fn wrap(self) -> BTreeMap<K, JoinResult<V>> {
        self.into_iter().map(|(k, v)| (k, v.wrap())).collect()
    }
}

/// Extension trait for `BTreeMap<K, JoinResult<V>>` — `is_ok` checks all values,
/// `try_unwrap` extracts the plain map or returns `self` on conflict.
#[allow(dead_code)]
pub(crate) trait ResultBTreeMapExt: Sized {
    type Key;
    type Value: PartialJoin;
    /// Returns `true` if every value in the map is `Ok` (no conflicts).
    fn is_ok(&self) -> bool;
    /// Extract the plain `BTreeMap<K, V>`, or return `self` if any value is a conflict.
    fn try_unwrap(self) -> Result<BTreeMap<Self::Key, Self::Value>, Self>;
}

impl<K: Ord, V: PartialJoin> ResultBTreeMapExt for BTreeMap<K, JoinResult<V>> {
    type Key = K;
    type Value = V;

    fn is_ok(&self) -> bool {
        self.values().all(|v| v.is_ok())
    }

    fn try_unwrap(self) -> Result<BTreeMap<K, V>, Self> {
        if !self.is_ok() {
            return Err(self);
        }
        self.into_iter()
            .map(|(k, v)| v.map(|v| (k, v)))
            .collect::<Result<_, _>>()
            .map_err(
                // .expect() needs V : Debug
                #[cfg_attr(coverage_nightly, coverage(off))]
                |_| unreachable!("all entries verified Ok"),
            )
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
    use crate::lattice::partial::Conflict;

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
        fn join_disjoint() {
            let a: BTreeMap<u8, ()> = [(0, ())].into();
            let b: BTreeMap<u8, ()> = [(1, ())].into();
            assert_eq!(a.join(b), [(0, ()), (1, ())].into());
        }

        #[test]
        fn join_idempotent() {
            let a: BTreeMap<u8, ()> = [(0, ())].into();
            assert_eq!(a.clone().join(a.clone()), a);

            let b: BTreeMap<u8, Val> = [(0, Val(1))].into();
            assert_eq!(b.clone().wrap().join(b.clone().wrap()), b.wrap());
        }

        #[test]
        fn join_empty() {
            let a: BTreeMap<u8, ()> = BTreeMap::new();
            assert_eq!(a.clone().join(a), BTreeMap::new());
        }

        #[test]
        fn wrap_lifts() {
            let a: BTreeMap<u8, Val> = [(0, Val(0))].into();
            assert_eq!(a.wrap(), [(0, Ok(Val(0)))].into());
        }

        #[test]
        fn wrapped_disjoint() {
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
        fn wrapped_conflict() {
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
        fn is_ok_true() {
            let m: BTreeMap<u8, JoinResult<Val>> = [(0, Ok(Val(0)))].into();
            assert!(m.is_ok());
        }

        #[test]
        fn is_ok_false() {
            let m: BTreeMap<u8, JoinResult<Val>> =
                [(0, Err(Conflict::from_values([Val(0), Val(1)])))].into();
            assert!(!m.is_ok());
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
            fn wrap_try_unwrap_roundtrip(
                m in proptest::collection::btree_map(0u8..4, arb_val(), 0..=3)
            ) {
                let len = m.len();
                let wrapped: BTreeMap<u8, JoinResult<Val>> = m.wrap();
                prop_assert_eq!(wrapped.len(), len);
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn is_ok_reflects_content(m in arb_btreemap()) {
                let has_conflict = m.values().any(|v| v.is_err());
                prop_assert_eq!(m.is_ok(), !has_conflict);
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
