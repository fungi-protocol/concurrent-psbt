// -- Deterministic sort key derivation --------------------------------------

/// Derive a sort key from a seed and an item identifier using HMAC-SHA256.
///
/// The derived key is the full 32-byte HMAC output, giving a uniform
/// lexicographic ordering that is deterministic given `seed` and `id`.
pub(crate) fn derive_sort_key(seed: &[u8], id: &[u8]) -> Vec<u8> {
    use bitcoin::hashes::{hmac, sha256, Hash, HashEngine};
    // FIXME use a taproot style hash with two copies of the hash of tag as full 1st block
    // (so midstate is cacheable) for the type, as a domain separator.
    // then two copies of the hash of the seed. this midstate is sharable for
    // all IDs. then just the id. output IDs are constrained to 16 bytes and and
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

/// Typestate types for the sort-mode parameter of `Constructor<M, S>`.
///
/// The sort mode encodes which sorting strategy is in use and whether a seed
/// has been provided, corresponding to the `PSBT_GLOBAL_SORT_DETERMINISTIC`
/// proprietary field:
///
/// - `0x00`  → [`ExplicitSortKeys`]
/// - `0x01`  → [`Deterministic<_>`]
/// - unset   → [`Relaxed<_>`]
///
/// [`Deterministic`] and [`Relaxed`] are further parameterised by a seed
/// state: [`Unseeded`] (seed not yet provided) or [`Seeded`] (seed present).

// -- Seed state --------------------------------------------------------------

mod sealed {
    pub trait SortMode {}
    pub trait SeedState {}
}

/// Seed state: seed not yet provided.
///
/// `try_sort` always fails in this state; call `set_seed` to transition to
/// [`Seeded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unseeded;

/// Seed state: seed has been provided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seeded;

impl sealed::SeedState for Unseeded {}
impl sealed::SeedState for Seeded {}

/// Marker trait for seed states.
pub trait SeedState: sealed::SeedState {}
impl SeedState for Unseeded {}
impl SeedState for Seeded {}

// -- Sort modes --------------------------------------------------------------

/// All sort keys are set explicitly on every input and output.
///
/// Both `sort()` (infallible) and `try_sort()` are available.
///
/// Corresponds to `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x00`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplicitSortKeys;

/// Sort keys are derived deterministically from a seed.
///
/// Explicit sort keys on individual inputs/outputs are **not** permitted.
///
/// Corresponds to `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deterministic<T: SeedState>(core::marker::PhantomData<T>);

/// Sort keys are derived from a seed, but individual inputs/outputs may also
/// carry explicit sort keys (which take precedence).
///
/// Corresponds to `PSBT_GLOBAL_SORT_DETERMINISTIC` being **unset**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relaxed<T: SeedState>(core::marker::PhantomData<T>);

impl sealed::SortMode for ExplicitSortKeys {}
impl<T: SeedState> sealed::SortMode for Deterministic<T> {}
impl<T: SeedState> sealed::SortMode for Relaxed<T> {}

/// Marker trait for sort modes, sealed against external implementation.
pub trait SortMode: sealed::SortMode {}
impl SortMode for ExplicitSortKeys {}
impl<T: SeedState> SortMode for Deterministic<T> {}
impl<T: SeedState> SortMode for Relaxed<T> {}

// -- Sort traits -------------------------------------------------------------

/// A sort mode that can always sort without failure.
///
/// Implemented by [`ExplicitSortKeys`], [`Deterministic<Seeded>`], and
/// [`Relaxed<Seeded>`].
pub trait CanSortInfallibly: SortMode {}
impl CanSortInfallibly for ExplicitSortKeys {}
impl CanSortInfallibly for Deterministic<Seeded> {}
impl CanSortInfallibly for Relaxed<Seeded> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_sort_mode<S: SortMode>() {}
    fn assert_infallible<S: CanSortInfallibly>() {}

    #[test]
    fn sort_modes_implement_trait() {
        assert_sort_mode::<ExplicitSortKeys>();
        assert_sort_mode::<Deterministic<Unseeded>>();
        assert_sort_mode::<Deterministic<Seeded>>();
        assert_sort_mode::<Relaxed<Unseeded>>();
        assert_sort_mode::<Relaxed<Seeded>>();
    }

    #[test]
    fn infallible_sort_modes() {
        assert_infallible::<ExplicitSortKeys>();
        assert_infallible::<Deterministic<Seeded>>();
        assert_infallible::<Relaxed<Seeded>>();
    }
}
