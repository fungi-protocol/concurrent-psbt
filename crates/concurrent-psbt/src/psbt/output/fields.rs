//! The subtypes of the output proprietary fields, and how each is stored.

use crate::psbt::proprietary::ProprietaryField;

use super::{InvalidOutput, UniqueId};

/// Subtype for `PSBT_OUT_UNIQUE_ID`.
pub const PSBT_OUT_UNIQUE_ID_SUBTYPE: u8 = 1;
/// Subtype for `PSBT_OUT_SORT_KEY`.
pub const PSBT_OUT_SORT_KEY_SUBTYPE: u8 = 16;

/// Stores an output's unique ID in `PSBT_OUT_UNIQUE_ID`.
pub(super) struct UniqueIdField;

impl ProprietaryField for UniqueIdField {
    const SUBTYPE: u8 = PSBT_OUT_UNIQUE_ID_SUBTYPE;
    type Value = UniqueId;
    type Error = InvalidOutput;

    fn decode(value: Option<Vec<u8>>) -> Result<UniqueId, InvalidOutput> {
        let bytes = value.ok_or(InvalidOutput::MissingUniqueId)?;
        UniqueId::try_from(bytes).map_err(InvalidOutput::InvalidUniqueId)
    }

    fn encode(unique_id: UniqueId) -> Option<Vec<u8>> {
        Some(unique_id.into_bytes())
    }
}
