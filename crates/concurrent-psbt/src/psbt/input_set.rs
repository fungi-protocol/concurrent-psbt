//! Input sets keyed by spent outpoint, in plain and result domains.

use bitcoin::OutPoint;

use crate::lattice::join::Join;
use crate::lattice::partial::JoinResult;

use super::input::{Input, ResultInput, out_point};
use super::keyed_set::{Freezable, KeyedSet, ResultKeyedSet};

/// Set of PSBT inputs keyed by the [`OutPoint`] they spend.
///
/// Every input in the set is conflict-free and spends a distinct outpoint.
/// Its result-domain counterpart is [`ResultInputSet`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct InputSet(Freezable<OutPoint, Input>);

/// Result-domain version of [`InputSet`]. Implements [`Join`] for concurrent merging.
///
/// Inputs at matching outpoints are joined field-by-field; disjoint outpoints are preserved.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResultInputSet(pub(crate) ResultKeyedSet<OutPoint, ResultInput>);

/// More than one input spends this outpoint, so they cannot be collected into
/// an [`InputSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateInput(pub OutPoint);

impl InputSet {
    /// Collect inputs into a set keyed by the outpoint each spends.
    ///
    /// # Errors
    /// Returns [`DuplicateInput`] for the first outpoint that repeats.
    pub fn try_from_inputs(iter: impl IntoIterator<Item = Input>) -> Result<Self, DuplicateInput> {
        KeyedSet::try_from_entries(
            iter.into_iter().map(|input| Ok((out_point(&input), input))),
            |outpoint| DuplicateInput(*outpoint),
        )
        .map(|set| Self(Freezable::Modifiable(set)))
    }

    /// Return the number of inputs in the set.
    pub fn len(&self) -> usize {
        self.0.set().len()
    }

    /// Return `true` if the set contains no inputs.
    pub fn is_empty(&self) -> bool {
        self.0.set().is_empty()
    }

    /// Return `true` if the set contains an input spending the given outpoint.
    pub fn spends_outpoint(&self, op: &OutPoint) -> bool {
        self.0.set().contains_key(op)
    }

    /// Lift into the result domain as a conflict-free [`ResultInputSet`].
    pub fn wrap(self) -> ResultInputSet {
        ResultInputSet(self.0.wrap(Input::wrap))
    }

    /// Freeze the set, as a PSBT's inputs are once they are no longer
    /// modifiable. After wrapping, it joins cleanly only with frozen sets of the
    /// same outpoints, and with modifiable sets whose outpoints it already
    /// contains.
    pub fn freeze(self) -> Self {
        Self(self.0.freeze())
    }
}

impl IntoIterator for InputSet {
    type Item = Input;
    type IntoIter = std::collections::hash_map::IntoValues<OutPoint, Input>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Join for ResultInputSet {
    fn join(self, other: Self) -> Self {
        ResultInputSet(self.0.join(other.0))
    }
}

impl ResultInputSet {
    /// Return the number of inputs, or the conflicting sizes of the frozen
    /// sets and the inputs if they disagree.
    pub fn len(&self) -> JoinResult<usize> {
        self.0.len()
    }

    /// Return `true` if the joined set contains no inputs.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return `true` if every input in the set is conflict-free and the
    /// outpoints agree with the frozen sets.
    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    /// Extract a clean [`InputSet`] if no conflicts remain.
    ///
    /// # Errors
    /// Returns `Err(self)` if any input contains a conflict, or the outpoints
    /// disagree with the frozen sets.
    pub fn try_unwrap(self) -> Result<InputSet, Self> {
        self.0.try_unwrap().map(InputSet).map_err(Self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    use bitcoin::Sequence;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use crate::psbt::input::tests::unit::make_input;

        /// A modifiable set holding only an input spending `txid_byte`:0.
        fn singleton(txid_byte: u8) -> InputSet {
            InputSet::try_from_inputs([make_input(txid_byte, 0)]).unwrap()
        }

        #[test]
        fn a_set_reports_the_outpoints_it_spends() {
            let empty = InputSet::default();
            assert!(empty.is_empty());
            assert_eq!(empty.len(), 0);
            assert!(empty.wrap().is_empty());

            let set = singleton(1);
            assert!(!set.is_empty());
            assert_eq!(set.len(), 1);
            assert!(set.spends_outpoint(&out_point(&make_input(1, 0))));
            assert!(!set.spends_outpoint(&out_point(&make_input(99, 0))));
            let wrapped = set.clone().wrap();
            assert!(!wrapped.is_empty());
            assert_eq!(wrapped.len(), Ok(1));
            assert_eq!(set.into_iter().collect::<Vec<_>>(), [make_input(1, 0)]);
        }

        #[test]
        fn a_repeated_outpoint_is_rejected() {
            let input = make_input(1, 0);
            let repeated = InputSet::try_from_inputs([input.clone(), input.clone()]);
            assert_eq!(repeated, Err(DuplicateInput(out_point(&input))));
        }

        #[test]
        fn disjoint_sets_join_into_their_union() {
            let joined = singleton(1).wrap().join(singleton(2).wrap());
            let union = InputSet::try_from_inputs([make_input(1, 0), make_input(2, 0)]);
            assert_eq!(joined.try_unwrap(), Ok(union.unwrap()));
        }

        #[test]
        fn a_frozen_set_rejects_other_outpoints() {
            let frozen = singleton(1).freeze().wrap();
            assert!(frozen.clone().join(singleton(1).wrap()).is_ok());
            assert!(!frozen.join(singleton(2).wrap()).is_ok());
        }

        #[test]
        fn inputs_with_different_sequences_for_one_outpoint_conflict() {
            let with_sequence = |sequence| {
                let input = Input {
                    sequence: Some(Sequence(sequence)),
                    ..make_input(1, 0)
                };
                InputSet::try_from_inputs([input]).unwrap().wrap()
            };
            let joined = with_sequence(1).join(with_sequence(2));
            assert!(!joined.is_ok());
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use crate::psbt::input::tests::prop::{arb_input, arb_input_with_fields, arb_outpoint};
        use proptest::prelude::*;

        /// Clean set holding the first input generated for each outpoint.
        fn distinct(inputs: Vec<Input>) -> InputSet {
            let mut seen = std::collections::HashSet::new();
            InputSet::try_from_inputs(inputs.into_iter().filter(|i| seen.insert(out_point(i))))
                .expect("inputs spending distinct outpoints cannot conflict")
        }

        fn arb_input_set() -> impl Strategy<Value = InputSet> {
            proptest::collection::vec(arb_input(), 0..=3).prop_map(distinct)
        }

        /// Join of singleton sets of field-rich inputs, some frozen, whose
        /// outpoints may collide, so it holds conflicts as well as clean joins.
        fn arb_result_input_set() -> impl Strategy<Value = ResultInputSet> {
            proptest::collection::vec((arb_input_with_fields(), any::<bool>()), 0..=4).prop_map(
                |inputs| {
                    inputs
                        .into_iter()
                        .map(|(input, frozen)| {
                            let set = InputSet::try_from_inputs([input])
                                .expect("a single input cannot repeat");
                            let set = if frozen { set.freeze() } else { set };
                            set.wrap()
                        })
                        .fold(ResultInputSet::default(), Join::join)
                },
            )
        }

        fn arb_input_set_with_fields() -> impl Strategy<Value = InputSet> {
            proptest::collection::vec(arb_input_with_fields(), 0..=4).prop_map(distinct)
        }

        assert_join_laws!(arb_result_input_set());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip_input_set(s in arb_input_set_with_fields()) {
                prop_assert_eq!(s.clone().wrap().try_unwrap(), Ok(s));
            }

            #[test]
            fn try_from_inputs_requires_distinct_outpoints(
                inputs in proptest::collection::vec(arb_input_with_fields(), 0..=4),
            ) {
                let mut seen = std::collections::HashSet::new();
                let first_repeat = inputs.iter().map(out_point).find(|op| !seen.insert(*op));
                let count = inputs.len();
                match (InputSet::try_from_inputs(inputs), first_repeat) {
                    (Ok(set), None) => prop_assert_eq!(set.len(), count),
                    (Err(error), Some(op)) => prop_assert_eq!(error, DuplicateInput(op)),
                    (result, repeat) => {
                        prop_assert!(false, "{result:?} disagrees with first repeat {repeat:?}");
                    }
                }
            }

            #[test]
            fn colliding_singletons_join_or_conflict(
                op in arb_outpoint(),
                s1 in 0u32..3,
                s2 in 0u32..3,
            ) {
                let singleton = |sequence| {
                    let input = Input { sequence: Some(Sequence(sequence)), ..Input::new(&op) };
                    InputSet::try_from_inputs([input]).unwrap().wrap()
                };
                let joined = singleton(s1).join(singleton(s2));
                prop_assert_eq!(joined.len(), Ok(1));
                prop_assert_eq!(joined.is_ok(), s1 == s2);
            }

            #[test]
            fn len_matches_iter_count(s in arb_input_set()) {
                prop_assert_eq!(s.len(), s.into_iter().count());
            }
        }
    }
}
