use std::fmt;

use bitcoin::OutPoint;

use crate::output::UniqueId;
use crate::sorter::{FlagsMismatch, SortModeError};
use crate::tx::{ResultUnorderedPsbt, UnorderedPsbtError};

/// Error when constructing a [`Constructor`](super::Constructor) from a PSBT.
#[derive(Debug, PartialEq)]
pub enum ConstructorError {
    /// The output at this position is missing the `PSBT_OUT_UNIQUE_ID` field.
    MissingUniqueId(usize),
    /// Two inputs spend the same outpoint.
    DuplicateInput(OutPoint),
    /// Two outputs carry the same `PSBT_OUT_UNIQUE_ID`.
    DuplicateUniqueId(UniqueId),
    /// An added input or output conflicts with the one already spending that
    /// outpoint or carrying that unique ID.
    Conflict(Box<ResultUnorderedPsbt>),
    /// The PSBT's declared counts disagree with its inputs and outputs, it is
    /// not marked unordered, its unordered or sort flags are malformed, its
    /// sort fields break its sort mode or repeat a sort key, or its sort seed,
    /// a sort key, or an output's unique ID has an invalid size.
    Unordered(UnorderedPsbtError),
    /// The PSBT's sort fields don't select the requested sort mode.
    SortMode(SortModeError),
    /// The PSBT's `tx_modifiable_flags` don't match the requested modifiability.
    FlagsMismatch {
        /// The flags value required by the modifiability type.
        expected: u8,
        /// The actual `tx_modifiable_flags & 0x03` from the PSBT.
        actual: u8,
    },
}

impl fmt::Display for ConstructorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingUniqueId(index) => {
                write!(f, "output {index} missing PSBT_OUT_UNIQUE_ID")
            }
            Self::DuplicateInput(outpoint) => write!(f, "two inputs spend {outpoint}"),
            Self::DuplicateUniqueId(_) => write!(f, "two outputs share a PSBT_OUT_UNIQUE_ID"),
            Self::Conflict(_) => write!(f, "unordered PSBT contains conflicting fields"),
            Self::Unordered(error) => error.fmt(f),
            Self::SortMode(error) => error.fmt(f),
            Self::FlagsMismatch { expected, actual } => {
                write!(
                    f,
                    "tx_modifiable_flags mismatch: expected 0x{expected:02x}, got 0x{actual:02x}"
                )
            }
        }
    }
}

impl std::error::Error for ConstructorError {}

impl From<FlagsMismatch> for ConstructorError {
    fn from(FlagsMismatch { expected, actual }: FlagsMismatch) -> Self {
        Self::FlagsMismatch { expected, actual }
    }
}

impl From<SortModeError> for ConstructorError {
    fn from(error: SortModeError) -> Self {
        Self::SortMode(error)
    }
}

impl From<UnorderedPsbtError> for ConstructorError {
    fn from(error: UnorderedPsbtError) -> Self {
        match error {
            UnorderedPsbtError::MissingOutputUniqueId(index) => Self::MissingUniqueId(index),
            UnorderedPsbtError::DuplicateInput(outpoint) => Self::DuplicateInput(outpoint),
            UnorderedPsbtError::DuplicateOutputUniqueId(unique_id) => {
                Self::DuplicateUniqueId(unique_id)
            }
            error @ (UnorderedPsbtError::InputCountMismatch { .. }
            | UnorderedPsbtError::OutputCountMismatch { .. }
            | UnorderedPsbtError::InvalidUnorderedFlag(_)
            | UnorderedPsbtError::InvalidSortKeyDerivation(_)
            | UnorderedPsbtError::NotUnordered
            | UnorderedPsbtError::UnorderedSighashSingle
            | UnorderedPsbtError::SortSeedTooShort(_)
            | UnorderedPsbtError::InputSortKeyTooLong(_)
            | UnorderedPsbtError::OutputSortKeyTooLong(_)
            | UnorderedPsbtError::UnexpectedSortSeed
            | UnorderedPsbtError::UnexpectedInputSortKey(_)
            | UnorderedPsbtError::UnexpectedOutputSortKey(_)
            | UnorderedPsbtError::DuplicateSortKey(_)
            | UnorderedPsbtError::InvalidOutputUniqueId(_)) => Self::Unordered(error),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    #[cfg(feature = "unit-tests")]
    mod unit {
        use bitcoin::hashes::Hash;
        use bitcoin::{OutPoint, Txid};

        use super::super::*;
        use crate::global::Global;
        use crate::input::InputSet;
        use crate::output::OutputSet;
        use crate::tx::UnorderedPsbt;

        #[test]
        fn errors_display_their_cause() {
            let conflict = UnorderedPsbt {
                global: Global::default(),
                inputs: InputSet::default(),
                outputs: OutputSet::default(),
            }
            .wrap();
            let outpoint = OutPoint {
                txid: Txid::from_byte_array([1; 32]),
                vout: 7,
            };
            let cases = [
                (
                    ConstructorError::MissingUniqueId(2),
                    "output 2 missing PSBT_OUT_UNIQUE_ID",
                ),
                (
                    ConstructorError::DuplicateInput(outpoint),
                    "two inputs spend 0101010101010101010101010101010101010101010101010101010101010101:7",
                ),
                (
                    ConstructorError::DuplicateUniqueId(UniqueId::new(vec![1])),
                    "two outputs share a PSBT_OUT_UNIQUE_ID",
                ),
                (
                    ConstructorError::Conflict(Box::new(conflict)),
                    "unordered PSBT contains conflicting fields",
                ),
                (
                    ConstructorError::FlagsMismatch {
                        expected: 0x03,
                        actual: 0x01,
                    },
                    "tx_modifiable_flags mismatch: expected 0x03, got 0x01",
                ),
                (
                    ConstructorError::Unordered(UnorderedPsbtError::NotUnordered),
                    "PSBT is not marked unordered",
                ),
                (
                    ConstructorError::SortMode(SortModeError::UnexpectedSortKey),
                    "a sort key is set in derived-only sort mode",
                ),
            ];
            for (error, message) in cases {
                assert_eq!(error.to_string(), message);
            }
        }
    }
}
