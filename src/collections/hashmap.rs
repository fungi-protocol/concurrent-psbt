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

// TODO impl Join
//
// TODO tests

// impl<K, V> PartialJoin for HashMap<K, V>
// where
//     K: std::hash::Hash + Eq + Clone,
//     V: PartialJoin + Clone,
// {
//     type Error = HashMap<K, JoinResult<V>>;

//     fn try_join<'a>(self, other: Self) -> JoinResult<Self> {
//         let mut new = HashMap::new();
//         let mut all_ok = true;

//         for (k, b) in self.into_iter().chain(other) {
//             use std::collections::hash_map::Entry::*;

//             match new.entry(k.clone()) {
//                 Occupied(mut entry) => {
//                     let lub: &mut JoinResult<V> = entry.get_mut();

//                     if let Ok(a) = lub {
//                         *lub = a.clone().try_join(b); // FIXME no need to clone

//                         if !lub.is_ok() {
//                             all_ok = false;
//                         }
//                     } else {
//                         panic!("should never happen: Vacant branch only inserts Ok() and there will be at most one key collision per pair of BTrees")
//                     }
//                 }
//                 Vacant(entry) => {
//                     entry.insert(Ok(b.clone()));
//                 }
//             }
//         }

//         if all_ok {
//             Ok(new
//                 .into_iter()
//                 .map(|(k, v)| {
//                     (
//                         k,
//                         v.unwrap_or_else(|_| panic!("verified all nested results are Ok")),
//                     )
//                 })
//                 .collect())
//         } else {
//             Err(new)
//         }
//     }
// }
