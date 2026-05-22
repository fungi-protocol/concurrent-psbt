//! The subtypes of the input proprietary fields, and how each is stored.

use crate::psbt::proprietary::ProprietaryField;

use super::{InvalidSortKey, SortKey};

/// Subtype for `PSBT_IN_SORT_KEY`.
pub const PSBT_IN_SORT_KEY_SUBTYPE: u8 = 16;

/// Stores a [`SortKey`] in `PSBT_IN_SORT_KEY` or `PSBT_OUT_SORT_KEY`, under
/// `SUBTYPE`. Absent means no sort key.
pub(crate) struct SortKeyField<const SUBTYPE: u8>;

impl<const S: u8> ProprietaryField for SortKeyField<S> {
    const SUBTYPE: u8 = S;
    type Value = Option<SortKey>;
    type Error = InvalidSortKey;

    fn decode(value: Option<Vec<u8>>) -> Result<Option<SortKey>, InvalidSortKey> {
        value.map(SortKey::try_from).transpose()
    }

    fn encode(sort_key: Option<SortKey>) -> Option<Vec<u8>> {
        sort_key.map(SortKey::into_bytes)
    }
}
