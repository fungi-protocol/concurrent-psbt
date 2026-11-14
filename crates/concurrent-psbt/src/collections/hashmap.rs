use std::collections::HashMap;
use std::hash::Hash;

use crate::lattice::join::{Join, JoinMut};
use crate::lattice::lift::ResultValue;

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

/// Extension trait for transforming `HashMap` values while preserving keys.
pub(crate) trait HashMapExt {
    type Key;
    type Value;
    /// Transform each value, preserving keys.
    fn map_values<ResultValue>(
        self,
        f: impl FnMut(Self::Value) -> ResultValue,
    ) -> HashMap<Self::Key, ResultValue>;
}

impl<K: Hash + Eq, V> HashMapExt for HashMap<K, V> {
    type Key = K;
    type Value = V;
    fn map_values<ResultValue>(
        self,
        mut f: impl FnMut(V) -> ResultValue,
    ) -> HashMap<K, ResultValue> {
        self.into_iter().map(|(k, v)| (k, f(v))).collect()
    }
}

/// Extension trait for result-domain `HashMap` values.
pub(crate) trait ResultHashMapExt: Sized {
    type Key;
    type Value: ResultValue;
    /// Returns `true` if every value in the map is conflict-free.
    fn is_ok(&self) -> bool;
    /// Extract the plain map, or return `self` if any value is a conflict.
    fn try_unwrap(self) -> Result<HashMap<Self::Key, <Self::Value as ResultValue>::Clean>, Self>;
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn unwrap_after_is_ok<V: ResultValue>(value: V) -> V::Clean {
    match value.try_unwrap() {
        Ok(value) => value,
        Err(_) => unreachable!("is_ok() guard verified all entries"),
    }
}

impl<K: Hash + Eq, V: ResultValue> ResultHashMapExt for HashMap<K, V> {
    type Key = K;
    type Value = V;

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

        fn wrap_map<K: Eq + Hash>(m: HashMap<K, Val>) -> HashMap<K, JoinResult<Val>> {
            m.map_values(PartialJoin::wrap)
        }

        #[test]
        fn map_values_preserves_keys() {
            let m: HashMap<&str, Val> = [("a", Val(1)), ("b", Val(2))].into();
            let w = wrap_map(m);
            assert_eq!(w.len(), 2);
            assert_eq!(w.get("a"), Some(&Ok(Val(1))));
        }

        #[test]
        fn try_unwrap_ok() {
            let m: HashMap<&str, Val> = [("a", Val(1))].into();
            assert_eq!(wrap_map(m.clone()).try_unwrap().unwrap(), m);
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
                wrap_map(a).join(wrap_map(c)),
                [(0, Err(Conflict::from_values([Val(0), Val(1)])))].into()
            );
        }

        #[test]
        fn try_unwrap_conflict() {
            let a: HashMap<u8, Val> = [(0, Val(0))].into();
            let c: HashMap<u8, Val> = [(0, Val(1))].into();
            assert_eq!(
                wrap_map(a).join(wrap_map(c)).try_unwrap(),
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
            fn wrap_try_unwrap_roundtrip(m in proptest::collection::hash_map(0u8..4, arb_val(), 0..=3)) {
                let len = m.len();
                let wrapped: HashMap<u8, JoinResult<Val>> = m.map_values(PartialJoin::wrap);
                prop_assert_eq!(wrapped.len(), len);
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn is_ok_reflects_content(
                m in proptest::collection::hash_map(0u8..4, arb_join_result(), 0..=3)
            ) {
                let has_conflict = m.values().any(|v| v.is_err());
                prop_assert_eq!(m.is_ok(), !has_conflict);
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
