//! Some types have a natural merge operation but that isn't defined for all pairs of values.
//! [`PartialJoin::try_join`] captures this.
//!
//! It returns [`JoinResult<V>`], an alias for `Result<V, Conflict<V>>`, where [`Conflict<V>`],
//! collects all the distinct values that couldn't be merged.
//!
//! [`JoinResult<V>`] implements [`Join`], which is an infallible join operation (the free
//! semilattice over `V`, more or less), in terms of [`Conflict<V>`]'s [`Join`] implementation.

use crate::lattice::join::{Join, JoinMut};

/// Fallible join for scalar value types where merge may produce a conflict.
///
/// Containers should not implement this, instead they should implement [`Join`] by wrapping
/// contained values in [`JoinResult`].
pub trait PartialJoin: Sized + PartialEq {
    /// Attempt to join two values.
    ///
    /// Returns `Ok(lub)` where `lub` is the least upper bound if one exists, or `Err(Conflict)`
    /// containing both values.
    ///
    /// Although `Conflict<V>` has set semantics (equality treats it as an unordered collection),
    /// insertion order is preserved during iteration: when two `Ok` values are joined and produce
    /// a binary conflict, the first value is the left operand and the second is the right.
    ///
    /// Provenance is only reliable when both operands were `Ok` before the join. When an existing
    /// `Conflict` is joined with another value, new entries are appended, so a multi-way fold does
    /// not preserve per-operand attribution.
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
/// The inner values are [`PartialJoin`] with equality based on [`PartialEq`]. Duplicate values
/// are omitted.
#[derive(Debug, Clone)]
pub struct Conflict<V: PartialJoin>(Vec<V>);

impl<V: PartialJoin> Conflict<V> {
    /// Wrap a single value into a conflict.
    fn singleton(v: V) -> Self {
        Self(vec![v])
    }

    /// Build from an iterator, deduplicating values.
    pub fn from_values(iter: impl IntoIterator<Item = V>) -> Self {
        let mut vals = Vec::new();
        for v in iter {
            if !vals.contains(&v) {
                vals.push(v);
            }
        }
        Self(vals)
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
            if !self.0.contains(&value) {
                self.0.push(value)
            }
        }
    }
}

/// Set equality: same elements, regardless of order.
///
/// Relies on the no-duplicate invariant maintained by `Conflict::from_values`:
/// each value in `self.0` is distinct (deduplicated on construction).
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

        pub fn arb_foo() -> impl Strategy<Value = Foo> {
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
