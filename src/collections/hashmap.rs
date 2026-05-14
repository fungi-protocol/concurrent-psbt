use std::collections::HashMap;
use std::hash::Hash;

use crate::lattice::join::Join;
use crate::lattice::partial::{JoinResult, PartialJoin};

impl<K, V> Join for HashMap<K, V>
where
    K: Hash + Eq,
    V: Join,
{
    fn join(mut self, other: Self) -> Self {
        for (k, v) in other {
            // TODO is it possible to use .entry() methods to replace the result
            // without unsafe/replace_with crate?
            let lub = match self.remove(&k) {
                Some(prev) => prev.join(v),
                None => v,
            };

            self.insert(k, lub);
        }

        self
    }
}

pub trait HashMapExt {
    type Key;
    type Value: PartialJoin;
    fn wrap(self) -> HashMap<Self::Key, JoinResult<Self::Value>>;
}

impl<K, V> HashMapExt for HashMap<K, V>
where
    K: Hash + Eq,
    V: PartialJoin,
{
    type Key = K;
    type Value = V;

    fn wrap(self) -> HashMap<K, JoinResult<V>> {
        self.into_iter().map(|(k, v)| (k, v.wrap())).collect()
    }
}

pub trait Transpose: Sized {
    type Key;
    type Value: PartialJoin;

    fn try_unwrap(self) -> Result<HashMap<Self::Key, Self::Value>, Self>;

    fn is_ok(&self) -> bool;
}

impl<K, V> Transpose for HashMap<K, JoinResult<V>>
where
    K: Hash + Eq,
    V: PartialJoin,
{
    type Key = K;
    type Value = V;

    fn is_ok(&self) -> bool {
        self.values().all(|v| v.is_ok())
    }

    fn try_unwrap(self) -> Result<HashMap<Self::Key, Self::Value>, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        self.into_iter()
            .map(|(k, v)| v.map(|v| (k, v)))
            .collect::<Result<_, _>>()
            .map_err(|_| panic!("all entries verified to be Ok"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct Val(u8);

    impl PartialJoin for Val {
        type Error = ();
        fn try_join(self, other: Self) -> JoinResult<Self> {
            if self == other { Ok(self) } else { Err(()) }
        }
    }

    #[test]
    fn wrap_preserves_entries() {
        let mut m = HashMap::new();
        m.insert("a", Val(1));
        m.insert("b", Val(2));

        let wrapped = m.clone().wrap();
        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped.get("a"), Some(&Ok(Val(1))));
    }

    #[test]
    fn wrap_empty_is_empty() {
        let m: HashMap<&str, Val> = HashMap::new();
        assert!(m.wrap().is_empty());
    }

    #[test]
    fn is_ok_true_when_all_ok() {
        let mut m = HashMap::new();
        m.insert("a", Val(1));
        let wrapped = m.wrap();
        assert!(wrapped.is_ok());
    }

    #[test]
    fn is_ok_false_when_any_err() {
        let mut wrapped: HashMap<&str, JoinResult<Val>> = HashMap::new();
        wrapped.insert("a", Ok(Val(1)));
        wrapped.insert("b", Err(()));
        assert!(!wrapped.is_ok());
    }

    #[test]
    fn try_unwrap_succeeds_when_all_ok() {
        let mut m = HashMap::new();
        m.insert("a", Val(1));
        let unwrapped = m.clone().wrap().try_unwrap().unwrap();
        assert_eq!(unwrapped, m);
    }

    #[test]
    fn try_unwrap_fails_when_any_err() {
        let mut wrapped: HashMap<&str, JoinResult<Val>> = HashMap::new();
        wrapped.insert("a", Ok(Val(1)));
        wrapped.insert("b", Err(()));
        assert!(wrapped.try_unwrap().is_err());
    }
}
