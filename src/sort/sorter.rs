//! [`Sorter<S>`] struct, sort-key derivation helpers, and `sort_by_extracted_key`.

use crate::tx::UnorderedPsbt;

use super::traits::SortMode;

// -- Key derivation ----------------------------------------------------------

/// Derive a sort key from a seed and an item identifier using HMAC-SHA256.
///
/// The derived key is the full 32-byte HMAC output, giving a uniform
/// lexicographic ordering that is deterministic given `seed` and `id`.
pub(crate) fn derive_sort_key(seed: &[u8], id: &[u8]) -> Vec<u8> {
    use bitcoin::hashes::{hmac, sha256, Hash, HashEngine};
    // FIXME use a taproot style hash with two copies of the hash of tag as full 1st block
    // (so midstate is cacheable) for the type, as a domain separator.
    // then two copies of the hash of the seed. this midstate is sharable for
    // all IDs. then just the id. output IDs are constrained to 16 bytes and
    // outpoints are 36, both fit in one block and don't allow length extension.
    let mut engine = hmac::HmacEngine::<sha256::Hash>::new(seed);
    engine.input(id);
    hmac::Hmac::<sha256::Hash>::from_engine(engine)
        .as_byte_array()
        .to_vec()
}

/// Serialize an `OutPoint` to bytes for use as an HMAC identifier.
///
/// Layout: txid bytes (32) || vout (4, little-endian).
pub(crate) trait OutPointIdentifier {
    fn to_identifier(&self) -> Vec<u8>;
}

impl OutPointIdentifier for bitcoin::OutPoint {
    fn to_identifier(&self) -> Vec<u8> {
        use bitcoin::hashes::Hash as _;
        let mut id = Vec::with_capacity(36);
        id.extend_from_slice(self.txid.as_byte_array());
        id.extend_from_slice(&self.vout.to_le_bytes());
        id
    }
}

// -- sort_by_extracted_key ---------------------------------------------------

/// Sort `items` by keys extracted via `take_key`, using a `BTreeMap` for
/// lexicographic order. Returns `Err` if any key is missing or duplicated.
pub(crate) fn sort_by_extracted_key<T>(
    items: impl IntoIterator<Item = T>,
    mut take_key: impl FnMut(&mut T) -> Option<Vec<u8>>,
) -> Result<Vec<T>, crate::constructor::SortingError> {
    use crate::constructor::SortingError;
    use std::collections::BTreeMap;
    let mut map: BTreeMap<Vec<u8>, T> = BTreeMap::new();
    for mut item in items {
        let key = take_key(&mut item).ok_or(SortingError::MissingSortKey)?;
        if map.insert(key, item).is_some() {
            return Err(SortingError::DuplicateSortKey);
        }
    }
    Ok(map.into_values().collect())
}

// -- SorterError -------------------------------------------------------------

/// Error returned when a [`Sorter`] is constructed from a [`UnorderedPsbt`]
/// whose flags do not match the requested sort mode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SorterError {
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` flag does not match the requested sort mode.
    #[error("PSBT_GLOBAL_SORT_DETERMINISTIC flag does not match the requested sort mode")]
    SortModeMismatch,
    /// A seed is required for this sort mode but `PSBT_GLOBAL_SORT_SEED` is absent.
    #[error("PSBT_GLOBAL_SORT_SEED is required for this sort mode but is not set")]
    MissingSeed,
}

// -- Sorter ------------------------------------------------------------------

/// Owns an [`UnorderedPsbt`] and sorts it according to sort mode `S`.
///
/// Obtain a `Sorter` via:
/// - [`crate::constructor::Constructor::into_sorter`] (flags already validated), or
/// - the checked `Sorter::<S>::new(psbt)` on each mode-specific impl.
///
/// Call `try_sort()` for a fallible sort. On [`super::traits::CanSortInfallibly`]
/// modes `sort()` is also available.
pub struct Sorter<S: SortMode>(
    pub(super) UnorderedPsbt,
    pub(super) core::marker::PhantomData<S>,
);

impl<S: SortMode> core::fmt::Debug for Sorter<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Sorter").field(&self.0).finish()
    }
}

impl<S: SortMode> PartialEq for Sorter<S> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S: SortMode> Eq for Sorter<S> {}

impl<S: SortMode> Sorter<S> {
    /// Wrap an [`UnorderedPsbt`] without checking its flags.
    ///
    /// Only use this when the flags have already been validated (e.g. from
    /// [`crate::constructor::Constructor::into_sorter`]).
    pub(crate) fn new_unchecked(psbt: UnorderedPsbt) -> Self {
        Sorter(psbt, core::marker::PhantomData)
    }

    /// Consume the sorter and return the inner [`UnorderedPsbt`].
    pub fn into_psbt(self) -> UnorderedPsbt {
        self.0
    }

    // FIXME add into_shuffled_psbt(self) -> Psbt, returns psbt without sorting

    // FIXME add try_into_sorted_psbt(self) -> Result<Psbt, UnsortedPsbt>
}
