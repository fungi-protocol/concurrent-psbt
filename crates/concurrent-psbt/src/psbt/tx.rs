#![allow(clippy::result_large_err)]

use std::fmt;

use bitcoin::OutPoint;
use psbt_v2::v2::Psbt;

use crate::lattice::join::Join;

use super::global::{Global, InvalidGlobal, ResultGlobal, SortKeyDerivation, SortSeed};
use super::input::{DuplicateInput, Input, InputSet, ResultInputSet, SortKey, out_point};
use super::output::{DuplicateOutput, InvalidOutput, Output, OutputSet, ResultOutputSet, UniqueId};

/// A BIP 370 PSBT decomposed into [`Global`], [`InputSet`], and [`OutputSet`], without ordering.
///
/// Inputs are keyed by outpoint and outputs by unique ID.
/// Use [`UnorderedPsbt::wrap`] to enter the result domain for concurrent merging.
///
/// [`try_from_psbt`](Self::try_from_psbt) [freezes](InputSet::freeze) the
/// inputs, or the outputs, when their `tx_modifiable_flags` bit is clear.
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
///
/// Frozen inputs or outputs join cleanly only with frozen sets of the same
/// members, or with modifiable sets whose members they already have.
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
    /// `PSBT_GLOBAL_TX_UNORDERED` is present but its value is not `0x03`.
    InvalidUnorderedFlag(Vec<u8>),
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is present but its value is neither
    /// `0x00` nor `0x01`.
    InvalidSortKeyDerivation(Vec<u8>),
    /// `PSBT_GLOBAL_SORT_SEED` is shorter than [`SortSeed::MIN_LEN`]; this is
    /// its length.
    SortSeedTooShort(usize),
    /// The input spending this outpoint has a `PSBT_IN_SORT_KEY` longer than
    /// [`SortKey::MAX_LEN`].
    InputSortKeyTooLong(OutPoint),
    /// The output at this position is missing the `PSBT_OUT_UNIQUE_ID`
    /// proprietary field.
    MissingOutputUniqueId(usize),
    /// The output at this position has a `PSBT_OUT_UNIQUE_ID` that is empty or
    /// longer than [`UniqueId::MAX_LEN`] bytes.
    InvalidOutputUniqueId(usize),
    /// The output at this position has a `PSBT_OUT_SORT_KEY` longer than
    /// [`SortKey::MAX_LEN`].
    OutputSortKeyTooLong(usize),
    /// Two inputs spend the same outpoint, so the transaction is invalid.
    DuplicateInput(OutPoint),
    /// Two outputs carry the same `PSBT_OUT_UNIQUE_ID`.
    DuplicateOutputUniqueId(UniqueId),
    /// `PSBT_GLOBAL_TX_UNORDERED` is absent, so the PSBT is ordered. Reading
    /// it as unordered would discard its order.
    NotUnordered,
    /// The PSBT is unordered but `tx_modifiable_flags` has the
    /// `Has SIGHASH_SINGLE` bit set.
    UnorderedSighashSingle,
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is `0x00`, so sort keys are never
    /// derived, but `PSBT_GLOBAL_SORT_SEED` is set.
    UnexpectedSortSeed,
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is `0x01`, so every sort key is
    /// derived, but the input spending this outpoint has a `PSBT_IN_SORT_KEY`.
    UnexpectedInputSortKey(OutPoint),
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is `0x01`, so every sort key is
    /// derived, but the output at this position has a `PSBT_OUT_SORT_KEY`.
    UnexpectedOutputSortKey(usize),
    /// Two inputs, or two outputs, have this explicit sort key.
    DuplicateSortKey(Vec<u8>),
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

impl From<InvalidGlobal> for UnorderedPsbtError {
    fn from(error: InvalidGlobal) -> Self {
        match error {
            InvalidGlobal::UnorderedFlag(value) => Self::InvalidUnorderedFlag(value),
            InvalidGlobal::SortKeyDerivation(value) => Self::InvalidSortKeyDerivation(value),
            InvalidGlobal::SortSeed(error) => Self::SortSeedTooShort(error.0.len()),
        }
    }
}

impl UnorderedPsbtError {
    /// The error for the output at `index`, which could not be converted.
    fn invalid_output(index: usize, error: InvalidOutput) -> Self {
        match error {
            InvalidOutput::MissingUniqueId => Self::MissingOutputUniqueId(index),
            InvalidOutput::InvalidUniqueId(_) => Self::InvalidOutputUniqueId(index),
            InvalidOutput::SortKeyTooLong(_) => Self::OutputSortKeyTooLong(index),
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
            Self::InvalidUnorderedFlag(value) => {
                write!(f, "PSBT_GLOBAL_TX_UNORDERED is {value:02x?}, not 0x03")
            }
            Self::InvalidSortKeyDerivation(value) => write!(
                f,
                "PSBT_GLOBAL_SORT_KEY_DERIVATION is {value:02x?}, not 0x00 or 0x01"
            ),
            Self::SortSeedTooShort(len) => write!(
                f,
                "PSBT_GLOBAL_SORT_SEED is {len} bytes, fewer than {}",
                SortSeed::MIN_LEN
            ),
            Self::InputSortKeyTooLong(outpoint) => write!(
                f,
                "sort key of the input spending {outpoint} is longer than {} bytes",
                SortKey::MAX_LEN
            ),
            Self::MissingOutputUniqueId(index) => {
                write!(f, "output {index} missing PSBT_OUT_UNIQUE_ID")
            }
            Self::InvalidOutputUniqueId(index) => write!(
                f,
                "PSBT_OUT_UNIQUE_ID of output {index} is empty or longer than {} bytes",
                UniqueId::MAX_LEN
            ),
            Self::OutputSortKeyTooLong(index) => write!(
                f,
                "sort key of output {index} is longer than {} bytes",
                SortKey::MAX_LEN
            ),
            Self::DuplicateInput(outpoint) => write!(f, "two inputs spend {outpoint}"),
            Self::DuplicateOutputUniqueId(_) => {
                write!(f, "two outputs share a PSBT_OUT_UNIQUE_ID")
            }
            Self::NotUnordered => write!(f, "PSBT is not marked unordered"),
            Self::UnorderedSighashSingle => {
                write!(f, "unordered PSBT has the SIGHASH_SINGLE flag set")
            }
            Self::UnexpectedSortSeed => write!(
                f,
                "PSBT_GLOBAL_SORT_SEED is set but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x00"
            ),
            Self::UnexpectedInputSortKey(outpoint) => write!(
                f,
                "the input spending {outpoint} has a sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01"
            ),
            Self::UnexpectedOutputSortKey(index) => write!(
                f,
                "output {index} has a sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01"
            ),
            Self::DuplicateSortKey(key) => write!(f, "duplicate sort key {key:02x?}"),
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

/// Check that a PSBT can be read as unordered: it is marked unordered, it does
/// not use `SIGHASH_SINGLE`, the sort fields present agree with
/// `PSBT_GLOBAL_SORT_KEY_DERIVATION`, and no two inputs or two outputs share an
/// explicit sort key.
fn validate_unordered(
    global: &Global,
    inputs: &[Input],
    outputs: &[Output],
) -> Result<(), UnorderedPsbtError> {
    if !global.is_unordered() {
        return Err(UnorderedPsbtError::NotUnordered);
    }
    if global.tx_modifiable_flags & HAS_SIGHASH_SINGLE != 0 {
        return Err(UnorderedPsbtError::UnorderedSighashSingle);
    }
    match global.sort_key_derivation() {
        Some(SortKeyDerivation::Explicit) if global.sort_seed().is_some() => {
            return Err(UnorderedPsbtError::UnexpectedSortSeed);
        }
        Some(SortKeyDerivation::Derived) => {
            if let Some(input) = inputs.iter().find(|input| input.sort_key().is_some()) {
                return Err(UnorderedPsbtError::UnexpectedInputSortKey(out_point(input)));
            }
            if let Some(index) = outputs
                .iter()
                .position(|output| output.sort_key().is_some())
            {
                return Err(UnorderedPsbtError::UnexpectedOutputSortKey(index));
            }
        }
        _ => {}
    }
    duplicate_sort_key(inputs.iter().filter_map(|input| input.sort_key()))
        .or_else(|| duplicate_sort_key(outputs.iter().filter_map(|output| output.sort_key())))
        .map_or(Ok(()), |key| {
            Err(UnorderedPsbtError::DuplicateSortKey(key.to_vec()))
        })
}

/// The first sort key that repeats, if any.
fn duplicate_sort_key<'a>(mut keys: impl Iterator<Item = &'a [u8]>) -> Option<&'a [u8]> {
    let mut seen = std::collections::HashSet::new();
    keys.find(|key| !seen.insert(*key))
}

impl UnorderedPsbt {
    /// Clear the inputs' `tx_modifiable_flags` bit and freeze the inputs.
    pub(crate) fn no_more_inputs(mut self) -> Self {
        self.global.tx_modifiable_flags &= !INPUTS_MODIFIABLE;
        self.inputs = self.inputs.freeze();
        self
    }

    /// Clear the outputs' `tx_modifiable_flags` bit and freeze the outputs.
    pub(crate) fn no_more_outputs(mut self) -> Self {
        self.global.tx_modifiable_flags &= !OUTPUTS_MODIFIABLE;
        self.outputs = self.outputs.freeze();
        self
    }
}

impl UnorderedPsbt {
    /// Parse a v2 PSBT into unordered representation.
    ///
    /// Two inputs spending the same outpoint, or two outputs sharing a unique
    /// ID, are rejected rather than joined. Such a PSBT is invalid, and
    /// joining the duplicates would hide the problem unless their fields
    /// conflicted.
    ///
    /// The PSBT must be marked unordered, since reading an ordered PSBT as
    /// unordered would discard its order.
    ///
    /// # Errors
    /// Returns a count mismatch if the declared input or output count is not
    /// the number of inputs or outputs, which only a `Psbt` assembled by hand
    /// can have. Otherwise returns a malformed global sort field, then the
    /// first input whose sort key is longer than [`SortKey::MAX_LEN`] bytes,
    /// then the first output whose `PSBT_OUT_UNIQUE_ID` is missing, empty or
    /// longer than [`UniqueId::MAX_LEN`] bytes, or whose sort key is longer
    /// than [`SortKey::MAX_LEN`] bytes.
    ///
    /// Then returns [`UnorderedPsbtError::NotUnordered`] for an ordered PSBT,
    /// [`UnorderedPsbtError::UnorderedSighashSingle`] if the `SIGHASH_SINGLE`
    /// flag is set, [`UnorderedPsbtError::UnexpectedSortSeed`],
    /// [`UnorderedPsbtError::UnexpectedInputSortKey`] or
    /// [`UnorderedPsbtError::UnexpectedOutputSortKey`] for a sort field that
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` rules out, and
    /// [`UnorderedPsbtError::DuplicateSortKey`] for the first sort key that
    /// repeats among the inputs, then among the outputs. Finally returns the
    /// first input whose outpoint repeats, then the first unique ID that
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

        let global = Global::try_from(global)?;
        let inputs = inputs
            .into_iter()
            .map(|input| {
                let outpoint = OutPoint {
                    txid: input.previous_txid,
                    vout: input.spent_output_index,
                };
                Input::try_from(input)
                    .map_err(|_| UnorderedPsbtError::InputSortKeyTooLong(outpoint))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let outputs = outputs
            .into_iter()
            .enumerate()
            .map(|(index, output)| {
                Output::try_from(output)
                    .map_err(|error| UnorderedPsbtError::invalid_output(index, error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_unordered(&global, &inputs, &outputs)?;

        let inputs = InputSet::try_from_inputs(inputs)?;
        let inputs = if global.tx_modifiable_flags & INPUTS_MODIFIABLE == 0 {
            inputs.freeze()
        } else {
            inputs
        };
        let outputs = OutputSet::try_from_outputs(outputs)?;
        let outputs = if global.tx_modifiable_flags & OUTPUTS_MODIFIABLE == 0 {
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

    /// Convert to a BIP 370 [`Psbt`].
    ///
    /// The inputs and outputs come out in an arbitrary order, so equal
    /// values may serialize differently. Use `Sorter` to order them
    /// deterministically before signing or comparing serializations.
    pub fn into_psbt(self) -> Psbt {
        let inputs: Vec<_> = self.inputs.into_iter().map(Into::into).collect();
        let outputs: Vec<_> = self.outputs.into_iter().map(Into::into).collect();
        Psbt {
            global: self.global.into_v2(inputs.len(), outputs.len()),
            inputs,
            outputs,
        }
    }

    /// Lift this [`UnorderedPsbt`] into the result domain as a [`ResultUnorderedPsbt`] with all fields `Ok`.
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
    use crate::psbt::output::OutputUniqueIdExt;

    /// A BIP 370 output with unique ID `uid` and every other field unset.
    fn v2_output_with_uid(uid: Vec<u8>) -> psbt_v2::v2::Output {
        let mut output = psbt_v2::v2::Output::default();
        output.set_unique_id(UniqueId::new(uid));
        output
    }

    /// A default [`Global`] marked unordered.
    fn unordered_global() -> Global {
        let mut global = Global::default();
        global.set_unordered();
        global
    }

    /// A BIP 370 PSBT with this global map, inputs and outputs, and
    /// consistent counts.
    fn v2_psbt_with(
        global: Global,
        inputs: Vec<psbt_v2::v2::Input>,
        outputs: Vec<psbt_v2::v2::Output>,
    ) -> Psbt {
        Psbt {
            global: global.into_v2(inputs.len(), outputs.len()),
            inputs,
            outputs,
        }
    }

    /// An unordered BIP 370 PSBT with these inputs and outputs.
    fn v2_psbt(inputs: Vec<psbt_v2::v2::Input>, outputs: Vec<psbt_v2::v2::Output>) -> Psbt {
        v2_psbt_with(unordered_global(), inputs, outputs)
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
                UniqueId::new(vec![byte; 16]),
                Amount::from_sat(100_000),
                ScriptBuf::new_op_return([byte; 20]),
            );
            UnorderedPsbt {
                global: Global {
                    tx_modifiable_flags: INPUTS_MODIFIABLE | OUTPUTS_MODIFIABLE,
                    ..unordered_global()
                },
                inputs: InputSet::try_from_inputs([input]).unwrap(),
                outputs: OutputSet::try_from_outputs([output]).unwrap(),
            }
        }

        #[test]
        fn different_tx_versions_conflict() {
            let with_version = |tx_version| {
                UnorderedPsbt {
                    global: Global {
                        tx_version,
                        ..Global::default()
                    },
                    ..singleton(1)
                }
                .wrap()
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
                    "PSBT_OUT_UNIQUE_ID of output 1 is empty or longer than 32 bytes".to_string(),
                ),
                (
                    UnorderedPsbtError::DuplicateInput(outpoint(1)),
                    format!("two inputs spend {}", outpoint(1)),
                ),
                (
                    UnorderedPsbtError::DuplicateOutputUniqueId(UniqueId::new(vec![1])),
                    "two outputs share a PSBT_OUT_UNIQUE_ID".to_string(),
                ),
                (
                    UnorderedPsbtError::InvalidUnorderedFlag(vec![1]),
                    "PSBT_GLOBAL_TX_UNORDERED is [01], not 0x03".to_string(),
                ),
                (
                    UnorderedPsbtError::InvalidSortKeyDerivation(vec![2]),
                    "PSBT_GLOBAL_SORT_KEY_DERIVATION is [02], not 0x00 or 0x01".to_string(),
                ),
                (
                    UnorderedPsbtError::SortSeedTooShort(15),
                    "PSBT_GLOBAL_SORT_SEED is 15 bytes, fewer than 16".to_string(),
                ),
                (
                    UnorderedPsbtError::InputSortKeyTooLong(outpoint(1)),
                    format!(
                        "sort key of the input spending {} is longer than 32 bytes",
                        outpoint(1)
                    ),
                ),
                (
                    UnorderedPsbtError::OutputSortKeyTooLong(1),
                    "sort key of output 1 is longer than 32 bytes".to_string(),
                ),
                (
                    UnorderedPsbtError::NotUnordered,
                    "PSBT is not marked unordered".to_string(),
                ),
                (
                    UnorderedPsbtError::UnorderedSighashSingle,
                    "unordered PSBT has the SIGHASH_SINGLE flag set".to_string(),
                ),
                (
                    UnorderedPsbtError::UnexpectedSortSeed,
                    "PSBT_GLOBAL_SORT_SEED is set but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x00"
                        .to_string(),
                ),
                (
                    UnorderedPsbtError::UnexpectedInputSortKey(outpoint(1)),
                    format!(
                        "the input spending {} has a sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01",
                        outpoint(1)
                    ),
                ),
                (
                    UnorderedPsbtError::UnexpectedOutputSortKey(1),
                    "output 1 has a sort key but PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01"
                        .to_string(),
                ),
                (
                    UnorderedPsbtError::DuplicateSortKey(vec![0xab]),
                    "duplicate sort key [ab]".to_string(),
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
                let mut output = v2_output_with_uid(vec![1]);
                output.amount = Amount::from_sat(sat);
                output
            };
            let field = |subtype| psbt_v2::raw::ProprietaryKey {
                prefix: crate::PROPRIETARY_PREFIX.to_vec(),
                subtype,
                key: vec![],
            };
            let with_global_field = |subtype, value: Vec<u8>| {
                let mut psbt = v2_psbt(vec![], vec![]);
                psbt.global.proprietaries.insert(field(subtype), value);
                psbt
            };
            let mut long_input_key = psbt_v2::v2::Input::new(&outpoint(1));
            long_input_key
                .proprietaries
                .insert(field(crate::input::PSBT_IN_SORT_KEY_SUBTYPE), vec![0; 33]);
            let mut long_output_key = v2_output_with_uid(vec![2]);
            long_output_key
                .proprietaries
                .insert(field(crate::output::PSBT_OUT_SORT_KEY_SUBTYPE), vec![0; 33]);
            let cases = [
                (
                    with_global_field(crate::global::PSBT_GLOBAL_TX_UNORDERED_SUBTYPE, vec![1]),
                    UnorderedPsbtError::InvalidUnorderedFlag(vec![1]),
                ),
                (
                    with_global_field(
                        crate::global::PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE,
                        vec![2],
                    ),
                    UnorderedPsbtError::InvalidSortKeyDerivation(vec![2]),
                ),
                (
                    with_global_field(crate::global::PSBT_GLOBAL_SORT_SEED_SUBTYPE, vec![0; 15]),
                    UnorderedPsbtError::SortSeedTooShort(15),
                ),
                (
                    v2_psbt(vec![long_input_key], vec![]),
                    UnorderedPsbtError::InputSortKeyTooLong(outpoint(1)),
                ),
                (
                    v2_psbt(vec![input_with_sequence(1), input_with_sequence(2)], vec![]),
                    UnorderedPsbtError::DuplicateInput(outpoint(1)),
                ),
                (
                    v2_psbt(
                        vec![],
                        vec![v2_output_with_uid(vec![1]), psbt_v2::v2::Output::default()],
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
                    v2_psbt(vec![], vec![v2_output_with_uid(vec![1]), long_output_key]),
                    UnorderedPsbtError::OutputSortKeyTooLong(1),
                ),
                (
                    v2_psbt(
                        vec![],
                        vec![output_with_amount(1000), output_with_amount(2000)],
                    ),
                    UnorderedPsbtError::DuplicateOutputUniqueId(UniqueId::new(vec![1])),
                ),
            ];
            for (psbt, error) in cases {
                assert_eq!(ResultUnorderedPsbt::try_from_psbt(psbt), Err(error));
            }
        }

        #[test]
        fn psbts_that_break_the_sort_rules_are_rejected() {
            use SortKeyDerivation::{Derived, Explicit};

            let global = |derivation: Option<SortKeyDerivation>, seeded: bool| {
                let mut global = unordered_global();
                if let Some(derivation) = derivation {
                    global.set_sort_key_derivation(derivation);
                }
                if seeded {
                    global.set_sort_seed(SortSeed::try_from(vec![0; 16]).unwrap());
                }
                global
            };
            let input = |txid_byte, sort_key: Option<u8>| {
                let mut input = Input::new(&outpoint(txid_byte));
                if let Some(key) = sort_key {
                    input.set_sort_key(SortKey::try_from(vec![key]).unwrap());
                }
                psbt_v2::v2::Input::from(input)
            };
            let output = |uid, sort_key: Option<u8>| {
                let mut output =
                    Output::new(UniqueId::new(vec![uid]), Amount::ZERO, ScriptBuf::new());
                if let Some(key) = sort_key {
                    output.set_sort_key(SortKey::try_from(vec![key]).unwrap());
                }
                psbt_v2::v2::Output::from(output)
            };
            let sighash_single = Global {
                tx_modifiable_flags: HAS_SIGHASH_SINGLE,
                ..unordered_global()
            };
            let cases = [
                (
                    v2_psbt_with(Global::default(), vec![], vec![]),
                    Err(UnorderedPsbtError::NotUnordered),
                ),
                (
                    v2_psbt_with(sighash_single, vec![], vec![]),
                    Err(UnorderedPsbtError::UnorderedSighashSingle),
                ),
                (
                    v2_psbt_with(global(Some(Explicit), true), vec![], vec![]),
                    Err(UnorderedPsbtError::UnexpectedSortSeed),
                ),
                (
                    v2_psbt_with(
                        global(Some(Derived), true),
                        vec![input(1, None), input(2, Some(0))],
                        vec![],
                    ),
                    Err(UnorderedPsbtError::UnexpectedInputSortKey(outpoint(2))),
                ),
                (
                    v2_psbt_with(
                        global(Some(Derived), true),
                        vec![],
                        vec![output(1, None), output(2, Some(0))],
                    ),
                    Err(UnorderedPsbtError::UnexpectedOutputSortKey(1)),
                ),
                (
                    v2_psbt_with(
                        global(Some(Explicit), false),
                        vec![input(1, Some(0)), input(2, Some(0))],
                        vec![],
                    ),
                    Err(UnorderedPsbtError::DuplicateSortKey(vec![0])),
                ),
                (
                    v2_psbt_with(
                        global(Some(Explicit), false),
                        vec![],
                        vec![output(1, Some(0)), output(2, Some(0))],
                    ),
                    Err(UnorderedPsbtError::DuplicateSortKey(vec![0])),
                ),
                // Seeded derivation accepts entries without explicit keys.
                (
                    v2_psbt_with(
                        global(Some(Derived), true),
                        vec![input(1, None)],
                        vec![output(1, None)],
                    ),
                    Ok(()),
                ),
                // An input and an output may share a sort key.
                (
                    v2_psbt_with(
                        global(Some(Explicit), false),
                        vec![input(1, Some(0))],
                        vec![output(1, Some(0))],
                    ),
                    Ok(()),
                ),
            ];
            for (psbt, expected) in cases {
                assert_eq!(UnorderedPsbt::try_from_psbt(psbt).map(|_| ()), expected);
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
                    UniqueId::new(vec![byte; 16]),
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
                                UniqueId::new(vec![uid_byte; 16]),
                                bitcoin::Amount::from_sat(100_000),
                                bitcoin::ScriptBuf::new_op_return([uid_byte; 20]),
                            )
                        }))
                        .expect("distinct UIDs");

                    let psbt = UnorderedPsbt {
                        global: Global {
                            tx_version: if use_v1 {
                                transaction::Version::ONE
                            } else {
                                transaction::Version::TWO
                            },
                            tx_modifiable_flags: flags,
                            ..unordered_global()
                        },
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
                b.global.tx_version = a.global.tx_version;
                // Reparse `b` so its sets are frozen according to `a`'s flags.
                b.global.tx_modifiable_flags = a.global.tx_modifiable_flags;
                let b = UnorderedPsbt::try_from_psbt(b.into_psbt()).expect("a valid unordered PSBT");
                let flags = a.global.tx_modifiable_flags;
                let inputs_agree = flags & INPUTS_MODIFIABLE != 0 || a.inputs == b.inputs;
                let outputs_agree = flags & OUTPUTS_MODIFIABLE != 0 || a.outputs == b.outputs;
                let joined = a.wrap().join(b.wrap());
                prop_assert_eq!(joined.is_ok(), inputs_agree && outputs_agree);
            }

            #[test]
            fn error_messages_are_distinct_and_name_the_offending_value(
                index in any::<usize>(),
                len in any::<usize>(),
                value in proptest::collection::vec(any::<u8>(), 0..4),
                vout in any::<u32>(),
            ) {
                let outpoint = OutPoint { vout, ..OutPoint::null() };
                let bytes = format!("{value:02x?}");
                let errors = [
                    (
                        UnorderedPsbtError::InputCountMismatch { declared: len, found: index },
                        Some(format!("{len} inputs but has {index}")),
                    ),
                    (
                        UnorderedPsbtError::OutputCountMismatch { declared: len, found: index },
                        Some(format!("{len} outputs but has {index}")),
                    ),
                    (UnorderedPsbtError::InvalidUnorderedFlag(value.clone()), Some(bytes.clone())),
                    (UnorderedPsbtError::InvalidSortKeyDerivation(value.clone()), Some(bytes.clone())),
                    (UnorderedPsbtError::SortSeedTooShort(len), Some(len.to_string())),
                    (UnorderedPsbtError::InputSortKeyTooLong(outpoint), Some(outpoint.to_string())),
                    (UnorderedPsbtError::MissingOutputUniqueId(index), Some(index.to_string())),
                    (UnorderedPsbtError::InvalidOutputUniqueId(index), Some(index.to_string())),
                    (UnorderedPsbtError::OutputSortKeyTooLong(index), Some(index.to_string())),
                    (UnorderedPsbtError::DuplicateInput(outpoint), Some(outpoint.to_string())),
                    (UnorderedPsbtError::NotUnordered, None),
                    (UnorderedPsbtError::UnorderedSighashSingle, None),
                    (UnorderedPsbtError::UnexpectedSortSeed, None),
                    (UnorderedPsbtError::UnexpectedInputSortKey(outpoint), Some(outpoint.to_string())),
                    (UnorderedPsbtError::UnexpectedOutputSortKey(index), Some(index.to_string())),
                    (UnorderedPsbtError::DuplicateSortKey(value.clone()), Some(format!("{value:02x?}"))),
                    // The unique ID is opaque bytes, so the message leaves it out.
                    (UnorderedPsbtError::DuplicateOutputUniqueId(UniqueId::new(value)), None),
                ];
                let messages: Vec<_> = errors.iter().map(|(error, _)| error.to_string()).collect();
                for ((_, payload), message) in errors.iter().zip(&messages) {
                    if let Some(payload) = payload {
                        prop_assert!(message.contains(payload.as_str()), "{message:?} lacks {payload:?}");
                    }
                }
                let distinct: std::collections::HashSet<_> = messages.iter().collect();
                prop_assert_eq!(distinct.len(), messages.len());
            }

            #[test]
            fn malformed_sort_fields_reject_the_psbt(
                a in arb_unordered_psbt(),
                key_len in (SortKey::MAX_LEN + 1)..=40,
                seed_len in 0..SortSeed::MIN_LEN,
            ) {
                let field = |subtype| psbt_v2::raw::ProprietaryKey {
                    prefix: crate::PROPRIETARY_PREFIX.to_vec(),
                    subtype,
                    key: vec![],
                };
                let mut psbt = a.into_psbt();
                if let Some(input) = psbt.inputs.first_mut() {
                    input
                        .proprietaries
                        .insert(field(crate::input::PSBT_IN_SORT_KEY_SUBTYPE), vec![0; key_len]);
                    let outpoint = OutPoint {
                        txid: input.previous_txid,
                        vout: input.spent_output_index,
                    };
                    prop_assert_eq!(
                        UnorderedPsbt::try_from_psbt(psbt.clone()),
                        Err(UnorderedPsbtError::InputSortKeyTooLong(outpoint))
                    );
                }
                // A malformed global field is reported before any input.
                psbt.global
                    .proprietaries
                    .insert(field(crate::global::PSBT_GLOBAL_SORT_SEED_SUBTYPE), vec![0; seed_len]);
                prop_assert_eq!(
                    UnorderedPsbt::try_from_psbt(psbt),
                    Err(UnorderedPsbtError::SortSeedTooShort(seed_len))
                );
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
                        Some(len) if !(1..=UniqueId::MAX_LEN).contains(len) => {
                            Some(UnorderedPsbtError::InvalidOutputUniqueId(index))
                        }
                        Some(_) => None,
                    })
                    .map_or(Ok(uid_lens.len()), Err);
                let psbt = v2_psbt(vec![], outputs);
                prop_assert_eq!(UnorderedPsbt::try_from_psbt(psbt).map(|u| u.outputs.len()), expected);
            }

            #[test]
            fn try_from_psbt_requires_unordered_without_sighash_single(
                flags in 0u8..8,
                unordered in proptest::bool::ANY,
            ) {
                let mut global = Global {
                    tx_modifiable_flags: flags,
                    ..Global::default()
                };
                if unordered {
                    global.set_unordered();
                }
                let psbt = v2_psbt_with(global, vec![], vec![]);
                let expected = if !unordered {
                    Err(UnorderedPsbtError::NotUnordered)
                } else if flags & HAS_SIGHASH_SINGLE != 0 {
                    Err(UnorderedPsbtError::UnorderedSighashSingle)
                } else {
                    Ok(())
                };
                prop_assert_eq!(ResultUnorderedPsbt::try_from_psbt(psbt).map(|_| ()), expected);
            }

            #[test]
            fn try_from_psbt_rejects_the_first_repeated_sort_key(
                distinct in proptest::collection::hash_set(any::<u8>(), 1..=3),
                repeat in any::<proptest::sample::Index>(),
                tail in proptest::collection::vec(any::<u8>(), 0..=2),
                on_outputs in proptest::bool::ANY,
            ) {
                let distinct: Vec<u8> = distinct.into_iter().collect();
                let repeated = distinct[repeat.index(distinct.len())];
                let keys: Vec<u8> = distinct.iter().copied().chain([repeated]).chain(tail).collect();
                let sort_key = |key| SortKey::try_from(vec![key]).expect("one byte");
                let entries = (0u8..).zip(keys);
                let psbt = if on_outputs {
                    let outputs = entries
                        .map(|(uid, key)| {
                            let mut output = Output::new(
                                UniqueId::new(vec![uid]),
                                bitcoin::Amount::ZERO,
                                bitcoin::ScriptBuf::new(),
                            );
                            output.set_sort_key(sort_key(key));
                            output.into()
                        })
                        .collect();
                    v2_psbt(vec![], outputs)
                } else {
                    let inputs = entries
                        .map(|(vout, key)| {
                            let mut input = Input::new(&OutPoint { vout: vout.into(), ..OutPoint::null() });
                            input.set_sort_key(sort_key(key));
                            input.into()
                        })
                        .collect();
                    v2_psbt(inputs, vec![])
                };
                prop_assert_eq!(
                    UnorderedPsbt::try_from_psbt(psbt).map(|_| ()),
                    Err(UnorderedPsbtError::DuplicateSortKey(vec![repeated]))
                );
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(r in arb_result_unordered_psbt()) {
                prop_assert_eq!(r.is_ok(), r.try_unwrap().is_ok());
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
                let outputs = uid_bytes.iter().map(|byte| v2_output_with_uid(vec![*byte])).collect();
                let mut seen = std::collections::HashSet::new();
                let first_repeat = uid_bytes.iter().find(|byte| !seen.insert(**byte));

                let psbt = v2_psbt(vec![], outputs);

                let result = ResultUnorderedPsbt::try_from_psbt(psbt.clone());
                let clean = UnorderedPsbt::try_from_psbt(psbt);
                match first_repeat {
                    // Rejected even when the duplicates are identical.
                    Some(byte) => {
                        let error = UnorderedPsbtError::DuplicateOutputUniqueId(UniqueId::new(vec![*byte]));
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
