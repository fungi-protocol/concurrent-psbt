#![allow(clippy::result_large_err)]

use std::fmt;

use psbt_v2::v2::{Global, Output, Psbt};

use crate::global::{GlobalExt, ResultGlobal};
use crate::input::{InputSet, ResultInputSet};
use crate::lattice::join::Join;
use crate::lattice::partial::{Conflict, JoinResult};
use crate::output::{OutputSet, ResultOutputSet};

/// A BIP 370 PSBT decomposed into [`Global`], [`InputSet`], and [`OutputSet`], without ordering.
///
/// Inputs are keyed by outpoint and outputs by unique ID, enabling conflict-safe joins.
/// Use [`UnorderedPsbt::wrap`] to enter the result domain for concurrent merging.
#[derive(Debug, Clone, PartialEq)]
pub struct UnorderedPsbt {
    pub global: Global,
    pub inputs: InputSet,
    pub outputs: OutputSet,
}

/// Result-domain version of [`UnorderedPsbt`], where every field tracks either
/// a clean value or accumulated conflicts.
///
/// Produced by [`UnorderedPsbt::wrap`] or [`ResultUnorderedPsbt::try_from_psbt`].
/// Implements [`Join`] so multiple PSBTs can be merged without information loss.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultUnorderedPsbt {
    pub(crate) global: ResultGlobal,
    pub(crate) inputs: ResultInputSet,
    pub(crate) outputs: ResultOutputSet,
}

/// Error returned when a clean [`UnorderedPsbt`] cannot be built from a PSBT.
#[derive(Debug, Clone, PartialEq)]
pub enum UnorderedPsbtError {
    /// An output is missing the `PSBT_OUT_UNIQUE_ID` proprietary field.
    MissingOutputUniqueId(Box<Output>),
    /// The PSBT accumulated conflicts while being converted to unordered form.
    Conflict(Box<ResultUnorderedPsbt>),
}

impl From<Output> for UnorderedPsbtError {
    fn from(output: Output) -> Self {
        Self::MissingOutputUniqueId(Box::new(output))
    }
}

impl fmt::Display for UnorderedPsbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingOutputUniqueId(_) => write!(f, "output missing PSBT_OUT_UNIQUE_ID"),
            Self::Conflict(_) => write!(f, "unordered PSBT contains conflicting fields"),
        }
    }
}

impl std::error::Error for UnorderedPsbtError {}

impl UnorderedPsbt {
    /// Parse a v2 PSBT into unordered representation.
    ///
    /// # Errors
    /// Returns an error if any output is missing `PSBT_OUT_UNIQUE_ID`, or if
    /// duplicate input/output keys accumulate conflicting field values.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        let result = ResultUnorderedPsbt::try_from_psbt(psbt)?;
        result
            .try_unwrap()
            .map_err(|result| UnorderedPsbtError::Conflict(Box::new(result)))
    }

    /// Convert to a BIP 370 [`Psbt`].
    ///
    /// **Warning:** Input and output ordering in the resulting PSBT is
    /// arbitrary (HashMap iteration order). Two UnorderedPsbt values
    /// that are join-equal may produce different Psbt serializations.
    /// Use `Sorter` to apply deterministic
    /// ordering before serializing for signing or comparison.
    ///
    /// The live-set projection is applied (tombstoned inputs/outputs are
    /// dropped) so callers see the same live set the sorter would produce. This
    /// is the NON-TERMINAL exit: unlike the sorter, it KEEPS the tombstone (and
    /// fee) proprietary fields in place, so the projection stays reproducible if
    /// the artifact is re-parsed back into the unordered domain. No-op when the
    /// `removal` feature is off (tombstones ignored, fail-safe).
    pub fn into_psbt(self) -> Psbt {
        let mut inputs: Vec<_> = self.inputs.into_iter().collect();
        let mut outputs: Vec<_> = self.outputs.into_iter().collect();
        let global = self.global;
        crate::removal::retain_live_inputs(&global, &mut inputs);
        crate::removal::retain_live_outputs(&global, &mut outputs);
        let mut global = global;
        global.input_count = inputs.len();
        global.output_count = outputs.len();
        Psbt {
            global,
            inputs,
            outputs,
        }
    }

    /// Lift this [`UnorderedPsbt`] into the result domain as a [`ResultUnorderedPsbt`] with all fields `Ok`.
    pub fn wrap(self) -> ResultUnorderedPsbt {
        let inputs = self.inputs.wrap();
        let outputs = self.outputs.wrap();
        let mut global = self.global;
        // Sync counts before wrapping so the ResultGlobal is consistent.
        global.input_count = inputs.len();
        global.output_count = outputs.len();
        ResultUnorderedPsbt {
            global: global.wrap(),
            inputs,
            outputs,
        }
    }
}

impl Join for ResultUnorderedPsbt {
    fn join(self, other: Self) -> Self {
        // Capture pre-join set sizes for consistency checks.
        let self_input_len = self.inputs.len();
        let self_output_len = self.outputs.len();
        let other_input_len = other.inputs.len();
        let other_output_len = other.outputs.len();

        let inputs = self.inputs.join(other.inputs);
        let outputs = self.outputs.join(other.outputs);
        let input_len = inputs.len();
        let output_len = outputs.len();

        // Count sync: check each operand's consistency, then update.
        //
        // For each operand, if its declared count matches its set size,
        // the count was consistent: update to the joined set size.
        // If not, the operand had an unflagged inconsistency: flag it.
        // If already Err(Conflict), preserve for joining.
        let mut self_global = self.global;
        let mut other_global = other.global;

        fn sync_count(count: &mut JoinResult<usize>, pre_len: usize, post_len: usize) {
            if let Ok(n) = *count {
                if n == pre_len {
                    *count = Ok(post_len);
                } else {
                    *count = Err(Conflict::from_values([n, pre_len]));
                }
            }
        }

        sync_count(&mut self_global.input_count, self_input_len, input_len);
        sync_count(&mut self_global.output_count, self_output_len, output_len);
        sync_count(&mut other_global.input_count, other_input_len, input_len);
        sync_count(&mut other_global.output_count, other_output_len, output_len);

        // Now join globals. Consistent counts are both Ok(joined_len)
        // and merge cleanly. Flagged inconsistencies join via Conflict.
        let global = self_global.join(other_global);

        ResultUnorderedPsbt {
            global,
            inputs,
            outputs,
        }
    }
}

impl ResultUnorderedPsbt {
    /// Parse a v2 PSBT directly into the result domain.
    ///
    /// Duplicate input outpoints and output UIDs produce conflicts rather than
    /// being overwritten.
    ///
    /// # Errors
    /// Returns the first output missing a `PSBT_OUT_UNIQUE_ID` field.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        let inputs = ResultInputSet::from_inputs(psbt.inputs);
        let outputs = ResultOutputSet::try_from_outputs(psbt.outputs)?;

        let mut global = psbt.global.wrap();

        // Flag inconsistent counts as conflicts rather than silently correcting.
        // The global count fields dictate parsing; mismatches indicate a
        // malformed PSBT, surfaced as a conflict singleton.
        if let Ok(n) = &global.input_count
            && *n != inputs.len()
        {
            global.input_count = Err(Conflict::from_values([*n, inputs.len()]));
        }
        if let Ok(n) = &global.output_count
            && *n != outputs.len()
        {
            global.output_count = Err(Conflict::from_values([*n, outputs.len()]));
        }

        Ok(Self {
            global,
            inputs,
            outputs,
        })
    }

    /// Return `true` if every field across global, inputs, and outputs is conflict-free.
    pub fn is_ok(&self) -> bool {
        self.global.is_ok() && self.inputs.is_ok() && self.outputs.is_ok()
    }

    /// Extract a clean [`UnorderedPsbt`] if there are no conflicts, otherwise return `self`.
    ///
    /// # Errors
    /// Returns `Err(self)` if any field contains a conflict.
    pub fn try_unwrap(self) -> Result<UnorderedPsbt, Self> {
        if !self.is_ok() {
            return Err(self);
        }
        Ok(UnorderedPsbt {
            global: self
                .global
                .try_unwrap()
                .expect("verified all fields are Ok"),
            inputs: self
                .inputs
                .try_unwrap()
                .expect("verified all fields are Ok"),
            outputs: self
                .outputs
                .try_unwrap()
                .expect("verified all fields are Ok"),
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;

    #[cfg(any(feature = "unit-tests", feature = "prop-tests"))]
    #[test]
    fn join_flags_preexisting_count_mismatch() {
        let empty = || UnorderedPsbt {
            global: Global::default(),
            inputs: InputSet::default(),
            outputs: OutputSet::default(),
        };
        let mut inconsistent = empty().wrap();
        inconsistent.global.input_count = Ok(1);

        let joined = inconsistent.join(empty().wrap());

        assert_eq!(
            joined.global.input_count,
            Err(Conflict::from_values([1, 0]))
        );

        let rejoined = joined.join(empty().wrap());
        assert_eq!(
            rejoined.global.input_count,
            Err(Conflict::from_values([1, 0]))
        );
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use bitcoin::transaction;

        fn make_unordered_psbt() -> UnorderedPsbt {
            UnorderedPsbt {
                global: Global::default(),
                inputs: InputSet::default(),
                outputs: OutputSet::default(),
            }
        }

        #[test]
        fn wrap_default_is_ok() {
            assert!(make_unordered_psbt().wrap().is_ok());
        }

        #[test]
        fn wrap_try_unwrap_roundtrip() {
            assert!(make_unordered_psbt().wrap().try_unwrap().is_ok());
        }

        #[test]
        fn join_identical_is_ok() {
            let joined = make_unordered_psbt()
                .wrap()
                .join(make_unordered_psbt().wrap());
            assert!(joined.is_ok());
        }

        #[test]
        fn join_conflicting_globals_try_unwrap_err() {
            let mut a = make_unordered_psbt();
            let mut b = make_unordered_psbt();
            a.global.tx_version = transaction::Version::ONE;
            b.global.tx_version = transaction::Version::TWO;
            let joined = a.wrap().join(b.wrap());
            assert!(!joined.is_ok());
            assert!(joined.try_unwrap().is_err());
        }

        #[test]
        fn unordered_error_display_covers_both_variants() {
            let missing = UnorderedPsbtError::MissingOutputUniqueId(Box::default());
            assert_eq!(missing.to_string(), "output missing PSBT_OUT_UNIQUE_ID");

            let conflict = UnorderedPsbtError::Conflict(Box::new(make_unordered_psbt().wrap()));
            assert_eq!(
                conflict.to_string(),
                "unordered PSBT contains conflicting fields"
            );
        }

        #[test]
        fn join_syncs_input_count() {
            let mut a = make_unordered_psbt();
            a.global.input_count = 0;
            let mut b = make_unordered_psbt();
            b.global.input_count = 0;
            let joined = a.wrap().join(b.wrap());
            // Both have 0 inputs, joined set has 0, count should be 0
            assert_eq!(joined.global.input_count, Ok(0));
        }

        #[test]
        fn into_psbt_empty() {
            let unordered = make_unordered_psbt();
            let psbt = unordered.into_psbt();
            assert!(psbt.inputs.is_empty());
            assert!(psbt.outputs.is_empty());
        }

        #[test]
        fn try_from_psbt_empty() {
            let psbt = Psbt {
                global: Global::default(),
                inputs: vec![],
                outputs: vec![],
            };
            let unordered = UnorderedPsbt::try_from_psbt(psbt).unwrap();
            assert!(unordered.inputs.is_empty());
            assert!(unordered.outputs.is_empty());
        }

        #[test]
        fn try_from_psbt_no_uid_err() {
            let psbt = Psbt {
                global: Global::default(),
                inputs: vec![],
                outputs: vec![Output::default()],
            };
            assert!(UnorderedPsbt::try_from_psbt(psbt).is_err());
        }

        #[test]
        fn result_try_from_psbt_empty() {
            let psbt = Psbt {
                global: Global::default(),
                inputs: vec![],
                outputs: vec![],
            };
            let result = ResultUnorderedPsbt::try_from_psbt(psbt).unwrap();
            assert!(result.is_ok());
        }

        #[test]
        fn result_try_from_psbt_no_uid_err() {
            let psbt = Psbt {
                global: Global::default(),
                inputs: vec![],
                outputs: vec![Output::default()],
            };
            assert!(ResultUnorderedPsbt::try_from_psbt(psbt).is_err());
        }

        #[test]
        fn result_try_from_psbt_duplicate_input_conflicts() {
            use bitcoin::hashes::Hash;

            let op = bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([1; 32]),
                vout: 0,
            };
            let mut a = psbt_v2::v2::Input::new(&op);
            let mut b = psbt_v2::v2::Input::new(&op);
            a.sequence = Some(bitcoin::Sequence(1));
            b.sequence = Some(bitcoin::Sequence(2));

            let psbt = Psbt {
                global: Global {
                    input_count: 1,
                    ..Global::default()
                },
                inputs: vec![a, b],
                outputs: vec![],
            };

            let result = ResultUnorderedPsbt::try_from_psbt(psbt).unwrap();
            assert_eq!(result.inputs.len(), 1);
            assert!(!result.is_ok());
            assert!(result.try_unwrap().is_err());
        }

        #[test]
        fn try_from_psbt_duplicate_input_conflict_is_err() {
            use bitcoin::hashes::Hash;

            let op = bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([1; 32]),
                vout: 0,
            };
            let mut a = psbt_v2::v2::Input::new(&op);
            let mut b = psbt_v2::v2::Input::new(&op);
            a.sequence = Some(bitcoin::Sequence(1));
            b.sequence = Some(bitcoin::Sequence(2));

            let psbt = Psbt {
                global: Global {
                    input_count: 1,
                    ..Global::default()
                },
                inputs: vec![a, b],
                outputs: vec![],
            };

            assert!(matches!(
                UnorderedPsbt::try_from_psbt(psbt),
                Err(UnorderedPsbtError::Conflict(_))
            ));
        }

        #[test]
        fn try_from_psbt_duplicate_output_conflict_is_err() {
            use crate::output::PSBT_OUT_UNIQUE_ID_SUBTYPE;
            use bitcoin::Amount;

            fn output_with_uid(uid: &[u8], amount: u64) -> Output {
                let mut output = Output {
                    amount: Amount::from_sat(amount),
                    ..Output::default()
                };
                let key = psbt_v2::raw::ProprietaryKey {
                    prefix: b"concurrent-psbt".to_vec(),
                    subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                    key: vec![],
                };
                output.proprietaries.insert(key, uid.to_vec());
                output
            }

            let psbt = Psbt {
                global: Global {
                    output_count: 1,
                    ..Global::default()
                },
                inputs: vec![],
                outputs: vec![output_with_uid(&[1], 1000), output_with_uid(&[1], 2000)],
            };

            assert!(matches!(
                UnorderedPsbt::try_from_psbt(psbt),
                Err(UnorderedPsbtError::Conflict(_))
            ));
        }

        #[test]
        fn result_try_from_psbt_count_mismatch_input() {
            // Global says 5 inputs, but PSBT has 0 → conflict
            let psbt = Psbt {
                global: Global {
                    input_count: 5,
                    ..Global::default()
                },
                inputs: vec![],
                outputs: vec![],
            };
            let result = ResultUnorderedPsbt::try_from_psbt(psbt).unwrap();
            assert!(!result.is_ok(), "count mismatch should produce a conflict");
        }

        #[test]
        fn result_try_from_psbt_count_mismatch_output() {
            // Global says 3 outputs, but PSBT has 0 → conflict
            let psbt = Psbt {
                global: Global {
                    output_count: 3,
                    ..Global::default()
                },
                inputs: vec![],
                outputs: vec![],
            };
            let result = ResultUnorderedPsbt::try_from_psbt(psbt).unwrap();
            assert!(!result.is_ok(), "count mismatch should produce a conflict");
        }

        #[test]
        fn result_try_from_psbt_consistent_counts_ok() {
            // Counts match → no conflict
            let psbt = Psbt {
                global: Global::default(), // input_count=0, output_count=0
                inputs: vec![],
                outputs: vec![],
            };
            let result = ResultUnorderedPsbt::try_from_psbt(psbt).unwrap();
            assert!(result.is_ok(), "consistent counts should be clean");
        }

        #[test]
        fn into_psbt_preserves_empty() {
            let unordered = make_unordered_psbt();
            let in_count = unordered.inputs.len();
            let out_count = unordered.outputs.len();
            let psbt = unordered.into_psbt();
            assert_eq!(psbt.inputs.len(), in_count);
            assert_eq!(psbt.outputs.len(), out_count);
        }

        #[test]
        fn join_syncs_input_output_counts() {
            let a = make_unordered_psbt();
            let b = make_unordered_psbt();
            let joined = a.wrap().join(b.wrap());
            assert_eq!(joined.global.input_count, Ok(joined.inputs.len()));
            assert_eq!(joined.global.output_count, Ok(joined.outputs.len()));
        }

        fn make_populated_psbt(txid_byte: u8) -> UnorderedPsbt {
            use crate::output::PSBT_OUT_UNIQUE_ID_SUBTYPE;
            use bitcoin::hashes::Hash;

            let input = psbt_v2::v2::Input::new(&bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                vout: 0,
            });
            let mut inputs = InputSet::default();
            inputs.add(input);

            let mut output = psbt_v2::v2::Output {
                amount: bitcoin::Amount::from_sat(100_000),
                script_pubkey: bitcoin::ScriptBuf::new_op_return([txid_byte; 20]),
                ..Default::default()
            };
            let key = psbt_v2::raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                key: vec![],
            };
            output.proprietaries.insert(key, vec![txid_byte; 16]);
            let mut outputs = OutputSet::default();
            outputs.add(output);

            UnorderedPsbt {
                global: Global::default(),
                inputs,
                outputs,
            }
        }

        #[test]
        fn into_psbt_syncs_counts() {
            let u = make_populated_psbt(1);
            assert_eq!(u.inputs.len(), 1);
            let psbt = u.into_psbt();
            assert_eq!(
                psbt.global.input_count,
                psbt.inputs.len(),
                "into_psbt must sync input_count"
            );
            assert_eq!(
                psbt.global.output_count,
                psbt.outputs.len(),
                "into_psbt must sync output_count"
            );
        }

        #[test]
        fn serialize_deserialize_roundtrip() {
            let psbt = make_populated_psbt(1).into_psbt();
            let bytes = Psbt::serialize(&psbt);
            let rt = Psbt::deserialize(&bytes).expect("deserialize should succeed");
            assert_eq!(rt.inputs.len(), 1, "roundtrip inputs");
            assert_eq!(rt.outputs.len(), 1, "roundtrip outputs");
        }

        #[test]
        fn join_disjoint_inputs_no_count_conflict() {
            let a = make_populated_psbt(1);
            let b = make_populated_psbt(2);
            let result = a.wrap().join(b.wrap());
            assert!(
                result.is_ok(),
                "join of disjoint inputs should be clean: {result:#?}"
            );
        }

        #[test]
        fn three_way_join_no_count_conflict() {
            let result = make_populated_psbt(1)
                .wrap()
                .join(make_populated_psbt(2).wrap())
                .join(make_populated_psbt(3).wrap());
            assert!(
                result.is_ok(),
                "three-way join should be clean: {result:#?}"
            );
            let unwrapped = result.try_unwrap().expect("clean");
            assert_eq!(unwrapped.inputs.len(), 3);
            assert_eq!(unwrapped.outputs.len(), 3);
        }

        #[test]
        fn serialize_join_roundtrip() {
            let psbts: Vec<Vec<u8>> = (1..=3)
                .map(|i| Psbt::serialize(&make_populated_psbt(i).into_psbt()))
                .collect();
            let joined = psbts
                .iter()
                .map(|bytes| {
                    let psbt = Psbt::deserialize(bytes).expect("deser");
                    UnorderedPsbt::try_from_psbt(psbt).expect("try_from").wrap()
                })
                .reduce(|a, b| a.join(b))
                .unwrap();
            assert!(
                joined.is_ok(),
                "serialize-join roundtrip should be clean: {joined:#?}"
            );
            let unwrapped = joined.try_unwrap().expect("clean");
            assert_eq!(unwrapped.inputs.len(), 3);
            assert_eq!(unwrapped.outputs.len(), 3);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use bitcoin::transaction;
        use proptest::prelude::*;

        /// Generate a random UnorderedPsbt with varied Global fields
        /// and populated InputSet/OutputSet (0-3 entries each).
        fn arb_unordered_psbt() -> impl Strategy<Value = UnorderedPsbt> {
            (
                proptest::bool::ANY,                                 // tx_version: ONE or TWO
                0u8..4,                                              // tx_modifiable_flags
                proptest::collection::vec((0u8..5, 0u32..2), 0..=3), // inputs: (txid_byte, vout)
                proptest::collection::vec(0u8..10, 0..=3),           // outputs: uid_byte
            )
                .prop_map(|(use_v1, flags, input_specs, output_uids)| {
                    use crate::output::PSBT_OUT_UNIQUE_ID_SUBTYPE;
                    use bitcoin::hashes::Hash;

                    let mut inputs = InputSet::default();
                    for (txid_byte, vout) in input_specs {
                        inputs.add(psbt_v2::v2::Input::new(&bitcoin::OutPoint {
                            txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                            vout,
                        }));
                    }

                    let mut outputs = Vec::new();
                    for uid_byte in output_uids {
                        let mut output = psbt_v2::v2::Output {
                            amount: bitcoin::Amount::from_sat(100_000),
                            script_pubkey: bitcoin::ScriptBuf::new_op_return([uid_byte; 20]),
                            ..Default::default()
                        };
                        let key = psbt_v2::raw::ProprietaryKey {
                            prefix: b"concurrent-psbt".to_vec(),
                            subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                            key: vec![],
                        };
                        output.proprietaries.insert(key, vec![uid_byte; 16]);
                        outputs.push(output);
                    }
                    let output_set =
                        OutputSet::try_from_outputs(outputs).expect("all outputs have UIDs");

                    UnorderedPsbt {
                        global: Global {
                            tx_version: if use_v1 {
                                transaction::Version::ONE
                            } else {
                                transaction::Version::TWO
                            },
                            tx_modifiable_flags: flags,
                            input_count: inputs.len(),
                            output_count: output_set.len(),
                            ..Global::default()
                        },
                        inputs,
                        outputs: output_set,
                    }
                })
        }

        /// Strategy producing ResultUnorderedPsbt: either a single wrapped value
        /// or a pre-conflicted value obtained by joining two different ones.
        fn arb_result_unordered_psbt() -> impl Strategy<Value = ResultUnorderedPsbt> {
            prop_oneof![
                arb_unordered_psbt().prop_map(|p| p.wrap()),
                (arb_unordered_psbt(), arb_unordered_psbt())
                    .prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        proptest! {
            #[test]
            fn idempotent(a in arb_result_unordered_psbt()) {
                let cloned = a.clone();
                prop_assert_eq!(a.clone().join(cloned), a);
            }

            #[test]
            fn commutative(a in arb_result_unordered_psbt(), b in arb_result_unordered_psbt()) {
                let ab = a.clone().join(b.clone());
                let ba = b.join(a);
                prop_assert_eq!(ab, ba);
            }

            #[test]
            fn associative(
                a in arb_result_unordered_psbt(),
                b in arb_result_unordered_psbt(),
                c in arb_result_unordered_psbt(),
            ) {
                let ab_c = a.clone().join(b.clone()).join(c.clone());
                let a_bc = a.join(b.join(c));
                prop_assert_eq!(ab_c, a_bc);
            }

            #[test]
            fn wrap_try_unwrap_roundtrip(a in arb_unordered_psbt()) {
                let wrapped = a.wrap();
                prop_assert_eq!(&wrapped.global.input_count, &Ok(wrapped.inputs.len()));
                prop_assert_eq!(&wrapped.global.output_count, &Ok(wrapped.outputs.len()));
                let unwrapped = wrapped.clone().try_unwrap().expect("freshly wrapped should unwrap");
                // Re-wrap and compare in the result domain
                let re_wrapped = unwrapped.wrap();
                prop_assert_eq!(re_wrapped, wrapped);
            }

            #[test]
            fn into_psbt_preserves_counts(a in arb_unordered_psbt()) {
                let in_count = a.inputs.len();
                let out_count = a.outputs.len();
                let psbt = a.into_psbt();
                prop_assert_eq!(psbt.inputs.len(), in_count);
                prop_assert_eq!(psbt.outputs.len(), out_count);
                prop_assert_eq!(psbt.global.input_count, psbt.inputs.len());
                prop_assert_eq!(psbt.global.output_count, psbt.outputs.len());
            }

            #[test]
            fn join_syncs_counts(a in arb_unordered_psbt(), b in arb_unordered_psbt()) {
                let joined = a.wrap().join(b.wrap());
                // Count fields should match the joined set sizes
                prop_assert_eq!(joined.global.input_count, Ok(joined.inputs.len()));
                prop_assert_eq!(joined.global.output_count, Ok(joined.outputs.len()));
            }

            #[test]
            fn is_ok_try_unwrap_consistent(r in arb_result_unordered_psbt()) {
                if r.is_ok() {
                    prop_assert!(r.try_unwrap().is_ok());
                } else {
                    prop_assert!(r.try_unwrap().is_err());
                }
            }

            #[test]
            fn try_from_psbt_empty_roundtrips(use_v1 in proptest::bool::ANY) {
                let psbt = Psbt {
                    global: Global {
                        tx_version: if use_v1 { transaction::Version::ONE } else { transaction::Version::TWO },
                        ..Global::default()
                    },
                    inputs: vec![],
                    outputs: vec![],
                };
                let unordered = UnorderedPsbt::try_from_psbt(psbt).unwrap();
                prop_assert!(unordered.inputs.is_empty());
                prop_assert!(unordered.outputs.is_empty());
            }

            #[test]
            fn result_try_from_psbt_empty_roundtrips(use_v1 in proptest::bool::ANY) {
                let psbt = Psbt {
                    global: Global {
                        tx_version: if use_v1 { transaction::Version::ONE } else { transaction::Version::TWO },
                        ..Global::default()
                    },
                    inputs: vec![],
                    outputs: vec![],
                };
                let result = ResultUnorderedPsbt::try_from_psbt(psbt).unwrap();
                prop_assert!(result.is_ok());
            }

            #[test]
            fn result_try_from_psbt_flags_count_mismatches(
                input_count in 1usize..10,
                output_count in 1usize..10,
            ) {
                let psbt = Psbt {
                    global: Global {
                        input_count,
                        output_count,
                        ..Global::default()
                    },
                    inputs: vec![],
                    outputs: vec![],
                };

                let result = ResultUnorderedPsbt::try_from_psbt(psbt).unwrap();

                prop_assert_eq!(
                    result.global.input_count,
                    Err(Conflict::from_values([input_count, 0]))
                );
                prop_assert_eq!(
                    result.global.output_count,
                    Err(Conflict::from_values([output_count, 0]))
                );
            }

            #[test]
            fn try_from_psbt_no_uid_is_err(use_v1 in proptest::bool::ANY) {
                let psbt = Psbt {
                    global: Global::default(),
                    inputs: vec![],
                    outputs: vec![Output::default()], // no UID
                };
                let _ = use_v1;
                prop_assert!(UnorderedPsbt::try_from_psbt(psbt).is_err());
            }

            #[test]
            fn result_try_from_psbt_no_uid_is_err(use_v1 in proptest::bool::ANY) {
                let psbt = Psbt {
                    global: Global::default(),
                    inputs: vec![],
                    outputs: vec![Output::default()],
                };
                let _ = use_v1;
                prop_assert!(ResultUnorderedPsbt::try_from_psbt(psbt).is_err());
            }

            #[test]
            fn conflicting_try_unwrap_err(use_v1 in proptest::bool::ANY) {
                let a = UnorderedPsbt {
                    global: Global {
                        tx_version: transaction::Version::ONE,
                        ..Global::default()
                    },
                    inputs: InputSet::default(),
                    outputs: OutputSet::default(),
                };
                let b = UnorderedPsbt {
                    global: Global {
                        tx_version: transaction::Version::TWO,
                        ..Global::default()
                    },
                    inputs: InputSet::default(),
                    outputs: OutputSet::default(),
                };
                let _ = use_v1; // satisfy proptest parameter requirement
                let joined = a.wrap().join(b.wrap());
                prop_assert!(!joined.is_ok());
                prop_assert!(joined.try_unwrap().is_err());
            }

            #[test]
            fn try_from_psbt_duplicate_input_conflict_is_err(
                txid_byte in any::<u8>(),
                first_sequence in 0u32..100,
                second_sequence in 100u32..200,
            ) {
                use bitcoin::hashes::Hash;

                let outpoint = bitcoin::OutPoint {
                    txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                    vout: 0,
                };
                let first = psbt_v2::v2::Input {
                    sequence: Some(bitcoin::Sequence(first_sequence)),
                    ..psbt_v2::v2::Input::new(&outpoint)
                };
                let second = psbt_v2::v2::Input {
                    sequence: Some(bitcoin::Sequence(second_sequence)),
                    ..psbt_v2::v2::Input::new(&outpoint)
                };
                let psbt = Psbt {
                    global: Global {
                        input_count: 2,
                        ..Global::default()
                    },
                    inputs: vec![first, second],
                    outputs: vec![],
                };

                prop_assert!(matches!(
                    UnorderedPsbt::try_from_psbt(psbt),
                    Err(UnorderedPsbtError::Conflict(_))
                ));
            }

            #[test]
            fn unordered_error_display(use_conflict in proptest::bool::ANY) {
                let error = if use_conflict {
                    UnorderedPsbtError::Conflict(Box::new(UnorderedPsbt {
                        global: Global::default(),
                        inputs: InputSet::default(),
                        outputs: OutputSet::default(),
                    }.wrap()))
                } else {
                    UnorderedPsbtError::MissingOutputUniqueId(Box::default())
                };

                let rendered = error.to_string();
                prop_assert!(rendered == "output missing PSBT_OUT_UNIQUE_ID"
                    || rendered == "unordered PSBT contains conflicting fields");
            }
        }
    }
}
