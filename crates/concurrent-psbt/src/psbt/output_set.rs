//! Output sets keyed by unique output ID, in plain and result domains.

use std::collections::HashMap;

use psbt_v2::v2::Output;

use crate::collections::hashmap::{HashMapExt, ResultHashMapExt};
use crate::lattice::join::Join;

use super::output::{OutputExt, OutputUniqueIdExt, ResultOutput, UniqueId};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct OutputSet(HashMap<UniqueId, Output>);
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResultOutputSet(pub(crate) HashMap<UniqueId, ResultOutput>);

impl OutputSet {
    /// Insert an output known to be fresh, keyed by its unique ID.
    ///
    /// # Panics
    /// Panics if the output has no `PSBT_OUT_UNIQUE_ID` proprietary field.
    ///
    /// # Note
    /// This is for constructing already conflict-free sets. Use
    /// [`ResultOutputSet::try_add`] when duplicates should be joined.
    pub fn add(&mut self, output: Output) {
        let id = OutputUniqueIdExt::unique_id(&output)
            .expect("output must have PSBT_OUT_UNIQUE_ID to be added to an OutputSet");
        self.0.insert(id, output);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn wrap(self) -> ResultOutputSet {
        ResultOutputSet(self.0.map_values(OutputExt::wrap))
    }

    /// Create an OutputSet from an iterator of outputs.
    ///
    /// # Errors
    /// Returns the first output that does not have a `PSBT_OUT_UNIQUE_ID` proprietary field set.
    #[allow(clippy::result_large_err)]
    pub fn try_from_outputs(iter: impl IntoIterator<Item = Output>) -> Result<Self, Output> {
        let mut map = HashMap::new();
        for output in iter {
            let uid = OutputUniqueIdExt::unique_id(&output).ok_or_else(|| output.clone())?;
            map.insert(uid, output);
        }
        Ok(Self(map))
    }
}

/// # Panics
/// Panics if any output is missing the `PSBT_OUT_UNIQUE_ID` proprietary field.
/// Use [`OutputSet::try_from_outputs`] for a fallible alternative.
impl FromIterator<Output> for OutputSet {
    fn from_iter<T: IntoIterator<Item = Output>>(iter: T) -> Self {
        Self::try_from_outputs(iter)
            .expect("all outputs must have PSBT_OUT_UNIQUE_ID to be added to an OutputSet")
    }
}

impl IntoIterator for OutputSet {
    type Item = Output;
    type IntoIter = std::collections::hash_map::IntoValues<UniqueId, Output>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_values()
    }
}

impl Join for ResultOutputSet {
    fn join(self, other: Self) -> Self {
        ResultOutputSet(self.0.join(other.0))
    }
}

impl ResultOutputSet {
    /// Add an output, joining with any existing value at the same unique ID.
    ///
    /// # Errors
    /// Returns the output unchanged if it lacks `PSBT_OUT_UNIQUE_ID`.
    // The large Err is deliberate: it hands the rejected Output back to the caller.
    #[allow(clippy::result_large_err)]
    pub fn try_add(&mut self, output: Output) -> Result<(), Output> {
        let uid = OutputUniqueIdExt::unique_id(&output).ok_or_else(|| output.clone())?;
        let value = output.wrap();
        match self.0.remove(&uid) {
            Some(existing) => {
                self.0.insert(uid, existing.join(value));
            }
            None => {
                self.0.insert(uid, value);
            }
        }
        Ok(())
    }

    /// Create a `ResultOutputSet` from an iterator of outputs.
    ///
    /// Outputs with the same `PSBT_OUT_UNIQUE_ID` are merged via `Join`,
    /// producing conflicts for differing fields. This is infallible with
    /// respect to duplicates — they become `JoinResult::Err(Conflict(...))`.
    ///
    /// # Errors
    /// Returns the first output missing a `PSBT_OUT_UNIQUE_ID` field.
    // The large Err is deliberate: it hands the rejected Output back to the caller.
    #[allow(clippy::result_large_err)]
    pub fn try_from_outputs(iter: impl IntoIterator<Item = Output>) -> Result<Self, Output> {
        let mut set = Self::default();
        for output in iter {
            set.try_add(output)?;
        }
        Ok(set)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }

    #[allow(clippy::result_large_err)]
    pub fn try_unwrap(self) -> Result<OutputSet, Self> {
        match self.0.try_unwrap() {
            Ok(map) => Ok(OutputSet(map)),
            Err(map) => Err(Self(map)),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    use bitcoin::ScriptBuf;

    use crate::psbt::output::PSBT_OUT_UNIQUE_ID_SUBTYPE;

    #[cfg(feature = "unit-tests")]
    mod unit_set {
        use super::*;

        fn output_with_uid(uid: &[u8]) -> Output {
            let mut output = Output::default();
            let key = psbt_v2::raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                key: vec![],
            };
            output.proprietaries.insert(key, uid.to_vec());
            output
        }

        #[test]
        fn empty_output_set() {
            let set = OutputSet::default();
            assert!(set.is_empty());
            assert_eq!(set.len(), 0);
        }

        #[test]
        fn add_output() {
            let mut set = OutputSet::default();
            set.add(output_with_uid(&[1]));
            assert_eq!(set.len(), 1);
            assert!(!set.is_empty());
        }

        #[test]
        fn wrap_empty_set_is_ok() {
            let set = OutputSet::default().wrap();
            assert!(set.is_ok());
            assert!(set.is_empty());
            assert_eq!(set.len(), 0);
        }

        #[test]
        fn output_set_try_unwrap_ok() {
            let mut set = OutputSet::default();
            set.add(output_with_uid(&[1]));
            assert!(set.wrap().try_unwrap().is_ok());
        }

        #[test]
        fn output_set_conflicting_try_unwrap_err() {
            // Same UniqueId but different amounts → conflict
            let mut a = OutputSet::default();
            let mut out_a = output_with_uid(&[1]);
            out_a.amount = bitcoin::Amount::from_sat(100);
            a.add(out_a);

            let mut b = OutputSet::default();
            let mut out_b = output_with_uid(&[1]);
            out_b.amount = bitcoin::Amount::from_sat(200);
            b.add(out_b);

            let joined = a.wrap().join(b.wrap());
            assert!(!joined.is_ok());
            assert!(joined.try_unwrap().is_err());
        }

        #[test]
        fn join_disjoint_output_sets() {
            let mut a = OutputSet::default();
            a.add(output_with_uid(&[1]));
            let mut b = OutputSet::default();
            b.add(output_with_uid(&[2]));
            assert!(a.wrap().join(b.wrap()).is_ok());
        }

        #[test]
        fn try_from_outputs_ok() {
            let outputs = vec![output_with_uid(&[1]), output_with_uid(&[2])];
            assert!(OutputSet::try_from_outputs(outputs).is_ok());
        }

        #[test]
        fn try_from_outputs_missing_uid() {
            let outputs = vec![Output::default()]; // no UniqueId
            assert!(OutputSet::try_from_outputs(outputs).is_err());
        }

        #[test]
        fn from_iter_collects_outputs() {
            let outputs = vec![output_with_uid(&[1]), output_with_uid(&[2])];
            let set: OutputSet = outputs.into_iter().collect();
            assert_eq!(set.len(), 2);
        }

        #[test]
        fn result_try_from_outputs_ok() {
            let outputs = vec![output_with_uid(&[1]), output_with_uid(&[2])];
            let result = ResultOutputSet::try_from_outputs(outputs);
            assert!(result.is_ok());
            assert!(result.unwrap().is_ok());
        }

        #[test]
        fn result_try_from_outputs_missing_uid() {
            let outputs = vec![Output::default()];
            assert!(ResultOutputSet::try_from_outputs(outputs).is_err());
        }

        #[test]
        fn result_try_from_outputs_duplicate_uid_identical() {
            let outputs = vec![output_with_uid(&[1]), output_with_uid(&[1])];
            let result = ResultOutputSet::try_from_outputs(outputs).unwrap();
            // Identical outputs merge cleanly
            assert!(result.is_ok());
        }

        #[test]
        fn result_try_from_outputs_duplicate_uid_conflicting() {
            use bitcoin::Amount;
            let mut a = output_with_uid(&[1]);
            let mut b = output_with_uid(&[1]);
            a.amount = Amount::from_sat(1000);
            b.amount = Amount::from_sat(2000);
            let result = ResultOutputSet::try_from_outputs(vec![a, b]).unwrap();
            // Different amounts on same UID → conflict
            assert!(!result.is_ok());
        }

        #[test]
        fn result_try_add_duplicate_uid_identical_is_ok() {
            let output = output_with_uid(&[1]);
            let mut result = ResultOutputSet::default();
            result.try_add(output.clone()).unwrap();
            result.try_add(output).unwrap();
            assert_eq!(result.len(), 1);
            assert!(result.is_ok());
            assert!(result.try_unwrap().is_ok());
        }

        #[test]
        fn result_try_add_duplicate_uid_conflicting_is_err() {
            use bitcoin::Amount;
            let mut a = output_with_uid(&[1]);
            let mut b = output_with_uid(&[1]);
            a.amount = Amount::from_sat(1000);
            b.amount = Amount::from_sat(2000);

            let mut result = ResultOutputSet::default();
            result.try_add(a).unwrap();
            result.try_add(b).unwrap();

            assert_eq!(result.len(), 1);
            assert!(!result.is_ok());
            assert!(result.try_unwrap().is_err());
        }

        #[test]
        fn result_try_add_missing_uid_is_err() {
            let mut result = ResultOutputSet::default();
            assert!(result.try_add(Output::default()).is_err());
            assert!(result.is_empty());
        }

        #[test]
        fn into_iter_yields_outputs() {
            let mut set = OutputSet::default();
            set.add(output_with_uid(&[1]));
            set.add(output_with_uid(&[2]));
            let values: Vec<Output> = set.into_iter().collect();
            assert_eq!(values.len(), 2);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop_set {
        use super::*;
        use proptest::prelude::*;

        fn arb_uid() -> impl Strategy<Value = Vec<u8>> {
            proptest::collection::vec(any::<u8>(), 1..=4)
        }

        fn arb_output_with_uid() -> impl Strategy<Value = Output> {
            (
                arb_uid(),
                any::<u64>(),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)), // redeem_script
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)), // witness_script
                proptest::collection::btree_map(
                    proptest::collection::vec(0u8..255, 1..=4),
                    proptest::collection::vec(0u8..255, 0..=4),
                    0..=2,
                ), // extra proprietaries
            )
                .prop_map(|(uid, amount, redeem, witness, extra_props)| {
                    let mut output = Output {
                        amount: bitcoin::Amount::from_sat(amount),
                        script_pubkey: ScriptBuf::new(),
                        redeem_script: redeem.map(ScriptBuf::from_bytes),
                        witness_script: witness.map(ScriptBuf::from_bytes),
                        ..Output::default()
                    };
                    // UID proprietary
                    let uid_key = psbt_v2::raw::ProprietaryKey {
                        prefix: b"concurrent-psbt".to_vec(),
                        subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                        key: vec![],
                    };
                    output.proprietaries.insert(uid_key, uid);
                    // Extra proprietaries to exercise BTreeMap join paths
                    for (k, v) in extra_props {
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

        fn arb_output_set() -> impl Strategy<Value = OutputSet> {
            proptest::collection::vec(arb_output_with_uid(), 0..=3).prop_map(|outputs| {
                OutputSet::try_from_outputs(outputs).expect("all outputs have UIDs")
            })
        }

        fn arb_result_output_set() -> impl Strategy<Value = ResultOutputSet> {
            arb_output_set().prop_map(|s| s.wrap())
        }

        proptest! {
            #[test]
            fn join_idempotent(a in arb_result_output_set()) {
                prop_assert_eq!(a.clone().join(a.clone()), a);
            }

            #[test]
            fn join_commutative(a in arb_result_output_set(), b in arb_result_output_set()) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn join_associative(
                a in arb_result_output_set(),
                b in arb_result_output_set(),
                c in arb_result_output_set(),
            ) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).join(c.clone()),
                    a.join(b.join(c)),
                );
            }

            #[test]
            fn wrap_try_unwrap_roundtrip(s in arb_output_set()) {
                let wrapped = s.wrap();
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn len_matches_iter_count(s in arb_output_set()) {
                prop_assert_eq!(s.len(), s.into_iter().count());
            }

            #[test]
            fn is_empty_matches_len(s in arb_output_set()) {
                #[allow(clippy::len_zero)]
                { prop_assert_eq!(s.is_empty(), s.len() == 0); }
            }

            #[test]
            fn add_to_empty_produces_nonempty(o in arb_output_with_uid()) {
                let mut s = OutputSet::default();
                prop_assert!(s.is_empty());
                s.add(o);
                prop_assert!(!s.is_empty());
                prop_assert_eq!(s.len(), 1);
            }

            #[test]
            fn add_increments_or_preserves_len(
                mut s in arb_output_set(),
                o in arb_output_with_uid(),
            ) {
                let before = s.len();
                s.add(o);
                prop_assert!(s.len() == before || s.len() == before + 1);
            }

            #[test]
            fn from_iter_equivalent_to_try_from(outputs in proptest::collection::vec(arb_output_with_uid(), 0..=3)) {
                let from_iter: OutputSet = outputs.clone().into_iter().collect();
                let try_from = OutputSet::try_from_outputs(outputs).unwrap();
                prop_assert_eq!(from_iter.len(), try_from.len());
            }

            #[test]
            fn is_ok_consistency(r in arb_result_output_set()) {
                #[allow(clippy::len_zero)]
                {
                    prop_assert_eq!(r.is_empty(), r.len() == 0);
                }
                if r.is_ok() {
                    prop_assert!(r.try_unwrap().is_ok());
                } else {
                    prop_assert!(r.try_unwrap().is_err());
                }
            }

            #[test]
            fn result_try_from_wraps_and_joins(outputs in proptest::collection::vec(arb_output_with_uid(), 1..=3)) {
                let result = ResultOutputSet::try_from_outputs(outputs);
                prop_assert!(result.is_ok());
            }

            #[test]
            fn result_try_from_duplicate_uid_merges(a in arb_output_with_uid(), b in arb_output_with_uid()) {
                // Force same UID on both
                let uid_key = psbt_v2::raw::ProprietaryKey {
                    prefix: b"concurrent-psbt".to_vec(),
                    subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                    key: vec![],
                };
                let uid = vec![42u8];
                let mut a = a;
                let mut b = b;
                a.proprietaries.insert(uid_key.clone(), uid.clone());
                b.proprietaries.insert(uid_key, uid);
                let result = ResultOutputSet::try_from_outputs(vec![a, b]);
                prop_assert!(result.is_ok()); // should not error, may have conflicts inside
            }

            #[test]
            fn try_from_no_uid_is_err(amount in any::<u64>()) {
                let output = Output {
                    amount: bitcoin::Amount::from_sat(amount),
                    ..Output::default()
                };
                prop_assert!(OutputSet::try_from_outputs(vec![output]).is_err());
            }

            #[test]
            fn result_try_from_no_uid_is_err(amount in any::<u64>()) {
                let output = Output {
                    amount: bitcoin::Amount::from_sat(amount),
                    ..Output::default()
                };
                prop_assert!(ResultOutputSet::try_from_outputs(vec![output]).is_err());
            }

            #[test]
            fn try_unwrap_err_on_conflict(
                uid in arb_uid(),
                a_amount in any::<u64>(),
                b_amount in any::<u64>(),
            ) {
                prop_assume!(a_amount != b_amount);
                let uid_key = psbt_v2::raw::ProprietaryKey {
                    prefix: b"concurrent-psbt".to_vec(),
                    subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                    key: vec![],
                };
                let mut out_a = Output {
                    amount: bitcoin::Amount::from_sat(a_amount),
                    ..Output::default()
                };
                out_a.proprietaries.insert(uid_key.clone(), uid.clone());
                let mut out_b = Output {
                    amount: bitcoin::Amount::from_sat(b_amount),
                    ..Output::default()
                };
                out_b.proprietaries.insert(uid_key, uid);
                let mut set_a = OutputSet::default();
                set_a.add(out_a);
                let mut set_b = OutputSet::default();
                set_b.add(out_b);
                let joined = set_a.wrap().join(set_b.wrap());
                prop_assert!(!joined.is_ok());
                prop_assert!(joined.try_unwrap().is_err());
            }
        }
    }
}
