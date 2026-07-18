//! Input sets keyed by spent outpoint, in plain and result domains.

use std::collections::HashMap;

use bitcoin::OutPoint;
use psbt_v2::v2::Input;

use crate::collections::hashmap::{HashMapExt, ResultHashMapExt};
use crate::lattice::join::Join;

use super::SetLen;
use super::input::{InputExt, ResultInput, out_point};

/// Set of conflict-free PSBT inputs keyed by the [`OutPoint`] they spend.
///
/// Use [`ResultInputSet`] while accumulating possibly duplicated inputs so
/// disagreements are retained in the result domain instead of overwritten.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct InputSet(HashMap<OutPoint, Input>);

/// Result-domain version of [`InputSet`]. Implements [`Join`] for concurrent merging.
///
/// Inputs at matching outpoints are joined field-by-field; disjoint outpoints are preserved.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResultInputSet(pub(crate) HashMap<OutPoint, ResultInput>);

impl SetLen for InputSet {
    fn len(&self) -> usize {
        InputSet::len(self)
    }
}

impl SetLen for ResultInputSet {
    fn len(&self) -> usize {
        ResultInputSet::len(self)
    }
}

impl InputSet {
    /// Insert an input known to be fresh, keyed by its outpoint.
    ///
    /// # Note
    /// This is for constructing already conflict-free sets. Use
    /// [`ResultInputSet::add`] when duplicates should be joined.
    pub fn add(&mut self, input: Input) {
        let op = out_point(&input);
        self.0.insert(op, input);
    }

    /// Return the number of inputs in the set.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return `true` if the set contains no inputs.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return `true` if the set contains an input spending the given outpoint.
    pub fn spends_outpoint(&self, op: &OutPoint) -> bool {
        self.0.contains_key(op)
    }

    /// Lift into the result domain as a conflict-free [`ResultInputSet`].
    pub fn wrap(self) -> ResultInputSet {
        ResultInputSet(self.0.map_values(InputExt::wrap))
    }
}

impl FromIterator<Input> for InputSet {
    fn from_iter<T: IntoIterator<Item = Input>>(iter: T) -> Self {
        Self(iter.into_iter().map(|i| (out_point(&i), i)).collect())
    }
}

impl IntoIterator for InputSet {
    type Item = Input;
    type IntoIter = std::collections::hash_map::IntoValues<OutPoint, Input>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_values()
    }
}

impl Join for ResultInputSet {
    fn join(self, other: Self) -> Self {
        ResultInputSet(self.0.join(other.0))
    }
}

impl ResultInputSet {
    /// Add an input, joining with any existing value at the same outpoint.
    pub fn add(&mut self, input: Input) {
        let op = out_point(&input);
        let value = input.wrap();
        match self.0.remove(&op) {
            Some(existing) => {
                self.0.insert(op, existing.join(value));
            }
            None => {
                self.0.insert(op, value);
            }
        }
    }

    /// Accumulate inputs into the result domain.
    ///
    /// Inputs with matching outpoints are joined field-by-field. Compatible
    /// duplicates collapse to a clean value; disagreements remain as conflicts.
    pub fn from_inputs(iter: impl IntoIterator<Item = Input>) -> Self {
        let mut set = Self::default();
        for input in iter {
            set.add(input);
        }
        set
    }

    /// Return the number of inputs in the joined set.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return `true` if the joined set contains no inputs.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return `true` if every input in the set is conflict-free.
    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    /// Extract a clean [`InputSet`] if no conflicts remain.
    ///
    /// # Errors
    /// Returns `Err(self)` if any input contains a conflict.
    #[allow(clippy::result_large_err)]
    pub fn try_unwrap(self) -> Result<InputSet, Self> {
        match self.0.try_unwrap() {
            Ok(map) => Ok(InputSet(map)),
            Err(map) => Err(Self(map)),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    use bitcoin::hashes::Hash;
    use bitcoin::{Sequence, Txid};

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use crate::psbt::input::tests::unit::make_input;

        #[test]
        fn empty_input_set() {
            let set = InputSet::default();
            assert!(set.is_empty());
            assert_eq!(set.len(), 0);
        }

        #[test]
        fn add_input() {
            let mut set = InputSet::default();
            let input = make_input(1, 0);
            let op = out_point(&input);
            set.add(input);
            assert_eq!(set.len(), 1);
            assert!(!set.is_empty());
            assert!(set.spends_outpoint(&op));
            // Absent outpoint
            let absent = OutPoint {
                txid: Txid::from_byte_array([99; 32]),
                vout: 0,
            };
            assert!(!set.spends_outpoint(&absent));
        }

        #[test]
        fn wrap_empty_set_is_ok() {
            assert!(InputSet::default().wrap().is_ok());
        }

        #[test]
        fn join_disjoint_inputs() {
            let mut a = InputSet::default();
            a.add(make_input(1, 0));
            let mut b = InputSet::default();
            b.add(make_input(2, 0));
            assert!(a.wrap().join(b.wrap()).is_ok());
        }

        #[test]
        fn input_set_try_unwrap_ok() {
            let mut set = InputSet::default();
            set.add(make_input(1, 0));
            assert!(set.wrap().try_unwrap().is_ok());
        }

        #[test]
        fn input_set_conflicting_try_unwrap_err() {
            let mut a = InputSet::default();
            let mut input_a = make_input(1, 0);
            input_a.sequence = Some(Sequence(1));
            a.add(input_a);

            let mut b = InputSet::default();
            let mut input_b = make_input(1, 0);
            input_b.sequence = Some(Sequence(2));
            b.add(input_b);

            let joined = a.wrap().join(b.wrap());
            assert!(!joined.is_ok());
            assert!(joined.try_unwrap().is_err());
        }

        #[test]
        fn from_iter_collects_inputs() {
            let inputs = vec![make_input(1, 0), make_input(2, 1)];
            let set: InputSet = inputs.into_iter().collect();
            assert_eq!(set.len(), 2);
        }

        #[test]
        fn into_iter_yields_inputs() {
            let mut set = InputSet::default();
            set.add(make_input(1, 0));
            set.add(make_input(2, 1));
            let values: Vec<Input> = set.into_iter().collect();
            assert_eq!(values.len(), 2);
        }

        #[test]
        fn result_input_set_len_and_empty() {
            let result = InputSet::default().wrap();
            assert!(result.is_empty());
            assert_eq!(result.len(), 0);

            let mut set = InputSet::default();
            set.add(make_input(1, 0));
            let result = set.wrap();
            assert!(!result.is_empty());
            assert_eq!(result.len(), 1);
        }

        #[test]
        fn result_input_set_add_duplicate_identical_is_ok() {
            let input = make_input(1, 0);
            let mut result = ResultInputSet::default();
            result.add(input.clone());
            result.add(input);
            assert_eq!(result.len(), 1);
            assert!(result.is_ok());
            assert!(result.try_unwrap().is_ok());
        }

        #[test]
        fn result_input_set_add_duplicate_conflicting_is_err() {
            let mut a = make_input(1, 0);
            let mut b = make_input(1, 0);
            a.sequence = Some(Sequence(1));
            b.sequence = Some(Sequence(2));

            let mut result = ResultInputSet::default();
            result.add(a);
            result.add(b);

            assert_eq!(result.len(), 1);
            assert!(!result.is_ok());
            assert!(result.try_unwrap().is_err());
        }

        #[test]
        fn result_input_set_from_inputs_joins_duplicates() {
            let mut a = make_input(1, 0);
            let mut b = make_input(1, 0);
            a.sequence = Some(Sequence(1));
            b.sequence = Some(Sequence(2));

            let result = ResultInputSet::from_inputs([a, b]);

            assert_eq!(result.len(), 1);
            assert!(!result.is_ok());
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use crate::psbt::input::tests::prop::{arb_input, arb_input_with_fields, arb_outpoint};
        use proptest::prelude::*;

        fn arb_input_set() -> impl Strategy<Value = InputSet> {
            proptest::collection::vec(arb_input(), 0..=3)
                .prop_map(|inputs| inputs.into_iter().collect::<InputSet>())
        }

        fn arb_result_input_set() -> impl Strategy<Value = ResultInputSet> {
            prop_oneof![
                arb_input_set().prop_map(|s| s.wrap()),
                (arb_input_set_with_fields(), arb_input_set_with_fields())
                    .prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        fn arb_input_set_with_fields() -> impl Strategy<Value = InputSet> {
            proptest::collection::vec(arb_input_with_fields(), 0..=4)
                .prop_map(|inputs| inputs.into_iter().collect::<InputSet>())
        }

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip_input_set(s in arb_input_set_with_fields()) {
                let wrapped = s.wrap();
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            // ── 3. InputSet from_iter: duplicates & union ───────────────
            #[test]
            fn from_iter_duplicate_outpoints_overwrites(
                op in arb_outpoint(),
                s1 in 0u32..100,
                s2 in 100u32..200,
            ) {
                let a = Input { sequence: Some(Sequence(s1)), ..Input::new(&op) };
                let b = Input { sequence: Some(Sequence(s2)), ..Input::new(&op) };
                let set: InputSet = vec![a, b].into_iter().collect();
                // HashMap collect keeps last value for duplicate keys
                prop_assert_eq!(set.len(), 1);
                prop_assert!(set.spends_outpoint(&op));
            }

            #[test]
            fn from_iter_disjoint_outpoints_union(
                b1 in 0u8..3,
                b2 in 3u8..6,
            ) {
                let op1 = OutPoint { txid: Txid::from_byte_array([b1; 32]), vout: 0 };
                let op2 = OutPoint { txid: Txid::from_byte_array([b2; 32]), vout: 0 };
                let set: InputSet = vec![Input::new(&op1), Input::new(&op2)]
                    .into_iter()
                    .collect();
                prop_assert_eq!(set.len(), 2);
                prop_assert!(set.spends_outpoint(&op1));
                prop_assert!(set.spends_outpoint(&op2));
            }

            // ResultInputSet lattice laws
            #[test]
            fn input_set_idempotent(a in arb_result_input_set()) {
                prop_assert_eq!(a.clone().join(a.clone()), a);
            }

            #[test]
            fn input_set_commutative(
                a in arb_result_input_set(),
                b in arb_result_input_set(),
            ) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn input_set_associative(
                a in arb_result_input_set(),
                b in arb_result_input_set(),
                c in arb_result_input_set(),
            ) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).join(c.clone()),
                    a.join(b.join(c))
                );
            }

            #[test]
            fn len_matches_iter_count(s in arb_input_set()) {
                prop_assert_eq!(s.len(), s.into_iter().count());
            }

            #[test]
            fn is_empty_matches_len(s in arb_input_set()) {
                #[allow(clippy::len_zero)]
                { prop_assert_eq!(s.is_empty(), s.len() == 0); }
            }

            #[test]
            fn add_preserves_or_increments_len(
                mut s in arb_input_set(),
                input in arb_input(),
            ) {
                let op = out_point(&input);
                let before = s.len();
                s.add(input);
                prop_assert!(s.len() == before || s.len() == before + 1);
                prop_assert!(s.spends_outpoint(&op));
            }

            #[test]
            fn spends_outpoint_false_for_absent(_dummy in 0u8..1) {
                let s = InputSet::default();
                let op = OutPoint {
                    txid: Txid::from_byte_array([99; 32]),
                    vout: 0,
                };
                prop_assert!(!s.spends_outpoint(&op));
            }

            #[test]
            fn is_ok_try_unwrap_consistent_set(r in arb_result_input_set()) {
                if r.is_ok() {
                    prop_assert!(r.try_unwrap().is_ok());
                } else {
                    prop_assert!(r.try_unwrap().is_err());
                }
            }

            #[test]
            fn result_set_len_and_empty(s in arb_result_input_set()) {
                #[allow(clippy::len_zero)]
                { prop_assert_eq!(s.is_empty(), s.len() == 0); }
            }

            #[test]
            fn join_conflicting_inputs_try_unwrap_err(
                op in arb_outpoint(),
                s1 in 0u32..100,
                s2 in 100u32..200,
            ) {
                let a: InputSet = vec![Input { sequence: Some(Sequence(s1)), ..Input::new(&op) }]
                    .into_iter().collect();
                let b: InputSet = vec![Input { sequence: Some(Sequence(s2)), ..Input::new(&op) }]
                    .into_iter().collect();
                let joined = a.wrap().join(b.wrap());
                prop_assert!(!joined.is_ok());
                prop_assert!(joined.try_unwrap().is_err());
            }

            #[test]
            fn result_from_inputs_joins_duplicate_outpoints(
                op in arb_outpoint(),
                first_sequence in 0u32..100,
                second_sequence in 100u32..200,
            ) {
                let first = Input {
                    sequence: Some(Sequence(first_sequence)),
                    ..Input::new(&op)
                };
                let second = Input {
                    sequence: Some(Sequence(second_sequence)),
                    ..Input::new(&op)
                };

                let result = ResultInputSet::from_inputs([first, second]);

                prop_assert_eq!(result.len(), 1);
                prop_assert!(!result.is_ok());
            }
        }
    }
}
