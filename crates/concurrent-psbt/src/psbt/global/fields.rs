//! The subtypes of the global proprietary fields, and how each is stored.

use crate::psbt::proprietary::ProprietaryField;

use super::{InvalidGlobal, SortKeyDerivation, SortSeed, TxUuid};

/// Subtype for `PSBT_GLOBAL_TX_UNORDERED`.
pub const PSBT_GLOBAL_TX_UNORDERED_SUBTYPE: u8 = 16;
/// Subtype for `PSBT_GLOBAL_SORT_SEED`.
pub const PSBT_GLOBAL_SORT_SEED_SUBTYPE: u8 = 17;
/// Subtype for `PSBT_GLOBAL_SORT_KEY_DERIVATION`.
pub const PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE: u8 = 18;
/// Subtype for `PSBT_GLOBAL_TX_UUID`.
pub const PSBT_GLOBAL_TX_UUID_SUBTYPE: u8 = 19;

/// Stores the unordered marker in `PSBT_GLOBAL_TX_UNORDERED`: `0x03` when
/// set, meaning that both inputs and outputs are unordered, and absent
/// otherwise.
pub(super) struct TxUnorderedField;

impl ProprietaryField for TxUnorderedField {
    const SUBTYPE: u8 = PSBT_GLOBAL_TX_UNORDERED_SUBTYPE;
    type Value = bool;
    type Error = InvalidGlobal;

    fn decode(value: Option<Vec<u8>>) -> Result<bool, InvalidGlobal> {
        match value {
            None => Ok(false),
            Some(value) if value == [0x03] => Ok(true),
            Some(value) => Err(InvalidGlobal::UnorderedFlag(value)),
        }
    }

    fn encode(unordered: bool) -> Option<Vec<u8>> {
        unordered.then(|| vec![0x03])
    }
}

/// Stores the [`SortKeyDerivation`] in `PSBT_GLOBAL_SORT_KEY_DERIVATION`.
pub(super) struct SortKeyDerivationField;

impl ProprietaryField for SortKeyDerivationField {
    const SUBTYPE: u8 = PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE;
    type Value = Option<SortKeyDerivation>;
    type Error = InvalidGlobal;

    fn decode(value: Option<Vec<u8>>) -> Result<Self::Value, InvalidGlobal> {
        value
            .map(SortKeyDerivation::from_value)
            .transpose()
            .map_err(InvalidGlobal::SortKeyDerivation)
    }

    fn encode(derivation: Self::Value) -> Option<Vec<u8>> {
        derivation.map(|derivation| vec![derivation.to_byte()])
    }
}

/// Stores the [`SortSeed`] in `PSBT_GLOBAL_SORT_SEED`.
pub(super) struct SortSeedField;

impl ProprietaryField for SortSeedField {
    const SUBTYPE: u8 = PSBT_GLOBAL_SORT_SEED_SUBTYPE;
    type Value = Option<SortSeed>;
    type Error = InvalidGlobal;

    fn decode(value: Option<Vec<u8>>) -> Result<Self::Value, InvalidGlobal> {
        value
            .map(SortSeed::try_from)
            .transpose()
            .map_err(InvalidGlobal::SortSeed)
    }

    fn encode(seed: Self::Value) -> Option<Vec<u8>> {
        seed.map(|seed| seed.0)
    }
}

/// Stores the [`TxUuid`] in `PSBT_GLOBAL_TX_UUID`.
pub(super) struct TxUuidField;

impl ProprietaryField for TxUuidField {
    const SUBTYPE: u8 = PSBT_GLOBAL_TX_UUID_SUBTYPE;
    type Value = Option<TxUuid>;
    type Error = InvalidGlobal;

    fn decode(value: Option<Vec<u8>>) -> Result<Self::Value, InvalidGlobal> {
        value
            .map(TxUuid::try_from)
            .transpose()
            .map_err(InvalidGlobal::TxUuid)
    }

    fn encode(uuid: Self::Value) -> Option<Vec<u8>> {
        uuid.map(|uuid| uuid.0)
    }
}
