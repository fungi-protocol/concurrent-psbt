//! Fields this crate defines, stored under its own proprietary prefix until
//! they are assigned standard keys.

use psbt_v2::raw;

use crate::PROPRIETARY_PREFIX;

/// The proprietary key with `subtype` under [`PROPRIETARY_PREFIX`].
pub(crate) fn key(subtype: u8) -> raw::ProprietaryKey {
    raw::ProprietaryKey {
        prefix: PROPRIETARY_PREFIX.to_vec(),
        subtype,
        key: vec![],
    }
}

/// How a field this crate defines is stored in a proprietary key.
///
/// A field's key and encoding are defined by its codec, so changing them does
/// not change the field's type or join.
pub(crate) trait ProprietaryField {
    /// The subtype of the field's key under [`PROPRIETARY_PREFIX`].
    const SUBTYPE: u8;
    /// The decoded field.
    type Value;
    /// Why a field cannot be decoded.
    type Error;
    /// Decode the field from its key's value, or from `None` if the key is
    /// absent.
    fn decode(value: Option<Vec<u8>>) -> Result<Self::Value, Self::Error>;
    /// Encode the field as its key's value, or `None` to leave the key out.
    fn encode(value: Self::Value) -> Option<Vec<u8>>;
}
