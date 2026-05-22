use psbt_v2::v2::DetermineLockTimeError;

use crate::global::SortKeyDerivation;

/// Error type for sort operations.
#[derive(Debug)]
pub enum SortError {
    /// An input is missing a sort key and no seed is available to derive one.
    MissingInputSortKey(bitcoin::OutPoint),
    /// An output is missing a sort key and no seed is available to derive one.
    MissingOutputSortKey,
    /// Two inputs, or two outputs, have this sort key, so their relative
    /// order is undetermined.
    DuplicateSortKey(Vec<u8>),
    /// Sort seed is required but not set.
    MissingSortSeed,
    /// The inputs' lock time requirements conflict, so the ordered PSBT
    /// cannot be handed to the [`Updater`](psbt_v2::v2::Updater).
    UndeterminedLockTime(DetermineLockTimeError),
}

impl std::fmt::Display for SortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingInputSortKey(op) => write!(f, "input {op} missing sort key"),
            Self::MissingOutputSortKey => write!(f, "output missing sort key"),
            Self::DuplicateSortKey(k) => {
                let encoded = k
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                write!(f, "duplicate sort key: {encoded}")
            }
            Self::MissingSortSeed => write!(f, "PSBT_GLOBAL_SORT_SEED not set"),
            Self::UndeterminedLockTime(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SortError {}

impl From<DetermineLockTimeError> for SortError {
    fn from(error: DetermineLockTimeError) -> Self {
        Self::UndeterminedLockTime(error)
    }
}

impl From<std::convert::Infallible> for SortError {
    // Uncallable: no `Infallible` value exists.
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn from(never: std::convert::Infallible) -> Self {
        match never {}
    }
}

/// The PSBT's `tx_modifiable_flags` don't match the requested modifiability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagsMismatch {
    /// The flags value required by the modifiability type.
    pub expected: u8,
    /// The actual `tx_modifiable_flags & 0x03` from the PSBT.
    pub actual: u8,
}

impl std::fmt::Display for FlagsMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tx_modifiable_flags mismatch: expected 0x{:02x}, got 0x{:02x}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for FlagsMismatch {}

/// Error when a PSBT's sort fields do not describe the expected
/// [`SortMode`](super::SortMode).
#[derive(Debug, Clone, PartialEq)]
pub enum SortModeError {
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is not the mode's value.
    SortKeyDerivation {
        /// The mode's value, `None` for unset.
        expected: Option<SortKeyDerivation>,
        /// The PSBT's value, `None` for unset.
        actual: Option<SortKeyDerivation>,
    },
    /// The mode is [`Seeded`](super::Seeded) but `PSBT_GLOBAL_SORT_SEED` is
    /// not set.
    MissingSortSeed,
    /// The mode is [`ExplicitOnly`](super::ExplicitOnly) but `PSBT_GLOBAL_SORT_SEED`
    /// is set.
    UnexpectedSortSeed,
    /// The mode is [`DerivedOnly`](super::DerivedOnly) but an input or
    /// output has a sort key.
    UnexpectedSortKey,
}

impl std::fmt::Display for SortModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let show = |value: &Option<SortKeyDerivation>| {
            value.map_or("unset".to_owned(), |v| format!("0x{:02x}", v.to_byte()))
        };
        match self {
            Self::SortKeyDerivation { expected, actual } => write!(
                f,
                "PSBT_GLOBAL_SORT_KEY_DERIVATION is {}, expected {}",
                show(actual),
                show(expected)
            ),
            Self::MissingSortSeed => write!(f, "PSBT_GLOBAL_SORT_SEED not set"),
            Self::UnexpectedSortSeed => {
                write!(f, "PSBT_GLOBAL_SORT_SEED is set in explicit-only sort mode")
            }
            Self::UnexpectedSortKey => write!(f, "a sort key is set in derived-only sort mode"),
        }
    }
}

impl std::error::Error for SortModeError {}

/// Error when an [`UnorderedPsbt`](crate::tx::UnorderedPsbt) cannot enter a
/// [`Sorter`](super::Sorter) of the requested modifiability and sort mode.
#[derive(Debug, Clone, PartialEq)]
pub enum SorterError {
    /// `tx_modifiable_flags` don't match the modifiability.
    Flags(FlagsMismatch),
    /// The sort fields don't select the sort mode.
    SortMode(SortModeError),
}

impl std::fmt::Display for SorterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Flags(error) => error.fmt(f),
            Self::SortMode(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SorterError {}

impl From<FlagsMismatch> for SorterError {
    fn from(error: FlagsMismatch) -> Self {
        Self::Flags(error)
    }
}

impl From<SortModeError> for SorterError {
    fn from(error: SortModeError) -> Self {
        Self::SortMode(error)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        /// The error `Psbt::determine_lock_time` returns for these inputs.
        fn lock_time_error() -> DetermineLockTimeError {
            let inputs = crate::test_support::conflicting_lock_time_inputs().map(Into::into);
            psbt_v2::v2::Psbt {
                global: crate::global::Global::default().into_v2(inputs.len(), 0),
                inputs: inputs.to_vec(),
                outputs: vec![],
            }
            .determine_lock_time()
            .unwrap_err()
        }

        #[test]
        fn errors_display_the_values_they_carry() {
            let outpoint = bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array([0xab; 32]),
                vout: 7,
            };
            let flags = FlagsMismatch {
                expected: 0x01,
                actual: 0x02,
            };
            let lock_time = lock_time_error();
            let examples = [
                (
                    SortError::MissingInputSortKey(outpoint).to_string(),
                    format!("input {}:7 missing sort key", "ab".repeat(32)),
                ),
                (
                    SortError::MissingOutputSortKey.to_string(),
                    "output missing sort key".to_owned(),
                ),
                (
                    SortError::DuplicateSortKey(vec![0x00, 0x0f, 0xff]).to_string(),
                    "duplicate sort key: 000fff".to_owned(),
                ),
                (
                    SortError::MissingSortSeed.to_string(),
                    "PSBT_GLOBAL_SORT_SEED not set".to_owned(),
                ),
                (
                    SortError::from(lock_time.clone()).to_string(),
                    lock_time.to_string(),
                ),
                (
                    flags.to_string(),
                    "tx_modifiable_flags mismatch: expected 0x01, got 0x02".to_owned(),
                ),
                (
                    SorterError::from(flags).to_string(),
                    "tx_modifiable_flags mismatch: expected 0x01, got 0x02".to_owned(),
                ),
                (
                    SortModeError::MissingSortSeed.to_string(),
                    "PSBT_GLOBAL_SORT_SEED not set".to_owned(),
                ),
                (
                    SorterError::from(SortModeError::UnexpectedSortSeed).to_string(),
                    "PSBT_GLOBAL_SORT_SEED is set in explicit-only sort mode".to_owned(),
                ),
                (
                    SortModeError::UnexpectedSortKey.to_string(),
                    "a sort key is set in derived-only sort mode".to_owned(),
                ),
            ];
            for (shown, expected) in examples {
                assert_eq!(shown, expected);
            }
        }

        #[test]
        fn a_derivation_mismatch_names_both_values() {
            use SortKeyDerivation::{Derived, Explicit};

            let examples = [
                (
                    None,
                    Some(Derived),
                    "PSBT_GLOBAL_SORT_KEY_DERIVATION is unset, expected 0x01",
                ),
                (
                    Some(Explicit),
                    None,
                    "PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x00, expected unset",
                ),
                (
                    Some(Derived),
                    Some(Explicit),
                    "PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x01, expected 0x00",
                ),
            ];
            for (actual, expected, message) in examples {
                let error = SortModeError::SortKeyDerivation { expected, actual };
                assert_eq!(error.to_string(), message);
                assert_eq!(SorterError::from(error).to_string(), message);
            }
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use bitcoin::hex::DisplayHex;
        use proptest::prelude::*;

        fn derivation() -> impl Strategy<Value = Option<SortKeyDerivation>> {
            proptest::option::of(prop_oneof![
                Just(SortKeyDerivation::Explicit),
                Just(SortKeyDerivation::Derived)
            ])
        }

        fn sort_mode_errors() -> impl Strategy<Value = SortModeError> {
            prop_oneof![
                (derivation(), derivation()).prop_map(|(expected, actual)| {
                    SortModeError::SortKeyDerivation { expected, actual }
                }),
                Just(SortModeError::MissingSortSeed),
                Just(SortModeError::UnexpectedSortSeed),
                Just(SortModeError::UnexpectedSortKey),
            ]
        }

        proptest! {
            #[test]
            fn sort_errors_name_the_offending_input_or_key(
                txid_byte in any::<u8>(),
                vout in any::<u32>(),
                key in proptest::collection::vec(any::<u8>(), 0..=8),
            ) {
                let outpoint = bitcoin::OutPoint {
                    txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
                    vout,
                };
                let txid = format!("{txid_byte:02x}").repeat(32);
                prop_assert_eq!(
                    SortError::MissingInputSortKey(outpoint).to_string(),
                    format!("input {txid}:{vout} missing sort key")
                );
                let hex = key.to_lower_hex_string();
                prop_assert_eq!(
                    SortError::DuplicateSortKey(key).to_string(),
                    format!("duplicate sort key: {hex}")
                );
            }

            #[test]
            fn sorter_error_displays_the_error_it_wraps(
                expected in any::<u8>(),
                actual in any::<u8>(),
                error in sort_mode_errors(),
            ) {
                let mismatch = FlagsMismatch { expected, actual };
                prop_assert_eq!(SorterError::from(mismatch).to_string(), mismatch.to_string());
                prop_assert_eq!(SorterError::from(error.clone()).to_string(), error.to_string());
            }
        }
    }
}
