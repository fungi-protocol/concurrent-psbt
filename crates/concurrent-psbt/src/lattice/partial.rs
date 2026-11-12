//! Some types have a natural merge operation but that isn't defined for all pairs of values.
//! [`PartialJoin::try_join`] captures this.
//!
//! It returns [`JoinResult<V>`], an alias for `Result<V, Conflict<V>>`, where [`Conflict<V>`],
//! collects the maximal values that couldn't be merged.
//!
//! [`JoinResult<V>`] implements [`Join`], which is an infallible join operation (the free
//! semilattice over `V`, more or less), in terms of [`Conflict<V>`]'s [`Join`] implementation.

use crate::lattice::join::{Join, JoinMut};

/// Fallible join for scalar value types where merge may produce a conflict.
///
/// Containers should not implement this, instead they should implement [`Join`] by wrapping
/// contained values in [`JoinResult`].
pub trait PartialJoin: Sized + PartialEq + Clone {
    /// Attempt to join two values.
    ///
    /// Returns `Ok` with the greater operand if the two are comparable, or `Err(Conflict)`
    /// containing both values if they aren't. The order must be a partial order, antisymmetric
    /// with respect to `==` (so `try_join(a, a)` is `Ok(a)`), and two values with a common upper
    /// bound must be comparable. Equality joins (only equal values join) satisfy this, and so
    /// does absence below any present value.
    ///
    /// Under these rules a conflict's maximal values determine it, which is what [`Conflict`]
    /// stores, so [`JoinResult`]'s join is a lattice join under `==`. A `try_join` that returns
    /// anything else, such as a merge of two values, makes that join a lattice join only up to
    /// the identifications its merges make, and `==` then tells equivalent results apart.
    ///
    /// Although `Conflict<V>` has set semantics (equality treats it as an unordered collection),
    /// insertion order is preserved during iteration: when two `Ok` values are joined and produce
    /// a binary conflict, the first value is the left operand and the second is the right.
    ///
    /// Provenance is only reliable when both operands were `Ok` before the join. When an existing
    /// `Conflict` is joined with another value, the value replaces the entry it is above, or is
    /// appended if it isn't comparable to any, so a multi-way fold does not preserve per-operand
    /// attribution.
    fn try_join(self, other: Self) -> JoinResult<Self>;

    /// Lift a value into the result domain as `Ok(self)`.
    fn wrap(self) -> JoinResult<Self> {
        Ok(self)
    }
}

/// Result of a fallible join: `Ok(v)` where `v` is the least upper bound if one exists,
/// `Err(Conflict)` otherwise.
///
/// `JoinResult<V>` implements [`Join`].
pub type JoinResult<V> = Result<V, Conflict<V>>;

impl<V> Join for JoinResult<V>
where
    V: PartialJoin,
{
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Ok(a), Ok(b)) => a.try_join(b),
            (Err(a), Err(b)) => Err(a.join(b)),
            (Err(a), Ok(b)) => Err(a.join(Conflict::singleton(b))),
            (Ok(a), Err(b)) => Err(Conflict::singleton(a).join(b)),
        }
    }
}

/// A set of conflicting values, produced when a join for those values does not exist.
///
/// When two participants set the same field to different values, neither can be chosen without
/// losing information. `Conflict` preserves the set of distinct conflicting values allowing the
/// caller to inspect or resolve the disagreement.
///
/// The inner values are [`PartialJoin`] values, none comparable to another: a value below an
/// existing one (including an equal one) is omitted, and a value above an existing one replaces
/// it.
#[derive(Debug, Clone)]
pub struct Conflict<V: PartialJoin>(Vec<V>);

impl<V: PartialJoin> Conflict<V> {
    /// Wrap a single value into a conflict.
    fn singleton(v: V) -> Self {
        Self(vec![v])
    }

    /// Build from an iterator, deduplicating values. An empty iterator yields an empty conflict.
    /// The distinct values must be pairwise incomparable, as in the `Err` of
    /// [`PartialJoin::try_join`]; this can't be checked here, because `try_join` itself builds
    /// its conflicts with this function.
    pub fn from_values(iter: impl IntoIterator<Item = V>) -> Self {
        let mut vals = Vec::new();
        for v in iter {
            if !vals.contains(&v) {
                vals.push(v);
            }
        }
        Self(vals)
    }

    /// Add a value, keeping only the maximal values. The first comparable entry decides: a value
    /// below it adds nothing, and a value above it is comparable to no other entry, since that
    /// entry would then share an upper bound with this one or lie below it.
    fn insert(&mut self, value: V) {
        for entry in &mut self.0 {
            if let Ok(lub) = entry.clone().try_join(value.clone()) {
                *entry = lub;
                return;
            }
        }
        self.0.push(value);
    }

    /// Iterates over references to the conflicted values.
    fn iter(&self) -> std::slice::Iter<'_, V> {
        self.0.iter()
    }

    /// Number of distinct conflicting values.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<V: PartialJoin> JoinMut for Conflict<V> {
    fn join_mut(&mut self, other: Self) {
        for value in other.0 {
            self.insert(value);
        }
    }
}

/// Set equality: same elements, regardless of order.
///
/// Relies on the invariant that `Conflict::insert` maintains and `Conflict::from_values`
/// requires: the values in `self.0` are pairwise incomparable, so in particular distinct.
/// Given no duplicates, same-length + subset → set equality. $O(n^2)$ but
/// $n$ is tiny in practice (typically 2).
impl<V: PartialJoin> PartialEq for Conflict<V> {
    fn eq(&self, other: &Self) -> bool {
        debug_assert!(
            !has_duplicates(&self.0) && !has_duplicates(&other.0),
            "Conflict invariant violated: contains duplicate values"
        );
        self.0.len() == other.0.len() && self.iter().all(|v| other.0.contains(v))
    }
}

/// Check for duplicates in a small slice. $O(n^2)$.
#[cfg_attr(coverage_nightly, coverage(off))]
fn has_duplicates<V: PartialEq>(vals: &[V]) -> bool {
    let mut rest = vals;
    while let Some((head, tail)) = rest.split_first() {
        if tail.contains(head) {
            return true;
        }
        rest = tail;
    }
    false
}

impl<V: PartialJoin + Eq> Eq for Conflict<V> {}

impl<V: PartialJoin> IntoIterator for Conflict<V> {
    type Item = V;
    type IntoIter = std::vec::IntoIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, V: PartialJoin> IntoIterator for &'a Conflict<V> {
    type Item = &'a V;
    type IntoIter = std::slice::Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Proptest macros for verifying `PartialJoin` laws and the `JoinResult` completion. Available
/// when `prop-tests` feature is enabled.
///
/// # `assert_partial_join_laws!($arb_clean, $arb_result)`
///
/// Generates:
/// - `try_join` laws on clean values: idempotent, commutative, associative (via wrap/join)
/// - `JoinResult` laws on pre-built Ok/Err values: idempotent, commutative, associative
/// - `wrap_roundtrip`: wrapping a clean value always produces `Ok`
///
/// The `$arb_clean` strategy produces clean (pre-join) values.
/// The `$arb_result` strategy produces `JoinResult` values (both Ok and Err).
#[cfg(feature = "prop-tests")]
#[allow(unused_macros)] // used by downstream commits (collection modules, domain types)
macro_rules! assert_partial_join_laws {
    ($arb_clean:expr, $arb_result:expr) => {
        proptest! {
            #[test]
            fn try_join_idempotent(a in $arb_clean) {
                prop_assert_eq!(a.clone().try_join(a.clone()), Ok(a));
            }

            #[test]
            fn try_join_commutative(a in $arb_clean, b in $arb_clean) {
                prop_assert_eq!(a.clone().try_join(b.clone()), b.try_join(a));
            }

            #[test]
            fn try_join_associative(a in $arb_clean, b in $arb_clean, c in $arb_clean) {
                prop_assert_eq!(
                    a.clone().try_join(b.clone()).join(c.clone().wrap()),
                    a.wrap().join(b.try_join(c)),
                );
            }

            #[test]
            fn wrap_roundtrip(a in $arb_clean) {
                let wrapped = a.clone().wrap();
                prop_assert!(wrapped.is_ok());
                prop_assert_eq!(wrapped.expect("should be Ok"), a);
            }
        }

        mod join_result {
            use super::*;
            assert_join_laws!($arb_result);
        }
    };
}

#[cfg(all(test, any(feature = "unit-tests", feature = "prop-tests")))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
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
        fn result_ok_ok_different_produces_conflict() {
            assert_eq!(
                Foo(0).wrap().join(Foo(1).wrap()),
                Err(Conflict(vec![Foo(0), Foo(1)])),
            );
        }

        #[test]
        fn result_err_ok_absorbs() {
            let err: JoinResult<Foo> = Err(Conflict(vec![Foo(0), Foo(1)]));
            assert_eq!(
                err.join(Ok(Foo(2))),
                Err(Conflict(vec![Foo(0), Foo(1), Foo(2)]))
            );
        }

        #[test]
        fn result_ok_err_absorbs() {
            let err: JoinResult<Foo> = Err(Conflict(vec![Foo(0), Foo(1)]));
            assert_eq!(
                Ok(Foo(2)).join(err),
                Err(Conflict(vec![Foo(2), Foo(0), Foo(1)]))
            );
        }

        #[test]
        fn result_err_err_merges_conflicts() {
            let a: JoinResult<Foo> = Err(Conflict(vec![Foo(0), Foo(1)]));
            let b: JoinResult<Foo> = Err(Conflict(vec![Foo(1), Foo(2)]));
            assert_eq!(a.join(b), Err(Conflict(vec![Foo(0), Foo(1), Foo(2)])));
        }

        #[test]
        fn different_conflicts_are_not_equal() {
            let a = Conflict::from_values([Foo(0), Foo(1)]);
            let b = Conflict::from_values([Foo(0), Foo(2)]);
            assert_ne!(a, b);
        }

        #[test]
        fn different_length_conflicts_are_not_equal() {
            let a = Conflict::from_values([Foo(0), Foo(1)]);
            let b = Conflict::from_values([Foo(0), Foo(1), Foo(2)]);
            assert_ne!(a, b);
        }

        #[test]
        fn conflict_from_equal_pair_deduplicates() {
            let c = Conflict::from_values([Foo(0), Foo(0)]);
            assert_eq!(c.len(), 1);
        }

        #[test]
        fn duplicate_detector_distinguishes_repeated_values() {
            assert!(has_duplicates(&[Foo(0), Foo(0)]));
            assert!(!has_duplicates(&[Foo(0), Foo(1)]));
        }

        #[test]
        fn len_distinct_values() {
            let c = Conflict::from_values([Foo(0), Foo(1)]);
            assert_eq!(c.len(), 2);
        }

        #[test]
        fn is_empty() {
            assert!(Conflict::<Foo>::from_values([]).is_empty());
            assert!(!Conflict::from_values([Foo(0)]).is_empty());
        }

        #[test]
        fn into_iter_yields_elements() {
            let c = Conflict::from_values([Foo(0), Foo(1)]);
            let v: Vec<_> = c.into_iter().collect();
            assert_eq!(v, vec![Foo(0), Foo(1)]);
        }

        #[test]
        fn borrowed_iteration() {
            let c = Conflict::from_values([Foo(0), Foo(1)]);
            let v: Vec<_> = (&c).into_iter().collect();
            assert_eq!(v, vec![&Foo(0), &Foo(1)]);
        }

        #[test]
        fn conflict_preserves_order() {
            let x = Conflict::from_values([Foo(0)]);
            let y = Conflict::from_values([Foo(1)]);

            let xy = x.clone().join(y.clone());
            let yx = y.clone().join(x.clone());

            assert_eq!(xy, yx);
            assert_ne!(xy.iter().collect::<Vec<_>>(), yx.iter().collect::<Vec<_>>());
            assert_ne!(
                xy.into_iter().collect::<Vec<_>>(),
                yx.into_iter().collect::<Vec<_>>()
            );
        }
    }

    // Small domain (4 values): high collision rate exercises the
    // equal-value (Ok) path while still covering the conflict (Err) path.
    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        fn arb_foo() -> impl Strategy<Value = Foo> {
            (0u8..4).prop_map(Foo)
        }

        /// Generate a JoinResult<Foo> which is either Ok(v) or Err(Conflict) with 1–5 values.
        /// Exercises all arms of JoinResult::join.
        fn arb_join_result() -> impl Strategy<Value = JoinResult<Foo>> {
            prop_oneof![
                arb_foo().prop_map(Ok),
                proptest::collection::vec(arb_foo(), 1..=5)
                    .prop_map(|v| Err(Conflict::from_values(v))),
            ]
        }

        assert_partial_join_laws!(arb_foo(), arb_join_result());

        /// The prefix order on bit strings of length at most 2: a tree, so two strings with a
        /// common extension are comparable, and `try_join` returns an operand without being an
        /// equality join. It covers both shapes beyond equality:
        /// - absence below every value (the empty string): before conflicts kept only their
        ///   maximal values, `(ε ⊔ 0) ⊔ 1` was `{0, 1}` but `ε ⊔ (0 ⊔ 1)` was `{ε, 0, 1}`;
        /// - refinement inside a conflict: `{0, 1} ⊔ 01` must be `{01, 1}`.
        mod prefix_order {
            use super::*;

            #[derive(Debug, Clone, PartialEq)]
            struct Bits(Vec<bool>);

            impl PartialJoin for Bits {
                fn try_join(self, other: Self) -> JoinResult<Self> {
                    if other.0.starts_with(&self.0) {
                        Ok(other)
                    } else if self.0.starts_with(&other.0) {
                        Ok(self)
                    } else {
                        Err(Conflict::from_values([self, other]))
                    }
                }
            }

            fn arb_bits() -> impl Strategy<Value = Bits> {
                proptest::collection::vec(any::<bool>(), 0..=2).prop_map(Bits)
            }

            /// `Ok(v)`, or a conflict reached by joining values into the empty one, since
            /// `from_values` needs incomparable values.
            fn arb_bits_result() -> impl Strategy<Value = JoinResult<Bits>> {
                prop_oneof![
                    arb_bits().prop_map(Ok),
                    proptest::collection::vec(arb_bits(), 0..=4).prop_map(|vs| {
                        let mut acc = Err(Conflict::from_values([]));
                        for v in vs {
                            acc = acc.join(Ok(v));
                        }
                        acc
                    }),
                ]
            }

            assert_partial_join_laws!(arb_bits(), arb_bits_result());
        }

        proptest! {
            #[test]
            fn conflict_into_iter_roundtrips(a in arb_foo(), b in arb_foo()) {
                let c = Conflict::from_values([a, b]);
                let vals = c.clone().into_iter();
                let rebuilt = Conflict::from_values(vals);
                prop_assert_eq!(c, rebuilt);
            }

            #[test]
            fn duplicate_detector_distinguishes_repeated_values(a in arb_foo(), b in arb_foo()) {
                prop_assert!(has_duplicates(&[a, a]));
                if a != b {
                    prop_assert!(!has_duplicates(&[a, b]));
                }
            }

            #[test]
            fn conflict_set_equality_ignores_order(a in arb_foo(), b in arb_foo()) {
                let ab = Conflict::from_values([a, b]);
                let ba = Conflict::from_values([b, a]);
                prop_assert_eq!(ab, ba);
            }

            #[test]
            fn borrowed_iter_matches_owned(a in arb_foo(), b in arb_foo()) {
                let c = Conflict::from_values([a, b]);
                let owned: Vec<_> = c.clone().into_iter().collect();
                let borrowed: Vec<_> = (&c).into_iter().copied().collect();
                prop_assert_eq!(owned, borrowed);
            }

            #[test]
            fn len_matches_iter_count(a in arb_foo(), b in arb_foo(), c in arb_foo()) {
                let conflict = Conflict::from_values([a, b, c]);
                prop_assert_eq!(conflict.len(), conflict.into_iter().count());
            }

            #[test]
            fn non_empty_after_construction(a in arb_foo()) {
                let c = Conflict::from_values([a]);
                prop_assert!(!c.is_empty());
            }

            #[test]
            fn empty_from_empty(a in arb_foo()) {
                let _ = a; // use the parameter to satisfy proptest
                let c = Conflict::<Foo>::from_values([]);
                prop_assert!(c.is_empty());
                prop_assert_eq!(c.len(), 0);
            }

            #[test]
            fn conflict_different_lengths_not_equal(
                a in arb_foo(),
                b in arb_foo(),
                c in arb_foo(),
            ) {
                let short = Conflict::from_values([a, b]);
                let long = Conflict::from_values([a, b, c]);
                if short.len() != long.len() {
                    prop_assert_ne!(short, long);
                }
            }

            /// Non-trivial LUB: join of two distinct Ok values produces a
            /// Conflict that is strictly above both (neither equal to a nor b).
            #[test]
            fn join_distinct_produces_lub(a in arb_foo(), b in arb_foo()) {
                prop_assume!(a != b);
                let ra: JoinResult<Foo> = Ok(a);
                let rb: JoinResult<Foo> = Ok(b);
                let joined = ra.clone().join(rb.clone());
                // The join of two distinct Ok values is Err(Conflict)
                prop_assert!(joined.is_err(), "distinct values must conflict");
                // The result is strictly above both inputs (not equal to either)
                prop_assert_ne!(joined.clone(), ra.clone());
                prop_assert_ne!(joined.clone(), rb.clone());
                // Joining with either original is idempotent (the LUB absorbs them)
                prop_assert_eq!(joined.clone().join(ra), joined.clone());
                prop_assert_eq!(joined.clone().join(rb), joined);
            }
        }
    }
}
