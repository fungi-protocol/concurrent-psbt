use psbt_v2::v2::DetermineLockTimeError;

use crate::global::{SortKeyDerivation, SortSeed, TxUuid};
use crate::lattice::partial::Conflict;

/// Error type for sort operations.
#[derive(Debug)]
pub enum SortError {
    /// In [`ExplicitKeysRequired`](super::ExplicitKeysRequired) mode, the input
    /// spending this outpoint has no sort key.
    MissingInputSortKey(bitcoin::OutPoint),
    /// In [`ExplicitKeysRequired`](super::ExplicitKeysRequired) mode, an output
    /// has no sort key.
    MissingOutputSortKey,
    /// In an unseeded mode, `PSBT_GLOBAL_TX_UUID` is unset, so no
    /// sort key can be derived.
    MissingUuid,
    /// The inputs' lock time requirements conflict, so the ordered PSBT
    /// cannot be handed to the [`Updater`](psbt_v2::v2::Updater).
    UndeterminedLockTime(DetermineLockTimeError),
}

impl std::fmt::Display for SortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingInputSortKey(op) => write!(f, "input {op} missing sort key"),
            Self::MissingOutputSortKey => write!(f, "output missing sort key"),
            Self::MissingUuid => write!(f, "PSBT_GLOBAL_TX_UUID unset"),
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
    /// The mode is [`Seeded`](super::Seeded) but `PSBT_GLOBAL_TX_UUID` is
    /// unset.
    MissingUuid,
    /// The mode is [`ExplicitKeysForbidden`](super::ExplicitKeysForbidden) but
    /// an input or output has a sort key.
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
            Self::MissingUuid => write!(f, "PSBT_GLOBAL_TX_UUID unset in a seeded sort mode"),
            Self::UnexpectedSortKey => write!(
                f,
                "a sort key is set in a sort mode that forbids explicit keys"
            ),
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

/// `PSBT_GLOBAL_SORT_SEED` is already a different seed. A seed is joined
/// rather than replaced, so that setting one is monotone, and two different
/// seeds do not join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedConflict {
    /// The seed the PSBT has.
    pub existing: SortSeed,
    /// The seed that was to be set.
    pub requested: SortSeed,
}

impl SeedConflict {
    /// Read the seeds off the conflict of joining `requested` into
    /// `existing`, which holds the two in that order.
    pub(crate) fn from_conflict(conflict: Conflict<SortSeed>) -> Self {
        let mut seeds = conflict.into_iter();
        let mut next = || seeds.next().expect("joining two seeds conflicts on both");
        let existing = next();
        Self {
            existing,
            requested: next(),
        }
    }
}

impl std::fmt::Display for SeedConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PSBT_GLOBAL_SORT_SEED is already set to a different seed"
        )
    }
}

impl std::error::Error for SeedConflict {}

/// Error when joining a UUID into `PSBT_GLOBAL_TX_UUID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UuidError {
    /// `PSBT_GLOBAL_TX_UUID` is already a different UUID. A UUID is joined
    /// rather than replaced, so that setting one is monotone, and two
    /// different UUIDs do not join.
    Conflict {
        /// The UUID the PSBT has.
        existing: TxUuid,
        /// The UUID that was to be set.
        requested: TxUuid,
    },
}

impl UuidError {
    /// Read the UUIDs off the conflict of joining `requested` into
    /// `existing`, which holds the two in that order.
    pub(crate) fn from_conflict(conflict: Conflict<TxUuid>) -> Self {
        let mut uuids = conflict.into_iter();
        let mut next = || uuids.next().expect("joining two UUIDs conflicts on both");
        let existing = next();
        Self::Conflict {
            existing,
            requested: next(),
        }
    }
}

impl std::fmt::Display for UuidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict { .. } => {
                write!(f, "PSBT_GLOBAL_TX_UUID is already set to a different UUID")
            }
        }
    }
}

impl std::error::Error for UuidError {}

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
                global: psbt_v2::v2::Global {
                    input_count: inputs.len(),
                    ..Default::default()
                },
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
                    SortError::MissingUuid.to_string(),
                    "PSBT_GLOBAL_TX_UUID unset".to_owned(),
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
                    SorterError::from(SortModeError::UnexpectedSortKey).to_string(),
                    "a sort key is set in a sort mode that forbids explicit keys".to_owned(),
                ),
                (
                    SorterError::from(SortModeError::MissingUuid).to_string(),
                    "PSBT_GLOBAL_TX_UUID unset in a seeded sort mode".to_owned(),
                ),
                (
                    UuidError::Conflict {
                        existing: TxUuid::try_from(vec![1; 16]).unwrap(),
                        requested: TxUuid::try_from(vec![2; 16]).unwrap(),
                    }
                    .to_string(),
                    "PSBT_GLOBAL_TX_UUID is already set to a different UUID".to_owned(),
                ),
                (
                    SeedConflict {
                        existing: SortSeed::from(vec![1; 16]),
                        requested: SortSeed::from(vec![2; 16]),
                    }
                    .to_string(),
                    "PSBT_GLOBAL_SORT_SEED is already set to a different seed".to_owned(),
                ),
            ];
            for (shown, expected) in examples {
                assert_eq!(shown, expected);
            }
        }

        #[test]
        fn a_derivation_mismatch_names_both_values() {
            use SortKeyDerivation::{ExplicitKeysForbidden, ExplicitKeysRequired};

            let examples = [
                (
                    None,
                    Some(ExplicitKeysForbidden),
                    "PSBT_GLOBAL_SORT_KEY_DERIVATION is unset, expected 0x01",
                ),
                (
                    Some(ExplicitKeysRequired),
                    None,
                    "PSBT_GLOBAL_SORT_KEY_DERIVATION is 0x00, expected unset",
                ),
                (
                    Some(ExplicitKeysForbidden),
                    Some(ExplicitKeysRequired),
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
        use proptest::prelude::*;

        fn derivation() -> impl Strategy<Value = Option<SortKeyDerivation>> {
            proptest::option::of(prop_oneof![
                Just(SortKeyDerivation::ExplicitKeysRequired),
                Just(SortKeyDerivation::ExplicitKeysForbidden)
            ])
        }

        fn sort_mode_errors() -> impl Strategy<Value = SortModeError> {
            prop_oneof![
                (derivation(), derivation()).prop_map(|(expected, actual)| {
                    SortModeError::SortKeyDerivation { expected, actual }
                }),
                Just(SortModeError::MissingUuid),
                Just(SortModeError::UnexpectedSortKey),
            ]
        }

        proptest! {
            #[test]
            fn sort_errors_name_the_offending_input(
                txid_byte in any::<u8>(),
                vout in any::<u32>(),
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
