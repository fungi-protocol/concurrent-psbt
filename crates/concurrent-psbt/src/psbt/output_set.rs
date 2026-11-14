//! Output sets keyed by unique output ID, in plain and result domains.

use crate::lattice::join::Join;
use crate::lattice::partial::JoinResult;

use super::keyed_set::{Freezable, KeyedSet, ResultKeyedSet};
use super::output::{Output, ResultOutput, UniqueId};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct OutputSet(Freezable<UniqueId, Output>);
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResultOutputSet(pub(crate) ResultKeyedSet<UniqueId, ResultOutput>);

/// More than one output carries this unique ID, so they cannot be collected
/// into an [`OutputSet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateOutput(pub UniqueId);

impl OutputSet {
    /// Collect outputs into a set keyed by unique ID.
    ///
    /// # Errors
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
        ResultOutputSet(self.0.wrap(Output::wrap))
    }

    /// Freeze the set, as a PSBT's outputs are once they are no longer
    /// modifiable. After wrapping, it joins cleanly only with frozen sets of the
    /// same unique IDs, and with modifiable sets whose unique IDs it already
    /// contains.
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
    /// Return the number of outputs, or the conflicting sizes of the frozen
    /// sets and the outputs if they disagree.
    pub fn len(&self) -> JoinResult<usize> {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    pub fn try_unwrap(self) -> Result<OutputSet, Self> {
        self.0.try_unwrap().map(OutputSet).map_err(Self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use bitcoin::{Amount, ScriptBuf};

    /// An output with unique ID `uid`, `sats` and every other field unset.
    fn output(uid: Vec<u8>, sats: u64) -> Output {
        Output::new(UniqueId::new(uid), Amount::from_sat(sats), ScriptBuf::new())
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        /// A modifiable set holding only an output with unique ID `[uid]`.
        fn singleton(uid: u8) -> OutputSet {
            OutputSet::try_from_outputs([output(vec![uid], 0)]).unwrap()
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
            assert_eq!(set.into_iter().collect::<Vec<_>>(), [output(vec![1], 0)]);
        }

        #[test]
        fn a_repeated_unique_id_is_rejected() {
            let repeated = OutputSet::try_from_outputs([output(vec![1], 0), output(vec![1], 5)]);
            assert_eq!(repeated, Err(DuplicateOutput(UniqueId::new(vec![1]))));
        }

        #[test]
        fn disjoint_sets_join_into_their_union() {
            let joined = singleton(1).wrap().join(singleton(2).wrap());
            let union = OutputSet::try_from_outputs([output(vec![1], 0), output(vec![2], 0)]);
            assert_eq!(joined.try_unwrap(), Ok(union.unwrap()));
        }

        #[test]
        fn a_frozen_set_rejects_other_unique_ids() {
            let frozen = singleton(1).freeze().wrap();
            assert!(frozen.clone().join(singleton(1).wrap()).is_ok());
            assert!(!frozen.join(singleton(2).wrap()).is_ok());
        }

        #[test]
        fn outputs_with_different_amounts_for_one_unique_id_conflict() {
            let with_amount = |sats| OutputSet::try_from_outputs([output(vec![1], sats)]).unwrap();
            let joined = with_amount(100).wrap().join(with_amount(200).wrap());
            assert!(!joined.is_ok());
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        /// Small unique-ID domain, so independently generated outputs collide.
        fn arb_uid() -> impl Strategy<Value = Vec<u8>> {
            (0u8..3).prop_map(|byte| vec![byte])
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
                    output.redeem_script = redeem.map(ScriptBuf::from_bytes);
                    output.witness_script = witness.map(ScriptBuf::from_bytes);
                    for (k, v) in props {
                        let prop_key = psbt_v2::raw::ProprietaryKey {
                            prefix: b"test".to_vec(),
                            subtype: k[0],
                            key: k,
                        };
                        output.proprietaries.insert(prop_key, v);
                    }
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
                prop_assert_eq!(s.clone().wrap().try_unwrap(), Ok(s));
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
                prop_assert_eq!(r.is_ok(), r.try_unwrap().is_ok());
            }
        }
    }
}
