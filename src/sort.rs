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
pub struct Unseeded;

/// Seed state: seed has been provided.
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
pub struct ExplicitSortKeys;

/// Sort keys are derived deterministically from a seed.
///
/// Explicit sort keys on individual inputs/outputs are **not** permitted.
///
/// Corresponds to `PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01`.
pub struct Deterministic<T: SeedState>(core::marker::PhantomData<T>);

/// Sort keys are derived from a seed, but individual inputs/outputs may also
/// carry explicit sort keys (which take precedence).
///
/// Corresponds to `PSBT_GLOBAL_SORT_DETERMINISTIC` being **unset**.
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
