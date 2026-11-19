//! An [`UnorderedPsbt`] represents the shared state of a transaction under construction.
//!
//! Start with each participant's parsed PSBTv2 [`Psbt`]. Read each copy with
//! [`UnorderedPsbt::try_from_psbt`], then call [`wrap`](UnorderedPsbt::wrap)
//! on it. The wrapped copies can be combined in any order with [`Join::join`].
//! Call [`try_unwrap`](ResultUnorderedPsbt::try_unwrap) on the result to get
//! the merged [`UnorderedPsbt`]. If conflicts remain, `Err` holds the
//! joined result.
//!
//! This is the low-level API for implementing construction protocols. Use it
//! for transactions whose input and output positions can still change.
//! [`UnorderedPsbt::into_psbt`] returns the inputs and outputs in arbitrary
//! order. Participants must agree on their order before signing.

#![allow(clippy::result_large_err)]

use std::fmt;

use bitcoin::OutPoint;
use psbt_v2::v2::Psbt;

use crate::lattice::join::Join;
use crate::{Lift, ResultWrapped};

use super::global::{Global, ResultGlobal};
use super::input::{DuplicateInput, Input, InputSet, ResultInputSet};
use super::output::{DuplicateOutput, InvalidOutput, Output, OutputSet, ResultOutputSet, UniqueId};

/// An unordered PSBT for concurrent transaction construction.
///
/// Under the [Concurrent PSBT Construction] draft, participants can add inputs
/// and outputs to separate copies of the same PSBT and combine their
/// contributions before agreeing on input and output positions. Each input
/// is identified by the outpoint it spends. Each output has a [`UniqueId`],
/// so two payments with the same amount and script can remain distinct.
///
/// [Concurrent PSBT Construction]: https://github.com/fungi-protocol/docs/blob/concurrent-psbt-rewrite/psbt-concurrent.md
#[derive(Debug, Clone, PartialEq)]
pub struct UnorderedPsbt {
    pub(crate) global: Global,
    pub(crate) inputs: InputSet,
    pub(crate) outputs: OutputSet,
}

/// Accumulates contributions to the same [`UnorderedPsbt`] with [`Join::join`].
///
/// The join combines global fields, matches inputs by outpoint, and matches
/// outputs by unique ID. A contribution that adds a member to a frozen set
/// creates a membership conflict. Use [`Self::try_unwrap`] to recover the
/// merged PSBT when there are no conflicts.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultUnorderedPsbt {
    pub(crate) global: ResultGlobal,
    pub(crate) inputs: ResultInputSet,
    pub(crate) outputs: ResultOutputSet,
}

/// Error returned when a PSBT cannot be read as an [`UnorderedPsbt`].
#[derive(Debug, Clone, PartialEq)]
pub enum UnorderedPsbtError {
    /// `PSBT_GLOBAL_INPUT_COUNT` is not the number of inputs.
    InputCountMismatch { declared: usize, found: usize },
    /// `PSBT_GLOBAL_OUTPUT_COUNT` is not the number of outputs.
    OutputCountMismatch { declared: usize, found: usize },
    /// The output at this position is missing the `PSBT_OUT_UNIQUE_ID`
    /// proprietary field.
    MissingOutputUniqueId(usize),
    /// The output at this position has a `PSBT_OUT_UNIQUE_ID` outside the
    /// [`UniqueId::MIN_LEN`] to [`UniqueId::MAX_LEN`] byte range.
    InvalidOutputUniqueId(usize),
    /// Two inputs spend the same outpoint, so the transaction is invalid.
    DuplicateInput(OutPoint),
    /// Two outputs carry the same `PSBT_OUT_UNIQUE_ID`.
    DuplicateOutputUniqueId(UniqueId),
    /// The global `Has SIGHASH_SINGLE` bit is set in `tx_modifiable_flags`.
    UnorderedSighashSingle,
}

impl From<DuplicateInput> for UnorderedPsbtError {
    fn from(DuplicateInput(outpoint): DuplicateInput) -> Self {
        Self::DuplicateInput(outpoint)
    }
}

impl From<DuplicateOutput> for UnorderedPsbtError {
    fn from(DuplicateOutput(unique_id): DuplicateOutput) -> Self {
        Self::DuplicateOutputUniqueId(unique_id)
    }
}

impl UnorderedPsbtError {
    /// The error for the output at `index`, which could not be converted.
    fn invalid_output(index: usize, error: InvalidOutput) -> Self {
        match error {
            InvalidOutput::MissingUniqueId => Self::MissingOutputUniqueId(index),
            InvalidOutput::InvalidUniqueId(_) => Self::InvalidOutputUniqueId(index),
        }
    }
}

impl fmt::Display for UnorderedPsbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputCountMismatch { declared, found } => {
                write!(f, "declares {declared} inputs but has {found}")
            }
            Self::OutputCountMismatch { declared, found } => {
                write!(f, "declares {declared} outputs but has {found}")
            }
            Self::MissingOutputUniqueId(index) => {
                write!(f, "output {index} missing PSBT_OUT_UNIQUE_ID")
            }
            Self::InvalidOutputUniqueId(index) => write!(
                f,
                "PSBT_OUT_UNIQUE_ID of output {index} must be {} to {} bytes",
                UniqueId::MIN_LEN,
                UniqueId::MAX_LEN
            ),
            Self::DuplicateInput(outpoint) => write!(f, "two inputs spend {outpoint}"),
            Self::DuplicateOutputUniqueId(_) => {
                write!(f, "two outputs share a PSBT_OUT_UNIQUE_ID")
            }
            Self::UnorderedSighashSingle => {
                write!(f, "unordered PSBT has the SIGHASH_SINGLE flag set")
            }
        }
    }
}

impl std::error::Error for UnorderedPsbtError {}

/// `tx_modifiable_flags` bit that allows inputs to be added (BIP 370).
const INPUTS_MODIFIABLE: u8 = 0x01;
/// `tx_modifiable_flags` bit that allows outputs to be added (BIP 370).
const OUTPUTS_MODIFIABLE: u8 = 0x02;
/// `tx_modifiable_flags` bit marking a `SIGHASH_SINGLE` signature (BIP 370).
const HAS_SIGHASH_SINGLE: u8 = 0x04;

/// Check that the global `Has SIGHASH_SINGLE` bit is clear.
fn check_unordered(global: &Global) -> Result<(), UnorderedPsbtError> {
    if global.tx_modifiable_flags() & HAS_SIGHASH_SINGLE != 0 {
        return Err(UnorderedPsbtError::UnorderedSighashSingle);
    }
    Ok(())
}

impl UnorderedPsbt {
    /// Read a PSBTv2 value, discarding its input and output positions. A clear
    /// input or output modifiability bit freezes the corresponding set.
    ///
    /// Within one PSBT, each input must spend a distinct outpoint and each
    /// output must carry a distinct unique ID.
    ///
    /// # Errors
    /// Returns a count mismatch if the declared input or output count is not
    /// the number of inputs or outputs, or
    /// [`UnorderedPsbtError::UnorderedSighashSingle`] if the global
    /// `Has SIGHASH_SINGLE` bit is set. Otherwise returns the first input whose
    /// outpoint repeats, then the first output whose `PSBT_OUT_UNIQUE_ID` is missing, empty or
    /// longer than [`UniqueId::MAX_LEN`] bytes, then the first unique ID that
    /// repeats.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        let Psbt {
            global,
            inputs,
            outputs,
        } = psbt;
        if global.input_count != inputs.len() {
            return Err(UnorderedPsbtError::InputCountMismatch {
                declared: global.input_count,
                found: inputs.len(),
            });
        }
        if global.output_count != outputs.len() {
            return Err(UnorderedPsbtError::OutputCountMismatch {
                declared: global.output_count,
                found: outputs.len(),
            });
        }

        let global = Global::from(global);
        check_unordered(&global)?;
        let inputs = InputSet::try_from_inputs(inputs.into_iter().map(Input::from))?;
        let inputs = if global.tx_modifiable_flags() & INPUTS_MODIFIABLE == 0 {
            inputs.freeze()
        } else {
            inputs
        };
        let outputs = outputs
            .into_iter()
            .enumerate()
            .map(|(index, output)| {
                Output::try_from(output)
                    .map_err(|error| UnorderedPsbtError::invalid_output(index, error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let outputs = OutputSet::try_from_outputs(outputs)?;
        let outputs = if global.tx_modifiable_flags() & OUTPUTS_MODIFIABLE == 0 {
            outputs.freeze()
        } else {
            outputs
        };

        Ok(Self {
            global,
            inputs,
            outputs,
        })
    }

    /// The global fields.
    pub fn global(&self) -> &Global {
        &self.global
    }

    /// The inputs, keyed by outpoint.
    pub fn inputs(&self) -> &InputSet {
        &self.inputs
    }

    /// The outputs, keyed by unique ID.
    pub fn outputs(&self) -> &OutputSet {
        &self.outputs
    }

    /// Return a PSBTv2 [`Psbt`] for exchange during construction. Its input and
    /// output vectors have arbitrary order. Their lengths supply the global counts.
    pub fn into_psbt(self) -> Psbt {
        let inputs: Vec<_> = self.inputs.into_iter().map(Into::into).collect();
        let outputs: Vec<_> = self.outputs.into_iter().map(Into::into).collect();
        Psbt {
            global: self.global.into_v2(inputs.len(), outputs.len()),
            inputs,
            outputs,
        }
    }

    /// Wrap this contribution for joining.
    pub fn wrap(self) -> ResultUnorderedPsbt {
        ResultUnorderedPsbt {
            global: self.global.wrap(),
            inputs: self.inputs.wrap(),
            outputs: self.outputs.wrap(),
        }
    }
}

impl Join for ResultUnorderedPsbt {
    fn join(self, other: Self) -> Self {
        ResultUnorderedPsbt {
            global: self.global.join(other.global),
            inputs: self.inputs.join(other.inputs),
            outputs: self.outputs.join(other.outputs),
        }
    }
}

impl ResultUnorderedPsbt {
    /// Parse a v2 PSBT directly into the result domain, as
    /// [`UnorderedPsbt::try_from_psbt`] then [`wrap`](UnorderedPsbt::wrap).
    ///
    /// # Errors
    /// Returns the errors of [`UnorderedPsbt::try_from_psbt`].
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        UnorderedPsbt::try_from_psbt(psbt).map(UnorderedPsbt::wrap)
    }

    /// Return `true` if there are no field or membership conflicts.
    pub fn is_ok(&self) -> bool {
        self.global.is_ok() && self.inputs.is_ok() && self.outputs.is_ok()
    }

    /// Return the clean [`UnorderedPsbt`], or `Err(self)` on a field or membership conflict.
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

impl Lift for UnorderedPsbt {
    type Result = ResultUnorderedPsbt;

    fn wrap(self) -> Self::Result {
        Self::wrap(self)
    }
}

impl ResultWrapped for ResultUnorderedPsbt {
    type Clean = UnorderedPsbt;

    fn is_ok(&self) -> bool {
        Self::is_ok(self)
    }

    fn try_unwrap(self) -> Result<Self::Clean, Self> {
        Self::try_unwrap(self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;
    use crate::psbt::output::OutputUniqueIdExt;

    /// A BIP 370 output with unique ID `uid` and every other field unset.
    fn v2_output_with_uid(uid: Vec<u8>) -> psbt_v2::v2::Output {
        let mut output = psbt_v2::v2::Output::default();
        output.set_unique_id_bytes(uid);
        output
    }

    /// A BIP 370 PSBT with these inputs and outputs, and consistent counts.
    fn v2_psbt(inputs: Vec<psbt_v2::v2::Input>, outputs: Vec<psbt_v2::v2::Output>) -> Psbt {
        Psbt {
            global: Global::default().into_v2(inputs.len(), outputs.len()),
            inputs,
            outputs,
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;
        use bitcoin::hashes::Hash;
        use bitcoin::{Amount, ScriptBuf, Sequence, Txid, transaction};

        fn outpoint(txid_byte: u8) -> OutPoint {
            OutPoint {
                txid: Txid::from_byte_array([txid_byte; 32]),
                vout: 0,
            }
        }

        /// A modifiable PSBT with one input and one output, both derived from `byte`.
        fn singleton(byte: u8) -> UnorderedPsbt {
            let input = Input::new(&outpoint(byte));
            let output = Output::new(
                UniqueId::try_from(vec![byte; 16]).unwrap(),
                Amount::from_sat(100_000),
                ScriptBuf::new_op_return([byte; 20]),
            );
            let mut global = Global::default();
            global.set_tx_modifiable_flags(INPUTS_MODIFIABLE | OUTPUTS_MODIFIABLE);
            UnorderedPsbt {
                global,
                inputs: InputSet::try_from_inputs([input]).unwrap(),
                outputs: OutputSet::try_from_outputs([output]).unwrap(),
            }
        }

        #[test]
        fn different_tx_versions_conflict() {
            let with_version = |tx_version| {
                let mut psbt = singleton(1);
                psbt.global.set_tx_version(tx_version);
                psbt.wrap()
            };
            let joined = with_version(transaction::Version::ONE)
                .join(with_version(transaction::Version::TWO));
            assert!(!joined.is_ok());
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }

        #[test]
        fn errors_display_their_cause() {
            let cases = [
                (
                    UnorderedPsbtError::InputCountMismatch {
                        declared: 1,
                        found: 0,
                    },
                    "declares 1 inputs but has 0".to_string(),
                ),
                (
                    UnorderedPsbtError::OutputCountMismatch {
                        declared: 0,
                        found: 2,
                    },
                    "declares 0 outputs but has 2".to_string(),
                ),
                (
                    UnorderedPsbtError::MissingOutputUniqueId(1),
                    "output 1 missing PSBT_OUT_UNIQUE_ID".to_string(),
                ),
                (
                    UnorderedPsbtError::InvalidOutputUniqueId(1),
                    "PSBT_OUT_UNIQUE_ID of output 1 must be 16 to 32 bytes".to_string(),
                ),
                (
                    UnorderedPsbtError::DuplicateInput(outpoint(1)),
                    format!("two inputs spend {}", outpoint(1)),
                ),
                (
                    UnorderedPsbtError::DuplicateOutputUniqueId(
                        UniqueId::try_from(vec![1; 16]).unwrap(),
                    ),
                    "two outputs share a PSBT_OUT_UNIQUE_ID".to_string(),
                ),
                (
                    UnorderedPsbtError::UnorderedSighashSingle,
                    "unordered PSBT has the SIGHASH_SINGLE flag set".to_string(),
                ),
            ];
            for (error, message) in cases {
                assert_eq!(error.to_string(), message);
            }
        }

        #[test]
        fn invalid_psbts_are_rejected() {
            let input_with_sequence = |sequence| {
                let mut input = psbt_v2::v2::Input::new(&outpoint(1));
                input.sequence = Some(Sequence(sequence));
                input
            };
            let output_with_amount = |sat| {
                let mut output = v2_output_with_uid(vec![1; 16]);
                output.amount = Amount::from_sat(sat);
                output
            };
            let cases = [
                (
                    v2_psbt(vec![input_with_sequence(1), input_with_sequence(2)], vec![]),
                    UnorderedPsbtError::DuplicateInput(outpoint(1)),
                ),
                (
                    v2_psbt(
                        vec![],
                        vec![
                            v2_output_with_uid(vec![1; 16]),
                            psbt_v2::v2::Output::default(),
                        ],
                    ),
                    UnorderedPsbtError::MissingOutputUniqueId(1),
                ),
                (
                    v2_psbt(vec![], vec![v2_output_with_uid(vec![])]),
                    UnorderedPsbtError::InvalidOutputUniqueId(0),
                ),
                (
                    v2_psbt(vec![], vec![v2_output_with_uid(vec![1; 33])]),
                    UnorderedPsbtError::InvalidOutputUniqueId(0),
                ),
                (
                    v2_psbt(
                        vec![],
                        vec![output_with_amount(1000), output_with_amount(2000)],
                    ),
                    UnorderedPsbtError::DuplicateOutputUniqueId(
                        UniqueId::try_from(vec![1; 16]).unwrap(),
                    ),
                ),
            ];
            for (psbt, error) in cases {
                assert_eq!(ResultUnorderedPsbt::try_from_psbt(psbt), Err(error));
            }
        }

        #[test]
        fn unordered_import_rejects_the_global_has_sighash_single_bit() {
            for flags in 0..=u8::MAX {
                let mut psbt = v2_psbt(vec![], vec![]);
                psbt.global.tx_modifiable_flags = flags;
                let expected = if flags & 0x04 == 0 {
                    Ok(())
                } else {
                    Err(UnorderedPsbtError::UnorderedSighashSingle)
                };
                assert_eq!(
                    UnorderedPsbt::try_from_psbt(psbt.clone()).map(|_| ()),
                    expected
                );
                assert_eq!(
                    ResultUnorderedPsbt::try_from_psbt(psbt).map(|_| ()),
                    expected
                );
            }
        }

        #[test]
        fn serialized_psbts_with_disjoint_entries_join_into_their_union() {
            let joined = (1..=3)
                .map(|byte| {
                    let bytes = singleton(byte).into_psbt().serialize();
                    let psbt = Psbt::deserialize(&bytes).unwrap();
                    ResultUnorderedPsbt::try_from_psbt(psbt).unwrap()
                })
                .reduce(Join::join)
                .unwrap();
            let joined = joined.try_unwrap().expect("disjoint entries join cleanly");
            let inputs = (1..=3).map(|byte| Input::new(&outpoint(byte)));
            let outputs = (1..=3).map(|byte| {
                Output::new(
                    UniqueId::try_from(vec![byte; 16]).unwrap(),
                    Amount::from_sat(100_000),
                    ScriptBuf::new_op_return([byte; 20]),
                )
            });
            assert_eq!(joined.inputs, InputSet::try_from_inputs(inputs).unwrap());
            assert_eq!(
                joined.outputs,
                OutputSet::try_from_outputs(outputs).unwrap()
            );
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use bitcoin::transaction;
        use proptest::prelude::*;

        /// An UnorderedPsbt with either transaction version, any modifiable
        /// flags, and up to three inputs and outputs.
        fn arb_unordered_psbt() -> impl Strategy<Value = UnorderedPsbt> {
            (
                proptest::bool::ANY, // tx_version: ONE or TWO
                0u8..4,              // tx_modifiable_flags
                proptest::collection::hash_set((0u8..5, 0u32..2), 0..=3), // inputs: (txid_byte, vout)
                proptest::collection::hash_set(0u8..10, 0..=3),           // outputs: uid_byte
            )
                .prop_map(|(use_v1, flags, input_specs, output_uids)| {
                    use bitcoin::hashes::Hash;

                    let inputs = InputSet::try_from_inputs(input_specs.into_iter().map(
                        |(txid_byte, vout)| {
                            Input::new(&bitcoin::OutPoint {
                                txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                                vout,
                            })
                        },
                    ))
                    .expect("distinct outpoints");

                    let outputs =
                        OutputSet::try_from_outputs(output_uids.into_iter().map(|uid_byte| {
                            Output::new(
                                UniqueId::try_from(vec![uid_byte; 16]).unwrap(),
                                bitcoin::Amount::from_sat(100_000),
                                bitcoin::ScriptBuf::new_op_return([uid_byte; 20]),
                            )
                        }))
                        .expect("distinct UIDs");

                    let mut global = Global::default();
                    global.set_tx_version(if use_v1 {
                        transaction::Version::ONE
                    } else {
                        transaction::Version::TWO
                    });
                    global.set_tx_modifiable_flags(flags);
                    let psbt = UnorderedPsbt {
                        global,
                        inputs,
                        outputs,
                    };
                    // Reparse so the sets are frozen according to the flags.
                    UnorderedPsbt::try_from_psbt(psbt.into_psbt()).expect("a valid unordered PSBT")
                })
        }

        /// A wrapped PSBT, or the join of two independently drawn ones, which
        /// may conflict.
        fn arb_result_unordered_psbt() -> impl Strategy<Value = ResultUnorderedPsbt> {
            prop_oneof![
                arb_unordered_psbt().prop_map(|p| p.wrap()),
                (arb_unordered_psbt(), arb_unordered_psbt())
                    .prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        assert_join_laws!(arb_result_unordered_psbt());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip(a in arb_unordered_psbt()) {
                prop_assert_eq!(a.clone().wrap().try_unwrap(), Ok(a));
            }

            #[test]
            fn getters_return_the_parts(a in arb_unordered_psbt()) {
                prop_assert_eq!(a.global(), &a.global);
                prop_assert_eq!(a.inputs(), &a.inputs);
                prop_assert_eq!(a.outputs(), &a.outputs);
            }

            #[test]
            fn result_try_from_psbt_agrees_with_wrap(a in arb_unordered_psbt()) {
                prop_assert_eq!(
                    ResultUnorderedPsbt::try_from_psbt(a.clone().into_psbt()),
                    Ok(a.wrap())
                );
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
            fn try_from_psbt_rejects_miscounted_psbt(
                a in arb_unordered_psbt(),
                input_count in proptest::option::of(0usize..4),
                output_count in proptest::option::of(0usize..4),
            ) {
                let mut psbt = a.clone().into_psbt();
                let (inputs, outputs) = (psbt.inputs.len(), psbt.outputs.len());
                psbt.global.input_count = input_count.unwrap_or(inputs);
                psbt.global.output_count = output_count.unwrap_or(outputs);
                let expected = if psbt.global.input_count != inputs {
                    Err(UnorderedPsbtError::InputCountMismatch {
                        declared: psbt.global.input_count,
                        found: inputs,
                    })
                } else if psbt.global.output_count != outputs {
                    Err(UnorderedPsbtError::OutputCountMismatch {
                        declared: psbt.global.output_count,
                        found: outputs,
                    })
                } else {
                    Ok(a)
                };
                prop_assert_eq!(UnorderedPsbt::try_from_psbt(psbt), expected);
            }

            #[test]
            fn frozen_sets_join_only_if_identical(
                a in arb_unordered_psbt(),
                mut b in arb_unordered_psbt(),
            ) {
                b.global.set_tx_version(*a.global.tx_version());
                // Reparse `b` so its sets are frozen according to `a`'s flags.
                b.global.set_tx_modifiable_flags(*a.global.tx_modifiable_flags());
                let b = UnorderedPsbt::try_from_psbt(b.into_psbt()).expect("a valid unordered PSBT");
                let flags = *a.global.tx_modifiable_flags();
                let inputs_agree = flags & INPUTS_MODIFIABLE != 0 || a.inputs == b.inputs;
                let outputs_agree = flags & OUTPUTS_MODIFIABLE != 0 || a.outputs == b.outputs;
                let joined = a.wrap().join(b.wrap());
                prop_assert_eq!(joined.is_ok(), inputs_agree && outputs_agree);
            }

            #[test]
            fn try_from_psbt_rejects_the_first_invalid_unique_id(
                uid_lens in proptest::collection::vec(proptest::option::of(0usize..=34), 0..=4),
            ) {
                let outputs = (0u8..)
                    .zip(&uid_lens)
                    .map(|(byte, len)| match len {
                        Some(len) => v2_output_with_uid(vec![byte; *len]),
                        None => psbt_v2::v2::Output::default(),
                    })
                    .collect();
                let expected = uid_lens
                    .iter()
                    .enumerate()
                    .find_map(|(index, len)| match len {
                        None => Some(UnorderedPsbtError::MissingOutputUniqueId(index)),
                        Some(len) if !(16..=32).contains(len) => {
                            Some(UnorderedPsbtError::InvalidOutputUniqueId(index))
                        }
                        Some(_) => None,
                    })
                    .map_or(Ok(uid_lens.len()), Err);
                let psbt = v2_psbt(vec![], outputs);
                prop_assert_eq!(UnorderedPsbt::try_from_psbt(psbt).map(|u| u.outputs.len()), expected);
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(r in arb_result_unordered_psbt()) {
                let is_ok = ResultWrapped::is_ok(&r);
                prop_assert_eq!(is_ok, ResultWrapped::try_unwrap(r).is_ok());
            }

            #[test]
            fn try_from_psbt_rejects_repeated_outpoints(
                specs in proptest::collection::vec((0u8..3, 0u32..2), 0..=4),
            ) {
                use bitcoin::hashes::Hash;

                let outpoints: Vec<OutPoint> = specs
                    .into_iter()
                    .map(|(txid_byte, vout)| OutPoint {
                        txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                        vout,
                    })
                    .collect();
                let mut seen = std::collections::HashSet::new();
                let first_repeat = outpoints.iter().copied().find(|op| !seen.insert(*op));

                let psbt = v2_psbt(outpoints.iter().map(psbt_v2::v2::Input::new).collect(), vec![]);

                let result = ResultUnorderedPsbt::try_from_psbt(psbt.clone());
                let clean = UnorderedPsbt::try_from_psbt(psbt);
                match first_repeat {
                    // Rejected even when the duplicates are identical.
                    Some(op) => {
                        prop_assert_eq!(result, Err(UnorderedPsbtError::DuplicateInput(op)));
                        prop_assert_eq!(clean, Err(UnorderedPsbtError::DuplicateInput(op)));
                    }
                    None => {
                        prop_assert_eq!(clean.map(|u| u.inputs.len()), Ok(outpoints.len()));
                    }
                }
            }

            #[test]
            fn try_from_psbt_rejects_repeated_unique_ids(
                uid_bytes in proptest::collection::vec(0u8..3, 0..=4),
            ) {
                let outputs = uid_bytes.iter().map(|byte| v2_output_with_uid(vec![*byte; 16])).collect();
                let mut seen = std::collections::HashSet::new();
                let first_repeat = uid_bytes.iter().find(|byte| !seen.insert(**byte));

                let psbt = v2_psbt(vec![], outputs);

                let result = ResultUnorderedPsbt::try_from_psbt(psbt.clone());
                let clean = UnorderedPsbt::try_from_psbt(psbt);
                match first_repeat {
                    // Rejected even when the duplicates are identical.
                    Some(byte) => {
                        let error = UnorderedPsbtError::DuplicateOutputUniqueId(UniqueId::try_from(vec![*byte; 16]).unwrap());
                        prop_assert_eq!(result, Err(error.clone()));
                        prop_assert_eq!(clean, Err(error));
                    }
                    None => {
                        prop_assert_eq!(clean.map(|u| u.outputs.len()), Ok(uid_bytes.len()));
                    }
                }
            }
        }
    }
}
