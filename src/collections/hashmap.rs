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
    fn into_ok(self) -> HashMap<Self::Key, JoinResult<Self::Value>>;
}

impl<K, V> HashMapExt for HashMap<K, V>
where
    K: Hash + Eq,
    V: PartialJoin,
{
    type Key = K;
    type Value = V;

    fn into_ok(self) -> HashMap<K, JoinResult<V>> {
        self.into_iter().map(|(k, v)| (k, v.into_ok())).collect()
    }
}

pub trait Transpose: Sized {
    type Key;
    type Value: PartialJoin;

    fn transpose(self) -> Result<HashMap<Self::Key, Self::Value>, Self>;

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

    fn transpose(self) -> Result<HashMap<Self::Key, Self::Value>, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        self.into_iter()
            .map(|(k, v)| v.map(|v| (k, v)))
            .collect::<Result<_, _>>()
            .map_err(|_| panic!("all entries verified to be Ok"))
    }
}

