use std::collections::HashMap;
use std::hash::Hash;

use crate::lattice::join::{Join, JoinMut};
use crate::lattice::partial::{JoinResult, PartialJoin};

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

/// Value stored in a result-domain map.
pub(crate) trait HashMapResultValue: Sized {
    /// Clean value extracted when the result-domain value has no conflicts.
    type Clean;
    /// Returns `true` when this value contains no conflicts.
    fn is_ok(&self) -> bool;
    /// Extract the clean value, or return the original result-domain value.
    fn try_unwrap(self) -> Result<Self::Clean, Self>;
}

impl<V: PartialJoin> HashMapResultValue for JoinResult<V> {
    type Clean = V;

    fn is_ok(&self) -> bool {
        Result::is_ok(self)
    }

    fn try_unwrap(self) -> Result<V, Self> {
        match self {
            Ok(value) => Ok(value),
            Err(conflict) => Err(Err(conflict)),
        }
    }
}

/// Extension trait for result-domain `HashMap` values.
pub(crate) trait ResultHashMapExt: Sized {
    type Key;
    type Value: HashMapResultValue;
    /// Returns `true` if every value in the map is conflict-free.
    fn is_ok(&self) -> bool;
    /// Extract the plain map, or return `self` if any value is a conflict.
    fn try_unwrap(
        self,
    ) -> Result<HashMap<Self::Key, <Self::Value as HashMapResultValue>::Clean>, Self>;
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn unwrap_after_is_ok<V: HashMapResultValue>(value: V) -> V::Clean {
    match value.try_unwrap() {
        Ok(value) => value,
        Err(_) => unreachable!("is_ok() guard verified all entries"),
    }
}

impl<K: Hash + Eq, V: HashMapResultValue> ResultHashMapExt for HashMap<K, V> {
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
        fn map_values_empty() {
            let m: HashMap<&str, Val> = HashMap::new();
            assert!(wrap_map(m).is_empty());
        }

        #[test]
        fn is_ok_true() {
            let m: HashMap<&str, Val> = [("a", Val(1))].into();
            assert!(wrap_map(m).is_ok());
        }

        #[test]
        fn is_ok_false() {
            let mut w: HashMap<&str, JoinResult<Val>> = HashMap::new();
            w.insert("a", Ok(Val(1)));
            w.insert("b", Err(Conflict::from_values([Val(2), Val(3)])));
            assert!(!w.is_ok());
        }

        #[test]
        fn try_unwrap_ok() {
            let m: HashMap<&str, Val> = [("a", Val(1))].into();
            assert_eq!(wrap_map(m.clone()).try_unwrap().unwrap(), m);
        }

        #[test]
        fn try_unwrap_err() {
            let mut w: HashMap<&str, JoinResult<Val>> = HashMap::new();
            w.insert("a", Ok(Val(1)));
            w.insert("b", Err(Conflict::from_values([Val(2), Val(3)])));
            assert!(w.try_unwrap().is_err());
        }

        #[test]
        fn join_disjoint() {
            let a: HashMap<&str, ()> = [("a", ())].into();
            let b: HashMap<&str, ()> = [("b", ())].into();
            let j = a.join(b);
            assert_eq!(j.len(), 2);
        }

        #[test]
        fn join_same() {
            let a: HashMap<&str, ()> = [("a", ())].into();
            assert_eq!(a.clone().join(a.clone()), a);
        }

        #[test]
        fn join_empty() {
            let a: HashMap<&str, ()> = HashMap::new();
            assert_eq!(a.clone().join(a), HashMap::new());
        }

        #[test]
        fn join_mut_in_place() {
            let mut a: HashMap<u8, ()> = [(0, ())].into();
            a.join_mut([(1, ())].into());
            assert_eq!(a.len(), 2);
        }

        #[test]
        fn join_idempotent() {
            let a: HashMap<u8, ()> = [(0, ())].into();
            assert_eq!(a.clone().join(a.clone()), a);

            let b: HashMap<u8, Val> = [(0, Val(1))].into();
            assert_eq!(wrap_map(b.clone()).join(wrap_map(b.clone())), wrap_map(b));
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

        #[test]
        fn result_value_try_unwrap_preserves_conflict() {
            let value: JoinResult<Val> = Err(Conflict::from_values([Val(0), Val(1)]));
            assert_eq!(HashMapResultValue::try_unwrap(value.clone()), Err(value));
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
            fn result_value_try_unwrap_preserves_conflict(a in arb_val(), b in arb_val()) {
                let value: JoinResult<Val> = Err(Conflict::from_values([a, b]));
                prop_assert_eq!(
                    HashMapResultValue::try_unwrap(value.clone()),
                    Err(value),
                );
            }

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
