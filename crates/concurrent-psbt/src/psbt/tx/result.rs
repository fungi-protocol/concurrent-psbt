use psbt_v2::v2::Psbt;

use crate::global::{GlobalExt, ResultGlobal};
use crate::input::ResultInputSet;
use crate::lattice::join::Join;
use crate::lattice::partial::Conflict;
use crate::output::ResultOutputSet;

use super::sized_set::SizedSet;
use super::tx_modifiability_flags::TxModifiableFlags;
use super::{UnorderedPsbt, UnorderedPsbtError};

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

impl ResultUnorderedPsbt {
    fn into_sized_sets(
        self,
    ) -> (
        ResultGlobal,
        SizedSet<ResultInputSet, { TxModifiableFlags::INPUTS_BIT }>,
        SizedSet<ResultOutputSet, { TxModifiableFlags::OUTPUTS_BIT }>,
    ) {
        let Self {
            global,
            inputs,
            outputs,
        } = self;

        let inputs = SizedSet::new(&global, inputs);
        let outputs = SizedSet::new(&global, outputs);

        (global, inputs, outputs)
    }
}

impl Join for ResultUnorderedPsbt {
    fn join(self, other: Self) -> Self {
        let (self_global, self_inputs, self_outputs) = self.into_sized_sets();
        let (other_global, other_inputs, other_outputs) = other.into_sized_sets();

        let inputs = self_inputs.join(other_inputs);
        let outputs = self_outputs.join(other_outputs);

        let inputs_modifiability = inputs.modifiability().clone();
        let outputs_modifiability = outputs.modifiability().clone();

        let self_flags = TxModifiableFlags::from(&self_global);
        let other_flags = TxModifiableFlags::from(&other_global);

        let tx_modifiable_flags =
            self_flags.complete_join(other_flags, inputs_modifiability, outputs_modifiability);

        let mut global = self_global.join(other_global);
        global.tx_modifiable_flags = tx_modifiable_flags;

        let (input_count, inputs) = inputs.into_parts();
        let (output_count, outputs) = outputs.into_parts();

        global.input_count = input_count;
        global.output_count = output_count;

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
        let outputs = ResultOutputSet::try_from_outputs(psbt.outputs)
            .map_err(|output| UnorderedPsbtError::MissingOutputUniqueId(Box::new(output)))?;

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
    use crate::input::InputSet;
    use crate::lattice::join::Join;
    use crate::output::OutputSet;
    use psbt_v2::v2::{Global, Output};

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

        #[test]
        fn into_sized_sets_preserves_global_and_synchronizes_sets() {
            let wrapped = make_psbt_with_sets(0x03, [1], [2]).wrap();
            let expected_global = wrapped.global.clone();

            let (global, inputs, outputs) = wrapped.into_sized_sets();

            assert_eq!(global, expected_global);
            assert_eq!(inputs.joined_count(), Ok(1));
            assert_eq!(outputs.joined_count(), Ok(1));
        }

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

        fn make_psbt_with_sets(
            flags: u8,
            input_ids: impl IntoIterator<Item = u8>,
            output_ids: impl IntoIterator<Item = u8>,
        ) -> UnorderedPsbt {
            use crate::output::PSBT_OUT_UNIQUE_ID_SUBTYPE;
            use bitcoin::hashes::Hash;

            let mut inputs = InputSet::default();
            for id in input_ids {
                inputs.add(psbt_v2::v2::Input::new(&bitcoin::OutPoint {
                    txid: bitcoin::Txid::from_byte_array([id; 32]),
                    vout: 0,
                }));
            }

            let mut outputs = OutputSet::default();
            for id in output_ids {
                let mut output = psbt_v2::v2::Output {
                    amount: bitcoin::Amount::from_sat(100_000),
                    script_pubkey: bitcoin::ScriptBuf::new_op_return([id; 20]),
                    ..Default::default()
                };
                let key = psbt_v2::raw::ProprietaryKey {
                    prefix: b"concurrent-psbt".to_vec(),
                    subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                    key: vec![],
                };
                output.proprietaries.insert(key, vec![id; 16]);
                outputs.add(output);
            }

            UnorderedPsbt {
                global: Global {
                    tx_modifiable_flags: flags,
                    ..Global::default()
                },
                inputs,
                outputs,
            }
        }

        fn make_populated_psbt(txid_byte: u8) -> UnorderedPsbt {
            make_psbt_with_sets(0x03, [txid_byte], [txid_byte])
        }

        #[test]
        fn differing_modifiability_joins_when_frozen_sets_cover_other_sets() {
            let inputs_frozen = make_psbt_with_sets(0x02, [1, 2], [10]);
            let outputs_frozen = make_psbt_with_sets(0x01, [1], [10, 20]);

            let joined = inputs_frozen.wrap().join(outputs_frozen.wrap());

            assert_eq!(joined.global.tx_modifiable_flags, Ok(0x00));
            assert_eq!(joined.global.input_count, Ok(2));
            assert_eq!(joined.global.output_count, Ok(2));
            assert!(
                joined.is_ok(),
                "cross-subset join should be clean: {joined:#?}"
            );
        }

        #[test]
        fn differing_modifiability_conflicts_when_input_subset_rule_fails() {
            let inputs_frozen = make_psbt_with_sets(0x02, [1], [10]);
            let outputs_frozen = make_psbt_with_sets(0x01, [2], [10]);

            let joined = inputs_frozen.wrap().join(outputs_frozen.wrap());

            assert_eq!(
                joined.global.tx_modifiable_flags,
                Err(Conflict::from_values([0x02, 0x01]))
            );
            assert!(joined.global.input_count.is_err());
            assert_eq!(joined.global.output_count, Ok(1));
        }

        #[test]
        fn differing_modifiability_conflicts_when_output_subset_rule_fails() {
            let inputs_frozen = make_psbt_with_sets(0x02, [1], [10]);
            let outputs_frozen = make_psbt_with_sets(0x01, [1], [20]);

            let joined = inputs_frozen.wrap().join(outputs_frozen.wrap());

            assert_eq!(
                joined.global.tx_modifiable_flags,
                Err(Conflict::from_values([0x02, 0x01]))
            );
            assert_eq!(joined.global.input_count, Ok(1));
            assert!(joined.global.output_count.is_err());
        }

        #[test]
        fn equal_non_modifiable_flags_conflict_when_frozen_sets_differ() {
            let empty = make_psbt_with_sets(0x00, [], []);
            let populated = make_psbt_with_sets(0x00, [1], [10]);

            let joined = empty.wrap().join(populated.wrap());

            assert_eq!(
                joined.global.tx_modifiable_flags,
                Err(Conflict::from_values([0x00]))
            );
            assert!(joined.global.input_count.is_err());
            assert!(joined.global.output_count.is_err());
        }

        #[test]
        fn failed_join_records_the_immediate_effective_flag_operands() {
            let a = make_psbt_with_sets(0x02, [1], []);
            let b = make_psbt_with_sets(0x03, [2], []);
            let c = make_psbt_with_sets(0x02, [1, 2], []);

            let first_failure = a.clone().wrap().join(b.clone().wrap());
            assert_eq!(
                first_failure.global.tx_modifiable_flags,
                Err(Conflict::from_values([0x02, 0x03])),
            );

            let coherent_bc = b.wrap().join(c.wrap());
            assert_eq!(coherent_bc.global.tx_modifiable_flags, Ok(0x02));

            let outer_failure = a.wrap().join(coherent_bc);
            assert_eq!(
                outer_failure.global.tx_modifiable_flags,
                Err(Conflict::from_values([0x02])),
            );
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

        fn equivalent_result_psbts(
            mut left: ResultUnorderedPsbt,
            mut right: ResultUnorderedPsbt,
        ) -> bool {
            let left_flags = TxModifiableFlags::from(&left.global);
            let right_flags = TxModifiableFlags::from(&right.global);
            if !left_flags.equivalent(&right_flags) {
                return false;
            }
            left.global.tx_modifiable_flags = left_flags.canonicalized();
            right.global.tx_modifiable_flags = right_flags.canonicalized();
            left == right
        }

        proptest! {
            #[test]
            fn idempotent(a in arb_result_unordered_psbt()) {
                let cloned = a.clone();
                prop_assert!(equivalent_result_psbts(a.clone().join(cloned), a));
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
                prop_assert!(equivalent_result_psbts(ab_c, a_bc));
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
            fn join_syncs_or_conflicts_counts_with_modifiability(
                a in arb_unordered_psbt(),
                b in arb_unordered_psbt(),
            ) {
                let joined = a.wrap().join(b.wrap());
                let clean_input_count = Ok(joined.inputs.len());
                let conflicted_input_count =
                    Err(Conflict::from_values([joined.inputs.len()]));
                let clean_output_count = Ok(joined.outputs.len());
                let conflicted_output_count =
                    Err(Conflict::from_values([joined.outputs.len()]));

                prop_assert!(
                    joined.global.input_count == clean_input_count
                        || joined.global.input_count == conflicted_input_count
                );
                prop_assert!(
                    joined.global.output_count == clean_output_count
                        || joined.global.output_count == conflicted_output_count
                );
                if joined.global.tx_modifiable_flags.is_ok() {
                    prop_assert_eq!(joined.global.input_count, clean_input_count);
                    prop_assert_eq!(joined.global.output_count, clean_output_count);
                } else {
                    prop_assert!(
                        joined.global.input_count.is_err()
                            || joined.global.output_count.is_err()
                    );
                }
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
