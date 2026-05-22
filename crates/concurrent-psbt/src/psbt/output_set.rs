//! Output sets keyed by unique output ID, in plain and result domains.

use bitcoin::hex::DisplayHex;

use crate::lattice::conflicts::Conflicts;
use crate::lattice::join::Join;
use crate::lattice::partial::JoinResult;
use crate::{Lift, ResultWrapped};

use super::keyed_set::{Freezable, KeyedSet, ResultKeyedSet, SetConflict};
use super::output::{Output, OutputConflict, ResultOutput, UniqueId};

/// PSBT outputs keyed by each output's [`UniqueId`].
///
/// A set can be modifiable or unmodifiable. [`Self::freeze`] makes the set
/// unmodifiable by fixing which output IDs belong to it.
///
/// Its result-domain counterpart is [`ResultOutputSet`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct OutputSet(Freezable<UniqueId, Output>);

/// Result-domain counterpart of [`OutputSet`].
///
/// The result contains the union of the output IDs in all joined sets. For an ID
/// present in both operands, the two [`ResultOutput`] records are joined.
///
/// The join records a membership conflict if any unmodifiable [`OutputSet`]
/// has a set of output IDs that differs from that union.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResultOutputSet(pub(crate) ResultKeyedSet<UniqueId, ResultOutput>);

/// More than one output carries this unique ID, so they cannot be collected
/// into an [`OutputSet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateOutput(pub UniqueId);

impl std::fmt::Display for DuplicateOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "two outputs share PSBT_OUT_UNIQUE_ID {}",
            self.0.as_bytes().as_hex()
        )
    }
}

impl std::error::Error for DuplicateOutput {}

impl OutputSet {
    /// Collect outputs into a set keyed by unique ID.
    ///
    /// Returns [`DuplicateOutput`] for the first unique ID that repeats.
    pub fn try_from_outputs(
        iter: impl IntoIterator<Item = Output>,
    ) -> Result<Self, DuplicateOutput> {
        KeyedSet::try_from_entries(
            iter.into_iter()
                .map(|output| Ok((output.unique_id().clone(), output))),
            |uid| DuplicateOutput(uid.clone()),
        )
        .map(|set| Self(Freezable::Modifiable(set)))
    }

    pub fn len(&self) -> usize {
        self.0.set().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.set().is_empty()
    }

    /// Iterate over the outputs, in no particular order.
    pub(crate) fn values(&self) -> impl Iterator<Item = &Output> {
        self.0.set().values()
    }

    pub fn wrap(self) -> ResultOutputSet {
        ResultOutputSet(self.0.wrap())
    }

    /// Mark the output set as no longer modifiable.
    pub fn freeze(self) -> Self {
        Self(self.0.freeze())
    }
}

impl IntoIterator for OutputSet {
    type Item = Output;
    type IntoIter = std::collections::hash_map::IntoValues<UniqueId, Output>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Join for ResultOutputSet {
    fn join(self, other: Self) -> Self {
        ResultOutputSet(self.0.join(other.0))
    }
}

impl ResultOutputSet {
    /// Borrow membership conflicts and output conflicts keyed by output ID.
    /// Iteration order between entries is unspecified.
    pub fn conflicts(&self) -> impl Iterator<Item = SetConflict<'_, UniqueId, OutputConflict<'_>>> {
        self.0.conflicts()
    }

    /// Count the distinct output IDs in the joined set. If a frozen set has fewer
    /// IDs, return the joined and frozen counts as a conflict.
    pub fn len(&self) -> JoinResult<usize> {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    /// Return the clean set, or `Err(self)` on a field or membership conflict.
    /// The returned set is frozen if any joined set was frozen.
    pub fn try_unwrap(self) -> Result<OutputSet, Self> {
        self.0.try_unwrap().map(OutputSet).map_err(Self)
    }
}

impl Lift for OutputSet {
    type Result = ResultOutputSet;

    fn wrap(self) -> Self::Result {
        Self::wrap(self)
    }
}

impl ResultWrapped for ResultOutputSet {
    type Clean = OutputSet;

    fn is_ok(&self) -> bool {
        Self::is_ok(self)
    }

    fn try_unwrap(self) -> Result<Self::Clean, Self> {
        Self::try_unwrap(self)
    }
}

impl Conflicts for ResultOutputSet {
    type Item<'a> = SetConflict<'a, UniqueId, OutputConflict<'a>>;

    fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>> {
        Self::conflicts(self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use bitcoin::{Amount, ScriptBuf};

    fn output(uid: Vec<u8>, sats: u64) -> Output {
        Output::new(
            UniqueId::try_from(uid).unwrap(),
            Amount::from_sat(sats),
            ScriptBuf::new(),
        )
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        /// A modifiable set holding one output with unique ID `[uid; 16]`.
        fn singleton(uid: u8) -> OutputSet {
            OutputSet::try_from_outputs([output(vec![uid; 16], 0)]).unwrap()
        }

        #[test]
        fn a_set_reports_its_size_and_outputs() {
            let empty = OutputSet::default();
            assert!(empty.is_empty());
            assert_eq!(empty.len(), 0);
            assert!(empty.wrap().is_empty());

            let set = singleton(1);
            assert!(!set.is_empty());
            assert_eq!(set.len(), 1);
            let wrapped = set.clone().wrap();
            assert!(!wrapped.is_empty());
            assert_eq!(wrapped.len(), Ok(1));
            assert_eq!(
                set.into_iter().collect::<Vec<_>>(),
                [output(vec![1; 16], 0)]
            );
        }

        #[test]
        fn a_repeated_unique_id_is_rejected() {
            let mut uid = vec![0; 16];
            uid[15] = 0xab;
            let repeated =
                OutputSet::try_from_outputs([output(uid.clone(), 0), output(uid.clone(), 5)])
                    .unwrap_err();
            assert_eq!(repeated, DuplicateOutput(UniqueId::try_from(uid).unwrap()));
            let error: &dyn std::error::Error = &repeated;
            assert_eq!(
                error.to_string(),
                "two outputs share PSBT_OUT_UNIQUE_ID 000000000000000000000000000000ab"
            );
        }

        #[test]
        fn disjoint_sets_join_into_their_union() {
            let joined = Lift::wrap(singleton(1)).join(Lift::wrap(singleton(2)));
            let union =
                OutputSet::try_from_outputs([output(vec![1; 16], 0), output(vec![2; 16], 0)]);
            assert!(ResultWrapped::is_ok(&joined));
            assert_eq!(ResultWrapped::try_unwrap(joined), Ok(union.unwrap()));
        }

        #[test]
        fn a_frozen_set_rejects_other_unique_ids() {
            let frozen = singleton(1).freeze().wrap();
            assert!(frozen.clone().join(singleton(1).wrap()).is_ok());
            assert!(!frozen.join(singleton(2).wrap()).is_ok());
        }

        #[test]
        fn outputs_with_different_amounts_for_one_unique_id_conflict() {
            let with_amount =
                |sats| OutputSet::try_from_outputs([output(vec![1; 16], sats)]).unwrap();
            let joined = Lift::wrap(with_amount(100)).join(Lift::wrap(with_amount(200)));
            assert!(!ResultWrapped::is_ok(&joined));
            assert_eq!(ResultWrapped::try_unwrap(joined.clone()), Err(joined));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        /// Small unique-ID domain, so independently generated outputs collide.
        fn arb_uid() -> impl Strategy<Value = Vec<u8>> {
            (0u8..3).prop_map(|byte| vec![byte; 16])
        }

        fn arb_output_with_uid() -> impl Strategy<Value = Output> {
            (
                arb_uid(),
                0u64..3, // small domain, so colliding outputs sometimes agree
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)), // redeem_script
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)), // witness_script
                proptest::collection::btree_map(
                    proptest::collection::vec(0u8..255, 1..=4),
                    proptest::collection::vec(0u8..255, 0..=4),
                    0..=2,
                ), // proprietaries, to exercise BTreeMap join paths
            )
                .prop_map(|(uid, amount, redeem, witness, props)| {
                    let mut output = output(uid, amount);
                    output.set_redeem_script(redeem.map(ScriptBuf::from_bytes));
                    output.set_witness_script(witness.map(ScriptBuf::from_bytes));
                    output.set_proprietaries(
                        props
                            .into_iter()
                            .map(|(k, v)| {
                                let prop_key = psbt_v2::raw::ProprietaryKey {
                                    prefix: b"test".to_vec(),
                                    subtype: k[0],
                                    key: k,
                                };
                                (prop_key, v)
                            })
                            .collect(),
                    );
                    output
                })
        }

        /// Clean set holding the first output generated for each unique ID.
        fn arb_output_set() -> impl Strategy<Value = OutputSet> {
            proptest::collection::vec(arb_output_with_uid(), 0..=3).prop_map(|outputs| {
                let mut seen = std::collections::HashSet::new();
                OutputSet::try_from_outputs(
                    outputs
                        .into_iter()
                        .filter(|output| seen.insert(output.unique_id().clone())),
                )
                .expect("outputs with distinct unique IDs cannot conflict")
            })
        }

        /// Join of singleton sets, some frozen, whose unique IDs may collide,
        /// so it holds conflicts as well as clean joins.
        fn arb_result_output_set() -> impl Strategy<Value = ResultOutputSet> {
            proptest::collection::vec((arb_output_with_uid(), any::<bool>()), 0..=4).prop_map(
                |outputs| {
                    outputs
                        .into_iter()
                        .map(|(output, frozen)| {
                            let set = OutputSet::try_from_outputs([output])
                                .expect("a single output cannot repeat a unique ID");
                            let set = if frozen { set.freeze() } else { set };
                            set.wrap()
                        })
                        .fold(ResultOutputSet::default(), Join::join)
                },
            )
        }

        assert_join_laws!(arb_result_output_set());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip(s in arb_output_set()) {
                let wrapped = Lift::wrap(s.clone());
                prop_assert!(ResultWrapped::is_ok(&wrapped));
                prop_assert_eq!(ResultWrapped::try_unwrap(wrapped), Ok(s));
            }

            #[test]
            fn len_matches_iter_count(s in arb_output_set()) {
                prop_assert_eq!(s.len(), s.into_iter().count());
            }

            #[test]
            fn try_from_outputs_requires_distinct_unique_ids(
                outputs in proptest::collection::vec(arb_output_with_uid(), 0..=4),
            ) {
                let mut seen = std::collections::HashSet::new();
                let first_repeat = outputs
                    .iter()
                    .map(|output| output.unique_id().clone())
                    .find(|uid| !seen.insert(uid.clone()));
                let count = outputs.len();
                match (OutputSet::try_from_outputs(outputs), first_repeat) {
                    (Ok(set), None) => prop_assert_eq!(set.len(), count),
                    (Err(error), Some(uid)) => {
                        prop_assert_eq!(error, DuplicateOutput(uid));
                    }
                    (result, repeat) => {
                        prop_assert!(false, "{result:?} disagrees with first repeat {repeat:?}");
                    }
                }
            }

            #[test]
            fn colliding_singletons_join_or_conflict(
                uid in arb_uid(),
                first in 0u64..3,
                second in 0u64..3,
            ) {
                let singleton = |sats| {
                    OutputSet::try_from_outputs([output(uid.clone(), sats)]).unwrap().wrap()
                };
                let joined = singleton(first).join(singleton(second));
                prop_assert_eq!(joined.len(), Ok(1));
                prop_assert_eq!(joined.is_ok(), first == second);
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(r in arb_result_output_set()) {
                prop_assert_eq!(r.is_empty(), r.len() == Ok(0));
                let is_ok = ResultWrapped::is_ok(&r);
                prop_assert_eq!(is_ok, ResultWrapped::try_unwrap(r).is_ok());
            }
        }
    }
}
