//! Input sets keyed by spent outpoint, in plain and result domains.

use bitcoin::OutPoint;

use crate::lattice::conflicts::Conflicts;
use crate::lattice::join::Join;
use crate::lattice::partial::JoinResult;
use crate::{Lift, ResultWrapped};

use super::input::{Input, InputConflict, ResultInput, out_point};
use super::keyed_set::{Freezable, KeyedSet, ResultKeyedSet, SetConflict};

/// PSBT inputs keyed by the [`OutPoint`] each input spends.
///
/// Every input in the set is conflict-free and spends a distinct outpoint.
/// A set can be modifiable or unmodifiable. [`Self::freeze`] makes the set
/// unmodifiable by fixing which outpoints belong to it.
///
/// Its result-domain counterpart is [`ResultInputSet`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct InputSet(Freezable<OutPoint, Input>);

/// Result-domain counterpart of [`InputSet`].
///
/// The result contains the union of the outpoints in all joined sets. For an outpoint
/// present in both operands, the two [`ResultInput`] records are joined.
///
/// The join records a membership conflict if any unmodifiable [`InputSet`]
/// has a set of outpoints that differs from that union.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResultInputSet(pub(crate) ResultKeyedSet<OutPoint, ResultInput>);

/// More than one input spends this outpoint, so they cannot be collected into
/// an [`InputSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateInput(pub OutPoint);

impl std::fmt::Display for DuplicateInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "two inputs spend {}", self.0)
    }
}

impl std::error::Error for DuplicateInput {}

impl InputSet {
    /// Collect inputs into a set keyed by the outpoint each spends.
    ///
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
        ResultInputSet(self.0.wrap())
    }

    /// Mark the input set as no longer modifiable.
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
    /// Borrow membership conflicts and input conflicts keyed by outpoint.
    /// Iteration order between entries is unspecified.
    pub fn conflicts(&self) -> impl Iterator<Item = SetConflict<'_, OutPoint, InputConflict<'_>>> {
        self.0.conflicts()
    }

    /// Count the distinct outpoints in the joined set. If a frozen set has fewer
    /// outpoints, return the joined and frozen counts as a conflict.
    pub fn len(&self) -> JoinResult<usize> {
        self.0.len()
    }

    /// Return `true` if the joined set contains no inputs.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return `true` if every input is conflict-free and each frozen set
    /// contains exactly the joined outpoints.
    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    /// Return the clean set, or `Err(self)` on a field or membership conflict.
    /// The returned set is frozen if any joined set was frozen.
    pub fn try_unwrap(self) -> Result<InputSet, Self> {
        self.0.try_unwrap().map(InputSet).map_err(Self)
    }
}

impl Lift for InputSet {
    type Result = ResultInputSet;

    fn wrap(self) -> Self::Result {
        Self::wrap(self)
    }
}

impl ResultWrapped for ResultInputSet {
    type Clean = InputSet;

    fn is_ok(&self) -> bool {
        Self::is_ok(self)
    }

    fn try_unwrap(self) -> Result<Self::Clean, Self> {
        Self::try_unwrap(self)
    }
}

impl Conflicts for ResultInputSet {
    type Item<'a> = SetConflict<'a, OutPoint, InputConflict<'a>>;

    fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>> {
        Self::conflicts(self)
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
            let outpoint = out_point(&input);
            let error = InputSet::try_from_inputs([input.clone(), input]).unwrap_err();
            assert_eq!(error, DuplicateInput(outpoint));
            let error: &dyn std::error::Error = &error;
            assert_eq!(error.to_string(), format!("two inputs spend {outpoint}"));
        }

        #[test]
        fn disjoint_sets_join_into_their_union() {
            let joined = Lift::wrap(singleton(1)).join(Lift::wrap(singleton(2)));
            let union = InputSet::try_from_inputs([make_input(1, 0), make_input(2, 0)]);
            assert!(ResultWrapped::is_ok(&joined));
            assert_eq!(ResultWrapped::try_unwrap(joined), Ok(union.unwrap()));
        }

        #[test]
        fn a_frozen_set_requires_matching_membership() {
            let frozen = InputSet::try_from_inputs([make_input(1, 0), make_input(2, 0)])
                .unwrap()
                .freeze()
                .wrap();
            assert!(frozen.clone().join(frozen.clone()).is_ok());
            assert!(frozen.clone().join(singleton(1).wrap()).is_ok());
            assert!(!frozen.clone().join(singleton(1).freeze().wrap()).is_ok());
            assert!(!frozen.join(singleton(3).wrap()).is_ok());
        }

        #[test]
        fn inputs_with_different_sequences_for_one_outpoint_conflict() {
            let with_sequence = |sequence| {
                let mut input = make_input(1, 0);
                input.set_sequence(Some(Sequence(sequence)));
                InputSet::try_from_inputs([input]).unwrap().wrap()
            };
            let joined = with_sequence(1).join(with_sequence(2));
            assert!(!ResultWrapped::is_ok(&joined));
            assert_eq!(ResultWrapped::try_unwrap(joined.clone()), Err(joined));
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
                let wrapped = Lift::wrap(s.clone());
                prop_assert!(ResultWrapped::is_ok(&wrapped));
                prop_assert_eq!(ResultWrapped::try_unwrap(wrapped), Ok(s));
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
                    (Err(error), Some(op)) => {
                        prop_assert_eq!(error, DuplicateInput(op));
                        prop_assert_eq!(error.to_string(), format!("two inputs spend {op}"));
                    }
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
                    let mut input = Input::new(&op);
                    input.set_sequence(Some(Sequence(sequence)));
                    InputSet::try_from_inputs([input]).unwrap().wrap()
                };
                let joined = singleton(s1).join(singleton(s2));
                prop_assert_eq!(joined.len(), Ok(1));
                prop_assert_eq!(ResultWrapped::is_ok(&joined), s1 == s2);
                if s1 != s2 {
                    prop_assert_eq!(ResultWrapped::try_unwrap(joined.clone()), Err(joined));
                }
            }

            #[test]
            fn len_matches_iter_count(s in arb_input_set()) {
                prop_assert_eq!(s.len(), s.into_iter().count());
            }
        }
    }
}
