use std::fmt;

use psbt_v2::v2::{Global, Output, Psbt};

use crate::global::GlobalExt;
use crate::input::InputSet;
use crate::output::OutputSet;

use super::ResultUnorderedPsbt;

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

/// Error returned when a clean [`UnorderedPsbt`] cannot be built from a PSBT.
#[derive(Debug, Clone, PartialEq)]
pub enum UnorderedPsbtError {
    /// An output is missing the `PSBT_OUT_UNIQUE_ID` proprietary field.
    MissingOutputUniqueId(Box<Output>),
    /// The PSBT accumulated conflicts while being converted to unordered form.
    Conflict(Box<ResultUnorderedPsbt>),
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
    pub fn into_psbt(self) -> Psbt {
        let inputs: Vec<_> = self.inputs.into_iter().collect();
        let outputs: Vec<_> = self.outputs.into_iter().collect();
        let mut global = self.global;
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn wrapping_synchronizes_counts() {
            let wrapped = UnorderedPsbt {
                global: Global {
                    input_count: 9,
                    output_count: 7,
                    ..Global::default()
                },
                inputs: InputSet::default(),
                outputs: OutputSet::default(),
            }
            .wrap();

            assert_eq!(wrapped.global.input_count, Ok(0));
            assert_eq!(wrapped.global.output_count, Ok(0));
        }

        #[test]
        fn error_display_covers_both_variants() {
            let missing = UnorderedPsbtError::MissingOutputUniqueId(Box::default());
            assert_eq!(missing.to_string(), "output missing PSBT_OUT_UNIQUE_ID");

            let conflict = UnorderedPsbtError::Conflict(Box::new(
                UnorderedPsbt {
                    global: Global::default(),
                    inputs: InputSet::default(),
                    outputs: OutputSet::default(),
                }
                .wrap(),
            ));
            assert_eq!(
                conflict.to_string(),
                "unordered PSBT contains conflicting fields"
            );
        }
    }
}
