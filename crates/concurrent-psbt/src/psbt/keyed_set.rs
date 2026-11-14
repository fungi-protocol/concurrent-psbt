//! Keyed sets of PSBT entries, some of which may be frozen.
//!
//! A [`KeyedSet`] holds entries with distinct keys: inputs keyed by their outpoints, or outputs
//! keyed by their unique IDs. A [`ResultKeyedSet`] is the join of any number of [`Freezable`]
//! keyed sets, and joins the entries that share a key.
//!
//! A PSBT's inputs or outputs are [`Frozen`](Freezable::Frozen) once they are no longer
//! modifiable. The keys of a frozen set are fixed, so no input or output can be added or removed,
//! but its entries can still be joined with entries that add information to them. A
//! [`ResultKeyedSet`] keeps every distinct frozen key set it joins, which keeps the join
//! idempotent, commutative and associative. It is clean only if the keys of its entries match
//! the frozen key sets.

use std::collections::hash_map::{Entry, IntoValues};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::collections::hashmap::{HashMapExt, ResultHashMapExt};
use crate::lattice::join::{Join, JoinMut};
use crate::lattice::lift::ResultValue;
use crate::lattice::partial::JoinResult;

/// Set of entries with distinct keys.
#[derive(Debug, Clone)]
pub(crate) struct KeyedSet<K, V>(HashMap<K, V>);

// Not derived, since the derive would require `K: Default` and `V: Default`.
impl<K, V> Default for KeyedSet<K, V> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<K: Hash + Eq, V: PartialEq> PartialEq for KeyedSet<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Hash + Eq, V> KeyedSet<K, V> {
    /// Collect keyed entries into a set.
    ///
    /// # Errors
    /// Returns the first error among `entries`, or `duplicate` of the first
    /// key that repeats, whichever comes first.
    pub(crate) fn try_from_entries<E>(
        entries: impl IntoIterator<Item = Result<(K, V), E>>,
        duplicate: impl FnOnce(&K) -> E,
    ) -> Result<Self, E> {
        let mut map = HashMap::new();
        for entry in entries {
            let (key, value) = entry?;
            match map.entry(key) {
                Entry::Occupied(entry) => return Err(duplicate(entry.key())),
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
            }
        }
        Ok(Self(map))
    }

    /// Return the number of entries in the set.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Return `true` if the set holds no entries.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return `true` if the set holds an entry with this key.
    pub(crate) fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }
}

impl<K, V> IntoIterator for KeyedSet<K, V> {
    type Item = V;
    type IntoIter = IntoValues<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_values()
    }
}

/// A [`KeyedSet`] whose keys may be frozen.
///
/// A PSBT's inputs or outputs are `Frozen` when the matching bit of `tx_modifiable_flags` is
/// clear.
#[derive(Debug, Clone)]
pub(crate) enum Freezable<K, V> {
    /// Modifiable sets can be joined, unioning their keys and joining values
    /// which share the same key.
    Modifiable(KeyedSet<K, V>),

    /// A frozen set's keys are fixed. In the result domain, it can be joined
    /// only with frozen sets that freeze the same keys, or modifiable sets
    /// whose keys are a subset of the frozen keys. In both cases the result
    /// is frozen.
    Frozen(KeyedSet<K, V>),
}

// Not derived, since the derive would require `K: Default` and `V: Default`.
impl<K, V> Default for Freezable<K, V> {
    fn default() -> Self {
        Self::Modifiable(KeyedSet::default())
    }
}

/// Equal if both are frozen or both are modifiable, and their entries are
/// equal.
impl<K: Hash + Eq, V: PartialEq> PartialEq for Freezable<K, V> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Modifiable(a), Self::Modifiable(b)) | (Self::Frozen(a), Self::Frozen(b)) => {
                a == b
            }
            _ => false,
        }
    }
}

impl<K, V> Freezable<K, V> {
    /// Returns the underlying KeyedSet
    pub(crate) fn set(&self) -> &KeyedSet<K, V> {
        match self {
            Self::Modifiable(set) | Self::Frozen(set) => set,
        }
    }

    fn into_set(self) -> KeyedSet<K, V> {
        match self {
            Self::Modifiable(set) | Self::Frozen(set) => set,
        }
    }

    /// Freeze the set, fixing its keys.
    pub(crate) fn freeze(self) -> Self {
        Self::Frozen(self.into_set())
    }
}

impl<K: Hash + Eq + Clone, V> Freezable<K, V> {
    /// Lift into the result domain, wrapping each entry.
    pub(crate) fn wrap<W>(self, wrap: impl FnMut(V) -> W) -> ResultKeyedSet<K, W> {
        let frozen = match &self {
            Self::Modifiable(_) => Vec::new(),
            Self::Frozen(set) => vec![set.0.keys().cloned().collect()],
        };
        ResultKeyedSet {
            entries: KeyedSet(self.into_set().0.map_values(wrap)),
            frozen,
        }
    }
}

impl<K, V> IntoIterator for Freezable<K, V> {
    type Item = V;
    type IntoIter = IntoValues<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_set().into_iter()
    }
}

/// The join of any number of [`Freezable`] keyed sets, in the result domain.
///
/// Entries with the same key are joined, and the rest are unioned. A frozen
/// set's keys are fixed, so each distinct set of frozen keys is also kept.
/// The join is clean only if there is at most one distinct frozen key set and
/// it equals the keys of the entries. So a frozen set joins cleanly with
/// frozen sets of the same keys, and with modifiable sets whose keys it
/// already has.
///
/// The default, the empty modifiable set, is the identity of the join.
#[derive(Debug, Clone)]
pub(crate) struct ResultKeyedSet<K, V> {
    /// Every joined set's entries, with those sharing a key joined into one.
    entries: KeyedSet<K, V>,

    /// The distinct key sets of the frozen sets, in the order first joined.
    frozen: Vec<HashSet<K>>,
}

// Not derived, since the derive would require `K: Default` and `V: Default`.
impl<K, V> Default for ResultKeyedSet<K, V> {
    fn default() -> Self {
        Self {
            entries: KeyedSet::default(),
            frozen: Vec::new(),
        }
    }
}

impl<K: Hash + Eq, V> ResultKeyedSet<K, V> {
    /// Return the number of entries, or the conflicting sizes of the frozen
    /// key sets and the entries.
    ///
    /// Every frozen key set is a subset of the keys of the entries, so the
    /// sizes are all equal only if the keys agree with the frozen sets.
    pub(crate) fn len(&self) -> JoinResult<usize> {
        self.frozen
            .iter()
            .map(|keys| Ok(keys.len()))
            .fold(Ok(self.entries.len()), Join::join)
    }

    /// Return `true` if the set holds no entries.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return `true` if there is at most one frozen key set and it holds the
    /// key of every entry, which is when [`len`](Self::len) is `Ok`.
    fn keys_agree(&self) -> bool {
        self.len().is_ok()
    }

    fn add_frozen(&mut self, keys: HashSet<K>) {
        if !self.frozen.contains(&keys) {
            self.frozen.push(keys);
        }
    }
}

impl<K: Hash + Eq, V: Join> JoinMut for ResultKeyedSet<K, V> {
    fn join_mut(&mut self, other: Self) {
        self.entries.0.join_mut(other.entries.0);
        for keys in other.frozen {
            self.add_frozen(keys);
        }
    }
}

impl<K: Hash + Eq, V: ResultValue> ResultKeyedSet<K, V> {
    /// Return `true` if the keys agree with the frozen sets and every entry
    /// is conflict-free.
    pub(crate) fn is_ok(&self) -> bool {
        self.keys_agree() && self.entries.0.is_ok()
    }

    /// Extract the clean set if [`is_ok`](Self::is_ok), frozen if any set
    /// joined into it was.
    ///
    /// # Errors
    /// Returns `Err(self)` if the keys disagree with the frozen sets or any
    /// entry is a conflict.
    pub(crate) fn try_unwrap(self) -> Result<Freezable<K, V::Clean>, Self> {
        if !self.keys_agree() {
            return Err(self);
        }
        let Self { entries, frozen } = self;
        match entries.0.try_unwrap() {
            Ok(entries) if frozen.is_empty() => Ok(Freezable::Modifiable(KeyedSet(entries))),
            Ok(entries) => Ok(Freezable::Frozen(KeyedSet(entries))),
            Err(entries) => Err(Self {
                entries: KeyedSet(entries),
                frozen,
            }),
        }
    }
}

/// Equal if the entries are equal and the sets of frozen keys are equal, in
/// any order.
impl<K: Hash + Eq, V: PartialEq> PartialEq for ResultKeyedSet<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
            && self.frozen.len() == other.frozen.len()
            && self.frozen.iter().all(|keys| other.frozen.contains(keys))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::partial::Conflict;

    type Set = ResultKeyedSet<u8, JoinResult<u8>>;

    /// `set`, frozen if `frozen`.
    fn tagged(set: KeyedSet<u8, u8>, frozen: bool) -> Freezable<u8, u8> {
        let set = Freezable::Modifiable(set);
        if frozen { set.freeze() } else { set }
    }

    /// A clean set of these entries, frozen if `frozen`.
    fn set_of(entries: &[(u8, u8)], frozen: bool) -> Set {
        tagged(KeyedSet(entries.iter().copied().collect()), frozen).wrap(Ok)
    }

    /// A clean set whose entries hold their own key, so entries never conflict
    /// and only the keys decide whether a join is clean.
    fn set(keys: &[u8], frozen: bool) -> Set {
        set_of(
            &keys.iter().map(|&key| (key, key)).collect::<Vec<_>>(),
            frozen,
        )
    }

    /// Collect `entries`, reporting a repeated key as the key plus 100.
    fn collect(entries: Vec<Result<(u8, u8), u8>>) -> Result<KeyedSet<u8, u8>, u8> {
        KeyedSet::try_from_entries(entries, |&key| key + 100)
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn try_from_entries_stops_at_the_first_error_or_repeat() {
            assert_eq!(
                collect(vec![Ok((1, 1)), Ok((2, 2))]).map(|set| set.len()),
                Ok(2)
            );
            assert_eq!(collect(vec![Ok((1, 1)), Err(7), Ok((1, 2))]), Err(7));
            assert_eq!(collect(vec![Ok((1, 1)), Ok((1, 2)), Err(7)]), Err(101));
        }

        #[test]
        fn clean_set_holds_its_entries() {
            assert!(KeyedSet::<u8, u8>::default().is_empty());

            let set = collect(vec![Ok((1, 10)), Ok((2, 20))]).unwrap();
            assert!(!set.is_empty());
            assert!(set.contains_key(&1));
            assert!(!set.contains_key(&3));
            let mut values: Vec<_> = set.clone().into_iter().collect();
            values.sort_unstable();
            assert_eq!(values, [10, 20]);

            let wrapped: Set = Freezable::Modifiable(set.clone()).wrap(Ok);
            assert!(wrapped.is_ok());
            assert_eq!(wrapped.try_unwrap(), Ok(Freezable::Modifiable(set)));
        }

        #[test]
        fn frozen_set_joins_modifiable_sets_whose_keys_it_holds() {
            let held = set(&[1, 2], true).join(set(&[1], false));
            assert!(matches!(held.try_unwrap(), Ok(Freezable::Frozen(set)) if set.len() == 2));

            let extra = set(&[1], true).join(set(&[1, 2], false));
            assert_eq!(extra.len(), Err(Conflict::from_values([2, 1])));
            assert!(!extra.is_ok());
            assert!(extra.try_unwrap().is_err());
        }

        #[test]
        fn frozen_sets_with_the_same_keys_join_entry_by_entry() {
            let same = set_of(&[(1, 1), (2, 2)], true).join(set_of(&[(2, 2), (1, 1)], true));
            let same = same.try_unwrap().unwrap();
            assert!(matches!(same, Freezable::Frozen(_)));
            assert_eq!(same.set().len(), 2);

            let different = set(&[1], true).join(set(&[2], true));
            assert_eq!(different.len(), Err(Conflict::from_values([2, 1])));
            assert!(different.try_unwrap().is_err());

            for frozen in [false, true] {
                let conflicting = set_of(&[(1, 1)], frozen).join(set_of(&[(1, 2)], frozen));
                assert!(!conflicting.is_ok());
                assert_eq!(conflicting.clone().try_unwrap(), Err(conflicting));
            }
        }

        #[test]
        fn empty_set_unwraps_empty() {
            let empty = Set::default();
            assert_eq!(Freezable::default().wrap(Ok), empty);
            assert!(empty.is_empty());
            assert_eq!(empty.try_unwrap(), Ok(Freezable::default()));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;
        use std::collections::BTreeSet;

        fn arb_keys() -> impl Strategy<Value = Vec<u8>> {
            proptest::collection::vec(0u8..4, 0..=3)
        }

        /// Join of small sets, some frozen, over a small key domain so frozen
        /// sets sometimes have the same keys.
        fn arb_set() -> impl Strategy<Value = Set> {
            proptest::collection::vec((arb_keys(), any::<bool>()), 0..=4).prop_map(|sets| {
                sets.iter()
                    .map(|(keys, frozen)| set(keys, *frozen))
                    .fold(Set::default(), Join::join)
            })
        }

        assert_join_laws!(arb_set());

        proptest! {
            #[test]
            fn try_from_entries_stops_at_the_first_failure(
                keys in proptest::collection::hash_set(0u8..8, 1..=4),
                repeat in any::<proptest::sample::Index>(),
                error in proptest::option::of(200u8..),
                tail in proptest::collection::vec(
                    prop_oneof![(0u8..8, any::<u8>()).prop_map(Ok), (200u8..).prop_map(Err)],
                    0..=3,
                ),
            ) {
                let keys: Vec<u8> = keys.into_iter().collect();
                let distinct: Vec<_> = keys.iter().map(|&key| Ok((key, key))).collect();
                let set = collect(distinct.clone()).expect("distinct keys");
                prop_assert_eq!(set.len(), keys.len());
                prop_assert!(keys.iter().all(|key| set.contains_key(key)));

                // A failure after distinct keys: an error, or a repeated key.
                let (failure, expected) = match error {
                    Some(error) => (Err(error), error),
                    None => {
                        let key = keys[repeat.index(keys.len())];
                        (Ok((key, 0)), key + 100)
                    }
                };
                let entries = distinct.into_iter().chain([failure]).chain(tail).collect();
                prop_assert_eq!(collect(entries), Err(expected));
            }

            #[test]
            fn wrap_try_unwrap_roundtrip(
                entries in proptest::collection::hash_map(0u8..8, any::<u8>(), 0..=4),
                frozen in any::<bool>(),
            ) {
                let set = tagged(KeyedSet(entries.clone()), frozen);
                let mut values: Vec<_> = set.clone().into_iter().collect();
                let mut expected: Vec<_> = entries.into_values().collect();
                values.sort_unstable();
                expected.sort_unstable();
                prop_assert_eq!(values, expected);

                let wrapped: Set = set.clone().wrap(Ok);
                prop_assert!(wrapped.is_ok());
                prop_assert_eq!(wrapped.try_unwrap(), Ok(set));
            }

            #[test]
            fn keyed_sets_are_equal_iff_their_tags_and_entries_are(
                a in proptest::collection::hash_map(0u8..2, 0u8..2, 0..=2),
                a_frozen in any::<bool>(),
                b in proptest::collection::hash_map(0u8..2, 0u8..2, 0..=2),
                b_frozen in any::<bool>(),
            ) {
                prop_assert_eq!(
                    tagged(KeyedSet(a.clone()), a_frozen) == tagged(KeyedSet(b.clone()), b_frozen),
                    a_frozen == b_frozen && a == b
                );
            }

            #[test]
            fn one_part_unwraps_iff_its_entries_are_clean(
                keys in arb_keys(),
                frozen in any::<bool>(),
                a in 0u8..2,
                b in 0u8..2,
            ) {
                let part = |value: u8| {
                    set_of(&keys.iter().map(|&key| (key, value)).collect::<Vec<_>>(), frozen)
                };
                let joined = part(a).join(part(b));
                let clean = keys.is_empty() || a == b;
                prop_assert_eq!(joined.is_ok(), clean);
                match joined.clone().try_unwrap() {
                    Ok(unwrapped) => prop_assert!(
                        clean
                            && Ok(unwrapped.set().len()) == joined.len()
                            && matches!(unwrapped, Freezable::Frozen(_)) == frozen
                    ),
                    Err(unchanged) => prop_assert!(!clean && unchanged == joined),
                }
            }

            #[test]
            fn frozen_sets_join_only_sets_whose_keys_they_hold(
                a in arb_keys(),
                a_frozen in any::<bool>(),
                b in arb_keys(),
                b_frozen in any::<bool>(),
            ) {
                let joined = set(&a, a_frozen).join(set(&b, b_frozen));
                let (a_keys, b_keys): (HashSet<_>, HashSet<_>) =
                    (a.iter().collect(), b.iter().collect());
                let expected = match (a_frozen, b_frozen) {
                    (false, false) => true,
                    (true, true) => a_keys == b_keys,
                    (true, false) => b_keys.is_subset(&a_keys),
                    (false, true) => a_keys.is_subset(&b_keys),
                };
                prop_assert_eq!(joined.is_ok(), expected);
                let union = a.iter().chain(&b).collect::<HashSet<_>>().len();
                let sizes = [
                    Some(union),
                    a_frozen.then_some(a_keys.len()),
                    b_frozen.then_some(b_keys.len()),
                ];
                prop_assert_eq!(
                    joined.len(),
                    if expected {
                        Ok(union)
                    } else {
                        Err(Conflict::from_values(sizes.into_iter().flatten()))
                    }
                );
                prop_assert_eq!(joined.is_empty(), a.is_empty() && b.is_empty());
                prop_assert_eq!(joined.clone().try_unwrap().is_ok(), joined.is_ok());
            }

            #[test]
            fn sets_are_equal_iff_their_keys_are(a in arb_set(), b in arb_set()) {
                // Entries of `arb_set` hold their own key, so a set is
                // determined by its keys and its sets of frozen keys.
                let keys = |set: &Set| {
                    let frozen = set.frozen.iter().map(|keys| keys.iter().copied().collect());
                    (
                        set.entries.0.keys().copied().collect::<BTreeSet<_>>(),
                        frozen.collect::<BTreeSet<BTreeSet<_>>>(),
                    )
                };
                prop_assert_eq!(a == b, keys(&a) == keys(&b));
            }

            #[test]
            fn default_is_the_join_identity(a in arb_set()) {
                prop_assert_eq!(a.clone().join(Set::default()), a);
            }
        }
    }
}
