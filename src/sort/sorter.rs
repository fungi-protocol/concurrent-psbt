//! [`Sorter<S>`] struct, sort-key derivation helpers, and `sort_by_extracted_key`.

use crate::psbt::tx::UnorderedPsbt;

use super::traits::SortMode;

// -- Key derivation ----------------------------------------------------------

/// Domain separator tag for deterministic sort-key derivation.
///
/// Per the multiparty-protocol spec: BIP341-style tagged hash with this tag
/// as the domain separator. The TODO in the spec to pick a real name is noted;
/// we use the placeholder until the BIP number is assigned.
const SORT_KEY_TAG: &[u8] = b"BIP ???? deterministic ordering";

/// Derive a sort key from a seed and an item identifier using a BIP341-style
/// tagged SHA256 hash.
///
/// The hash is `SHA256(SHA256(tag) || SHA256(tag) || seed || id)`, giving a
/// 32-byte output. The tag midstate (`SHA256(tag) || SHA256(tag)`) is computed
/// once per call; when many keys share the same seed the seed can be hashed
/// into the midstate separately for efficiency (not done here).
///
/// Per spec:
/// - For inputs: `id = TXID || vout_LE`  (36 bytes, see [`OutPointIdentifier`])
/// - For outputs: `id = PSBT_OUT_UNIQUE_ID` (16 bytes)
pub(crate) fn derive_sort_key(seed: &[u8], id: &[u8]) -> Vec<u8> {
    use bitcoin::hashes::{sha256, sha256t, Hash, HashEngine};

    struct SortKeyTag;
    impl sha256t::Tag for SortKeyTag {
        fn engine() -> sha256::HashEngine {
            let tag_hash = sha256::Hash::hash(SORT_KEY_TAG);
            let mut engine = sha256::HashEngine::default();
            engine.input(tag_hash.as_byte_array());
            engine.input(tag_hash.as_byte_array());
            engine
        }
    }

    let mut engine = <SortKeyTag as sha256t::Tag>::engine();
    engine.input(seed);
    engine.input(id);
    sha256t::Hash::<SortKeyTag>::from_engine(engine)
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

/// Error returned when a [`Sorter`] is constructed from an unordered PSBT
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

/// Owns an unordered PSBT and sorts it according to sort mode `S`.
///
/// Obtain a `Sorter` via:
/// - [`crate::constructor::Constructor::into_sorter`] (flags already validated), or
/// - the checked `Sorter::<S>::new(psbt)` on each mode-specific impl.
///
/// Call `try_sort()` for a fallible sort. On seeded or explicit-key modes
/// `sort()` is also available (infallible).
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
    /// Wrap an unordered PSBT without checking its flags.
    ///
    /// Only use this when the flags have already been validated (e.g. from
    /// [`crate::constructor::Constructor::into_sorter`]).
    pub(crate) fn new_unchecked(psbt: UnorderedPsbt) -> Self {
        Sorter(psbt, core::marker::PhantomData)
    }

    /// Consume the sorter and return the inner unordered PSBT.
    // TODO: becomes `pub` when UnorderedPsbt is published.
    #[allow(dead_code)]
    pub(crate) fn into_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Consume the sorter and return the PSBT in arbitrary (hash-map) order,
    /// without stripping the `PSBT_GLOBAL_TX_UNORDERED` flag.
    ///
    /// Use this when you want the PSBT fields accessible but don't need a
    /// canonical ordering. For a properly sorted result use `try_sort()` /
    /// `sort()`.
    pub fn into_shuffled_psbt(self) -> psbt_v2::v2::Psbt {
        self.0.to_shuffled_psbt()
    }
}


impl<S> Sorter<S>
where
    S: SortMode,
    Sorter<S>: super::traits::Sortable,
{
    /// Sort infallibly and return the sorted [`psbt_v2::v2::Psbt`].
    ///
    /// Only available when the sort mode can always produce a sorted result
    /// (seeded or explicit-key modes).
    pub fn try_into_sorted_psbt(self) -> psbt_v2::v2::Psbt {
        use super::traits::Sortable as _;
        self.sort_psbt()
    }
}
