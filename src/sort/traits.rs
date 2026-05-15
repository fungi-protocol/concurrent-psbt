//! Sort-mode typestate types and sorting traits.

mod sealed {
    pub trait SortMode {}
    pub trait SeedState {}
}

// -- Seed state --------------------------------------------------------------

/// Seed state: seed not yet provided.
///
/// Call `set_seed` to transition to [`Seeded`].
#[derive(Debug)]
pub enum Unseeded {}

/// Seed state: seed has been provided.
#[derive(Debug)]
pub enum Seeded {}

impl sealed::SeedState for Unseeded {}
impl sealed::SeedState for Seeded {}

/// Marker trait for seed states, sealed against external implementation.
pub trait SeedState: sealed::SeedState {}
impl SeedState for Unseeded {}
impl SeedState for Seeded {}

// -- Sort modes --------------------------------------------------------------

/// Typestate types for the sort-mode parameter of `Constructor<M, S>`.
///
/// - `0x00`  → [`ExplicitSortKeys`]
/// - `0x01`  → [`Deterministic<_>`]
/// - unset   → [`Relaxed<_>`]
///
/// All sort keys are set explicitly on every input and output.
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

// -- Sort capability traits --------------------------------------------------

/// A sort mode that can always sort without failure.
///
/// Implemented by [`ExplicitSortKeys`], [`Deterministic<Seeded>`], and
/// [`Relaxed<Seeded>`].
pub trait CanSortInfallibly: SortMode {}
impl CanSortInfallibly for ExplicitSortKeys {}
impl CanSortInfallibly for Deterministic<Seeded> {}
impl CanSortInfallibly for Relaxed<Seeded> {}

/// A [`super::Sorter`] that can produce a sorted [`psbt_v2::v2::Psbt`], or
/// return a [`crate::constructor::SortingError`].
pub trait TrySortable: Sized {
    fn try_sort_psbt(self) -> Result<psbt_v2::v2::Psbt, crate::constructor::SortingError>;
}

/// A [`super::Sorter`] that can produce a sorted [`psbt_v2::v2::Psbt`] infallibly.
pub trait Sortable: TrySortable {
    fn sort_psbt(self) -> psbt_v2::v2::Psbt;
}
