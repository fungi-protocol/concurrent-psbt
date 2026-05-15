// -- Deterministic sort key derivation --------------------------------------

/// Derive a sort key from a seed and an item identifier using HMAC-SHA256.
///
/// The derived key is the full 32-byte HMAC output, giving a uniform
/// lexicographic ordering that is deterministic given `seed` and `id`.
pub(crate) fn derive_sort_key(seed: &[u8], id: &[u8]) -> Vec<u8> {
    use bitcoin::hashes::{Hash, HashEngine, hmac, sha256};
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
/// `try_sort` always fails in this state; call `set_deterministic_sort_seed` to transition to
/// [`Seeded`].
#[derive(Debug)]
pub enum Unseeded {}

/// Seed state: seed has been provided.
#[derive(Debug)]
pub enum Seeded {}

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
#[derive(Debug)]
pub enum ExplicitSortKeys {}

/// Sort keys are derived deterministically from a seed.
///
/// Explicit sort keys on individual inputs/outputs are **not** permitted.
///
/// Corresponds to `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01`.
#[derive(Debug)]
pub enum Deterministic<T: SeedState> {
    _Phantom(core::marker::PhantomData<T>, core::convert::Infallible),
}

/// Sort keys are derived from a seed, but individual inputs/outputs may also
/// carry explicit sort keys (which take precedence).
///
/// Corresponds to `PSBT_GLOBAL_SORT_DETERMINISTIC` being **unset**.
#[derive(Debug)]
pub enum Relaxed<T: SeedState> {
    _Phantom(core::marker::PhantomData<T>, core::convert::Infallible),
}

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

// -- Sorter ------------------------------------------------------------------

use crate::tx::UnorderedPsbt;
use psbt_v2::v2::Psbt;

/// Error returned when a [`Sorter`] is constructed from a [`UnorderedPsbt`]
/// whose flags do not match the requested sort mode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SorterError {
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is absent but `ExplicitSortKeys` or
    /// `Deterministic` mode was requested, or it is present but `Relaxed` was
    /// requested.
    #[error("PSBT_GLOBAL_SORT_DETERMINISTIC flag does not match the requested sort mode")]
    SortModeMismatch,
    /// A seed is required for this sort mode but `PSBT_GLOBAL_SORT_SEED` is absent.
    #[error("PSBT_GLOBAL_SORT_SEED is required for this sort mode but is not set")]
    MissingSeed,
}

/// Owns an [`UnorderedPsbt`] and sorts it according to sort mode `S`.
///
/// Obtain a `Sorter` from a [`crate::constructor::Constructor`] via
/// [`crate::constructor::Constructor::into_sorter`], or directly from an
/// [`UnorderedPsbt`] using the checked `new` on each mode-specific impl.
///
/// Call [`Sorter::try_sort`] for a fallible sort (returns `Err` only if
/// explicit keys are missing or duplicated). On [`CanSortInfallibly`] modes
/// [`Sorter::sort`] is also available.
pub struct Sorter<S: SortMode>(UnorderedPsbt, core::marker::PhantomData<S>);

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
    /// Only use this when the PSBT's flags have already been validated (e.g.
    /// from [`crate::constructor::Constructor::into_sorter`]).
    pub(crate) fn new_unchecked(psbt: UnorderedPsbt) -> Self {
        Sorter(psbt, core::marker::PhantomData)
    }

    /// Consume the sorter and return the inner [`UnorderedPsbt`].
    pub fn into_psbt(self) -> UnorderedPsbt {
        self.0
    }
}

impl Sorter<ExplicitSortKeys> {
    /// Construct from a [`UnorderedPsbt`], validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is `0x00`.
    pub fn new(psbt: UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.is_sort_explicit() {
            return Err(SorterError::SortModeMismatch);
        }
        Ok(Self::new_unchecked(psbt))
    }
}

impl Sorter<Deterministic<Seeded>> {
    /// Construct from a [`UnorderedPsbt`], validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is `0x01` and a seed is present.
    pub fn new(psbt: UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.is_sort_deterministic() {
            return Err(SorterError::SortModeMismatch);
        }
        if psbt.global.sort_seed().is_none() {
            return Err(SorterError::MissingSeed);
        }
        Ok(Self::new_unchecked(psbt))
    }
}

impl Sorter<Relaxed<Seeded>> {
    /// Construct from a [`UnorderedPsbt`], validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is absent and a seed is present.
    pub fn new(psbt: UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.sort_deterministic_absent() {
            return Err(SorterError::SortModeMismatch);
        }
        if psbt.global.sort_seed().is_none() {
            return Err(SorterError::MissingSeed);
        }
        Ok(Self::new_unchecked(psbt))
    }
}

impl Sorter<Relaxed<Unseeded>> {
    /// Construct from a [`UnorderedPsbt`], validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is absent and no seed is set.
    pub fn new(psbt: UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.sort_deterministic_absent() {
            return Err(SorterError::SortModeMismatch);
        }
        Ok(Self::new_unchecked(psbt))
    }

    // FIXME needs set_deterministic_sort_seed
}

impl Sorter<ExplicitSortKeys> {
    /// Sort by explicit per-input/output sort keys.
    ///
    /// Returns `Err` if any key is missing or two items share the same key.
    pub fn try_sort(self) -> Result<Psbt, crate::constructor::SortingError> {
        sort_explicit(self.0)
    }

    /// Sort by explicit per-input/output sort keys (infallible variant).
    ///
    /// Only available when the sort mode guarantees all keys are present.
    pub fn sort(self) -> Psbt {
        self.try_sort()
            .expect("ExplicitSortKeys: all sort keys must be present and distinct")
    }
}

impl Sorter<Deterministic<Seeded>> {
    /// Sort by seed-derived keys
    ///
    /// This is infallible — keys are always derivable from the seed.
    pub fn try_sort(self) -> Result<Psbt, crate::constructor::SortingError> {
        Ok(self.sort())
    }

    /// Sort by seed-derived keys (infallible).
    pub fn sort(self) -> Psbt {
        sort_deterministic(self.0)
    }
}

impl Sorter<Relaxed<Seeded>> {
    /// Sort using explicit keys where present, otherwise seed-derived.
    ///
    /// This is infallible — any missing explicit key is derived from the seed.
    pub fn try_sort(self) -> Result<Psbt, crate::constructor::SortingError> {
        Ok(self.sort())
    }

    /// Sort using explicit keys where present, otherwise seed-derived (infallible).
    pub fn sort(self) -> Psbt {
        sort_relaxed_seeded(self.0)
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

// -- Internal sort helpers ---------------------------------------------------

// FIXME this function is not misuse resistant, it should be folded into Sorter<ExplicitSortKeys> since that's the only place it's well defined
fn sort_explicit(psbt: UnorderedPsbt) -> Result<Psbt, crate::constructor::SortingError> {
    use crate::fields::GlobalFieldsExt as _;
    use crate::input::InputExt as _;
    use crate::output::OutputExt as _;

    let inputs = sort_by_extracted_key(psbt.inputs, |i| i.take_sort_key())?;
    let outputs = sort_by_extracted_key(psbt.outputs, |o| o.take_sort_key())?;
    let mut global = psbt.global;
    global.clear_tx_unordered();
    Ok(Psbt {
        global,
        inputs,
        outputs,
    })
}

// FIXME this function is not misuse resistant, it should be folded into Sorter<Deterministic<Seeded>> since that's the only place it's well defined
fn sort_deterministic(psbt: UnorderedPsbt) -> Psbt {
    use crate::fields::GlobalFieldsExt as _;
    use crate::input::InputExt as _;
    use crate::output::OutputExt as _;
    use crate::sort::OutPointIdentifier as _;

    let seed = psbt
        .global
        .sort_seed()
        .expect("Deterministic<Seeded> always has a seed")
        .clone();
    let inputs = sort_by_extracted_key(psbt.inputs, |i| {
        Some(derive_sort_key(&seed, &i.out_point().to_identifier()))
    })
    .expect("derived keys are always present and distinct");
    let outputs = sort_by_extracted_key(psbt.outputs, |o| {
        Some(derive_sort_key(&seed, &o.unique_id()))
    })
    .expect("derived keys are always present and distinct");
    let mut global = psbt.global;
    global.clear_tx_unordered();
    Psbt {
        global,
        inputs,
        outputs,
    }
}

fn sort_relaxed_seeded(psbt: UnorderedPsbt) -> Psbt {
    use crate::fields::GlobalFieldsExt as _;
    use crate::input::InputExt as _;
    use crate::output::OutputExt as _;

    let seed = psbt
        .global
        .sort_seed()
        .expect("Relaxed<Seeded> always has a seed")
        .clone();
    let inputs = sort_by_extracted_key(psbt.inputs, |i| Some(i.take_or_derive_sort_key(&seed)))
        .expect("take_or_derive always returns Some");
    let outputs = sort_by_extracted_key(psbt.outputs, |o| Some(o.take_or_derive_sort_key(&seed)))
        .expect("take_or_derive always returns Some");
    let mut global = psbt.global;
    global.clear_tx_unordered();
    Psbt {
        global,
        inputs,
        outputs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constructor::Creator;
    use crate::input::InputExt as _;

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

    // -- Sorter::new checked constructor tests --------------------------------

    #[test]
    fn sorter_explicit_new_rejects_wrong_flag() {
        // Relaxed PSBT → ExplicitSortKeys sorter should fail
        let u = Creator::new().into_unordered_psbt();
        assert_eq!(
            Sorter::<ExplicitSortKeys>::new(u),
            Err(SorterError::SortModeMismatch)
        );
    }

    #[test]
    fn sorter_explicit_new_accepts_correct_flag() {
        let u = Creator::new().explicit_sort_keys().into_unordered_psbt();
        assert!(Sorter::<ExplicitSortKeys>::new(u).is_ok());
    }

    #[test]
    fn sorter_deterministic_seeded_new_rejects_missing_seed() {
        let u = Creator::new().deterministic_sorting().into_unordered_psbt();
        assert_eq!(
            Sorter::<Deterministic<Seeded>>::new(u),
            Err(SorterError::MissingSeed)
        );
    }

    #[test]
    fn sorter_deterministic_seeded_new_accepts_seed() {
        let u = Creator::new()
            .deterministic_sorting()
            .set_deterministic_sort_seed(b"seed-16-bytes!!!".to_vec())
            .into_unordered_psbt();
        assert!(Sorter::<Deterministic<Seeded>>::new(u).is_ok());
    }

    #[test]
    fn sorter_relaxed_seeded_new_rejects_wrong_flag() {
        // ExplicitSortKeys PSBT → Relaxed<Seeded> sorter should fail
        let u = Creator::new().explicit_sort_keys().into_unordered_psbt();
        assert_eq!(
            Sorter::<Relaxed<Seeded>>::new(u),
            Err(SorterError::SortModeMismatch)
        );
    }

    #[test]
    fn sorter_relaxed_seeded_new_accepts_seed() {
        let u = Creator::new()
            .set_deterministic_sort_seed(b"seed-16-bytes!!!".to_vec())
            .into_unordered_psbt();
        assert!(Sorter::<Relaxed<Seeded>>::new(u).is_ok());
    }

    #[test]
    fn sorter_explicit_standalone() {
        // Use Sorter<ExplicitSortKeys> directly from an UnorderedPsbt, without Constructor.
        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let mut unordered = Creator::new().explicit_sort_keys().into_unordered_psbt();

        let mut input_b = psbt_v2::v2::Input::new(&op_b);
        input_b.set_sort_key(vec![0x01]); // sorts first
        let mut input_a = psbt_v2::v2::Input::new(&op_a);
        input_a.set_sort_key(vec![0x02]); // sorts second
        unordered.global.input_count = 2;
        unordered.inputs = [input_b, input_a].into_iter().collect();

        let sorter = Sorter::<ExplicitSortKeys>::new(unordered).unwrap();
        let psbt = sorter.sort();

        assert_eq!(psbt.inputs[0].spent_output_index, 1); // op_b (key 0x01)
        assert_eq!(psbt.inputs[1].spent_output_index, 0); // op_a (key 0x02)
    }

    #[test]
    fn sorter_deterministic_seeded_standalone() {
        // Use Sorter<Deterministic<Seeded>> directly.
        let seed = b"standalone-seed!!".to_vec();

        let mut op_a = bitcoin::OutPoint::null();
        op_a.vout = 0;
        let mut op_b = bitcoin::OutPoint::null();
        op_b.vout = 1;

        let mut unordered = Creator::new()
            .deterministic_sorting()
            .set_deterministic_sort_seed(seed.clone())
            .into_unordered_psbt();
        unordered.global.input_count = 2;
        unordered.inputs = [
            psbt_v2::v2::Input::new(&op_a),
            psbt_v2::v2::Input::new(&op_b),
        ]
        .into_iter()
        .collect();

        let sorter = Sorter::<Deterministic<Seeded>>::new(unordered).unwrap();
        let psbt = sorter.sort();
        assert_eq!(psbt.inputs.len(), 2);
        // Verify determinism: same seed → same order.
        let mut unordered2 = Creator::new()
            .deterministic_sorting()
            .set_deterministic_sort_seed(seed)
            .into_unordered_psbt();
        unordered2.global.input_count = 2;
        unordered2.inputs = [
            psbt_v2::v2::Input::new(&op_b),
            psbt_v2::v2::Input::new(&op_a),
        ]
        .into_iter()
        .collect();
        let psbt2 = Sorter::<Deterministic<Seeded>>::new(unordered2)
            .unwrap()
            .sort();
        assert_eq!(
            psbt.inputs
                .iter()
                .map(|i| i.spent_output_index)
                .collect::<Vec<_>>(),
            psbt2
                .inputs
                .iter()
                .map(|i| i.spent_output_index)
                .collect::<Vec<_>>(),
        );
    }
}
